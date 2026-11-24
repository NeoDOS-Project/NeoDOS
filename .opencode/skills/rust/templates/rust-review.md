# Rust review template

Use this template for PR reviews, audits, or hardening tasks. Keep it evidence-based: cite
`file:line`, explain the risk, and propose the smallest fix that removes it.

## 1. Context

- Crate / binary / service:
- Layer and exposure (internal CLI, library, public API, multi-tenant service):
- Trust boundaries (network, user input, third parties, files, queues):
- Sensitive data involved (credentials, tokens, PII, keys):
- Async runtime and web framework, if any:

## 2. Mechanical scan

Run and record what you find before reading deeply:

```bash
rg -n 'unwrap\(|expect\(|panic!|unreachable!|todo!|unimplemented!' --type rust
rg -n 'unsafe\s*\{' --type rust
rg -n 'as\s+(u8|u16|u32|u64|usize|i8|i16|i32|i64|isize)\b' --type rust
rg -n 'std::fs::|std::thread::sleep|\.block_on\(|block_in_place' --type rust
rg -n 'Mutex|RwLock|static\s+mut|RefCell|Cell<' --type rust
rg -n 'format!\(.*(SELECT|INSERT|UPDATE|DELETE|WHERE)' --type rust
rg -n 'Command::new|sh\s+-c|eval' --type rust
rg -n '(password|token|secret|key).*(trace|debug|info|warn|error)!' --type rust
```

Then run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
cargo deny check
cargo audit
```

## 3. Findings

For each finding:

| Field | Content |
|---|---|
| Severity | Critical / High / Medium / Low / Info |
| Location | `path:line` |
| Category | Errors / Unsafe / Memory / Concurrency / Crypto / Auth / Web / Dependencies / Other |
| Evidence | Minimal code snippet or command output |
| Risk | Concrete impact and preconditions |
| Fix | Smallest change that removes the risk |
| Test | Test that proves the fix (including the negative case) |
| Status | Open / Accepted risk (with owner and expiry) |

Severity guide:

- **Critical**: remote code execution, auth bypass, secret exposure, memory unsafety reachable from
  untrusted input.
- **High**: privilege escalation, cross-tenant access, SQL injection, SSRF to internal networks,
  weak password hashing, panic reachable from a request.
- **Medium**: missing rate limiting, weak headers, verbose errors, unbounded channels, blocking in
  async under load.
- **Low**: lint gaps, missing negative test, style/consistency with security impact.
- **Info**: observation without current impact.

## 4. Area checklist

### Errors

- [ ] Public APIs return `Result`; `?` used with context.
- [ ] Typed error per layer; boundary maps to stable codes without internal detail.
- [ ] No `unwrap`/`expect`/`panic` reachable in production.
- [ ] No ignored errors (`let _ =`) without justification.
- [ ] Negative tests for each error variant.

### Memory and `unsafe`

- [ ] `#![forbid(unsafe_code)]` unless documented otherwise.
- [ ] Every `unsafe` block has `// SAFETY:` with invariants and a rejected alternative.
- [ ] No mutable aliasing, dangling pointers, or unchecked FFI.
- [ ] Sensitive types have redacted `Debug` and controlled serialization.
- [ ] `zeroize`/`secrecy` used where risk warrants it.

### Async and concurrency

- [ ] No blocking I/O or CPU in async without `spawn_blocking`.
- [ ] No lock held across `await`.
- [ ] Bounded channels and explicit backpressure.
- [ ] Timeouts on all external I/O; retries bounded and idempotent.
- [ ] Cancellation safety documented for state-mutating futures.
- [ ] Graceful shutdown drains work.

### Crypto, secrets, auth

- [ ] Argon2id with calibrated parameters and rehash.
- [ ] CSPRNG for tokens; constant-time comparisons.
- [ ] Cookies `HttpOnly`/`Secure`/`SameSite`; sessions revocable and rotated.
- [ ] JWT algorithm fixed and all claims validated.
- [ ] Secrets outside code/logs with documented rotation.
- [ ] Authorization server-side, default deny, tenant check per object.
- [ ] Tests for enumeration, brute force, refresh reuse.

### Web

- [ ] Body/time/size limits per route.
- [ ] Security headers, TLS/HSTS, explicit CORS.
- [ ] CSRF when cookies are used.
- [ ] SSRF allowlist and redirect policy.
- [ ] Parameterized SQL and tenant filtering.
- [ ] Rate limiting on sensitive surfaces.
- [ ] Logs redacted; audit trail for sensitive actions.

### Dependencies and CI

- [ ] Lockfile committed; `rust-version` declared.
- [ ] `cargo deny` and `cargo audit` in CI.
- [ ] Minimal features; `default-features = false` where it helps.
- [ ] New dependencies reviewed (maintenance, license, `unsafe`, advisories).
- [ ] Clippy/fmt enforced; no blanket allows.

## 5. Summary

- Total findings by severity:
- Blocking issues (must fix before merge):
- Accepted risks (owner, reason, expiry):
- Follow-ups (out of scope for this change):
- Security review performed and by whom:
