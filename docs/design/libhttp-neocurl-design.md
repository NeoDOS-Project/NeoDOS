# HTTP Client (`libhttp`) + `neocurl` — Design Document

> **Version:** v0.3 — implemented through M5 (2026-11-16)
> **Status:** Implemented: `libhttp` core + framing (M1/M4), byte-stream
> prerequisites (M2), DNS TTL cache (M2b), `libnet::TcpTransport` (M3),
> `neocurl` (M5) and the reproducible E2E harness (M6: `httptest` +
> `tools/http-e2e`). Pending: TLS (later milestone).
> Supersedes v0.1, whose baseline (broken userland TCP connect, no blocking
> wait, RDTSC deadlines) is obsolete.
> **Target Release:** v0.58 — Official Tools
> **Related:** #307 (socket waits), #467 (monotonic clock), #484 (loopback),
> #486 (TCP data path), #593 (shared UDP helper), #692 (TCP E2E), #696 (wait
> timeouts), #710 (page cache); `docs/networking/stack.md`,
> `docs/networking/userland.md`, `docs/design/http-tls-update-prerequisites-design.md`,
> `docs/design/libtls-https-design.md`
> **ABI Impact:** none for HTTP. The optional byte-stream fixes (§3.4 of the
> prerequisites doc) touch only `ObInfoClass` semantics, no new syscall.

---

## 1. Research

