# Cryptography, secrets, and authentication in Rust

## Golden rule

Do not implement cryptographic primitives. Use established, maintained crates with current
parameters, system RNG, and constant-time comparison. If you doubt a crate, do not use it.

| Need | Recommended crate | Notes |
|---|---|---|
| Password hashing | `argon2` (Argon2id) | Parameters calibrated to the hardware |
| Cryptographic randomness | `rand` with `OsRng`, `getrandom` | Never `thread_rng` for security tokens |
| Constant-time comparison | `subtle` | `ConstantTimeEq` |
| AEAD | `aes-gcm`, `chacha20poly1305` (RustCrypto) | Unique nonce, never reuse |
| Signatures / TLS | `ring`, `aws-lc-rs`, `rustls` | Not `openssl` unless integration requires it |
| JWT/JOSE | `jsonwebtoken` or maintained JOSE | Validate the algorithm explicitly |
| Memory zeroing | `zeroize`, `secrecy` | Reduces, does not guarantee, erasure |
| Key derivation | `hkdf`, `argon2` | Unique context/info per use |

Forbidden: MD5, SHA-1, plain SHA-256 for passwords, ECB, RC4, `rand::random()` for credentials,
embedded keys, `==` comparison of secrets, `unwrap()` in verification.

## Passwords with Argon2id

```rust
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};

fn hasher() -> Argon2<'static> {
    // Calibrate on the target hardware: ~0.5 s and ~64 MiB is a starting point.
    let params = Params::new(64 * 1024, 3, 1, Some(32)).expect("valid parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(password: &str) -> Result<String, DomainError> {
    let salt = SaltString::generate(&mut OsRng);
    hasher()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| DomainError::Internal(e.into()))
}

pub fn verify_password(password: &str, stored: &str) -> Result<bool, DomainError> {
    let parsed = PasswordHash::new(stored).map_err(|e| DomainError::Internal(e.into()))?;
    Ok(hasher().verify_password(password.as_bytes(), &parsed).is_ok())
}
```

Additional rules:

- Unique salt per user; never a global or deterministic salt.
- If parameters change, rehash after a successful login (opportunistic rehash).
- Do not distinguish "user does not exist" from "wrong password": same message, status, and timing.
  If the user does not exist, verify against a dummy hash anyway to equalize cost.
- Always verify with `verify_password`; never compare hash strings.
- Never put the hash in logs, metrics, or traces.

## Tokens and sessions

```rust
use rand::RngCore;

fn new_session_token() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes
}
```

- Minimum 128 bits of entropy; 256 bits recommended for session tokens.
- Store only the token hash on the server (SHA-256 is acceptable here; it is not a password) so a DB
  leak does not allow impersonation.
- Rotate the identifier on authentication and privilege elevation.
- Absolute + idle expiry, individual and global revocation, and real logout.
- Invalidate after a password change, role change, or incident.
- Cookies: `HttpOnly`, `Secure`, `SameSite=Lax` or `Strict`, minimal `Path`, `__Host-` when
  applicable.
- Short-lived access tokens; refresh with rotation and reuse detection (if an already-rotated
  refresh token is reused, revoke the entire family).

### JWT

- Fix the expected algorithm and reject any other, including `none`.
- Validate `exp`, `nbf`, `iss`, `aud`, and `kid` against a known key set.
- In multi-service systems, prefer asymmetric (EdDSA/ES256/RS256) over HMAC with a shared secret.
- Do not put sensitive data in the payload: it is readable, not encrypted.
- Keep keys out of code and rotate them; support several `kid`s during the transition.

```rust
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

let mut validation = Validation::new(Algorithm::EdDSA);
validation.set_audience(&["api"]);
validation.set_issuer(&["https://auth.example"]);

let data = decode::<Claims>(&token, &decoding_key, &validation)
    .map_err(|_| DomainError::Forbidden)?;
```

## Constant-time comparison

```rust
use subtle::ConstantTimeEq;

pub fn secret_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}
```

Apply to session tokens, API keys, HMAC signatures, recovery codes, and CSRF. Do not use `==`.

## Secrets in configuration

- `secrecy::SecretString` in the configuration struct.
- Load from environment variables, a `0600` file, or a secret manager.
- `.env` out of git; `.env.example` without real values.
- Never in `Debug`, `Display`, `Serialize`, logs, URLs, or error messages.
- Validate at startup: if a required secret is missing, fail closed (do not start with an insecure
  default).
- Documented and tested rotation: the application must accept the new key before the old one is
  retired.

## Authorization

- Identity first, then permissions with an explicit policy and default deny.
- The UI hides; the server decides. Every endpoint declares the permission it requires.
- Check tenant membership on the server for **every** object, not only in listings.
- Use typed extractors that already validate session + tenant + permission; avoid repeating the
  logic and forgetting it in a new handler.
- Tests per cell of the role × permission matrix, including denied access and another tenant's
  resource.

## Brute force and enumeration

- Rate limiting per account, IP, and combination, with progressive backoff.
- Temporary lockout without allowing a permanent DoS against a victim; alerting and safe recovery.
- Uniform responses and timings on login, registration, recovery, and invitations.
- MFA/step-up on sensitive operations; the model must support multiple factors and passkeys even if
  the implementation comes later.

## Crypto review checklist

- [ ] No hand-rolled primitives or obsolete modes.
- [ ] Argon2id with calibrated parameters and rehash.
- [ ] System RNG for every token.
- [ ] Constant-time secret comparisons.
- [ ] `HttpOnly` + `Secure` + `SameSite` cookies.
- [ ] Revocable sessions and rotation on login/step-up.
- [ ] JWT with a fixed algorithm and validated claims.
- [ ] Secrets out of code/logs and documented rotation.
- [ ] Tests for denied access, enumeration, and refresh reuse.
