# TLS / HTTPS (`libcrypto` + `libtls`) — Design Document

> **Version:** v0.1-draft
> **Status:** Design (no implementation)
> **Target Release:** v0.61–v0.62 — Security Hardening
> **Related:** #89 (NXE signature verification), #57 (module signing), #361 (NTP
> auth), #582 (NLT signatures), #411; `docs/design/http-tls-update-prerequisites-design.md`,
> `docs/design/libhttp-neocurl-design.md`
> **ABI Impact:** none directly; depends on `ObInfoClass::Random` (prerequisite)

---

## 1. Research

| Source | What it establishes |
| --- | --- |
| `libnlt/src/signature.rs`, `libnlt/Cargo.toml` | the **only** crypto in the repo: Ed25519 sign/verify via `ed25519-compact` (feature `signatures`, locked 2.6.0), RFC 8032 test vectors |
| `libneodos/src/i18n.rs` | `i18n_verify_signature`, `NLT_DEV_PUBLIC_KEY`, `REQUIRE_SIGNED`, features `i18n-signatures`/`i18n-require-signed` |
| `neodos-kernel/src/hal/mod.rs`, `hal/raw/cpu.rs` | `rdrand` (kernel-only); no RDSEED; used for ASLR only |
| `userbin/cpuinfo/src/main.rs` | reports AES/RDRAND CPUID bits (no use) |
| `libnlt/src/lzss.rs` | the only compression (LZSS) — **no deflate/gzip** |
| all `Cargo.toml` / `Cargo.lock` | **no Cargo workspace**; ~70 independent crates; the sole external userland dependency is optional `ed25519-compact`; target `x86_64-unknown-none`, `no_std`, nightly |
| `docs/architecture/package-manager-arch.md`, `docs/userland/nxe-ecosystem.md` | Ed25519 signing is *designed* for packages/NXE but **not implemented** |
| repo-wide grep | **no TLS/SSL, no SHA-256, no HMAC, no AEAD, no X25519, no RSA, no CSPRNG syscall** |

Verified: Ed25519 verify is real and tested (NLT only); everything a TLS stack
needs beyond Ed25519 is absent, including a Ring-3 entropy source.

---

## 2. Problem analysis

HTTPS requires, at minimum:

1. **A byte-stream transport** (provided by `libhttp`/the TCP fix).
2. **Cryptographic primitives**: a hash (SHA-256), a MAC (HMAC-SHA256), an AEAD
   (AES-GCM or ChaCha20-Poly1305), a key-agreement (X25519 and/or ECDHE), and
   Ed25519/RSA for certificate signatures.
3. **A CSPRNG** for nonces and ephemeral keys.
4. **Certificate parsing** (X.509 DER), **chain validation**, **hostname
   validation**, and **trust-anchor storage**.
5. **A correct clock**, because certificate validity is time-bounded.

None of 2–5 exist. Why the existing abstractions cannot solve it:

- **Ed25519 (`libnlt`)** is a single signature primitive, scoped to NLT, and
  does not provide hashing, AEAD, or key agreement.
- **`rdrand`** is kernel-internal with no object boundary.
- There is **no certificate/PKI code and no trust store**.
- The **clock** can be wrong pre-NTP; TLS validity must fail closed.

Implementing crypto from scratch is explicitly forbidden by the task and is
unsafe. The design must choose a maintained implementation compatible with the
`no_std` target and the repo's dependency posture.

---

## 3. Solution design

Two new userland crates plus one kernel capability (already designed in the
prerequisites doc).

### 3.1 Types / structs / enums

```rust
// libcrypto/src/lib.rs  (plain no_std crate)
pub mod sha256;        // digest
pub mod hmac;          // HMAC-SHA256
pub mod aead;          // AesGcm128/256 or ChaCha20Poly1305
pub mod x25519;        // ECDHE key agreement
pub mod ed25519;       // verify (reuse ed25519-compact), sign (host tools only)
pub mod rng;           // Rng trait backed by \Device\Random

pub trait Rng { fn fill(&self, out: &mut [u8]) -> Result<(), CryptoError>; }

// libtls/src/lib.rs  (plain no_std crate)
pub enum TlsVersion { Tls12, Tls13 }
pub struct TrustStore { /* DER roots */ }
pub struct ClientConfig {
    pub trust_anchors: TrustStore,
    pub min_version: TlsVersion,
    pub alpn: &'static [&'static str],      // e.g. ["http/1.1"]
    pub rng: &'static dyn Rng,
    pub now: Instant,                        // wall clock (validity checks)
}
pub enum TlsError {
    NotTrusted, Expired, NotYetValid, HostnameMismatch, BadCertificate,
    UnsupportedVersion, NoSharedCipher, Alert(u8), Io, RngUnavailable,
}
pub struct TlsStream<C> { /* wraps C: Read+Write+Close */ }
pub fn connect<C>(cfg: &ClientConfig, host: &str, c: C) -> Result<TlsStream<C>, TlsError>;
```

