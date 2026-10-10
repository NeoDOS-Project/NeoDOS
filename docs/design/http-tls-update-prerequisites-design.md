# HTTP/TLS/Update Kernel Prerequisites — Design Document

> **Version:** v0.1-draft
> **Status:** Design (no implementation)
> **Target Release:** v0.55 (WAL + VFS Advanced) / v0.53 (Integrity + Signing) for CSPRNG
> **Related:** #307, #315, #411, #467, #491, #593; `docs/design/system-configuration-time-network-security-update-design.md`
> **ABI Impact:** new `ObInfoClass::Random=44`, `RandomCaps=45`; new
> `ObSetInfoClass::FileSync=53`, `FileReplace=54`; **no new syscall number**
> (all via `sys_ob_query_info` / `sys_ob_set_info`)

This document designs the three kernel/ABI capabilities that the HTTP, TLS and
update features depend on. It is deliberately scoped to *prerequisites*; it does
not design the userland features themselves (see the sibling docs).

---

## 1. Research

| Source | What it establishes |
| --- | --- |
| `neodos-kernel/src/syscall/ob/set/net.rs` | `SocketConnect=18` reads `{ip,port}`, calls `socket_set_remote` + `socket_set_connected`; it never calls `socket_connect` |
| `neodos-kernel/src/net/socket.rs` | `socket_connect(id, remote)` performs the real connect (TCP: `tcp_connect`/SYN; UDP: records remote); used only by kernel DNS + tests |
| `neodos-kernel/src/net/tcp.rs` | full state machine, `tcp_connect`, `tcp_tick` flush, RTO; `tcp_send` buffers only |
| `neodos-kernel/src/hal/mod.rs`, `hal/raw/cpu.rs` | `has_rdrand`, `rdrand` (10-retry); kernel-only, used for ASLR; **no RDSEED**, no Ring-3 exposure |
| `neodos-kernel/src/fs/vfs/mod.rs` | `FileSystem` trait has `read/write/create/rename/...` but **no `sync`/`fsync`** |
| `neodos-kernel/src/fs/neofs/neodos_v2.rs` | `write()` does not call `save_sb`; `rename()` = delete-then-insert + one `save_sb`; same-directory only |
| `neodos-kernel/src/object/types.rs` | free class slots: `ObInfoClass` 43–45; `ObSetInfoClass` 52–55 |
| `docs/kernel/objects.md`, `docs/kernel/syscalls.md` | ObType list, `ob_query_info`/`ob_set_info` semantics, `sys_ob_*` rule |
| `docs/design/system-configuration-time-network-security-update-design.md` | the master plan that motivates these prerequisites |

Verified facts (see the master design evidence table):

- **Userland TCP connect is broken** (no SYN is sent).
- **No CSPRNG is reachable from Ring 3.**
- **No `fsync` and no proven atomic replacement.**

---

## 2. Problem analysis

1. **TCP connect (blocker for HTTP/TLS).** `ObSetInfoClass::SocketConnect` marks
   the socket connected but never initiates the TCP handshake, so
   `socket_send` fails (`tcp_send` requires `Established`). No userland TCP
   client can work.
2. **No entropy (blocker for TLS).** TLS needs unpredictable random bytes
   (nonces, ephemeral keys). RDRAND exists but is kernel-only and used only for
   ASLR; there is no Ob/syscall path and no RDSEED fallback.
3. **No durability (blocker for updates).** There is no way to force data +
   metadata to stable storage, and no atomic replace. `NeoDosFsV2::write` never
   commits the superblock, and `Vfs::rename` is same-directory
   delete-then-insert, untested. An updater cannot safely activate.

Why existing abstractions cannot solve them:

- Sockets expose only non-blocking `recv` and a `Connect` that is a flag set.
- `rdrand` is a kernel-internal helper with no object boundary.
- VFS has no sync primitive and no replace-onto-target operation; `Rename=6`
  moves a name but is neither atomic nor a replace.

---

## 3. Solution design

### 3.1 Fix userland TCP connect

Change `neodos-kernel/src/syscall/ob/set/net.rs` for
`ObSetInfoClass::SocketConnect`:

- Determine the socket type (`SocketType::Tcp` vs `Udp`).
- **TCP:** call `crate::net::socket::socket_connect(socket_id, remote)`; return
  `0` on success, `-Io` if the connect could not be initiated. The handshake
  completes asynchronously; `tcp_handle_ack` wakes `SocketConnect` waiters
  (existing `wake_socket_connect_waiters`).
- **UDP:** preserve current semantics (record remote + mark connected), matching
  the kernel DNS path that already uses `socket_connect` for UDP.

No new class or syscall: this is a correctness fix to an existing handler. A
companion read-only query to observe connection progress already exists
(`ObInfoClass::TcpStatus=19`), so callers can poll state until `Established`.

Optionally add `ObSetInfoClass::SocketAccept=30` (free slot) to expose the
server side (`socket_next_accept_id` currently always returns `None`). This is
**out of scope** for the HTTP *client* milestone and listed only for completeness.

