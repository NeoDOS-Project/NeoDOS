# HTTP/TLS/Update Kernel Prerequisites — Design Document

> **Version:** v0.2 — reconciled with `develop` @ `532d554` (2026-11-14)
> **Status:** Design. Step 1 (TCP connect) is **implemented**; the CSPRNG, the
> durability primitives and the userland byte-stream gaps below are not.
> **Target Release:** v0.55 (WAL + VFS Advanced) / v0.56 for CSPRNG
> **Related:** #307, #315, #411, #467, #484, #486, #491, #593, #692, #696, #710;
> `docs/design/system-configuration-time-network-security-update-design.md`,
> `docs/design/libhttp-neocurl-design.md`, `docs/design/libtls-https-design.md`
> **ABI Impact:** new `ObInfoClass::Random=48`, `RandomCaps=49` (44/45 are taken
> by `KeyboardRead`/`KeyboardPoll`); proposed `ObSetInfoClass::FileSync=52`,
> `FileReplace=54`; **no new syscall number** (all via `sys_ob_query_info` /
> `sys_ob_set_info`)

This document designs the kernel/ABI capabilities and the two byte-stream fixes
that the HTTP, TLS and update features depend on. It is deliberately scoped to
*prerequisites*; it does not design the userland features themselves (see the
sibling docs). v0.2 re-verifies every claim against `develop` @ `532d554`: the
TCP-connect prerequisite is implemented, and the remaining gaps are entropy,
durability and byte-stream semantics.

---

## 1. Research