### 3.2 New syscalls / ObType / classes

**None new here.** TLS consumes the prerequisite `\Device\Random`
(`ObInfoClass::Random=44`, `RandomCaps=45`) and the `libhttp` byte-stream
transport. No new `ObType`, no new class in this feature.

### 3.3 New files / modules

```text
libcrypto/Cargo.toml
libcrypto/src/{lib,sha256,hmac,aead,x25519,ed25519,rng,error}.rs
libcrypto/src/tests.rs
libtls/Cargo.toml
libtls/src/{lib,record,handshake,x509,verify,trust,error}.rs
libtls/src/tests.rs
C:\System\Certs\roots.pem        (trust anchors, image content)
```

No new kernel module (the kernel side is the `\Device\Random` device from the
prerequisites doc).

### 3.4 Changes to existing files

| Path | Change |
| --- | --- |
| `libhttp` | Allow an `Https` scheme to be served by a TLS-wrapping transport |
| `libnet` | No change (transport stays TLS-agnostic) |
| `libnlt` | Optionally re-point Ed25519 at `libcrypto` to avoid two copies |
| `neodev/src/image.rs` | Ship `C:\System\Certs\roots.pem` |
| `docs/security/security.md` | Document the trust store and verification policy |

### 3.5 Dependency decision (the crux)

The target is `x86_64-unknown-none`, `no_std`, nightly, no C toolchain, `rust-lld`.
Options evaluated:

| Option | Verdict |
| --- | --- |
| **Pure-Rust RustCrypto stack** (`sha2`, `hmac`, `aes-gcm`/`chacha20poly1305`, `x25519-dalek`, `ed25519-compact`) wired into a minimal TLS record/handshake layer | **Recommended for evaluation**: `no_std`-capable, pure Rust, permissively licensed; precedent exists (`ed25519-compact`) |
| `rustls` with a custom `CryptoProvider` built from the above | Possible; `rustls` is `no_std`-capable but pulls a larger API surface; provider must avoid `ring`/`aws-lc` (need C + OS randomness) |
| Vendored C (`mbedTLS`, `BearSSL`) | Rejected: C toolchain, `no_std` friction, larger attack surface to vendor |
| Custom TLS | **Rejected**: forbidden and unsafe |

This decision must be ratified before implementation; it is recorded as an open
question (§9).

### 3.6 Verification policy

- **Server authentication is mandatory.** `connect` always validates the chain
  and hostname. The public API exposes **no** "insecure" flag.
- A separate, clearly named test-only type may accept a pinned test root; it is
  not importable by `libupdate`/`neoupdate`.
- **Clock dependency:** if `now` is before any plausible issuance (unsynced
  clock) the handshake fails closed (`Expired`/`NotYetValid`).
- **Trust anchors:** read-only, provisioned at image build, replaceable only via
  a signed update; no runtime "install any root".
- **TLS 1.2 and 1.3**, modern AEAD cipher suites only; TLS 1.0/1.1 disabled.

---

## 4. Alternatives

- **Use TLS only for `libupdate` via a pinned certificate.** Rejected as the
  general design: it is not a general HTTPS capability and still needs all the
  primitives.
- **Skip TLS; rely on artifact signatures over HTTP.** Rejected: transport
  encryption and server authentication are separate guarantees (see the master
  design §8); metadata authenticity does not protect against transport tampering
  and traffic analysis.
- **Reuse the kernel RDRAND directly from userland.** Rejected: no object
  boundary, unwhitened, no reseed; the prerequisites doc designs `\Device\Random`.
- **Vendor a C TLS library.** Rejected (toolchain/target).
- **Custom crypto.** Rejected (explicitly forbidden).

---

## 5. Affected components