### 3.2 Ring-3 CSPRNG

Add a kernel random device object and two read classes. **No new `ObType`**:
reuse `ObType::Device` with a new `\Device\Random` object created at boot.

```rust
// neodos-kernel/src/syscall/ob/types.rs  (NEW)
/// Header is not used; the whole buffer is filled. Return value = bytes written.
// ObInfoClass::Random (44): buf_ptr/buf_size = number of random bytes requested.

/// neodos-kernel/src/syscall/ob/types.rs  (NEW)
#[repr(C)]
pub struct SysRandomCaps {
    pub hw_rdrand: u8,   // 1 if RDRAND is available
    pub hw_rdseed: u8,   // 1 if RDSEED is available (0 today)
    pub pool_ready: u8,  // 1 if the kernel entropy pool is seeded
    pub _pad: u8,
}
```

Implementation approach (kernel, minimal):

1. **Source:** use RDRAND when present; mix in RDSEED when present, else TSC and
   interrupt jitter. Keep the existing `hal::rdrand` and add `hal::rdseed`
   (feature-detected).
2. **Conditioning:** run the output through a small kernel DRBG so raw RDRAND
   never reaches userland unwhitened and the pool survives a source failure.
   A ChaCha20-based DRBG or a SHA-256 counter DRBG is acceptable; the primitive
   chosen here is a **dependency of the crypto design** and is flagged as an
   open question (§9).
3. **Reseeding:** reseed the DRBG from the hardware source periodically and on
   demand; never return more than a bounded number of bytes per call.
4. **Failure policy:** if no hardware source is present, the pool is seeded from
   TSC/jitter and `pool_ready` may be reported; callers that require a CSPRNG
   (TLS) MUST check `RandomCaps` and fail closed if `hw_rdrand == 0` and no
   vetted pool exists.

Exposure:

- `ObInfoClass::Random = 44` — fill the buffer with random bytes; returns the
  byte count (or `-Inval` if `buf_size == 0` / too large).
- `ObInfoClass::RandomCaps = 45` — return `SysRandomCaps` (4 bytes).
- Object: `\Device\Random` (`ObType::Device`). READ-only.

### 3.3 `fsync` + atomic replace

**VFS trait** (`neodos-kernel/src/fs/vfs/mod.rs`):

```rust
// PROPOSED additions to `trait FileSystem`
fn sync(&mut self) -> Result<(), VfsError>;                       // flush dirty data + metadata + superblock
fn replace(&mut self, src: &str, dst: &str) -> Result<(), VfsError>; // atomically make dst == src
```

**NeoFS v2** (`neodos-kernel/src/fs/neofs/neodos_v2.rs`):

- `sync` = flush the page cache for the mounted FS and call `save_sb()`.
- `replace` = ensure `src` is fully written and synced, then perform a
  single-commit replacement of `dst` (COW: insert the new DirEntry for `dst`,
  drop the old one, then one `save_sb`), preserving the existing COW
  crash-consistency model. Cross-directory replace is supported here (unlike the
  current `rename`).
- **Bug fix:** `write()` should commit the superblock (call `save_sb`) or the
  caller must `sync`; document the chosen contract. This also fixes the Registry
  durability gap.

**Ob exposure** (`ObSetInfoClass`, free slots 53/54):

| Value | Name | Payload | Notes |
| --- | --- | --- | --- |
| 53 | `FileSync` | none | fsync the object referenced by the handle |
| 54 | `FileReplace` | `{ src_len: u16, dst_len: u16, src, dst }` UTF-8 | atomic replace; admin-only |

`FileSync` is intentionally a set operation (it has an effect), not a query.

---

## 4. Alternatives

- **TCP connect:** *fix in userland by calling `socket_connect` via a raw
  syscall* — rejected; the flag-setting lives in the kernel handler and userland
  cannot reach `socket_connect`.
- **CSPRNG:** *expose raw RDRAND directly* — rejected; unwhitened hardware
  output is not a general-purpose CSPRNG and has no reseed/failure policy.
  *Add a `getrandom`-style external crate* — rejected; no external crates in
  Ring 3 and the entropy must come from the kernel.
- **Durability:** *assume `rename` is atomic* — rejected; it is delete+insert
  with no test and no superblock commit. *Write-then-reboot-apply only* —
  viable fallback, but `sync` + `replace` is smaller, testable now, and also
  fixes Registry persistence. Chosen.
- **New syscalls** (`sys_ob_fsync`, `sys_ob_random`) — rejected; the
  `ob_query_info`/`ob_set_info` class pattern suffices and keeps the SSDT
  stable.

---

## 5. Affected components

