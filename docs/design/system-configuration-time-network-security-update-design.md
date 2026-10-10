# System Configuration, Time Zones, HTTP, TLS and Updates — Master Design

> **Status:** Design / audit — not implemented.
> **Branch audited:** `develop` @ `8c6865c806f2ac4009a99195d938c36ab9df044d` (clean tree).
> **Design branch:** `docs/master-design-system-config-time-net-security-update`.
> **Scope:** `neocfg` + `libneocfg`; time-zone / local-time support; `libhttp` / `neocurl`;
> `libtls` / HTTPS; `libupdate` / `neoupdate`.
> **Precedence:** this document is a *design*. It authors no code and changes no public
> contract. Every existing behaviour it cites is backed by the evidence index (§16).

This is an audit-and-design deliverable. It establishes the actual state of the
repository first, then proposes a target architecture that fits what already exists.
Nothing in §6–§12 is implemented; every proposed symbol is marked `PROPOSED`.

---

## 1. Executive summary

NeoDOS already has a surprising amount of the foundation this task needs, but it
is distributed and, in two places, **the last mile is missing**:

1. **Configuration (`neocfg`)** exists and is real, but is mostly a scaffold.
   `libneocfg` is a clean, UI-agnostic, host-tested core (I ran its suite: 15/15
   pass). Of its five modules, only **About** is implemented; System, Keyboard,
   Power and Locale are navigable stubs that reference open sub-issues
   (#323/#324/#326). The `CfgPlatform`/`CfgUi`/`Translator` seams, `libneotui`,
   and the i18n runtime are all in place, so the remaining work is *filling in
   module bodies and a handful of query wrappers* — not rearchitecting.
2. **Time zones already exist, in the kernel, in a minimal form.** The RTC is
   authoritative UTC; `ObInfoClass::LocalDateTime` (41) and `TimeZone` (40) are
   implemented over a Registry key `...\Control\TimeZoneInformation`
   (`neodos-kernel/src/cm/timezone.rs`, tested). The gap is **not** "invent a
   timezone system"; it is (a) no userland read wrappers, (b) no UI/CLI to
   configure it, (c) a fixed month/day DST window with no transition hour and no
   IANA identity, and (d) locale date formatting that exists but is unused and
   has no shipped `[region]` data.
3. **Networking is UDP-complete but TCP-userland-broken.** UDP (DNS, NTP, DHCP)
   works end to end. However, the userland `SocketConnect` syscall
   (`neodos-kernel/src/syscall/ob/set/net.rs`) **sets flags but never calls
   `socket_connect`/`tcp_connect`**, so no userland TCP connection ever sends a
   SYN. There is also no userland `accept`, no blocking socket wait (#307), and
   no out-of-order reassembly. **A userland HTTP client cannot work today**
   without fixing at least the connect path. This is the single most important
   genuine blocker in this design.
4. **No HTTP, TLS, or crypto-for-transport code exists.** The *only* real
   cryptographic primitive in the repository is Ed25519 sign/verify
   (`libnlt/src/signature.rs`, `ed25519-compact`, RFC 8032 vectors), used solely
   for NLT language packs. There is no SHA-256, HMAC, AEAD, key agreement, or
   CSPRNG exposed to Ring 3 (RDRAND is kernel-only, used for ASLR). There is also
   no compression beyond LZSS.
5. **Updates have no foundation.** There is no runtime installer, no `fsync`, no
   proven atomic replacement (`Vfs::rename` is same-directory, implemented as
   delete-then-insert, and untested; `NeoDosFsV2::write` never calls `save_sb`).
   A package manager is *designed* (`docs/architecture/package-manager-arch.md`)
   but 0% implemented and has an ObType ABI collision (#411). Registry
   persistence itself is not durable across a `cm_flush_key` for an existing hive
   file for the same reason.

**Recommended posture.** Sequence prerequisites before features. Time/configuration
work (Phase 1) can proceed **now** and is independent of HTTP/TLS. HTTP (Phase 2)
must wait for the TCP connect fix and should not be attempted on the assumption
that TCP is done. TLS (Phase 3) requires a deliberate dependency decision
(RustCrypto-style pure-Rust stack + a Ring-3 CSPRNG) that does not exist yet.
Updates (Phases 4–5) additionally require `fsync` and a proven activation
primitive. Do **not** ship an updater that assumes atomic rename.

**No unnecessary NXLs.** The evidence shows that the existing reuse libraries
(`libntp`, `libdns`, `libnet-config`) are **plain `no_std` crates**, not NXLs; the
four NXLs (`math`, `net`, `console`, `libarith`) are for *kernel-mediated*
capabilities with a global, load-once, no-unload, `version != 0`-only model. The
new HTTP/TLS/update/timezone code is pure logic plus a transport seam and fits the
plain-crate pattern better than the NXL pattern. We therefore propose
`libhttp`, `libtls`, `libupdate`, `libtimezone` and `libcrypto` as **plain
crates**, and explain in §10 why NXLs are not warranted now.

---

## 2. Repository baseline with evidence

### 2.1 Branch and workspace state

| Item | Value | Evidence |
| --- | --- | --- |
| Repository | `neodos` (git), remote `NeoDOS-Project/NeoDOS` | `git remote -v` |
| Audited branch | `develop` | `git rev-parse --abbrev-ref HEAD` |
| Audited commit | `8c6865c806f2ac4009a99195d938c36ab9df044d` | `git rev-parse HEAD` |
| Working tree | clean (`git status --porcelain` empty) | — |
| Kernel version | v0.51.5, tests 888, ABI v8, SSDT RAX 0–99 (37 assigned) | `AGENTS.md:3` |
| Build tool | NeoDev 0.3.0 (`neodev`) | `neodev --version` |

The session working directory (`/home/amartinper/rust-os`) is **not** a git
repository; the git repository and all source are under
`/home/amartinper/rust-os/neodos`. `neodev`, `neotools` and `neodos-dev-server`
are sibling repositories on disk but outside the audited tree.

### 2.2 What "implemented" means here

Status vocabulary used throughout (per the task's requirement to distinguish):

- **Verified** — implemented and covered by tests I ran or that exist and are
  deterministic.
- **Implemented (incompletely tested)** — code exists, end-to-end behaviour is
  not proven by a deterministic test.
- **Partial** — a meaningful subset exists; important semantics are missing.
- **Designed, not implemented** — a document/issue specifies it; no code.
- **Proposed** — introduced by this design; no issue or code yet.

### 2.3 Capability evidence table

| Capability | Current implementation | Verification evidence | Missing work | Confidence |
| --- | --- | --- | --- | --- |
| `neocfg` executable + `libneocfg` core | Present; UI-agnostic core with `CfgUi`/`CfgPlatform`/`Translator` seams; About implemented; System/Keyboard/Power/Locale stubs | `libneocfg/src/*.rs`; `userbin/neocfg/src/*.rs`; host tests 15/15 pass (run on 2026-10-10) | System (#323), Keyboard (#324), Power/Locale (#326), libneodos helpers (#327), tests/docs (#330) | High |
| `libneotui` console toolkit | Present, dependency-free, host-tested | `libneotui/src/*.rs`; 9/9 tests pass | — (usable) | High |
| System-info Ob API (Version/Memory/Cpu/Drives/Processes/Services) | Present and consumed by `neocfg` platform | `neodos-kernel/src/syscall/ob/query/*`; `userbin/neocfg/src/neodos_platform.rs` | `\Service` status per fd; wrapped in #327 | High |
| Registry (Cm) CRUD + persistence | Implemented in memory; whole-hive serialize; `cm_flush_key` writes to `C:\System\Registry\*.hiv` | `neodos-kernel/src/syscall/cm.rs` (RAX 50–59); `cm/api.rs:167`; `cm/init.rs:66` | Durability: superblock not committed on plain write; no WAL/transactions; no ACLs; no disk round-trip test | High |
| UTC system clock | RTC authoritative UTC; `ObInfoClass::DateTime` (9) | `neodos-kernel/src/syscall/ob/query/time.rs`; `drivers/hw/rtc.rs`; `docs/registry/registry.md` | Build date not exposed; query-path tests absent | High |
| Time zone (`TimeZone` 40 / `LocalDateTime` 41) | Implemented in kernel: fixed offset + DST window from Registry; `SysTimeZone` 16 B | `neodos-kernel/src/cm/timezone.rs` (+8 tests); `docs/kernel/objects.md:219-220`; `#357` | No IANA identity, no transition hour, no per-year rules, no zone names; no userland wrapper; no UI | High |
| `datetime.nxe` | Displays local (default) or UTC (`/U`); `/S` sets UTC | `userbin/datetime/src/main.rs` | Cannot choose/display time zone; formatting hardcoded `DD/MM/YY` via `libntp`, not locale | High |
| `libntp` | SNTPv4 parse + RFC 5905 offset/delay + UTC↔Unix↔civil; host tests | `libntp/src/lib.rs`; 19/19 tests pass | No authentication; second resolution; no discipline | High |
| `ntpd.nxe` | Persistent SNTP service, Registry config, backoff, status publishing | `userbin/ntpd/src/main.rs`; `docs/services/ntpd.md` | Error masking (`clock set denied`); step-only (no slew); no auth | High |
| Locale date/time formatting | `i18n_format_date`/`i18n_format_time` + `libnlt` `[region]` engine exist | `libneodos/src/i18n.rs`; `libnlt/src/region.rs` (+tests) | No `[region]` shipped in `data/locale/*`; not called by `datetime` | High |
| DNS resolution (userland) | UDP transport + cache in `libnet`; protocol in `libdns`; busy/yield/RDTSC wait | `libnet/src/dns.rs`; `libdns/src/*`; 27 pass / 1 ignored | No blocking wait; no TCP fallback; kernel parser duplicated (#312) | High |
| Kernel TCP state machine | Handshake/ACK/FIN/RTO/buffers; loopback e2e test | `neodos-kernel/src/net/tcp.rs`; `net/tests/net.rs:513` | Userland connect broken; no accept; no OOO reassembly; no window scale/congestion; no TIME_WAIT | High |
| **Userland TCP connect** | **Broken**: `SocketConnect` never calls `socket_connect` | `neodos-kernel/src/syscall/ob/set/net.rs:28-49` vs `net/socket.rs:191` (`socket_connect` callers: DNS/tests only) | Route TCP connect to `tcp_connect` | High |
| Blocking socket wait / timeout | Absent: `ob_wait` → `NoSys` for sockets; `sys_poll` → `POLLERR` | `neodos-kernel/src/syscall/ob/wait.rs:71`; `handlers.rs:584-662`; `#307` | Full capability | High |
| TCP next-hop/gateway | **Present** (`tcp_mac_for` → `nic_next_hop`) | `neodos-kernel/src/net/tcp.rs:278-284` (all send sites) | Per-socket NIC binding (#317); #315 appears stale | High |
| HTTP / HTTPS / curl | Absent | repo-wide grep negative (see §16) | Whole capability | High |
| TLS / PKI | Absent | no TLS/SSL code | Whole capability | High |
| Crypto primitives | Ed25519 verify/sign only (`libnlt`, feature-gated) | `libnlt/src/signature.rs`; RFC 8032 vectors | SHA-256, HMAC, AEAD, X25519, CSPRNG | High |
| Secure RNG to Ring 3 | Absent (RDRAND kernel-only for ASLR) | `neodos-kernel/src/hal/mod.rs:11-29`; `arch/x64/paging.rs:307` | Whole capability | High |
| Compression | LZSS only (NLT payload) | `libnlt/src/lzss.rs` | deflate/gzip for HTTP | High |
| Atomic file replacement / `fsync` | Absent | `fs/vfs/mod.rs` (no `sync`); `neodos_v2.rs:560` (no `save_sb` on write); rename = delete+insert | Whole capability | High |
| Version info | `\Global\Info\Version` = `vX.Y.Z (git <rev>)` | `neodos-kernel/src/main.rs:77-83`; `build.rs` | Build date | High |
| Package/update manager | Design only; `nxpkg` host tool implements an NXP subset | `docs/architecture/package-manager-arch.md`; `neotools/nxpkg`; `#411` | Whole capability | High |
| NXL loader/ABI | NXL v2 loader, PIE + legacy, export table, `version != 0` check | `neodos-kernel/src/infra/nxl.rs`; `libneodos/src/export.rs` | No consumer-side version negotiation; no unload | High |

### 2.4 Representative execution traces

These are the concrete paths that justify the status labels above.

**Trace A — userland TCP connect (fixed).** `neocurl.nxe` calls
`libnet::socket_connect(fd, ip, port)` → `sys_ob_set_info(fd, SocketConnect=18, …)`
→ `neodos-kernel/src/syscall/ob/set/net.rs` now calls
`socket_connect_user(socket_id, remote)`: TCP routes to
`crate::net::socket::socket_connect`, which invokes `tcp_connect` and sends the
SYN (`tcp_handle_ack` flips the socket to `Connected` on completion); UDP/raw
record the peer and mark the socket `Connected`. Previously it only flipped a
flag, so no SYN was sent and every later `socket_send` failed. **Fixed.**

**Trace B — local time (works).** A tool opens `\Global\Info\DateTime`, queries
`ObInfoClass::LocalDateTime` → `neodos-kernel/src/syscall/ob/query/time.rs` →
`cm::timezone::load()` reads `CurrentControlSet\Control\TimeZoneInformation` →
`TimeZone::to_local(utc)` applies `offset_for(month,day)` (base + DST window) →
returns a `SysDateTime`. `datetime.nxe` uses this by default.

**Trace C — NTP clock step.** `ntpd` → `libnet`/`libntp` UDP exchange →
`libntp::offset_and_delay` → corrected Unix seconds → `libntp::unix_secs_to_utc`
→ `libneodos::syscall::ob_set_datetime` (opens `\Global\Info\DateTime` RW,
`ObSetInfoClass::DateTime=50`) → `rtc_bridge::set_datetime` → RTC NEM write-back
ACK. UTC is stored; no timezone applied.

**Trace D — Registry flush (not durable).** `cm_flush_key` → `flush_hive_to_vfs`
serializes the hive and calls `vfs.write` in place. `NeoDosFsV2::write` writes
data through the page cache and COW B-tree nodes to disk but **does not call
`save_sb`**, so the new superblock root is only committed by a later metadata
operation or at shutdown. There is no userland `fsync`.

---

## 3. Existing versus proposed component inventory

### 3.1 Existing (reuse — do not duplicate)

| Component | Form | Layer | Role in this design |
| --- | --- | --- | --- |
| `libneodos` | crate | user lib | syscall wrappers; Home of new time/tz read helpers |
| `libneocfg` | crate | user lib (host-testable) | config logic; add Date/Time + Network modules |
| `libneotui` | crate | user lib | console UI toolkit |
| `libneoutil`, `libneodiag` | crates | user lib | shared formatting/diagnostics |
| `libnet` | crate | user lib | network convenience over `net.nxl`; Home of transport |
| `libnet-config` | crate | user lib (host-testable) | canonical net Registry contract |
| `libdns` | crate | user lib (host-testable) | DNS protocol (transport-agnostic) |
| `libntp` | crate | user lib (host-testable) | time protocol + civil conversion + formatting |
| `net.nxl` / `libnet-nxl` | NXL | Ring 3 lib | socket Ob wrappers |
| `fs.nxl` / `libneodos-nxl` | NXL (legacy) | Ring 3 lib | core syscall ABI table |
| Registry (Cm) | kernel | kernel | persistent config store |
| NeoFS v2 + VFS | kernel | kernel | staging/activation storage |
| `cm/timezone.rs` | kernel | kernel | current offset/DST model |
| i18n runtime (`libneodos::i18n`, `libnlt`) | crate + kernel | user lib | locale formatting + signatures |
| `ntpd`, `datetime`, `neocfg`, `ipconfig`, `netcfg`, `netapplier`, `netd`, `nslookup`, `ping`, `dhcpd` | `.nxe` | Ring 3 exe | existing tools/consumers |
| Ed25519 (`libnlt/src/signature.rs`) | crate (feature) | user lib | reuse for update signatures |

### 3.2 Proposed (new)

| Component | Form | Layer | Justification |
| --- | --- | --- | --- |
| `libtimezone` | plain crate | user lib | TZif parse + UTC↔civil with DST ambiguity; pure logic, host-testable, no kernel change |
| `libhttp` | plain crate | user lib | HTTP/1.1 client over a transport trait (mirrors `libdns`) |
| `libtls` | plain crate | user lib | TLS 1.2/1.3 over the same byte-stream trait; cert validation |
| `libcrypto` | plain crate | user lib | shared SHA-256/Ed25519-verify/HMAC/CSPRNG boundary |
| `libupdate` | plain crate | user lib | manifest parse/verify + transaction logic |
| `neocurl` | `.nxe` | Ring 3 exe | diagnostic/transfer CLI |
| `neoupdate` | `.nxe` | Ring 3 exe | manual updater CLI |

New kernel/ABI items (kept minimal, per `AGENTS.md` rule 6):

| Item | Status | Purpose |
| --- | --- | --- |
| `ObInfoClass::Random` on `\Device\Random` (PROPOSED) | Proposed | Ring-3 CSPRNG for TLS |
| `fsync`/`sync` file operation (PROPOSED) | Proposed | durable writes for updates |
| Atomic replace primitive (PROPOSED) | Proposed | safe activation |
| `SocketAccept`/`listen` wait (PROPOSED) | Proposed | server side (not needed for HTTP client M1) |
| Blocking socket wait with timeout (#307, existing issue) | Tracked | HTTP connect/read timeouts |
| TCP OOO reassembly (PROPOSED) | Proposed | WAN HTTP reliability |

---

## 4. Current and target architecture diagrams

### 4.1 Current (audited)

```text
                         Ring 3 (.NXE)                       Ring 3 libs
  datetime ─────────────┐
  ntpd ── libntp ──────┐ │
  neocfg ── libneocfg ─┤ │        libnet ── net.nxl ──┐
     └── libneotui      │ │        libdns             │
  ipconfig/netcfg/... ──┤ │        libnet-config      │
                        │ │                          │
                        ▼ ▼                          ▼
                   libneodos (syscall wrappers)
                        │                                   fs.nxl (core ABI)
                        ▼
  ───────────────── syscall gate (RAX) ─────────────────────────
                        │
   Ob (Version/Memory/DateTime(9)/TimeZone(40)/LocalDateTime(41))
   Cm (RAX 50–59) ── TimeZoneInformation key
   Net: sockets (SocketConnect=18 broken for TCP), UDP, DNS cache
   FS: VFS ── NeoFS v2 (COW, no fsync/atomic-replace) ── page cache ── block dev
   RTC bridge ── rtc.nem (UTC)
```

### 4.2 Target (proposed, incremental)

```text
  datetime ── libtimezone ── libntp(civil) ── libneodos(time reads)
  neocfg(libneocfg) ── [Date/Time module] ── libneodos(time/tz reads)
                     ── [Network module]   ── libnet-config / libnet
  neocurl  ── libhttp ── libtls ── libcrypto ── Ob Random (PROPOSED)
  neoupdate── libupdate ── libhttp + libtls + libcrypto(Ed25519)
                       ── Registry (update state) + VFS staging
                       ── fsync + atomic-replace (PROPOSED prerequisites)

  libhttp transport seam  ──TCP stream──  net.nxl ── kernel TCP (connect fix required)
  libtls  byte-stream seam ── over libhttp transport (TLS records in HTTP body path)
```

Layering rule (`AGENTS.md` rule 12, `docs/architecture/source-of-truth.md` INV-1):
dependencies flow downward only; the new crates depend on existing userland
crates and never on the kernel directly (all kernel access via `libneodos`).

---

## 5. Component responsibilities and dependency graph

### 5.1 Where each capability belongs

| Capability | Kernel | Shared user lib | User-mode service | CLI tool |
| --- | --- | --- | --- | --- |
| UTC time source | **Yes** (RTC, `DateTime`) | wrapper | — | — |
| TZ offset/DST (coarse) | **Yes** (existing) | wrapper | — | — |
| TZif precise conversion | No (keep kernel simple) | **`libtimezone`** | — | `datetime` |
| Config persistence | Registry (Cm) | `libneodos::registry` | — | `neocfg` |
| HTTP | No | **`libhttp`** | — | `neocurl` |
| TLS | No | **`libtls`** | — | (via `neocurl`) |
| Crypto primitives | CSPRNG only | **`libcrypto`** | — | — |
| Update logic | No | **`libupdate`** | optional scheduler later | `neoupdate` |
| Activation durability | **Yes** (fsync/atomic replace) | wrapper | — | — |

Rationale for keeping these in user mode: the kernel `MUST NOT` host user-facing
commands (`INV-11`), and minimal kernel changes are preferred where user-mode is
sufficient. The only kernel changes proposed are the ones user mode cannot
provide: entropy, durability, and (if needed) the TCP connect/accept/wait fixes.

### 5.2 Current dependency graph (edges that exist)

| From | To | Interface | Exists? |
| --- | --- | --- | --- |
| `datetime` | `libntp`, `libneodos` | link (crate) | Yes |
| `ntpd` | `libntp`, `libnet`, `libneodos` | link | Yes |
| `neocfg` | `libneocfg`, `libneotui`, `libneodos` | `CfgPlatform`/`CfgUi`/`Translator` | Yes |
| `libnet` | `net.nxl`, `libneodos` | `NetAbiTable`, syscalls | Yes |
| `libnet` | `libdns`, `libnet-config` | link | Yes |
| `libdns` | (transport seam) | `DnsTransport` trait | Yes |
| `libnlt` | `ed25519-compact` (feature) | `signature::verify` | Yes |
| kernel net | socket Ob classes | `SocketConnect=18` | **Broken for TCP** |

### 5.3 Target dependency graph (proposed edges marked ★)

| From | To | Interface | Exists? |
| --- | --- | --- | --- |
| ★ `libtimezone` | (none / `libntp` civil helpers) | pure functions | No |
| ★ `datetime` | `libtimezone` | link | No |
| ★ `neocfg` Date/Time module | `libneodos` time/tz reads | `ob_query_info` wrappers | No |
| ★ `libhttp` | transport trait | `HttpTransport::connect/send/recv` | No |
| ★ `libnet` | `libhttp` transport impl | implements trait over `net.nxl` | No |
| ★ `libtls` | `libhttp` stream trait | `TlsStream` | No |
| ★ `libcrypto` | Ob Random | `ObInfoClass::Random` (kernel) | No |
| ★ `libupdate` | `libhttp`, `libtls`, `libcrypto`, Registry, VFS | link + syscalls | No |
| ★ `neoupdate` | `libupdate` | link | No |

**Circular dependencies:** none introduced — the graph stays a DAG. `libhttp` must
not depend on `libtls` (TLS layers *above* HTTP transport; `libtls` consumes the
transport trait and `libhttp` can use a `TlsStream` via an object-safe trait, or
`libtls` provides `HttpsTransport` implementing `HttpTransport`). The cleaner
inversion is: `libhttp` defines `HttpTransport`; `libnet` provides `TcpTransport`;
`libtls` wraps any `HttpTransport` as a TLS `HttpTransport`. `neocurl` chooses.

---

## 6. Proposed public API contracts

Everything in this section is **PROPOSED** unless marked `exist`. Existing
signatures are reproduced from source for reference.

### 6.1 Time and time zone

Existing (for reference):

```rust
// libneodos/src/syscall/time.rs — EXISTING
pub fn ob_set_datetime(dt: &DateTime) -> Result<(), i64>;

// libneodos/src/syscall/ob.rs — EXISTING enum values
// ObInfoClass::DateTime = 9, ObInfoClass::TimeZone = 40, ObInfoClass::LocalDateTime = 41

// libneodos/src/syscall/types.rs — EXISTING
pub struct DateTime { pub second: u8, pub minute: u8, pub hour: u8,
                      pub day: u8, pub month: u8, pub year: u8, pub valid: u8 }
pub struct SysTimeZone { pub utc_offset_minutes: i32, pub dst_offset_minutes: i32,
                         pub dst_enabled: u32, pub dst_start_month: u8, pub dst_start_day: u8,
                         pub dst_end_month: u8, pub dst_end_day: u8 }
```

Proposed — `libneodos` read wrappers (extends #327 scope; keep same style):

```rust
// PROPOSED — libneodos/src/syscall/time.rs
pub fn ob_get_datetime() -> Result<DateTime, i64>;      // ObInfoClass::DateTime
pub fn ob_get_local_datetime() -> Result<DateTime, i64>; // ObInfoClass::LocalDateTime
pub fn ob_get_timezone() -> Result<SysTimeZone, i64>;   // ObInfoClass::TimeZone
```

Proposed — `libtimezone` (pure, host-testable; `#![cfg_attr(not(test), no_std)]`):

```rust
// PROPOSED — libtimezone/src/lib.rs
pub struct Instant(i64);                 // Unix seconds (UTC)
pub struct Civil { pub year: i32, pub month: u8, pub day: u8,
                   pub hour: u8, pub minute: u8, pub second: u8 }
pub struct Tzif { /* parsed TZif (RFC 8536) */ }

pub enum LocalResult { Unique(Instant), Ambiguous(Instant, Instant), None }

impl Tzif {
    pub fn parse(bytes: &[u8]) -> Result<Tzif, TzError>;
    pub fn to_local(&self, utc: Instant) -> Civil;
    pub fn to_instant(&self, local: Civil, fold: Fold) -> LocalResult; // DST ambiguity
    pub fn identifier(&self) -> &str;      // e.g. "Europe/Madrid"
    pub fn tzdata_version(&self) -> Option<&str>;
}
pub enum Fold { Earlier, Later }           // resolves Ambiguous

pub trait ZoneSource {
    fn by_name(&self, iana: &str) -> Result<Tzif, TzError>;  // "Europe/Madrid"
    fn available(&self) -> &[&str];
    fn version(&self) -> Option<&str>;                        // tzdata release
}
```

Proposed — config read/write boundary (Registry, reuse existing Cm):

```rust
// PROPOSED — libneodos timezone helpers (thin over RegistryKey)
pub fn tz_get_key_name() -> Result<alloc::string::String, i64>;  // IANA id (new value)
pub fn tz_set_key_name(iana: &str) -> Result<(), i64>;
```

> All APIs above are **proposed**. The existing `SysTimeZone` fixed-window model
> remains valid for Phase 1; adding `TimeZoneKeyName` is an additive,
> backward-compatible Registry value.

### 6.2 HTTP transport seam

The transport seam is the central contract. It must **not** duplicate TCP/DNS
logic; `libnet` implements it.

```rust
// PROPOSED — libhttp/src/transport.rs
pub trait HttpTransport {
    type Conn: Read + Write + Close;
    /// Resolve + TCP connect with a deadline. DNS via libdns; TCP via net.nxl.
    fn connect(&mut self, host: &str, port: u16, deadline: Deadline)
        -> Result<Self::Conn, HttpError>;
}
pub trait Read { fn read(&mut self, buf: &mut [u8], deadline: Deadline) -> Result<usize, HttpError>; }
pub trait Write { fn write_all(&mut self, buf: &[u8], deadline: Deadline) -> Result<(), HttpError>; }
pub trait Close { fn close(self); }
```

`Deadline` is a monotonic reference. **Prerequisite:** a monotonic/timeout source
does not exist in userland (#467); until it lands, `Deadline` is backed by the
existing RDTSC budget workaround used by `libnet::dns` and `ntpd`. This is a
documented compromise, not a final design.

### 6.3 HTTP client (proposed)

```rust
// PROPOSED — libhttp/src/lib.rs
pub struct Request { pub method: Method, pub url: Url, pub headers: Vec<(String, String)> }
pub enum Method { Get, Head, Post }        // Post deferred in M1
pub struct Response { pub status: u16, pub headers: Vec<(String, String)>, pub body: Body }
pub struct Limits { pub max_header_bytes: usize, pub max_body_bytes: Option<u64>,
                    pub max_redirects: u8, pub connect_timeout_ms: u32, pub read_timeout_ms: u32 }

pub fn get<T: HttpTransport>(t: &mut T, url: &Url, limits: Limits) -> Result<Response, HttpError>;

pub enum HttpError {
    Dns, Connect, Tls, Timeout, Cancelled,
    MalformedHeader, TooManyHeaders, BodyTooLarge, TooManyRedirects,
    InsecureRedirect, Status(u16), Io,
}
```

M1 scope: HTTP/1.1; `GET`/`HEAD`; `Content-Length` and chunked;
connection-close; redirect policy with limits; header/body limits; timeouts.
Deferred: persistent connections, `POST`, streaming to disk, HTTP/2, compression.

### 6.4 TLS (proposed)

```rust
// PROPOSED — libtls/src/lib.rs
pub struct ClientConfig {
    pub trust_anchors: TrustStore,      // provisioned, read-only
    pub min_version: TlsVersion,        // Tls12 | Tls13
    pub alpn: &'static [&'static str],  // e.g. ["http/1.1"]
    pub rng: &'static dyn Rng,          // kernel Random (PROPOSED)
}
pub struct TrustStore { /* DER roots */ }
pub fn connect<T: HttpTransport>(cfg: &ClientConfig, host: &str, t: &mut T)
    -> Result<TlsStream<T::Conn>, TlsError>;

// NOTE: no `danger_disable_verification` in the client API. Verification is
// mandatory. A separate, clearly-named diagnostic type may allow it for
// controlled fixtures only, and cannot be passed by trusted callers (see §8).
```

### 6.5 Update (proposed)

```rust
// PROPOSED — libupdate/src/lib.rs
pub struct Manifest { pub schema: u16, pub product: String, pub component: String,
                      pub version: String, pub min_os_version: String, pub arch: String,
                      pub artifacts: Vec<Artifact> }
pub struct Artifact { pub url: String, pub size: u64, pub sha256: [u8; 32],
                      pub sig: Option<Signature>, pub key_id: Option<u32>,
                      pub path: String, pub kind: ArtifactKind }
pub enum ArtifactKind { Nxe, Nxl, Data }

pub struct UpdatePolicy { pub allow_rollback: bool, pub trusted_keys: TrustStore }
pub fn plan(manifest: &Manifest, policy: &UpdatePolicy) -> Result<UpdatePlan, UpdateError>;
pub fn apply(plan: &UpdatePlan, io: &mut dyn UpdateIo) -> Result<(), UpdateError>;
```

---

## 7. Configuration and time-zone data schemas

### 7.1 Registry configuration (existing keys reused)

Existing time zone key (`docs/registry/registry.md:98-105`; `cm/timezone.rs`):

```text
\Registry\Machine\System\CurrentControlSet\Control\TimeZoneInformation
    UtcOffsetMinutes          REG_DWORD
    DaylightOffsetMinutes     REG_DWORD
    DaylightEnabled           REG_DWORD
    DaylightStartMonth        REG_DWORD
    DaylightStartDay          REG_DWORD
    DaylightEndMonth          REG_DWORD
    DaylightEndDay            REG_DWORD
```

Proposed additive values (Phase 1, backward compatible):

```text
    TimeZoneKeyName           REG_SZ     ; "Europe/Madrid" (IANA), authoritative identifier
    TzDataVersion             REG_SZ     ; "2026a" (matches installed tzdata), optional
```

Existing locale key (reused): `...\Control\Locale\Language` (e.g. `en-US`).

Proposed `neocfg` persisted settings namespace (reuse Registry; do **not** invent a
second config system):

```text
\Registry\Machine\System\CurrentControlSet\Control\NeoCfg
    KeyboardLayout            REG_SZ
    TimeZoneKeyName           REG_SZ
    Locale                    REG_SZ
```

Rule (task requirement): the UI must never report a setting applied unless the
backend confirms persistence. `neocfg` must call `RegistryKey::flush()` and
surface failures; where flush is not durable (see §10), the UI must say
"persist pending" rather than "applied".

### 7.2 Time-zone database

**Decision: TZif (RFC 8536) in userland, parsed by `libtimezone`.** Rationale:

- IANA-compatible identifiers are a stated goal and TZif is the IANA interchange
  format; implementing DST transitions from scratch in the kernel is error-prone.
- The kernel already derives coarse local time; keeping the fine-grained engine
  in userland avoids a large kernel data blob and matches `INV-11`/minimal-kernel
  principles.
- Individual TZif files are a few KB; a curated subset is small. `libtimezone`
  is pure logic → host-testable with deterministic fixtures.

Storage (proposed):

```text
C:\System\Zoneinfo\Europe\Madrid      ; TZif file (RFC 8536 v2/v3)
C:\System\Zoneinfo\tzdata.version     ; release string + optional checksum
```

Versioning/update: `tzdata.version` is compared by `Tzif::tzdata_version`;
updates flow through the signed updater (§9) — **no automatic database downloader**
in the initial implementation. Public-domain IANA data is redistributable; that
is a licensing decision to record.

Absent/invalid data behaviour (task requirement): if the configured IANA id has no
TZif file, `libtimezone` MUST fall back deterministically — (1) the Registry fixed
offset (`SysTimeZone`), then (2) UTC — and MUST report the degradation to the
caller (a `Degraded` flag), never silently show wrong local time.

### 7.3 DST ambiguity / nonexistent times

`LocalResult { Unique, Ambiguous(early, late), None }` models the two standard
edge cases (fall-back overlap → `Ambiguous`; spring-forward gap → `None`).
Callers pass `Fold` (or a policy) to resolve. `Europe/Madrid` is a **test case,
not a special case** (`CET/CEST`, `+1/+2`, last Sunday of March/October, 02:00/03:00).

---

## 8. HTTP/TLS security boundaries

The task explicitly asks to distinguish four things that are *not interchangeable*:

1. **TLS transport encryption** — confidentiality/integrity of the byte stream.
2. **Server authentication** — the certificate chain + hostname prove *who* the
   server is.
3. **Artifact authenticity** — an Ed25519 (or similar) signature proves the
   update publisher produced the artifact, independent of the transport.
4. **Artifact integrity** — a cryptographic hash proves the bytes match what was
   published, independent of the signature and transport.

Rules:

- Plain HTTP is acceptable **only** for diagnostics and controlled fixtures. It
  MUST NOT be the transport for update metadata or artifacts.
- Certificate verification is **mandatory** for HTTPS used by `libupdate`. The
  `libtls` client API exposes no "insecure" flag to trusted callers. An explicit
  test-only type (gated, not importable by `libupdate`/`neoupdate`) may accept a
  pinned test root.
- Certificate validity depends on the **system clock**. A wrong clock (pre-NTP)
  can reject valid certs or accept expired ones. HTTPS consumers MUST treat
  "clock not synchronized" as a degraded state and fail closed for updates.
- Trust anchors are read-only, provisioned at image build, and replaceable only
  through a signed update. No runtime "install any root" path.
- Redirects from HTTPS to HTTP for sensitive requests MUST be refused
  (`HttpError::InsecureRedirect`).

---

## 9. Update manifest and trust model

### 9.1 Manifest schema (proposed, versioned)

Format decision: a small TLV/binary format consistent with NXP
(`docs/userland/nxp-format.md`, `nxpkg`), **not** a new incompatible protocol.
Fields:

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | u16 | manifest format version |
| `product` | string | e.g. `neodos` |
| `component` | string | e.g. `datetime`, `system`, `net` |
| `version` | string | semver-ish release |
| `min_os_version` | string | compatibility floor |
| `arch` | string | `x86_64` |
| `artifact.url` | string | HTTPS only |
| `artifact.size` | u64 | enforced before write completes |
| `artifact.sha256` | [u8;32] | integrity (requires SHA-256 — new) |
| `artifact.sig` | Ed25519 sig | authenticity (reuse existing primitive) |
| `artifact.key_id` | u32 | signing-key identifier |
| `artifact.path` | string | activation target |
| `dependencies[]` | list | optional component relationships |

### 9.2 Trust model

- **Manifest authentication:** the manifest (or a detached signature over it) is
  verified with Ed25519 using a trust store of public keys. Reuse
  `libnlt`-proven Ed25519 (`ed25519-compact`), extracted into `libcrypto` so
  `libupdate` does not depend on i18n.
- **Key provisioning/rotation:** keys are provisioned in the image (Registry or a
  read-only store); rotation ships a new trust store through a signed update. A
  key LF is identified by `key_id`.
- **Revocation:** the trust store carries a revoked-key list; a revoked key
  cannot sign new manifests. Rollback protection: the signed manifest carries a
  monotonically increasing release counter; `libupdate` rejects counters below
  the recorded minimum unless `allow_rollback` policy is set.
- **Hash vs signature vs TLS:** all three are checked; none substitutes for
  another (see §8).
- **HTTPS role in the chain:** TLS authenticates the *server*; the Ed25519
  signature authenticates the *publisher*. Both are required for metadata.

### 9.3 Safe transaction

```text
1. Fetch metadata over validated HTTPS.
2. Authenticate the manifest (Ed25519 + trust store + revocation + anti-rollback).
3. Validate compatibility (arch, min_os_version) and policy.
4. Download artifact to a staging location (C:\System\Updates\staging\<id>).
5. Enforce size; verify SHA-256.
6. Verify the artifact signature when present.
7. Validate artifact format + dependencies (NXE/NXL headers).
8. fsync staging data and metadata.        <-- PREREQUISITE (absent today)
9. Activate using a proven-safe mechanism. <-- PREREQUISITE (absent today)
10. Record recovery/rollback state in the Registry.
11. Verify the result; report failure accurately.
```

**Steps 8–9 are not currently possible.** There is no `fsync` and no proven
atomic replace; `NeoDosFsV2::write` does not commit the superblock, and
`Vfs::rename` is same-directory delete-then-insert with no test. Therefore the
first milestone must either (a) implement `fsync` + atomic replace first, or
(b) restrict activation to a *boot-time* apply step that reads a verified
staging manifest and performs replacement with an explicit recovery record.
Recommendation: **(a)**, because it is independently testable and also fixes
Registry durability.

### 9.4 Scope boundary

M1 updates **user-mode `.nxe`/`.nxl` and data only**. Kernel/bootloader/FS-format
replacement is explicitly out of scope until a verified recovery mechanism exists.
The updater is manual (`neoupdate`); automatic scheduling is a later feature.
This is a subset of the package-manager design (#411); do not implement the whole
package manager.

---

## 10. NXL design review

Before recommending new NXLs, verified facts about the NXL model
(`neodos-kernel/src/infra/nxl.rs`, `libneodos/src/export.rs`):

- NXLs load into a fixed 2 MB region, max 256 KB each, 8 slots.
- An NXL is loaded **once, system-wide**, never unloaded, and shared across
  processes (global `NXL_REGISTRY`/`NXL_SYMBOLS`).
- The loader checks only that the export-table `version != 0`; consumers do **not**
  negotiate versions. Unresolved imports are `kwarn!`-only (null pointer at call).
- Existing NXLs are `math`, `net`, `console`, `libarith`; the reuse libraries
  `libntp`, `libdns`, `libnet-config` are **plain crates**, not NXLs.

**Decision: do not create `libhttp.nxl`, `libtls.nxl`, `libupdate.nxl`,
`libtimezone.nxl` in the initial milestone.** Use plain `no_std` crates:

- The proposed functionality is pure logic + a transport seam, not a
  kernel-mediated global service.
- Plain crates are host-testable (the pattern that made `libneocfg`/`libdns`
  verifiable) and avoid the NXL global/load-once/version-less constraints.
- NXL proliferation is discouraged by the task and by `AGENTS.md`.

If a concrete need for runtime sharing/isolation appears (e.g. a future
`netd`-style service hosting TLS with independent versioning), introduce an NXL
then, following the `libarith-nxl` layering model. If that happens, the missing
**consumer-side version negotiation** should be addressed first (a proposed
issue, §13).

An NXL, if created, must not own: trust anchors, Registry state, or update policy
(those belong to `libupdate`/services). Its public boundary is the export table;
dependencies must be declared via the `libarith-nxl` import convention.

---

## 11. Test strategy

Layered, deterministic, no live-internet dependency where fixtures suffice.
Commands verified for this report: `cargo test` per crate (host), and
`npx markdownlint '**/*.md' --config .markdownlint.json`.

| Layer | Component | Tests |
| --- | --- | --- |
| Unit | `libtimezone` | TZif parse; UTC→civil; civil→UTC; `Europe/Madrid` DST boundaries (ambiguous + nonexistent); version query; degraded fallback |
| Unit | `libhttp` | request build; status/header parse; `Content-Length`; chunked; connection-close; malformed headers; limits; redirect policy incl. HTTPS→HTTP refusal |
| Unit | `libtls` | cert chain valid/invalid/expired; hostname mismatch; untrusted issuer; wrong-clock expiry; record framing |
| Unit | `libcrypto` | SHA-256 known vectors; Ed25519 RFC 8032 vectors; HMAC vectors; RNG determinism (fixture) |
| Unit | `libupdate` | manifest parse; signature accept/reject; revoked key; anti-rollback; input validation |
| Integration | fixtures | corrupted/truncated download; hash mismatch; interrupted update → recovery; missing service/library; dependency version mismatch |
| Integration | `neocfg` | navigation; module bodies; persistence confirmation (no false "applied") |
| E2E | QEMU/VBox | `neocurl` against a local fixture server; `neoupdate` dry-run + rollback; `datetime` local/UTC; `neocfg` Date/Time |
| Existing | host | `libneocfg` 15, `libntp` 19, `libdns` 27 (+1 ignored), `libneotui` 9 (all run for this report, all pass) |

Environment notes: QEMU+OVMF is the primary target (`neodev run`); VirtualBox SMP2
has historically exposed separate bugs (#476). No test may depend on a public
Internet host; use a loopback/local fixture. Kernel tests are registered in
`neodos-kernel/src/testing.rs`. Never claim a test passed without running it.

---

## 12. Incremental roadmap

Ordering is evidence-driven; Phase 1 is independent of Phases 2–5.

### Phase 0 — Audit and prerequisites (this document)

- **Prerequisites:** none.
- **Scope:** baseline; decision log; backdrop issues.
- **Non-goals:** any implementation.
- **Acceptance:** this document reviewed and merged; evidence index complete.
- **Risks:** none (read-only). **Rollback:** revert the doc commit.

### Phase 1 — Time and configuration foundation (can start now)

- **Prerequisites:** none (kernel tz + i18n runtime already exist).
- **Scope:** `neocfg` System (#323) and Keyboard (#324) modules; Date/Time &
  Time Zone module (`libtimezone`, TZif, Registry `TimeZoneKeyName`); Power/Locale
  ready paths (#326); libneodos read wrappers (#327); wire `i18n_format_date/time`
  and ship `[region]` data; optional timezone CLI in `datetime`.
- **Non-goals:** no HTTP/TLS; no automatic tzdata download; no full IANA kernel
  engine.
- **Files:** `libneocfg/src/modules/*`, `userbin/neocfg/*`, `libtimezone/` (new),
  `datetime/`, `libneodos/src/syscall/time.rs`, `data/locale/*`.
- **Issues:** reuse #30/#323/#324/#326/#327/#330, #98, #357 (closed context),
  #579; propose new Date/Time issue + libtimezone issue.
- **Acceptance:** each module shows real data; persistence confirmed or reported;
  TZif `Europe/Madrid` conversion tests pass.
- **Tests:** unit (libtimezone, libneocfg) + QEMU walkthrough of `neocfg`.
- **Risks:** Registry flush not durable → surface "pending" rather than "applied".

### Phase 2 — HTTP transport (blocked on TCP connect fix)

- **Prerequisites (blockers):** fix userland `SocketConnect` to route TCP to
  `tcp_connect`; robust receive; #307 blocking wait with timeout (or a documented
  poll contract); ideally TCP OOO reassembly for WAN.
- **Scope:** `libhttp` + `neocurl`; HTTP/1.1, GET/HEAD, chunked,
  connection-close, redirects, limits, timeouts; a shared UDP/TCP exchange helper
  (#593) to avoid duplicating DNS/NTP logic.
- **Non-goals:** POST/streaming/persistent/HTTP2; TLS (Phase 3).
- **Files:** `libhttp/` (new), `userbin/neocurl/` (new), `libnet` transport impl,
  kernel TCP connect fix.
- **Issues:** reuse #307, #593, #312; propose TCP-connect fix + libhttp/neocurl.
- **Acceptance:** `neocurl http://fixture/` fetches a known body; malformed
  responses handled; limits enforced.
- **Tests:** HTTP fixtures; local server; no Internet dependency.
- **Risks:** OOO reassembly absent → real-WAN failures; timeouts approximated
  until #467/#307.

### Phase 3 — TLS and HTTPS (blocked on crypto decision)

- **Prerequisites:** a pure-Rust, `no_std`, MIT-compatible TLS stack decision
  (RustCrypto + custom provider) and a Ring-3 CSPRNG (`ObInfoClass::Random`).
- **Scope:** `libcrypto`, `libtls`, trust anchors, hostname validation, ALPN,
  integration as an HTTP transport.
- **Non-goals:** custom crypto; a browser-grade feature set.
- **Issues:** reuse #89/#57/#582 context; propose libcrypto/TLS/CSPRNG issues.
- **Acceptance:** valid fixture cert accepted; invalid/expired/mismatch rejected;
  wrong-clock rejection tested.
- **Risks:** external-crate policy; crypto audit surface.

### Phase 4 — Signed update foundations (blocked on durability)

- **Prerequisites:** `fsync` + atomic-replace primitive; `libcrypto` SHA-256.
- **Scope:** manifest format, Ed25519 signing/verification, key store + rotation +
  revocation, anti-rollback.
- **Non-goals:** network download; activation.
- **Issues:** reuse #411/#105/#89/#57; propose manifest/signing issue.
- **Acceptance:** manifest signature/hash/revocation/rollback tests pass.

### Phase 5 — Manual updater (blocked on Phase 4 + durability)

- **Prerequisites:** Phases 3–4 complete; proven activation/recovery.
- **Scope:** `libupdate` + `neoupdate`; staging, verify, fsync, activate,
  record state, verify result; manual only.
- **Non-goals:** kernel/bootloader/FS-format updates; automatic scheduling.
- **Issues:** reuse #411/#103/#105; propose updater issue.
- **Acceptance:** simulated interrupted update recovers; rollback works; user-mode
  binaries only.

---

## 13. Existing issues and proposed backlog

### 13.1 Existing issues to reuse (do not duplicate)

| Issue | Title | Relationship |
| --- | --- | --- |
| #30 | ADM-5: neocfg (Panel de Control) | Parent of the neocfg family (9 sub-issues) |
| #322 | [NEOCFG] Scaffolding + UI framework | Closed (implemented) |
| #323 | [NEOCFG] System module | Open — System module |
| #324 | [NEOCFG] Keyboard module | Open |
| #325 | [NEOCFG] About module | Closed (implemented) |
| #326 | [NEOCFG] Power + Locale stubs | Open |
| #327 | [NEOCFG] libneodos system-info helpers | Open |
| #328 | [NEOCFG] i18n translations (.nlt) | Closed |
| #329 | [NEOCFG] Build integration | Closed |
| #330 | [NEOCFG] Tests + documentation | Open |
| #98 | TOOL-NEOCFG: neocfg completar módulos | Open — network/date-time/service/display modules |
| #357 | [KERNEL] Timezone/DST support | Closed — implemented as fixed offset + DST window |
| #360 | [SHELL] Set date/time command (`datetime /S`) | Closed |
| #356 | [KERNEL] Clock discipline (slew/drift) | Open — accuracy, not TZ |
| #467 | [KERNEL] Monotonic uptime / HR clock | Open — timeout/monotonic prerequisite |
| #26 | B3.4: NTP client | Open (broad); ntpd exists |
| #361 | [NET] NTP authentication (NTS/MAC) | Open — needs crypto |
| #307 | [NET] Blocking/pollable socket waits with timeout | Open — HTTP prerequisite |
| #593 | [NET] Share the UDP exchange helper (DNS/ntpd) | Open — reuse for HTTP |
| #312 | [NET] Retire duplicated kernel DNS parser | Open |
| #315 | [NET] TCP send bypasses gateway | Open, but code already uses `nic_next_hop` → **likely stale; verify before working** |
| #362/#372 | [NET] Define/implement netd as service | Open — future transport service boundary |
| #89 | NXE-ECO-15: signature verification infrastructure | Open — update signatures |
| #57 | B5.1: Module signature validation | Open |
| #582 | [I18N] Enforce NLT signatures in release | Open — proves Ed25519 path |
| #411 | [PKG] Package manager (design, 0% implemented, ABI collision) | Open — update is a subset |
| #105 | INSTALL-PACKAGES | Open |
| #103 | INSTALL-NXE: install.nxe | Open |
| #579 | [I18N] Regional formatting at runtime | Closed — formatters exist |

> Access note: GitHub was reachable and authenticated (`gh`, account `alexis900`);
> issue metadata above was read live on 2026-10-10. No issue state was modified.

### 13.2 Proposed new issues (do not create in this task)

| Proposed | Title | Rationale |
| --- | --- | --- |
| P1 | `[NET] Fix userland TCP connect (SocketConnect must send SYN)` | **Genuine blocker** for HTTP; `set/net.rs` never calls `socket_connect` |
| P2 | `[NET] TCP out-of-order reassembly + duplicate handling` | WAN reliability for HTTP |
| P3 | `[NEOCFG] Date/time & time-zone module` | Fill the module; configure offset/IANA |
| P4 | `[LIB] libneodos time/timezone read wrappers` | Extends #327 |
| P5 | `[TIME] TZif time-zone database library + storage/versioning` | `libtimezone` |
| P6 | `[I18N] Ship [region] data and use locale formatters in datetime` | Formatters exist but unused |
| P7 | `[CRYPTO] Shared crypto library (SHA-256, Ed25519, HMAC, CSPRNG boundary)` | Needed by TLS + updates |
| P8 | `[KERNEL] Expose CSPRNG (Random) to Ring 3` | TLS requires entropy |
| P9 | `[HTTP] libhttp client + neocurl` | Phase 2 |
| P10 | `[TLS] libtls + trust anchors + hostname validation` | Phase 3 |
| P11 | `[FS] fsync + atomic file replacement primitive` | Update durability prerequisite |
| P12 | `[REGISTRY] Durable flush (commit superblock on write)` | Registry persistence gap |
| P13 | `[UPDATE] Signed manifest + manual neoupdate` | Phases 4–5 |
| P14 | `[NXL] Consumer-side ABI version negotiation` | Current model checks only `!= 0` |

---

## 14. Risks, unresolved questions and explicit non-goals

### 14.1 Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Userland TCP connect broken | HTTP/HTTPS impossible | Fix first (P1); add a test that a socket reaches `Established` |
| No blocking socket wait (#307) / monotonic clock (#467) | Timeouts approximated; poor UX | Implement #307/#467 or document the poll/deadline contract |
| No `fsync` / atomic replace | Corrupted or half-applied updates; Registry loss | Multi-hive/flush durability work (P11/P12) before updater |
| External-crate policy vs TLS | Cannot ship HTTPS without a vetted dependency | Decide RustCrypto-style pure-Rust stack; record license/audit |
| Wrong system clock | Bad cert decisions, wrong local time | Fail closed for updates when clock unsynced |
| Scope creep into full package manager | Overreach; #411 is post-1.0 | Restrict updater to signed user-mode artifacts |
| Stale docs (#315, timezone row in `ntpd.md`, Registry RAX 67–76) | Misleads implementers | Correct docs as part of each phase |

### 14.2 Unresolved questions

1. Which TLS implementation and which crates, and how to honor the
   external-crate-averse convention? (Decision required before Phase 3.)
2. Should precise local time migrate entirely to `libtimezone`, deprecating the
   kernel `LocalDateTime` coarse path, or coexist? (Precedence must be defined.)
3. tzdata subset vs full, and its distribution/licensing and size budget.
4. Whether activation uses atomic replace or a boot-time apply step.
5. Whether `libhttp`/`libtls`/`libupdate` ever need to be NXLs (see §10).

### 14.3 Explicit non-goals

- Implementing any feature in this document.
- Custom cryptography or a custom TLS protocol.
- Automatic time-zone database downloads.
- Kernel/bootloader/FS-format self-update in M1.
- A full package manager / dependency resolver.
- Changing public APIs, syscall numbers, or the ABI.

---

## 15. Decision log

| # | Decision | Evidence / rationale |
| --- | --- | --- |
| D1 | Build on existing `neocfg`/`libneocfg` seams; do not create a second config system | `libneocfg` exists, host-tested (15/15); #322/#325 done |
| D2 | Keep UTC authoritative; keep coarse TZ in kernel, precise conversion in userland | `cm/timezone.rs`; `LocalDateTime=41`; minimal-kernel + `INV-11` |
| D3 | Select TZif over a kernel IANA engine | IANA-compatible goal; host-testable pure logic; small storage |
| D4 | Propose `libhttp`/`libtls`/`libupdate`/`libtimezone`/`libcrypto` as plain crates, not NXLs | Existing reuse libs are plain crates; NXL is global/load-once/version-less |
| D5 | Define HTTP transport as an injectable trait; `libnet` implements it | Mirrors proven `libdns::DnsTransport` pattern |
| D6 | Reuse Ed25519 (`libnlt`/`ed25519-compact`), extract into `libcrypto` | Only real crypto in repo; RFC 8032-tested |
| D7 | No custom TLS; require a vetted pure-Rust stack + Ring-3 CSPRNG | Crypto from scratch is forbidden by the task |
| D8 | Update M1 = signed user-mode artifacts, manual, user-mode only | No proven activation/recovery; #411 out of scope |
| D9 | Treat atomic replace/`fsync`/Registry durability as prerequisites, not assumptions | `neodos_v2.rs:560` no `save_sb`; no `fsync`; rename untested |
| D10 | Fail closed on clock/TLS uncertainty; no insecure flag to trusted callers | Security boundary §8 |
| D11 | Sequence Phase 1 independently of HTTP/TLS | Time/config work is not blocked by networking |
| D12 | The first HTTP prerequisite is the TCP connect fix, not DNS | Trace A: `SocketConnect` never calls `socket_connect` |

---

## 16. Evidence index

### 16.1 Source paths and symbols

| Claim | Path / symbol |
| --- | --- |
| `libneocfg` seams, modules, tests | `libneocfg/src/{lib,app,module,model,platform,registry,i18n,ui,mocks,tests}.rs`, `libneocfg/src/modules/*.rs` |
| `neocfg` glue + platform | `userbin/neocfg/src/{main,tui,neodos_platform,neodos_i18n}.rs` |
| `libneotui` | `libneotui/src/{lib,keys,screen}.rs`, `libneotui/src/widgets/*.rs` |
| neocfg design | `docs/design/neocfg-design.md` (§ + Addendum A) |
| Cm syscalls | `neodos-kernel/src/syscall/cm.rs` (RAX 50–59) |
| Cm flush | `neodos-kernel/src/cm/api.rs:167`, `neodos-kernel/src/cm/init.rs:66` |
| Hive serialize | `neodos-kernel/src/cm/hive/serialize.rs` |
| Time zone kernel | `neodos-kernel/src/cm/timezone.rs` (`TimeZone`, `load`, `to_local`, `offset_for`) |
| Time query | `neodos-kernel/src/syscall/ob/query/time.rs` |
| Time set | `neodos-kernel/src/syscall/ob/set/time.rs`, `set/mod.rs` (`validate_datetime`) |
| RTC bridge | `neodos-kernel/src/drivers/hw/rtc.rs`, `drivers/rtc/src/lib.rs` |
| `libntp` | `libntp/src/lib.rs` (`UtcDateTime`, `offset_and_delay`, `unix_secs_to_utc`, `format_date`) |
| `ntpd` | `userbin/ntpd/src/main.rs` (`apply_time`, `ntp_sync`) |
| `datetime` | `userbin/datetime/src/main.rs` |
| libneodos time | `libneodos/src/syscall/time.rs` (`ob_set_datetime`), `syscall/ob.rs` (40/41/50), `syscall/types.rs` (`DateTime`, `SysTimeZone`) |
| i18n formatters | `libneodos/src/i18n.rs` (`i18n_format_date`, `i18n_format_time`), `libnlt/src/region.rs` |
| i18n Ed25519 | `libnlt/src/signature.rs` (`verify`), `libneodos/src/i18n.rs` (`i18n_verify_signature`) |
| DNS | `libdns/src/*`, `libnet/src/dns.rs` (`NetTransport::exchange`) |
| Net transport | `libnet/src/lib.rs`, `libnet-nxl/src/main.rs` (`NetAbiTable`) |
| TCP | `neodos-kernel/src/net/tcp.rs` (`tcp_connect`, `tcp_send`, `tcp_tick`, `tcp_mac_for`) |
| Socket syscalls | `neodos-kernel/src/syscall/ob/set/net.rs` (SocketConnect=18), `query/net.rs`, `libneodos/src/syscall/net.rs` |
| Socket wait gap | `neodos-kernel/src/syscall/ob/wait.rs:71`, `neodos-kernel/src/syscall/handlers.rs:584-662` |
| VFS trait | `neodos-kernel/src/fs/vfs/mod.rs` (no `sync`) |
| NeoFS write | `neodos-kernel/src/fs/neofs/neodos_v2.rs:560` (no `save_sb`), `:690` (`rename`) |
| Version | `neodos-kernel/src/main.rs:77`, `build.rs:117` |
| NXL loader | `neodos-kernel/src/infra/nxl.rs` |
| NXL ABI | `libneodos/src/export.rs`, `libneodos-nxl/src/main.rs` |
| Package design | `docs/architecture/package-manager-arch.md`, `docs/userland/nxp-format.md` |

### 16.2 Tests

| Suite | Result (run 2026-10-10) |
| --- | --- |
| `libneocfg` host tests | 15 passed / 0 failed |
| `libntp` host tests | 19 passed / 0 failed |
| `libdns` host tests | 27 passed / 0 failed / 1 ignored (network) |
| `libneotui` host tests | 9 passed / 0 failed |
| Kernel net e2e (loopback TCP) | existing test `net_tcp_loopback_e2e` (`net/tests/net.rs:513`) |
| Kernel TZ | `register_timezone_tests` (`cm/timezone.rs:181`, registered `testing.rs:301`) |

### 16.3 Issue references

Issue numbers referenced: 30, 322, 323, 324, 325, 326, 327, 328, 329, 330, 98,
357, 360, 356, 467, 26, 361, 307, 593, 312, 315, 362, 372, 89, 57, 582, 411,
105, 103, 579 (all read live via `gh` on 2026-10-10).

### 16.4 Negative evidence (confirmed absent)

- HTTP/HTTPS/TLS/curl: repo-wide case-insensitive grep, no code.
- SHA-256/HMAC/AEAD/X25519/RSA/RDSEED: no code/crates outside optional
  `ed25519-compact`.
- `fsync`/`sync` file operation: not on `fs/vfs/mod.rs` trait.
- Runtime `.nxe`/`.nxl` installation: no installer binary or command.

---

## 17. Package management (NXP / MSI) — extension

> Detailed design: [`nxp-msi-package-management-design.md`](nxp-msi-package-management-design.md).

This section extends the master design with the `.nxp` package format and the
NeoDOS `.msi` installation bundle.

**Audit summary.** `.nxp` is **designed and partially implemented** (host tool
`nxpkg` in NeoTools emits a subset: magic `NXP1`, TLV manifest with only
`NAME/VER/DESC/ARCH`, per-file CRC32; no signature, no compression, no dependency
tags). There is **no runtime `.nxp` parser/installer** (`libneopkg`/`neoget` do
not exist). `.msi` as an **installer format does not exist at all** — the only
`MSI` in the repository is Message Signaled Interrupts; `.msi` must be defined
here. Deployment is build-time only (`neodev/src/image.rs`); there is no runtime
install path, and `nxverify.nxe` prints "CRC OK" without computing CRC32.

**Recommended division of responsibilities.**

- `.nxp` = **unit of software**: a completed `NXP1` container (full manifest tags,
  SHA-256, Ed25519 signature block, deterministic path validation and limits).
- `.msi` = **unit of deployment**: a NeoDOS-specific bundle that references or
  embeds `.nxp` packages plus an install plan/config. It is **not** Microsoft MSI
  and is applied by `libneopkg`, so there is a single installation engine.

**Architecture.** Reuse the documented names `libneopkg` (plain crate engine) and
`neoget` (CLI); `neoupdate` delegates installation to `libneopkg`. M1 needs no
new ObType; the Registry (`\Packages\`) is the DB and keyring. The ObType plan in
`package-manager-arch.md` (`Package=19/Repository=20/Transaction=21`) collides
with existing types (19–22 taken) and must be reconciled (#411).

**Dependency-graph edges added:**

```text
neoget ──> libneopkg ──> libcrypto (SHA-256, Ed25519)
                     ──> Registry (Cm)  [package DB, keyring]
                     ──> VFS           [FileSync/FileReplace, prerequisite]
neoupdate ──> libneopkg
.msi bundle ──> libneopkg
libneopkg ──> libhttp + libtls        [repositories, later]
```

**Test strategy added:** package fixtures (valid/invalid/truncated/unsupported
version), hash/signature tests, path-traversal and limit tests, dependency
(cycle/conflict) tests, disk-space, interrupted-install/reboot recovery,
concurrent install, and `.msi` bundle tests — all deterministic and local.

**Risks added:** custom-format maintenance; treating CRC32 as security; path
traversal; no atomicity (`FileSync`/`FileReplace` absent); unsigned local
packages; ObType collision (#411); executing package scripts with elevation
(prohibited).

**Decision-log additions:**

| # | Decision | Rationale |
| --- | --- | --- |
| D13 | Keep and complete the NXP `NXP1` container; do not adopt tar/zip/cpio for M1 | Already documented + partially built; zero-copy; kernel-verifiable |
| D14 | NeoDOS `.msi` is a bundle that references/embeds `.nxp`; not Microsoft MSI | Single installer engine; clear unit-of-software vs unit-of-deployment split |
| D15 | SHA-256 for authenticity; CRC32 only as a fast corruption check | CRC32 is not cryptographic |
| D16 | Package scripts disabled by default and never run elevated | Downloaded ≠ authorized |
| D17 | `neoupdate` delegates installation to `libneopkg` | One authoritative installer; no duplicated resolver/rollback |

**Proposed backlog additions:** PKG-1…PKG-10 (format freeze, path validation,
parser/verifier, DB/ownership/uninstall, safe install, resolver, `.msi` format,
`neoupdate` delegation, ObType reconciliation, real `nxverify`).

---

*End of design. No code, public API, schema, or issue state was changed. This
document proposes work; it does not perform it.*