| Subsystem | Change | Impact |
| --- | --- | --- |
| Kernel | `\Device\Random` (prerequisite doc) | Low |
| `libcrypto` (new) | primitives + RNG boundary | New crate |
| `libtls` (new) | record/handshake/X.509/verify | New crate |
| `libhttp` | HTTPS transport wrapping | Low |
| `libupdate` | uses `libtls` (later) | Low |
| Image | `roots.pem` | Low |
| `libnlt` | optional Ed25519 de-duplication | Low |
| Scheduler/VFS/memory/drivers | None | None |

---

## 6. API contract

| Function | Args | Returns | Errors |
| --- | --- | --- | --- |
| `Rng::fill` | `&mut [u8]` | `()` | `RngUnavailable` |
| `sha256::digest` | `&[u8]` | `[u8;32]` | — |
| `hmac::mac` | key, data | tag | — |
| `aead::seal/open` | key, nonce, aad, data | ct/pt | `CryptoError::AuthFailed` |
| `x25519::dh` | sk, pk | shared | — |
| `ed25519::verify` | pk, msg, sig | bool | — |
| `libtls::connect` | `ClientConfig`, host, transport | `TlsStream` | `NotTrusted`, `Expired`, `NotYetValid`, `HostnameMismatch`, `BadCertificate`, `UnsupportedVersion`, `NoSharedCipher`, `Alert`, `Io`, `RngUnavailable` |

Preconditions: `ClientConfig.rng` is a CSPRNG (checked via `RandomCaps`);
`trust_anchors` non-empty; `host` matches the certificate. Postconditions: the
stream is authenticated and encrypted; on any error the transport is closed and
no plaintext is returned.

---

## 7. Test plan (≥3 per invariant)

**INV-1 — Primitives match known vectors.**

| Test | Expected |
| --- | --- |
| `sha256_vectors` | NIST/FIPS vectors match |
| `hmac_sha256_vectors` | RFC 4231 vectors match |
| `aead_roundtrip_and_tamper` | seal/open round-trips; bit-flip → `AuthFailed` |
| `x25519_rfc7748` | RFC 7748 vectors match |
| `ed25519_rfc8032` | existing vectors still pass |

**INV-2 — Server authentication is enforced.**

| Test | Expected |
| --- | --- |
| `tls_valid_chain_accepted` | fixture chain accepted |
| `tls_untrusted_issuer_rejected` | `NotTrusted` |
| `tls_hostname_mismatch_rejected` | `HostnameMismatch` |
| `tls_expired_rejected` | `Expired` (fixture clock) |
| `tls_not_yet_valid_rejected` | `NotYetValid` (unsynced clock) |

**INV-3 — Protocol policy.**

| Test | Expected |
| --- | --- |
| `tls_rejects_tls10` | `UnsupportedVersion` |
| `tls13_handshake_fixture` | completes with TLS 1.3 |
| `tls_no_shared_cipher` | `NoSharedCipher` |

**INV-4 — Entropy and failure handling.**

| Test | Expected |
| --- | --- |
| `tls_fails_closed_without_rng` | `RngUnavailable` when caps report no CSPRNG |
| `tls_closes_on_error` | transport closed after a handshake failure |
| `tls_record_limits` | oversized record → `Alert`/`Io`, no OOM |

**Integration:** `neocurl https://fixture/` against a local TLS fixture server.

---

## 8. Implementation plan

1. **Ratify the dependency decision** (§3.5) and vendor/wire the crates.
2. **`libcrypto`** primitives + tests (INV-1).
3. **`\Device\Random` + `RandomCaps`** (prerequisite doc) + `libcrypto::rng`.
4. **X.509 parse + chain/hostname validation + trust store** + tests (INV-2).
5. **TLS record layer + handshake (1.3, then 1.2)** + tests (INV-3).
6. **`libhttp` HTTPS integration** and `neocurl https://`.
7. **Policy hardening**: no insecure flag; fail closed on clock.
8. **Docs + tests + markdownlint.**

Dependencies: steps 1–3 gate everything; TLS must not start before the CSPRNG
exists.

---

## 9. Open questions

1. Exact crate set and licenses (the external-crate policy).
2. TLS 1.2 support in M1 or TLS 1.3 only?
3. Trust-anchor format (PEM vs DER) and the update path for the bundle.
4. Where certificate validation errors surface in the HTTP layer.
5. Whether to re-point `libnlt` Ed25519 at `libcrypto`.

---

*End of design. No code, public API, or issue state is changed by this document.*