| Source | What it establishes |
| --- | --- |
| `libdns/src/*`, `libntp/src/lib.rs` | the proven pattern: protocol crate with an injectable transport, `no_std` + `alloc`, host-testable (`#![cfg_attr(not(test), no_std)]`) |
| `libnet/src/lib.rs` | `net.nxl` `NetAbiTable` v1 (`socket_create/bind/connect/listen/send/recv/close`) and the thin `libnet` wrappers |
| `libnet/src/dns.rs` | resolver + Registry servers + TTL cache (`libdns::AnswerCache`) |
| `neodos-kernel/src/syscall/ob/set/net.rs` | `SocketConnect=18` routes to `socket_connect_user`; TCP sends a SYN. **Fixed.** |
| `neodos-kernel/src/syscall/ob/query/net.rs` | `SocketRecv=23` (non-blocking), `TcpStatus=19`, `SocketAccept=43` |
| `neodos-kernel/src/syscall/ob/wait.rs`, `libneodos/src/syscall/ob.rs` | `ob_wait` blocks on Connect/Read; `sys_ob_wait_timeout` bounds it (#696) |
| `libneodos/src/syscall/time.rs` | `ob_uptime_ms()`/`ob_high_res_time()` (#467) for deadlines |
| `neodos-kernel/src/net/tcp.rs` | 16 KiB TX/RX, MSS 1460, RTO 200 ms, in-order-only RX, 32 connections |
| `neodos-kernel/src/arch/x64/paging.rs` | 2 MiB heap per process, bump allocator in most tools |
| `libnlt/src/lzss.rs` | the only compression in the tree: **no deflate/gzip** |
| `userbin/netcli`, `userbin/tcptest` | working Ring-3 TCP client/server over the public ABI |
| repo-wide grep | **no HTTP client, no HTTP parser, no `neocurl`** |

Verified: userland TCP works end to end; receive is non-blocking. The TX-buffer
and EOF gaps were fixed by the byte-stream milestone: a full buffer is `EAGAIN`
with a writable wait (`ob_wait` type 2) and `SocketRecv` returns `0` on EOF.
DNS answers still never expire from the cache.

---

## 2. Problem analysis

NeoDOS has no way to fetch an HTTP resource. Building one inside a tool would
duplicate:

- TCP socket setup, connect waiting and deadlines,
- DNS resolution and the "is this an IP literal?" check,
- HTTP parsing (status line, headers, chunked bodies, limits),
- error mapping and cleanup on every path.

Why existing abstractions cannot solve it:

- **`libdns`** is DNS-only; its transport returns a datagram, not a stream.
- **`libnet`** exposes sockets, not a byte stream with deadlines and EOF.
- **`libntp`** is an NTP codec, not HTTP.
- There is no HTTP parser or response model anywhere.

The feature therefore needs a **reusable HTTP/1.1 client with an injectable
transport**, so TLS can layer on top without duplicating TCP/DNS/HTTP.

---

## 3. Solution design

### 3.1 Types / structs / enums

No new `ObType`, no new syscall: HTTP is pure userland over the existing socket
classes. New crate `libhttp`, dependency-free.

```rust
// libhttp/src/transport.rs
/// Absolute deadline on the monotonic clock (`ob_uptime_ms`, #467).
pub struct Deadline { pub at_ms: u64 }
impl Deadline {
    pub fn after(ms: u32) -> Result<Self, HttpError>; // reads ob_uptime_ms
    pub fn expired(&self) -> bool;
    pub fn remaining_ms(&self) -> u32;
}

pub trait Transport {
    type Conn: Read + Write + Close;
    fn connect(&mut self, host: &str, port: u16, dl: Deadline) -> Result<Self::Conn, HttpError>;
}
pub trait Read  { fn read(&mut self, buf: &mut [u8], dl: Deadline) -> Result<usize, HttpError>; }
pub trait Write { fn write_all(&mut self, buf: &[u8], dl: Deadline) -> Result<(), HttpError>; }
pub trait Close { fn close(self); }

// libhttp/src/lib.rs
pub enum Method { Get, Head, Post }
pub enum Scheme { Http, Https }
pub struct Url { pub scheme: Scheme, pub host: String, pub port: u16, pub path: String }
pub struct HeaderMap { /* ordered, case-insensitive lookup, duplicates preserved */ }
pub struct Request<'a> {
    pub method: Method,
    pub url: Url,
    pub headers: Vec<(String, String)>,   // user headers, validated
    pub body: Option<BodySource<'a>>,      // POST
}
pub enum BodySource<'a> { Bytes(&'a [u8]), Stdin, File(&'a str) }
pub struct ResponseHead { pub version: u8, pub status: u16, pub reason: String, pub headers: HeaderMap }
pub trait BodySink { fn write(&mut self, chunk: &[u8]) -> Result<(), HttpError>; }

pub struct Limits {
    pub max_header_bytes: usize,      // 16 KiB
    pub max_headers: usize,           // 100
    pub max_body_bytes: u64,          // 1 MiB default; 0 = unlimited (streaming sink)
    pub connect_timeout_ms: u32,      // 10 s
    pub read_timeout_ms: u32,         // 10 s of inactivity
    pub write_timeout_ms: u32,        // 10 s
    pub total_timeout_ms: u32,        // 30 s
}

pub enum HttpError {
    InvalidUrl, Dns, Connect, Timeout, Tls,
    MalformedStatus, MalformedHeader, TooManyHeaders,
    UnsupportedTransfer, UnsupportedEncoding,
    BodyTooLarge, IncompleteBody, Io, Cancelled,
}

pub fn execute<T: Transport>(
    t: &mut T, req: &Request<'_>, limits: &Limits, sink: &mut dyn BodySink,
) -> Result<ResponseHead, HttpError>;
```

`Response` is deliberately *not* a struct holding the body: the body is streamed
to a `BodySink`, so a 1 MiB download never needs 1 MiB of heap (the process heap
is 2 MiB).

### 3.2 New syscalls / ObType / classes

**None.** HTTP uses the existing socket Ob classes through `libnet`.

The two byte-stream fixes from the prerequisites doc §3.4 are **implemented**:

- a writable wait reason (`ob_wait` `wait_type = 2`) so a full 16 KiB TX buffer
  blocks instead of returning `-Io`; the send reports `EAGAIN`;
- `SocketRecv` returns `Ok(0)` on EOF.

### 3.3 New files / modules

```text
libhttp/Cargo.toml
libhttp/src/lib.rs          — request/response API, redirect-free flow
libhttp/src/transport.rs    — Transport / Read / Write / Close / Deadline
libhttp/src/url.rs          — minimal URL parse (scheme/host/port/path+query)
libhttp/src/parse.rs        — status line, headers, framing, chunked
libhttp/src/headers.rs      — HeaderMap
libhttp/src/error.rs        — HttpError
libhttp/src/tests.rs        — fixtures + fragmentation tests
libnet/src/http_transport.rs — TcpTransport over `net.nxl` sockets
userbin/neocurl/Cargo.toml
userbin/neocurl/src/main.rs — CLI (flags, sinks, exit codes)
```

### 3.4 Changes to existing files

| Path | Change |
| --- | --- |
| `libnet/src/lib.rs` | register `http_transport` |
| `libnet/src/dns.rs` | TTL-aware cache via `libdns::AnswerCache` — **done** (M2b) |
| `libnet/src/http_transport.rs` | new: connect/read/write with deadlines, IPv4 literal fast path, EOF via `TcpStatus`, TX backpressure |
| `neodos-kernel/src/net/{socket,tcp}.rs` | writable wait + EOF — **already landed** (prerequisite doc §3.4) |
| NeoDev (external) | add `neocurl` to the image program list |
| `docs/networking/userland.md`, `docs/userland/shell.md` | document `neocurl` |

### 3.5 Semantics

- **HTTP/1.1 only**; requests `GET`, `HEAD`, `POST`. One request per
  connection, `Connection: close`, no keep-alive, no pipelining.
- **Request body:** `Content-Length` exactly; from memory, stdin or file. No
  chunked request bodies. Default `Content-Type:
  application/x-www-form-urlencoded` with `-d`.
- **Response framing:** 1xx informational responses are skipped (bounded);
  `HEAD`, 204 and 304 have no body; `Transfer-Encoding` takes precedence over
  `Content-Length`; `chunked` honours extensions and ignores trailers (bounded);
  no length + `Connection: close` reads until EOF. A short `Content-Length`
  body is `IncompleteBody`.
- **Compression:** none. The client sends `Accept-Encoding: identity`; a
  response with `Content-Encoding: gzip|deflate|br` is rejected as
  `UnsupportedEncoding` rather than returned corrupt (the tree has no inflate).
- **Redirects:** out of scope (not required for this initiative). `Location` is
  parsed and exposed for a later policy.
- **Limits:** header bytes, header count, body bytes, chunk size and total
  deadline; every violation maps to a specific error.
- **Timeouts:** connect/read/write/total, all on `ob_uptime_ms`. Network calls
  use `sys_ob_wait_timeout` and re-check readiness (the kernel returns `Ok` on
  expiry).
- **EOF:** reads stop on `Ok(0)` (`SocketRecv` reports EOF directly; `TcpStatus`
  stays available as a diagnostic).
- **Backpressure:** `write_all` loops over partial sends; on `EAGAIN` it waits
  for send-buffer space with `sys_ob_wait_write_timeout` (`wait_type = 2`) and
  retries until the write deadline.
- **Cleanup:** close on every path (success, parse error, timeout, TLS error).
- **Host:** `Host` mandatory, with `:port` when not 80/443. IPv4 literals bypass
  DNS (and are recorded for TLS validation).
- **User-Agent:** `NeoDOS-neocurl/0.1`.

### 3.6 `neocurl` CLI contract

```text
neocurl URL                    GET; body to stdout
neocurl -I URL | --head        HEAD; response headers to stdout
neocurl -i URL                 include response headers before the body
neocurl -d DATO URL            POST (x-www-form-urlencoded)
neocurl -d @- URL              POST reading the body from stdin
neocurl -o FICHERO URL         stream the body to a file
neocurl --max-time MS URL      total deadline
neocurl --max-bytes N URL      body limit
neocurl -s URL                 silent (no progress/status on stderr)
```

Body bytes go to stdout unmodified; headers/status/errors go to stderr or
stdout only with `-i`/`-I`. Exit codes: `0` success, `1` transfer/parse/TLS
error, `2` usage error. No other flags.

---

## 4. Alternatives

- **HTTP inside each tool** (`neocurl`, updater). Rejected: duplicates
  TCP/DNS/parser/cleanup logic; the task forbids duplicating TCP/DNS.
- **External Rust HTTP crate** (`ureq`/`reqwest`). Rejected: assume `std`,
  blocking sockets, a TLS backend and a different error model.
- **Package `libhttp` as an NXL.** Rejected: NXLs are global/load-once with no
  version negotiation; the shared protocol crates are plain static crates.
- **HTTP/2 first.** Rejected: needs framing, ALPN and multiplexing; HTTP/1.1 is
  the minimal, testable milestone.
- **Skip HTTP, go straight to TLS.** Rejected: TLS layers on the same byte
  stream and would be debugged with no working transport.
- **Buffer the whole body in RAM.** Rejected: 2 MiB heap, bump allocator.

---

## 5. Affected components

| Subsystem | Change | Impact |
| --- | --- | --- |
| Kernel net | Optional writable wait + EOF (no new ABI) | Low/Medium |
| `libnet` | `TcpTransport`; TTL-aware DNS cache (`libdns::AnswerCache`) | **Done** |
| `libhttp` (new) | HTTP/1.1 client | New crate |
| `neocurl` (new) | CLI | New `.nxe` |
| Image (NeoDev) | Add `neocurl` | Low |
| TLS/`libcrypto` | Later transport wrapper, not part of this crate | None |
| Scheduler / VFS / memory / drivers | None | None |

---

## 6. API contract

| Function | Args | Returns | Errors |
| --- | --- | --- | --- |
| `Transport::connect` | `host`, `port`, `Deadline` | `Conn` | `Dns`, `Connect`, `Timeout` |
| `Read::read` | `buf`, `Deadline` | bytes (`0` = EOF) | `Timeout`, `Io` |
| `Write::write_all` | `buf`, `Deadline` | `()` | `Timeout`, `Io` |
| `execute` | `&mut Transport`, `&Request`, `&Limits`, `&mut dyn BodySink` | `ResponseHead` | any `HttpError` |
| `Url::parse` | `&str` | `Url` | `InvalidUrl` |
| `neocurl` | see §3.6 | stdout/stderr + exit code | `1`/`2` |

Preconditions: `Url` scheme is `Http` (until `libtls` lands, `Https` returns
`Tls`); `Limits` finite and non-zero. Postconditions: the connection is closed;
the body was delivered exactly once; body bytes never exceed `max_body_bytes`
when set.

Error mapping: DNS failure → `Dns`; refused/unreachable → `Connect`; deadline →
`Timeout`; oversized body → `BodyTooLarge`; bad syntax →
`MalformedStatus`/`MalformedHeader`; unknown transfer coding →
`UnsupportedTransfer`.

---

## 7. Test plan (≥3 per invariant)

**INV-1 — Requests are well-formed and responses parsed.**

| Test | Expected |
| --- | --- |
| `http_get_request_line` | `GET /path HTTP/1.1` + `Host:` present |
| `http_post_content_length` | exact `Content-Length`, body bytes counted |
| `http_parse_status_and_headers` | status + headers parsed, case-insensitive |
| `http_parse_duplicate_headers` | duplicates preserved in order |
| `http_parse_malformed_status` | garbage status → `MalformedStatus` |
| `http_header_injection_rejected` | CR/LF in path or header value → `InvalidUrl` |

**INV-2 — Body framing is correct under fragmentation.**

| Test | Expected |
| --- | --- |
| `http_body_content_length` | exactly N bytes |
| `http_body_chunked` | de-chunked body equals payload |
| `http_body_chunked_one_byte_reads` | same body with 1-byte reads |
| `http_body_chunked_noise` | extensions + trailer tolerated within limits |
| `http_body_close_delimited` | reads until EOF when no length |
| `http_body_short_content_length` | `IncompleteBody` |
| `http_body_unknown_transfer` | `UnsupportedTransfer` |
| `http_body_gzip_rejected` | `UnsupportedEncoding` |

**INV-3 — Limits and timeouts are enforced.**

| Test | Expected |
| --- | --- |
| `http_header_limit` | oversized headers → `TooManyHeaders` |
| `http_body_limit` | body > `max_body_bytes` → `BodyTooLarge` |
| `http_read_timeout` | stalled server → `Timeout` |
| `http_connect_timeout` | unreachable host → `Timeout`/`Connect` |
| `http_total_deadline` | slow drip → `Timeout` |

**INV-4 — Resource cleanup and edge statuses.**

| Test | Expected |
| --- | --- |
| `http_closes_on_error` | socket closed after a parse error |
| `http_head_has_no_body` | `HEAD` returns headers, empty body |
| `http_204_304_no_body` | no body read for 204/304 |
| `http_1xx_skipped` | `100 Continue` ignored before the final response |
| `http_dns_failure_maps` | unknown host → `Dns` |

**INV-5 — Streaming respects the heap budget.**

| Test | Expected |
| --- | --- |
| `http_sink_streams_1mib` | 1 MiB body delivered in chunks, peak allocation bounded |
| `http_sink_error_propagates` | sink error aborts and closes |
| `http_ipv4_literal_skips_dns` | `http://10.0.2.2/` never queries DNS |

**Integration (QEMU):** `neocurl` against a host fixture server on `10.0.2.2`
(SLiRP); no public-Internet dependency in routine tests.

---

## 8. Implementation plan

1. **M1 — `libhttp` core (offline): DONE.** `Url`, `Request`, `HeaderMap`,
   `parse.rs`, `Limits`, `HttpError`; unit tests.
2. **M2 — byte-stream fixes: DONE.** Prerequisites doc §3.4; kernel tests.
3. **M3 — `libnet::TcpTransport`: DONE.** Connect/read/write deadlines, IPv4
   literal, EOF, backpressure. `libhttp::execute` drives the exchange and
   closes the connection on every path.
4. **M4 — framing complete: DONE.** Chunked/close/HEAD/204/304/1xx,
   fragmentation tests, limits.
5. **M5 — `neocurl`: DONE.** Flags, sinks (`-i`, `-I`, `-o`, `-d`), exit codes;
   discovered by NeoDev and installed in the image.
6. **M6 — automation: DONE.** `httptest.nxe` runs the acceptance matrix in
   the guest (11 cases, counting sink for the 64 KiB/1 MiB bodies) and prints
   `HTTP-E2E: PASS n/n`. `tools/http-e2e` is the host orchestrator: it serves
   the fixture, boots the VM headless, types the command over the VirtualBox
   scancode interface and greps the serial summary. No Python or bash driver.

Dependencies: M3 needs M1; M4 needs M3; M5 needs M4. TLS is a later transport
wrapper (`libtls`), not a change to `libhttp`.

---

## 9. Open questions

1. `neocurl` output: is `-o` needed, or is shell redirection (`> file`) enough?
   (Recommendation: keep `-o`, it also gives a clear "bytes written" summary.)
2. POST body upper bound (heap is 2 MiB): cap at 256 KiB, or stream from file?
3. Should `libhttp` expose a pull-based body reader (iterator) instead of only a
   `BodySink`? (Recommendation: sink first, pull later if a caller needs it.)
4. Redirects: confirm they stay out of the initial scope.
5. Keep-alive: confirm one request per connection for now.

---

*End of design. No code, public API, or issue state is changed by this document.*
