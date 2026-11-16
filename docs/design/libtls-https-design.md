# TLS / HTTPS (`libcrypto` + `libtls`) — Design Document

> **Version:** v0.2 — reconciled with `develop` @ `532d554` (2026-11-14)
> **Status:** Design (no implementation). Blocked on the Ring-3 CSPRNG (M8) and
> the time-trust policy (M7); the TLS-stack choice is the open decision D1.
> **Target Release:** v0.61–v0.62 — Security Hardening
> **Related:** #89 (NXE signatures), #57 (module signing), #361 (NTP auth),
> #467 (monotonic clock), #582 (NLT signatures), #696 (wait timeouts), #411;
> `docs/design/http-tls-update-prerequisites-design.md`,
> `docs/design/libhttp-neocurl-design.md`
> **ABI Impact:** depends on `ObInfoClass::Random=48` / `RandomCaps=49`; no new
> syscall number and no new `ObType`.

---

## 1. Research

| Source | What it establishes |
| --- | --- |
| `libnlt/src/signature.rs`, `libnlt/Cargo.toml` | the **only** crypto in the repo: Ed25519 sign/verify via `ed25519-compact` (feature `signatures`, `no_std`, no transitive deps), RFC 8032 vectors |
| `libneodos/src/i18n.rs` | `i18n_verify_signature`, `NLT_DEV_PUBLIC_KEY`, `REQUIRE_SIGNED`, features `i18n-signatures`/`i18n-require-signed` |
| `neodos-kernel/src/hal/mod.rs`, `hal/raw/cpu.rs` | `rdrand` (kernel-only, ASLR); **no RDSEED**, no Ring-3 exposure, no `\Device\Random` |
| `neodos-kernel/src/arch/x64/paging.rs` | 2 MiB heap per process — the TLS stack must be bounded |
| `neodos-kernel/src/object/types.rs` | `ObInfoClass` 44/45 taken; next free 48 (see prerequisites doc) |
| `neodos-kernel/src/syscall/ob/query/time.rs`, `drivers/hw/rtc.rs` | `DateTime` from the RTC: **1 s resolution**, no UTC/local flag, no "was it synchronised?" bit |
| `userbin/ntpd/src/main.rs` | NTP step + `Services\Ntpd\Status` (`State`, `LastSync`, `Stratum`, `OffsetMs`) |
| `libntp/src/lib.rs` | civil-time math (`unix_secs_to_utc`, `utc_to_unix_secs`) — reusable, no dependency cycle |
| `libnlt/src/lzss.rs` | LZSS only: **no deflate/gzip** (irrelevant for TLS, relevant for HTTP) |
| `docs.rs/rustls` (0.23.46, 2026-10) | `default-features = false` removes `aws-lc-rs`; `custom-provider` disables built-in providers; `std` is a feature, so `no_std` + `alloc` is supported; `hashbrown` feature for no-std collections |
| repo-wide grep | **no TLS/SSL, no SHA-256, no HMAC, no AEAD, no X25519, no RSA, no X.509, no trust store, no CSPRNG** |

Verified: Ed25519 verify is real and tested (NLT only); everything else a TLS
client needs is absent, including a Ring-3 entropy source and a trustworthy
wall clock.

---

## 2. Problem analysis

HTTPS requires, at minimum:

1. **A byte-stream transport** (provided by `libhttp`/`libnet`).
2. **Cryptographic primitives**: SHA-256, HMAC/HKDF, an AEAD (AES-GCM or
   ChaCha20-Poly1305), key agreement (X25519), and signature verification for
   the certificate chain and `CertificateVerify` (RSA-PSS/PKCS#1, ECDSA P-256,
   optionally Ed25519).
3. **A CSPRNG** for nonces and ephemeral keys.
4. **X.509 parsing**, chain building, hostname (SAN) validation and a
   read-only trust store.
5. **A correct clock**, because certificate validity is time-bounded.

None of 2–5 exist. Why existing abstractions cannot solve them:

- **Ed25519** is one signature primitive, scoped to NLT; no hashing, AEAD or key
  agreement.
- **`rdrand`** is a kernel helper with no object boundary and no reseed policy.
- There is **no PKI code and no trust store**.
- The **clock can be wrong pre-NTP**; TLS validity must fail closed, and today
  nothing tells userland whether the RTC was ever synchronised.

Implementing crypto from scratch is unsafe and out of scope. The design must
choose a maintained implementation compatible with the `no_std` target and the
repo's dependency posture.

---

## 3. Solution design

Two new userland crates plus two kernel capabilities (entropy and time trust).

### 3.1 Types / structs / enums

```rust
// libcrypto/src/lib.rs  (plain no_std + alloc crate)
pub mod sha256;   // digest
pub mod hmac;     // HMAC-SHA256
pub mod hkdf;     // TLS 1.3 key schedule
pub mod aead;     // Aes128Gcm / Aes256Gcm / ChaCha20Poly1305
pub mod x25519;   // ECDHE
pub mod sig;      // verify: RSA-PSS, RSA-PKCS#1 v1.5, ECDSA P-256, Ed25519
pub mod rng;      // Rng trait backed by \Device\Random

pub trait Rng { fn fill(&mut self, out: &mut [u8]) -> Result<(), CryptoError>; }
pub trait Clock { fn now_unix(&self) -> Option<i64>; }

// libtls/src/lib.rs  (plain no_std + alloc crate)
pub enum TlsVersion { Tls13, Tls12 }          // TLS 1.0/1.1 never enabled
pub struct TrustStore { /* DER roots, read-only */ }
pub struct ClientConfig<'a> {
    pub trust_anchors: &'a TrustStore,
    pub min_version: TlsVersion,
    pub alpn: &'static [&'static str],        // ["http/1.1"]
    pub rng: &'a mut dyn Rng,
    pub clock: &'a dyn Clock,
    pub limits: TlsLimits,                    // record / handshake / chain sizes
}
pub enum TlsError {
    NotTrusted, Expired, NotYetValid, HostnameMismatch, BadCertificate,
    UnsupportedVersion, NoSharedCipher, UnsupportedSignature, Alert(u8),
    ClockUnsynced, RngUnavailable, Io, RecordLimit,
}
pub struct TlsStream<C> { /* wraps C: Read + Write + Close */ }
pub fn connect<C>(cfg: &mut ClientConfig<'_>, host: &str, c: C)
    -> Result<TlsStream<C>, TlsError>;
```

The clock is **injected**, so host tests can pin any instant and the production
implementation is the only place that touches the RTC.

### 3.2 New syscalls / ObType / classes

Only the **`\Device\Random`** device from the prerequisites doc, exposing
`ObInfoClass::Random=48` and `RandomCaps=49`. No new `ObType`, no new syscall
number. This document adds no kernel class of its own.

### 3.3 New files / modules

```text
libcrypto/Cargo.toml
libcrypto/src/{lib,sha256,hmac,hkdf,aead,x25519,sig,rng,error}.rs
libcrypto/src/tests.rs
libtls/Cargo.toml
libtls/src/{lib,record,handshake,schedule,x509,verify,trust,error,limits}.rs
libtls/src/tests.rs
\System\Certs\roots.der        (trust anchors, image content, read-only)
```

### 3.4 Changes to existing files

| Path | Change |
| --- | --- |
| `libhttp` | Allow `Scheme::Https` to be served by a TLS-wrapping transport |
| `libnet` | No change (transport stays TLS-agnostic) |
| `libnlt` | Optionally re-point Ed25519 at `libcrypto` to avoid two copies |
| `libneodos/src/syscall/time.rs` | `ob_unix_seconds()` (+ clock-trust helper) |
| NeoDev (external) | Ship `\System\Certs\roots.der` |
| `docs/security/security.md` | Trust store and verification policy |

### 3.5 Dependency decision (D1 — the crux)

The target is `x86_64-unknown-none`, `no_std` + `alloc`, no C toolchain,
`rust-lld`. Options:

| Option | Composition | Verdict |
| --- | --- | --- |
| **A. `rustls` `no_std` + custom provider** | `rustls` (`default-features=false`, `custom-provider`, `tls12` optional, `hashbrown`), `rustls-pki-types`, `rustls-webpki`; provider built from RustCrypto (`sha2`, `hmac`, `hkdf`, `aes-gcm`/`chacha20poly1305`, `x25519-dalek`, `p256`, `rsa`, `ed25519-compact`) | **Preferred if it builds.** The protocol state machine, key schedule and record layer are audited and maintained; we only supply primitives. Risk: no_std wiring and provider API surface |
| **B. Minimal TLS 1.3 client over RustCrypto** | `sha2`, `hkdf`, `chacha20poly1305`/`aes-gcm`, `x25519-dalek`, `x509-cert`/`der`, `p256`/`rsa`; our own record + handshake + verification | Fallback. Smaller dependency graph, but we own protocol correctness (HRR, alerts, fragmentation, key updates) |
| **C. Vendored C (`mbedTLS`, `BearSSL`)** | — | **Rejected:** no C toolchain, `no_std` friction, large vendored surface |
| **D. Custom crypto** | — | **Rejected:** unsafe and explicitly out of scope |

**Spike gate (timeboxed, 1–2 days):** compile `rustls` for
`x86_64-unknown-none` with `default-features = false`, `custom-provider`,
`hashbrown` and a stub provider; then a real one. Decision criteria:

1. builds and links into an NXE without `std` or a C toolchain;
2. binary-size delta acceptable under the NXE cap (#470);
3. provider exposes the suites TLS 1.3 needs (X25519, AES-128-GCM,
   ChaCha20-Poly1305, SHA-256, RSA-PSS/ECDSA verify);
4. no unbounded allocations in the dependency graph.

If the spike fails, choose B with TLS 1.3 only. Either way the crates must be
`no_std` + `alloc`, pure Rust, permissively licensed (MIT/Apache-2.0/ISC), and
pinned.

### 3.6 Verification policy

- **Server authentication is mandatory.** `connect` always verifies the chain,
  the certificate's key usage/EKU, the hostname and the validity window.
  **No "insecure" flag exists** in `libtls`.
- A separate `PinnedTestRoot` type may accept a test root for fixtures; it is
  compiled into test artifacts only and is **not** importable by `libupdate`.
- **SNI** is sent for DNS names. For an IPv4 literal (e.g. `10.0.2.2`) SNI is
  omitted (RFC 6066 forbids IP literals) and the `iPAddress` SAN is verified.
- **Hostname rules:** SAN `dNSName` (exact, case-insensitive, single-label
  wildcard `*.example.com` only in the leftmost label and never for IP
  literals); CN is **not** accepted as a fallback unless the certificate has no
  SAN at all, and then only for compatibility with legacy fixtures.
- **Chain rules:** every signature verified, validity checked per certificate,
  issuer/subject chaining, `basicConstraints` CA on intermediates, path length,
  and a bounded chain depth (e.g. 8) and total size (e.g. 64 KiB).
- **Clock policy (D3): IMPLEMENTED (M7).** `libclock::Clock` +
  `libclock::system::SystemClock`: the RTC must be a valid calendar value,
  `Services\Ntpd\Status\State` must be `"Synced"` with a non-zero `LastSync`,
  and the RTC must not predate that sync (300 s skew tolerance). Otherwise
  `ClockError::{RtcInvalid, Unsynced, Implausible}` and TLS returns
  `ClockUnsynced`. Pure policy + host tests live in `libclock`; `libtls` will
  re-export the trait.
  A RTC date before the issuance range of every trusted root is rejected. TLS
  fails closed; there is no bypass.
- **Versions/suites:** TLS 1.3 required; TLS 1.2 only if the chosen stack
  provides it and the suite is AEAD (ECDHE + AES-GCM/ChaCha20-Poly1305). Legacy
  CBC, RSA key exchange, TLS 1.0/1.1 and renegotiation are refused.
- **ALPN:** advertise `http/1.1` only.
- **Out of scope:** client certificates, session resumption/tickets, OCSP and
  CRL revocation (documented as a known gap), Encrypted ClientHello,
  post-quantum groups.

---

## 4. Alternatives

- **TLS only for the updater with a pinned certificate.** Rejected as the
  general design: it is not HTTPS and still needs every primitive.
- **Rely on artifact signatures over HTTP.** Rejected: authenticity of a file is
  not transport confidentiality or server authentication.
- **Expose raw RDRAND to userland.** Rejected: unwhitened, no object boundary,
  no reseed/failure policy; `\Device\Random` is the designed boundary.
- **`rustls-rustcrypto` (third-party provider) as-is.** Not chosen outright: it
  is labelled experimental; it is acceptable as a reference for our provider but
  must not become an unaudited dependency.
- **Vendored C TLS.** Rejected (toolchain/target).
- **Custom crypto.** Rejected.

---

## 5. Affected components

| Subsystem | Change | Impact |
| --- | --- | --- |
| Kernel | `\Device\Random` + classes 48/49 (prerequisite doc) | Low |
| `libneodos` | `ob_unix_seconds()`, clock-trust helper, RNG wrapper | Low |
| `libcrypto` (new) | primitives + RNG boundary | New crate |
| `libtls` (new) | record/handshake/X.509/verify | New crate |
| `libhttp` | HTTPS transport wrapping | Low |
| `libupdate` | uses `libtls` (later) | Low |
| Image (NeoDev) | `\System\Certs\roots.der` | Low |
| `libnlt` | optional Ed25519 de-duplication | Low |
| Scheduler/VFS/memory/drivers | None | None |

---

## 6. API contract

| Function | Args | Returns | Errors |
| --- | --- | --- | --- |
| `Rng::fill` | `&mut [u8]` | `()` | `RngUnavailable` |
| `Clock::now_unix` | — | `Option<i64>` | — (None = unknown) |
| `sha256::digest` | `&[u8]` | `[u8; 32]` | — |
| `hmac::mac` | key, data | tag | — |
| `aead::seal/open` | key, nonce, aad, data | ct/pt | `AuthFailed` |
| `x25519::dh` | sk, pk | shared | — |
| `sig::verify` | alg, pk, msg, sig | `bool` | `UnsupportedSignature` |
| `libtls::connect` | `ClientConfig`, host, transport | `TlsStream` | `NotTrusted`, `Expired`, `NotYetValid`, `HostnameMismatch`, `BadCertificate`, `UnsupportedVersion`, `NoSharedCipher`, `UnsupportedSignature`, `Alert`, `ClockUnsynced`, `RngUnavailable`, `Io`, `RecordLimit` |

Preconditions: `trust_anchors` non-empty; `rng` reports a vetted source;
`clock.now_unix()` yields a trusted instant; `host` is the DNS name or IP the
caller intends to reach. Postconditions: the stream is authenticated and
encrypted; on any error the transport is closed and no plaintext is returned.

---

## 7. Test plan (≥3 per invariant)

**INV-1 — Primitives match known vectors.**

| Test | Expected |
| --- | --- |
| `sha256_vectors` | NIST/FIPS vectors |
| `hmac_sha256_vectors` | RFC 4231 vectors |
| `hkdf_vectors` | RFC 5869 vectors |
| `aead_roundtrip_and_tamper` | round-trip; bit flip → `AuthFailed` |
| `x25519_rfc7748` | RFC 7748 vectors |
| `rsa_pss_verify` / `ecdsa_p256_verify` | known-certificate vectors |
| `ed25519_rfc8032` | existing NLT vectors |

**INV-2 — Server authentication is enforced.**

| Test | Expected |
| --- | --- |
| `tls_valid_chain_accepted` | fixture chain accepted |
| `tls_untrusted_issuer_rejected` | `NotTrusted` |
| `tls_self_signed_rejected` | `NotTrusted` |
| `tls_hostname_mismatch_rejected` | `HostnameMismatch` |
| `tls_ip_san_accepted` | `10.0.2.2` fixture with iPAddress SAN |
| `tls_expired_rejected` | `Expired` (fixed clock) |
| `tls_not_yet_valid_rejected` | `NotYetValid` |
| `tls_intermediate_chain_accepted` | root → intermediate → leaf |

**INV-3 — Protocol policy.**

| Test | Expected |
| --- | --- |
| `tls_rejects_tls10` | `UnsupportedVersion` |
| `tls13_handshake_fixture` | completes |
| `tls_no_shared_cipher` | `NoSharedCipher` |
| `tls_alpn_http11` | server sees `http/1.1` |
| `tls_sni_sent_for_dns_name` | SNI equals the host |
| `tls_sni_omitted_for_ip` | no SNI with an IP literal |

**INV-4 — Entropy, clock and failure handling.**

| Test | Expected |
| --- | --- |
| `tls_fails_closed_without_rng` | `RngUnavailable` |
| `tls_fails_closed_unsynced_clock` | `ClockUnsynced` |
| `tls_closes_on_error` | transport closed after handshake failure |
| `tls_record_limits` | oversized record → `RecordLimit`, no OOM |
| `tls_chain_size_limit` | oversized chain rejected |

**Integration:** `neocurl https://fixture/` against a local TLS fixture server;
invalid/expired/mismatch cases return the documented error and exit code.

---

## 8. Implementation plan

1. **M7 — time trust (first):** `ob_unix_seconds()`, `Clock` impl, ntpd-status
   policy; tests. Blocks M10.
2. **M8 — Ring-3 CSPRNG:** `\Device\Random`, classes 48/49, DRBG, wrapper;
   tests. Blocks M10 (and gates everything).
3. **M9 — `libcrypto`:** primitives + vectors (INV-1).
4. **D1 spike:** `rustls` no_std + provider build gate (timeboxed).
5. **M10 — `libtls`:** X.509 + chain/hostname/clock validation, trust store,
   record + handshake, SNI, ALPN; tests (INV-2/3/4).
6. **M11 — HTTPS integration:** `Scheme::Https` in `libhttp`, `neocurl https://`,
   fixture matrix, no insecure flag.
7. **Hardening + docs:** policy tests, `docs/security/security.md`, markdownlint.

Dependencies: M7 and M8 gate M10; M9 and the D1 spike gate the record layer. TLS
must not start before the CSPRNG and the clock policy exist.

---

## 9. Open questions

1. **D1:** `rustls` `no_std` + custom provider, or a minimal TLS 1.3 client over
   RustCrypto? (Resolve with the spike; see §3.5.)
2. **D2:** is TLS 1.2 in scope for M10, or is 1.3-only acceptable for the first
   release? (Recommendation: 1.3-only unless `rustls` makes 1.2 free.)
3. **D3:** exact clock-trust rule (State=Synced + non-zero `LastSync` + minimum
   plausible date). Where is it enforced: `libneodos`, `libtls`, or both?
4. **D8:** trust-anchor bundle as an image file (needs NeoDev) or embedded in the
   binary? (Recommendation: image file; embedding couples root rotation to a
   rebuild.)
5. Should `libnlt` Ed25519 move to `libcrypto` (one implementation) or stay
   independent to keep the NLT feature dependency-free?
6. Revocation (OCSP/CRL) is out of scope: confirm it is acceptable with a
   documented gap.

---

*End of design. No code, public API, or issue state is changed by this document.*