| Subsystem | Change | Impact |
| --- | --- | --- |
| `syscall/ob/set/net.rs` | TCP connect routing | Low (bug fix) |
| `net/socket.rs` | (unchanged; already correct) | None |
| `hal` | `rdseed` feature detection | Low |
| `syscall/ob/query` | `Random`, `RandomCaps` handlers | Low |
| `syscall/ob/set` | `FileSync`, `FileReplace` handlers | Low |
| `object/types.rs` | 4 new class constants | Low |
| `syscall/ob/types.rs` | `SysRandomCaps` | Low |
| `fs/vfs/mod.rs` | `sync`, `replace` trait methods | Medium |
| `fs/neofs/neodos_v2.rs` | `sync`, `replace`, `save_sb` on write | Medium |
| `fs/other backends` | Implement `sync`/`replace` (FAT32: best-effort) | Medium |
| `boot` | Create `\Device\Random` | Low |
| `libneodos` | Wrappers (`ob_random`, `ob_random_caps`, `ob_file_sync`, `ob_file_replace`) | Low |
| Scheduler / memory | None | None |

---

## 6. API contract

| Operation | Object | Args | Returns | Errors |
| --- | --- | --- | --- | --- |
| `SocketConnect` (TCP) | Socket (TCP) | `{ip:4,port:2}` | `0` (handshake started) | `Inval`, `BadF`, `Io` |
| `Random` (44) | `\Device\Random` | `buf_size` bytes | bytes written | `Inval` (0/too large), `BadF` |
| `RandomCaps` (45) | `\Device\Random` | 4-byte buffer | 4 | `Inval`, `BadF` |
| `FileSync` (53) | file handle | none | `0` (durable) | `Acces`, `BadF`, `Io` |
| `FileReplace` (54) | file/dir handle | `{src,dst}` | `0` | `Acces`, `BadF`, `Inval`, `Io` |

Preconditions: `FileReplace` requires admin; both paths must resolve on the same
mounted FS; `src` must exist. Postconditions: after `FileSync`/`FileReplace`,
data + metadata + superblock are committed (subject to the block device's own
flush semantics).

---

## 7. Test plan (≥3 per invariant)

**INV-1 — TCP connect sends a SYN and reaches `Established`.**

| Test | Expected |
| --- | --- |
| `socket_connect_tcp_sends_syn` | After `SocketConnect`, `TcpStatus == SynSent` |
| `socket_connect_tcp_established` | With a loopback server, `TcpStatus` reaches `Established` |
| `socket_connect_udp_unchanged` | UDP connect still records the remote and returns 0 |

**INV-2 — Random bytes are unpredictable and capability-reported.**

| Test | Expected |
| --- | --- |
| `random_returns_requested_len` | N bytes written; return == N |
| `random_two_reads_differ` | Two consecutive buffers differ (statistical, fixed seed-free) |
| `random_caps_reports_hw` | `RandomCaps.hw_rdrand` matches CPUID |
| `random_zero_size_inval` | `buf_size == 0` → `Inval` |

**INV-3 — `FileSync` commits data + superblock.**

| Test | Expected |
| --- | --- |
| `fsync_then_remount_persists` | Data written + `FileSync` survives a simulated remount |
| `write_without_sync_contract` | Documented contract: data present in cache, not necessarily committed |
| `fsync_bad_handle` | `FileSync` on a non-file handle → `BadF` |

**INV-4 — `FileReplace` is atomic and cross-directory.**

| Test | Expected |
| --- | --- |
| `replace_swaps_content` | `dst` content == `src` content after replace |
| `replace_cross_directory` | Works across directories (unlike `rename`) |
| `replace_missing_src` | `Inval`/`NoEnt`, `dst` unchanged |
| `replace_requires_admin` | Non-admin → `Acces` |

**Integration:** `ntpd`/Registry flush survives a reboot; a TLS handshake can
obtain entropy; an HTTP client connects over TCP.

---

## 8. Implementation plan

1. **TCP connect fix.** `syscall/ob/set/net.rs`; tests INV-1. (Unblocks HTTP.)
2. **`hal::rdseed` + DRBG.** `hal/mod.rs`, `hal/raw/cpu.rs`, new
   `kernel/src/random.rs`; tests INV-2.
3. **`\Device\Random` + classes 44/45.** `boot`, `object/types.rs`,
   `syscall/ob/query`; tests INV-2.
4. **VFS `sync`/`replace`.** `fs/vfs/mod.rs`; implement in
   `fs/neofs/neodos_v2.rs` (+ FAT32 best-effort); commit `save_sb` on write.
5. **Classes 53/54.** `syscall/ob/set`; tests INV-3/INV-4.
6. **libneodos wrappers.**
7. **Docs + tests + markdownlint.**

Dependencies: step 1 is independent and highest priority. Steps 2–3 gate TLS.
Steps 4–5 gate the updater and also fix Registry durability.

---

## 9. Open questions

1. Which DRBG primitive in the kernel (ChaCha20 vs SHA-256 counter), given no
   crypto primitives exist yet? (Shared decision with the TLS doc.)
2. Should `FileReplace` be one class or split into `FileSync` + the existing
   `VfsRename` with a new atomic flag?
3. FAT32 `sync`/`replace` semantics (no COW) — best-effort vs unsupported.
4. Should `\Device\Random` be readable by any process (yes proposed) and rate
   limited?

---

*End of design. No code, public API, or issue state is changed by this document.*
