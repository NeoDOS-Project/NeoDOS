# HTTP Client (`libhttp`) + `neocurl` — Design Document

> **Version:** v0.1-draft
> **Status:** Design (no implementation)
> **Target Release:** v0.58 — Official Tools
> **Related:** #307 (blocking socket waits), #593 (shared UDP helper), #315,
> #467 (monotonic clock), #98; `docs/design/http-tls-update-prerequisites-design.md`,
> `docs/networking/stack.md`, `docs/networking/userland.md`
> **ABI Impact:** none (userland); depends on the TCP-connect fix and, for good
> timeouts, on #307/#467

---

## 1. Research

| Source | What it establishes |
| --- | --- |
| `libdns/src/types.rs`, `libdns/src/resolve.rs` | the proven "protocol crate + injectable transport" pattern (`DnsTransport::exchange`) |
| `libnet/src/lib.rs`, `libnet-nxl/src/main.rs` | `net.nxl` `NetAbiTable` (v1): `socket_create/bind/connect/listen/send/recv/close`; `libnet` convenience layer |
| `libnet/src/dns.rs` | `NetTransport::exchange`: UDP + DNS; RDTSC/yield wait workaround (no blocking primitive) |
| `neodos-kernel/src/syscall/ob/set/net.rs` | `SocketConnect=18` **broken for TCP** (no SYN) |
| `neodos-kernel/src/syscall/ob/query/net.rs` | `SocketRecv=23` non-blocking; `TcpStatus=19` |
| `docs/networking/stack.md`, `docs/networking/userland.md` | socket model, `net.nxl`, userland tools |
| `libntp/src/lib.rs`, `userbin/ntpd/src/main.rs` | the duplicated UDP exchange pattern that #593 wants to unify |
| repo-wide grep | **no HTTP client, no HTTP parser, no `neocurl`** |

Verified: UDP works end-to-end; **userland TCP connect does not**; receive is
non-blocking and poll-driven; no shared UDP/TCP exchange helper exists.

---

## 2. Problem analysis

NeoDOS has no way to fetch an HTTP resource. Building one by hand inside a tool
would duplicate:

- TCP socket setup (`ob_socket_create/connect/send/recv/close`),
- DNS resolution (`libnet::dns`),
- the send-retry/ARP workaround and the RDTSC receive loop (already duplicated
  between `libnet::dns` and `ntpd`, #593),
- HTTP parsing (status line, headers, chunked bodies, limits).

Why existing abstractions cannot solve it:

- **`libdns`** is DNS-only; its transport returns a single datagram, not a byte
  stream.
- **`libnet`** exposes sockets, not a stream protocol or a deadline-aware read.
- **`libntp`** is a protocol codec for NTP, not HTTP.
- There is no HTTP parser and no request/response model anywhere.

The feature therefore needs a **reusable HTTP/1.1 client** with an injectable
transport, so TLS can layer on top without duplicating TCP/DNS.

---

## 3. Solution design

### 3.1 Types / structs / enums

No new `ObType`, no new syscall: HTTP is pure userland over the existing socket
classes. New crate `libhttp` (plain `no_std` crate, `std` under tests — the
`libdns`/`libntp` convention).

```rust
// libhttp/src/transport.rs
pub struct Deadline { /* monotonic instant; backed by RDTSC until #467 */ }
pub trait HttpTransport {
    type Conn: Read + Write + Close;
    fn connect(&mut self, host: &str, port: u16, dl: Deadline)
        -> Result<Self::Conn, HttpError>;
}
pub trait Read  { fn read(&mut self, buf: &mut [u8], dl: Deadline) -> Result<usize, HttpError>; }
pub trait Write { fn write_all(&mut self, buf: &[u8], dl: Deadline) -> Result<(), HttpError>; }
pub trait Close { fn close(self); }

// libhttp/src/lib.rs
pub enum Method { Get, Head, Post }          // Post deferred in M1
pub struct Url { pub scheme: Scheme, pub host: String, pub port: u16, pub path: String }
pub enum Scheme { Http, Https }
pub struct Request { pub method: Method, pub url: Url, pub headers: Vec<(String, String)> }
pub struct Response { pub status: u16, pub headers: Vec<(String, String)>, pub body: Body }
pub enum Body { Empty, Bytes(Vec<u8>) }
pub struct Limits {
    pub max_header_bytes: usize,     // e.g. 16 KiB
    pub max_body_bytes: Option<u64>, // None = stream until close
    pub max_redirects: u8,           // e.g. 5
    pub connect_timeout_ms: u32,
    pub read_timeout_ms: u32,
    pub write_timeout_ms: u32,
}
pub enum HttpError {
    Dns, Connect, Tls, Timeout, Cancelled,
    MalformedStatus, MalformedHeader, TooManyHeaders, BodyTooLarge,
    TooManyRedirects, InsecureRedirect, UnsupportedTransfer, Status(u16), Io,
}
pub fn get<T: HttpTransport>(t: &mut T, url: &Url, limits: Limits) -> Result<Response, HttpError>;
pub fn head<T: HttpTransport>(t: &mut T, url: &Url, limits: Limits) -> Result<Response, HttpError>;
```

### 3.2 New syscalls / ObType / classes

**None.** HTTP uses the existing socket Ob classes through `net.nxl`/`libnet`.
The only ABI dependency is the **TCP-connect fix** (sibling doc) and, for real
timeouts, #307 (blocking wait) / #467 (monotonic clock).

### 3.3 New files / modules

Userland (repo root), following existing conventions:

```text
libhttp/Cargo.toml
libhttp/src/lib.rs          — request/response API, redirects, limits
libhttp/src/transport.rs    — HttpTransport / Read / Write / Close / Deadline
libhttp/src/parse.rs        — status line, headers, Content-Length, chunked
libhttp/src/url.rs          — minimal URL parse (scheme/host/port/path)
libhttp/src/error.rs        — HttpError
libhttp/src/tests.rs        — fixtures
userbin/neocurl/Cargo.toml
userbin/neocurl/src/main.rs — CLI
```

No new kernel module. `libnet` gains a `TcpTransport` implementation of
`HttpTransport` (new file `libnet/src/http_transport.rs`).

### 3.4 Changes to existing files

| Path | Change |
| --- | --- |
| `libnet/src/lib.rs` | `TcpTransport` (implements `HttpTransport` over `net.nxl` sockets) |
| `libnet/src/dns.rs` | Reuse the shared exchange helper (#593) instead of a private loop |
| `userbin/ntpd/src/main.rs` | Reuse the shared exchange helper (#593) |
| `neodev/src/image.rs` | Add `neocurl` to the binary list |
| `docs/networking/userland.md`, `docs/userland/shell.md` | Document `neocurl` |

### 3.5 Semantics / M1 scope

- **HTTP/1.1 only.** `GET` and `HEAD`.
- **Bodies:** `Content-Length`, `Transfer-Encoding: chunked`, and
  connection-close-delimited. Unknown transfer encodings → `UnsupportedTransfer`.
- **Redirects:** 301/302/303/307/308 up to `max_redirects`; HTTPS→HTTP for a
  sensitive request → `InsecureRedirect`.
- **Limits:** header bytes, body bytes, redirect count; malformed responses fail
  with a specific `HttpError`.
- **Timeouts:** connect/read/write from `Deadline`; `Timeout` on expiry.
- **Cancellation:** via the deadline; explicit cancellation waits on the runtime
  (#307).
- **Cleanup:** `Conn` is closed on every error path (RAII `Close`).
- **Persistent connections, `POST`, streaming-to-disk, HTTP/2, compression:**
  deferred. M1 opens one connection per request and honors `Connection: close`.

---

## 4. Alternatives

- **Implement HTTP inside each tool** (`neocurl`, updater). Rejected:
  duplicates TCP/DNS/HTTP logic; the task explicitly forbids duplicating TCP/DNS.
- **Reuse an external Rust HTTP crate (e.g. `ureq`/`reqwest`).** Rejected for
  M1: they assume `std`, sockets, and a TLS backend that do not exist on
  `x86_64-unknown-none`; the repo is external-crate-averse in Ring 3.
- **Package `libhttp` as an NXL.** Rejected: NXLs are global/load-once with no
  version negotiation; the reuse libraries are plain crates. See the master
  design §10.
- **HTTP/2 first.** Rejected: needs a byte-stream transport, ALPN, and framing;
  HTTP/1.1 is the minimal, testable milestone.
- **Skip HTTP, go straight to TLS.** Rejected: TLS layers on a byte stream and
  still needs the same transport; HTTP/1.1 is the smallest end-to-end proof.

---

## 5. Affected components

| Subsystem | Change | Impact |
| --- | --- | --- |
| Kernel net | TCP-connect fix (prerequisite, sibling doc) | Low |
| `libnet` | `TcpTransport`; shared exchange helper (#593) | Medium |
| `libdns` | (unchanged protocol; transport may move behind the shared helper) | Low |
| `libhttp` (new) | HTTP/1.1 client | New crate |
| `neocurl` (new) | CLI | New `.nxe` |
| Image list | Add `neocurl` | Low |
| Scheduler/VFS/memory/drivers | None | None |

---

## 6. API contract

| Function | Args | Returns | Errors |
| --- | --- | --- | --- |
| `HttpTransport::connect` | `host`, `port`, `Deadline` | `Conn` | `Dns`, `Connect`, `Timeout` |
| `Read::read` | `buf`, `Deadline` | bytes read (0 = EOF) | `Timeout`, `Io` |
| `Write::write_all` | `buf`, `Deadline` | `()` | `Timeout`, `Io` |
| `get` / `head` | `&mut T`, `Url`, `Limits` | `Response` | any `HttpError` |
| `neocurl` | `neocurl <url> [--head] [--max-bytes N]` | body to stdout; status to stderr | exit code 1 on error |

Preconditions: `Url` scheme is `Http` for M1 (`Https` returns `Tls` until
`libtls` lands). `Limits` are finite. Postconditions: the connection is closed;
the returned `Response` body is bounded by `max_body_bytes`.

Error-code mapping: `-NoEnt`-style DNS failure → `Dns`; connect refusal →
`Connect`; deadline → `Timeout`; oversized body → `BodyTooLarge`; malformed
input → `MalformedStatus`/`MalformedHeader`.

---

## 7. Test plan (≥3 per invariant)

**INV-1 — Requests are well-formed and responses parsed.**

| Test | Expected |
| --- | --- |
| `http_get_request_line` | `GET /path HTTP/1.1`, `Host:` present |
| `http_parse_status_and_headers` | status + headers parsed, case-insensitive |
| `http_parse_malformed_status` | garbage status → `MalformedStatus` |
| `http_parse_duplicate_headers` | duplicates preserved in order |

**INV-2 — Body framing is correct.**

| Test | Expected |
| --- | --- |
| `http_body_content_length` | exactly N bytes |
| `http_body_chunked` | de-chunked body equals payload |
| `http_body_connection_close` | reads until EOF when no length |
| `http_body_unknown_transfer` | `UnsupportedTransfer` |

**INV-3 — Limits and timeouts are enforced.**

| Test | Expected |
| --- | --- |
| `http_header_limit` | oversized headers → `TooManyHeaders` |
| `http_body_limit` | body > `max_body_bytes` → `BodyTooLarge` |
| `http_read_timeout` | stalled server → `Timeout` |
| `http_connect_timeout` | unreachable host → `Timeout`/`Connect` |

**INV-4 — Redirect policy.**

| Test | Expected |
| --- | --- |
| `http_redirect_follows` | 302 to a relative URL followed |
| `http_redirect_limit` | > `max_redirects` → `TooManyRedirects` |
| `http_redirect_https_to_http_refused` | `InsecureRedirect` |

**INV-5 — Resource cleanup.**

| Test | Expected |
| --- | --- |
| `http_closes_on_error` | socket closed after a parse error |
| `http_head_has_no_body` | `HEAD` returns headers with empty body |
| `http_dns_failure_maps` | unknown host → `Dns` |

**Integration (QEMU/VBox):** `neocurl` against a local fixture server; no
public-Internet dependency in routine tests.

---

## 8. Implementation plan

1. **TCP-connect fix** (sibling doc) + a test that a socket reaches `Established`.
2. **Shared exchange helper** (#593) and migrate `libnet::dns`/`ntpd`.
3. **`libhttp` core**: `Url`, `Request`, `parse.rs`, `Limits`, `HttpError`.
4. **`libnet::TcpTransport`** implementing `HttpTransport`.
5. **Redirects, chunked, connection-close, limits, timeouts.**
6. **`neocurl` CLI** + image list.
7. **Fixtures** and tests (no Internet).
8. **Docs** + markdownlint.

Dependencies: step 1 is a hard blocker; step 2 should precede step 4 to avoid a
third copy of the exchange loop. TLS is a later transport wrapper, not a change
to `libhttp`.

---

## 9. Open questions

1. `Deadline` source: RDTSC approximation (today) vs #467 monotonic clock.
2. Should `neocurl` write bodies to a file, or stdout only, in M1?
3. Whether `libhttp` should expose streaming callbacks before M2.
4. Whether the shared exchange helper (#593) lives in `libnet` or a new
   `libnetio` crate.

---

*End of design. No code, public API, or issue state is changed by this document.*