| Source | What it establishes |
| --- | --- |
| `neodos-kernel/src/syscall/ob/set/net.rs` | `SocketConnect=18` routes to `socket_connect_user` (TCP → `socket_connect`/SYN; UDP → records peer + `Connected`). Previously it only flipped a flag. **Fixed** (v0.51.x, #692). |
| `neodos-kernel/src/net/socket.rs` | `socket_connect`/`tcp_connect` real connect; `tcp_dispatch` never holds `SOCKET_MANAGER` across protocol actions; `socket_recv` returns `Again` for both "no data" and EOF; `socket_send` returns `Io` when the 16 KiB buffer is full |
| `neodos-kernel/src/net/tcp.rs` | full state machine, RTO (200 ms, 8 retries), `fin_pending` drain, 32 connections, 16 KiB TX/RX, MSS 1460, fixed ISS (1000/2000), in-order-only RX |
| `neodos-kernel/src/net/nic.rs`, `loopback.rs` | next-hop/gateway selection, per-socket NIC binding (#317), 127/8 loopback without a NIC (#484) |
| `syscall/ob/wait.rs`, `libneodos/src/syscall/ob.rs` | `ob_wait` on sockets blocks on Connect/Accept/Read; `sys_ob_wait_timeout` exists (#696) and returns `Ok` on expiry |
| `neodos-kernel/src/hal/mod.rs`, `hal/raw/cpu.rs` | `has_rdrand`, `rdrand` (10-retry); kernel-only, used for ASLR; **no RDSEED**, no Ring-3 exposure |
| `neodos-kernel/src/object/types.rs` | `ObInfoClass` 44/45 are `KeyboardRead`/`KeyboardPoll`; **next free is 48**. `ObSetInfoClass` 52 and 54 are free (55 is `KeyboardSetRawMode`) |
| `neodos-kernel/src/fs/vfs/mod.rs` | `FileSystem` trait has `read/write/create/rename/...` but **no `sync`/`fsync`** and no atomic replace |
| `neodos-kernel/src/fs/neofs/neodos_v2.rs` | `write()`, `create()`, `mkdir()`, `remove_*`, `rename()` and snapshots now call `save_sb()`; `rename()` is still same-directory delete-then-insert, not a replace |
| `neodos-kernel/src/arch/x64/paging.rs` | 2 MiB of heap per process, 16 slots (relevant to buffering) |
| `libneodos/src/syscall/time.rs`, `syscall/ob/query/time.rs` | `DateTime=9` (RTC, 1 s), `Uptime=46` and `HighResTime=47` (#467); no UTC-seconds helper in userland |
| `libntp/src/lib.rs`, `userbin/ntpd/src/main.rs` | SNTP client, civil-time math, RTC step via `ob_set_datetime`; publishes `Services\Ntpd\Status` (`State`, `LastSync`, `Stratum`, `OffsetMs`) |
| `libnet/src/dns.rs` | resolver + Registry servers + cache **without TTL expiry** |
| `docs/kernel/objects.md`, `docs/kernel/syscalls.md` | ObType list, `ob_query_info`/`ob_set_info` semantics, `sys_ob_*` rule |
| `docs/design/system-configuration-time-network-security-update-design.md` | the master plan that motivates these prerequisites |

Verified facts (re-audited 2026-11-14 on `develop` @ `532d554`):

- **Userland TCP connect works.** `netcli`, `netsrv` and `tcptest` exercise the
  public socket ABI end to end (connect/accept, data, clean close); the kernel
  suite covers loopback and a scripted NIC path.
- **No CSPRNG is reachable from Ring 3.** `rdrand` is a kernel helper with no
  object boundary; there is no `\Device\Random`.
- **No `fsync` and no atomic replacement.** `write()` is durable (it commits the
  superblock), but there is no way to force a flush after the fact and no
  replace-onto-target operation.
- **No userland UTC-seconds reference.** The RTC is readable and NTP can step
  it, but no API converts `DateTime` to Unix seconds and nothing states whether
  the clock was ever synchronised.
- **The byte-stream API has two rough edges** that a robust HTTP client needs
  fixed: writes cannot wait for TX buffer space (a full buffer is reported as
  `-Io`), and `SocketRecv` does not distinguish EOF from `EAGAIN`.

---

## 2. Problem analysis

1. ~~**TCP connect (blocker for HTTP/TLS).**~~ **Resolved.** `SocketConnect`
   now initiates the handshake and wakes the connect waiter when the socket
   reaches `Established`. HTTP/1.1 no longer depends on a kernel fix to start.
2. **No entropy (blocker for TLS).** TLS needs unpredictable random bytes
   (nonces, ephemeral keys). RDRAND exists but is kernel-only and used only for
   ASLR; there is no Ob/syscall path and no RDSEED fallback.
3. **No durability (blocker for updates).** There is no way to force data +
   metadata to stable storage, and no atomic replace. `NeoDosFsV2::write`
   commits the superblock, but `Vfs::rename` is still same-directory
   delete-then-insert and there is no replace-onto-target operation. An updater
   cannot safely activate.
4. **Byte-stream gaps (blocker for *robust* HTTP).** A client that uploads more
   than 16 KiB or reads a body delimited by connection close needs two things
   the socket API does not offer cleanly today: waiting for TX space and
   distinguishing EOF from `EAGAIN`. Both are workaround-able (timed waits,
   `TcpStatus` queries) but should be fixed once.

Why existing abstractions cannot solve them:

- Sockets expose non-blocking `recv` with an ambiguous empty-buffer result, and
  a send that fails with a generic `Io` when its buffer is full.
- `rdrand` is a kernel-internal helper with no object boundary.
- VFS has no sync primitive and no replace-onto-target operation; `Rename=6`
  moves a name but is neither atomic nor a replace.

---

## 3. Solution design

### 3.1 Fix userland TCP connect — **IMPLEMENTED**

`ObSetInfoClass::SocketConnect` (`neodos-kernel/src/syscall/ob/set/net.rs`)
routes to `socket::socket_connect_user`, which:

- **TCP:** records the peer, marks the socket `Connecting` and calls
  `tcp_connect` (sends the SYN). The handshake completes asynchronously;
  `tcp_handle_ack` flips the socket to `Connected` and wakes the
  `SocketConnect` waiters.
- **UDP/raw:** records the peer and marks the socket `Connected` immediately.

No new class or syscall: it is a correctness fix to an existing handler.
`ObInfoClass::TcpStatus=19` reports progress and `ob_wait` with a finite timeout
bounds the wait.

The server side is also available: `ObInfoClass::SocketAccept=43` wraps an
accepted connection in a fresh handle (`socket_next_accept_id` is no longer the
only path). This is not required by the HTTP client.

Evidence: `tcptest` (auto-run self-test), `netcli`/`netsrv` (loopback and
VirtualBox NAT), and the kernel tests `net_tcp_loopback_e2e` and
`net_tcp_nic_path_syn_synack_no_deadlock`.

### 3.2 Ring-3 CSPRNG

Add a kernel random device object and two read classes. **No new `ObType`**:
reuse `ObType::Device` with a new `\Device\Random` object created at boot.

Class slots: `ObInfoClass` 44 (`KeyboardRead`) and 45 (`KeyboardPoll`) are
taken since #466; the design uses **48 (`Random`) and 49 (`RandomCaps`)**.

```rust
// neodos-kernel/src/object/types.rs  (NEW)
//   Random = 48, RandomCaps = 49

// neodos-kernel/src/syscall/ob/types.rs  (NEW)
/// Header is not used; the whole buffer is filled. Return value = bytes written.
// ObInfoClass::Random (48): buf_ptr/buf_size = number of random bytes requested.

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

- `ObInfoClass::Random = 48` — fill the buffer with random bytes; returns the
  byte count (or `-Inval` if `buf_size == 0` / too large).
- `ObInfoClass::RandomCaps = 49` — return `SysRandomCaps` (4 bytes).
- Object: `\Device\Random` (`ObType::Device`). READ-only.

TLS consumers MUST check `RandomCaps` and fail closed (`RngUnavailable`) when no
vetted source is reported; they must never silently fall back to TSC or a
userland PRNG.

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
- **Bug fix (already landed):** `write()`, `create()`, `mkdir()`, the
  `remove_*` operations, `rename()` and snapshot operations now commit the
  superblock via `save_sb()`, so the old "write is lost on remount" gap is
  closed. Registry durability is therefore no longer blocked on this;
  `FileSync` remains useful for *forcing* a flush after a batch of writes.

**Ob exposure** (`ObSetInfoClass`, free slots 52/54):

| Value | Name | Payload | Notes |
| --- | --- | --- | --- |
| 52 | `FileSync` | none | fsync the object referenced by the handle |
| 54 | `FileReplace` | `{ src_len: u16, dst_len: u16, src, dst }` UTF-8 | atomic replace; admin-only |

`FileSync` is intentionally a set operation (it has an effect), not a query.

### 3.4 Byte-stream gaps for a robust HTTP client

These are not blockers for a basic client, but they turn two polling
workarounds into correct blocking semantics. Recommended before the `neocurl`
milestone.

**(a) Wait for TX buffer space.** `tcp_send` only appends to `TCP_SEND_BUF`
(16 KiB) and returns `Err(())` when full; the syscall layer maps that to `-Io`,
indistinguishable from a network error, and `tcp_handle_ack` wakes no writer.

- Add a writable wait reason for sockets (`WaitReason::SocketWrite`).
- `socket_wait_reason` returns ready when the connection is `Established` and
  `TCP_SEND_BUF` has free space, or when the socket is closed/failed.
- `tcp_handle_ack` wakes `SocketWrite` waiters after draining `send_buf`.
- Map "buffer full" to `-Again` (retryable) rather than `-Io`.

**(b) Distinguish EOF from `EAGAIN`.** `socket_recv` returns `Err(())` both when
nothing has arrived and when the peer closed. Today a reader must query
`TcpStatus=19` to tell them apart.

- Return `Ok(0)` when the TCP connection is `Closed`/`CloseWait` and
  `recv_buf` is empty. `socket_wait_reason` already reports these states as
  ready, so a `read()` loop terminates instead of spinning.
- Keep `TcpStatus` as the authoritative state for diagnostics.

**(c) Honour DNS TTL.** `libnet/src/dns.rs` caches an address forever. HTTP
clients are long-lived enough for this to matter: store `ttl` and the insertion
time (monotonic ms), and evict on expiry. Userland-only change, no ABI impact.

Without (a) and (b), `libhttp` can still work by polling `TcpStatus` and using
`ob_wait_timeout` on a connected socket; it costs latency and code, and a full
TX buffer looks like an I/O error to the caller.

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
| `syscall/ob/set/net.rs` | TCP connect routing | **Done** (bug fix) |
| `net/socket.rs` | Writable wait reason; EOF on empty closed socket; `Again` instead of `Io` for a full TX buffer | Low/Medium |
| `net/tcp.rs` | Wake writers on ACK; expose free TX space | Low |
| `libnet/src/dns.rs` | Cache with TTL expiry | Low |
| `hal` | `rdseed` feature detection | Low |
| `syscall/ob/query` | `Random`, `RandomCaps` handlers | Low |
| `syscall/ob/set` | `FileSync`, `FileReplace` handlers | Low |
| `object/types.rs` | 4 new class constants (48, 49, 52, 54) | Low |
| `syscall/ob/types.rs` | `SysRandomCaps` | Low |
| `fs/vfs/mod.rs` | `sync`, `replace` trait methods | Medium |
| `fs/neofs/neodos_v2.rs` | `sync`, `replace` (superblock commit on write is **done**) | Medium |
| `fs/other backends` | Implement `sync`/`replace` (FAT32: best-effort) | Medium |
| `boot` | Create `\Device\Random` | Low |
| `libneodos` | Wrappers (`ob_random`, `ob_random_caps`, `ob_file_sync`, `ob_file_replace`) | Low |
| Scheduler / memory | None | None |

---

## 6. API contract

| Operation | Object | Args | Returns | Errors |
| --- | --- | --- | --- | --- |
| `SocketConnect` (TCP) | Socket (TCP) | `{ip:4,port:2}` | `0` (handshake started) | `Inval`, `BadF` |
| `Random` (48) | `\Device\Random` | `buf_size` bytes | bytes written | `Inval` (0/too large), `BadF` |
| `RandomCaps` (49) | `\Device\Random` | 4-byte buffer | 4 | `Inval`, `BadF` |
| `FileSync` (52) | file handle | none | `0` (durable) | `Acces`, `BadF`, `Io` |
| `FileReplace` (54) | file/dir handle | `{src,dst}` | `0` | `Acces`, `BadF`, `Inval`, `Io` |

Preconditions: `FileReplace` requires admin; both paths must resolve on the same
mounted FS; `src` must exist. Postconditions: after `FileSync`/`FileReplace`,
data + metadata + superblock are committed (subject to the block device's own
flush semantics).

---

## 7. Test plan (≥3 per invariant)

**INV-1 — TCP connect sends a SYN and reaches `Established`. (IMPLEMENTED)**

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

**INV-5 — The byte stream reports writability and EOF unambiguously.**

| Test | Expected |
| --- | --- |
| `socket_wait_writable_after_ack` | A writer blocked on a full TX buffer wakes after the peer ACKs |
| `socket_recv_eof_returns_zero` | Closed connection + empty buffer → `Ok(0)`, not `Again` |
| `socket_send_full_is_again` | Full TX buffer → retryable `Again`, not `Io` |
| `dns_cache_expires` | Entry past its TTL is re-resolved |

**Integration:** `ntpd`/Registry flush survives a reboot; a TLS handshake can
obtain entropy; an HTTP client connects over TCP.

---

## 8. Implementation plan

1. ~~**TCP connect fix.**~~ **Done** (`syscall/ob/set/net.rs`, INV-1).
2. **Byte-stream gaps (3.4).** `net/socket.rs`, `net/tcp.rs`, `libnet/src/dns.rs`;
   tests INV-5. Order: before the `neocurl` milestone.
3. **`hal::rdseed` + DRBG.** `hal/mod.rs`, `hal/raw/cpu.rs`, new
   `kernel/src/random.rs`; tests INV-2.
4. **`\Device\Random` + classes 48/49.** `boot`, `object/types.rs`,
   `syscall/ob/query`; tests INV-2.
5. **VFS `sync`/`replace`.** `fs/vfs/mod.rs`; implement in
   `fs/neofs/neodos_v2.rs` (+ FAT32 best-effort). The `save_sb`-on-write part is
   done.
6. **Classes 52/54.** `syscall/ob/set`; tests INV-3/INV-4.
7. **libneodos wrappers** (`ob_random`, `ob_random_caps`, `ob_file_sync`,
   `ob_file_replace`).
8. **Docs + tests + markdownlint.**

Dependencies: step 1 is done. Step 2 is independent of TLS and unblocks robust
HTTP. Steps 3–4 gate TLS. Steps 5–6 gate the updater.

Milestone mapping for the HTTP/HTTPS initiative: step 2 → M2; step 3–4 → M8;
VFS remains outside it (updater).

---

## 9. Open questions

1. Which DRBG primitive in the kernel (ChaCha20 vs SHA-256 counter), given no
   crypto primitives exist yet? (Shared decision with the TLS doc.)
2. Should `FileReplace` be one class or split into `FileSync` + the existing
   `VfsRename` with a new atomic flag?
3. FAT32 `sync`/`replace` semantics (no COW) — best-effort vs unsupported.
4. Should `\Device\Random` be readable by any process (yes proposed) and rate
   limited?
5. Does `SocketRecv` returning `Ok(0)` on EOF break any existing caller
   (`netcli`, `netsrv`, `tcptest`, `ping`, `nslookup`) or do they already treat
   `n == 0` as terminal?

---

*End of design. No code, public API, or issue state is changed by this document.*
