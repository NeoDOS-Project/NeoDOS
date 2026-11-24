---
name: rust
description: Apply a rigorous Rust engineering and security baseline when writing, reviewing or refactoring Rust code, Cargo projects, error handling, unsafe code, async/Tokio services, cryptography, authentication, dependencies and CI. Use for any Rust task: implementing features, code review, security audits, dependency selection, Clippy configuration or hardening.
---

# Rust: engineering and security standards

This skill is a self-contained reference for writing and reviewing Rust code with a strong
engineering and security baseline. It defines its own criteria and does not derive from or defer to
any external document.

It applies to any Rust task: writing features, reviewing PRs, auditing a crate, selecting
dependencies, configuring lints/CI, or hardening a service.

## Non-negotiable rules

1. **Explicit errors.** Production APIs and routes return `Result` and propagate with `?`.
   Typed errors per layer (`thiserror`), translated at the boundary without leaking internal
   details. No `let _ =` unless the discard is intentional and justified.
2. **No `unwrap()`/`expect()` in production.** Only in tests, examples, build scripts, or at
   startup where the failure is unrecoverable and documented. Also no `panic!`, direct indexing,
   or arithmetic that can overflow in request-serving code.
3. **`unsafe` forbidden by default.** `#![forbid(unsafe_code)]` at the crate root. Any block
   requires `// SAFETY:`, tests, explicit review, and a safe alternative rejected for a measurable
   reason.
4. **Strong types for sensitive data.** Newtypes for IDs, tenants, tokens, keys, and permissions.
   Redacted `Debug`, no accidental `Serialize`, constant-time comparison, and `zeroize` when the
   risk warrants it.
5. **Safe concurrency.** No shared mutable state without synchronization. Bounded channels,
   concurrency limits, and backpressure. Never hold a lock across an `await`. Document the
   cancellation safety of futures that mutate state.
6. **Reviewed dependencies.** Minimal, maintained, with a lockfile, `cargo deny` + `cargo audit` in
   CI, allowed licenses, and audited `unsafe`. `default-features = false` when it reduces surface.
7. **Cryptography from established libraries.** Never hand-roll primitives. RustCrypto, `ring`,
   `aws-lc-rs`, `argon2`, maintained JOSE. Current parameters, system RNG, and constant-time
   comparison. No obsolete modes or embedded keys.
8. **Never block the async runtime.** File I/O, heavy CPU, or blocking calls go to
   `spawn_blocking` or a dedicated runtime. External operations get timeouts, bounded size, and
   cancellation. Expensive work is limited and measured.

## Workflow

### When writing or changing code

1. Identify the layer: library, binary, HTTP handler, worker, CLI. Each has a different error
   strategy and panic budget.
2. Define error types per layer before implementing the logic. Convert at the boundary.
3. Model external data with types that cannot represent invalid states (newtypes, enums,
   `NonZero*`, `TryFrom`), and validate at the edge.
4. Implement the happy path and, in the same change, the negative scenarios: invalid input,
   permission denied, another tenant's resource, limit exceeded, timeout, cancellation.
5. Add lints and tests that prevent regression (see `references/dependencies-ci.md`).
6. Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
   `cargo test --all-features`, `cargo deny check`, and `cargo audit`.
7. Review the threat delta if you touch auth, cryptography, secrets, personal data, or networking.

### When reviewing Rust code

Follow `templates/rust-review.md`. Find risk patterns first with `grep`/`rg`, then read the context
before reporting. Do not propose large rewrites without justifying the risk they remove. Classify
each finding by severity and cite `file:line`.

Minimum patterns to search for:

```text
unwrap\(|expect\(|panic!|unreachable!|todo!|unimplemented!
unsafe\s*\{
as\s+(u8|u16|u32|u64|usize|i8|i16|i32|i64|isize)\b
std::fs::|std::thread::sleep|\.block_on\(|block_in_place
Mutex|RwLock|static\s+mut|RefCell|Cell<
format!\(.*(SELECT|INSERT|UPDATE|DELETE|WHERE)
Command::new|sh\s+-c|eval
println!|dbg!|eprintln!|tracing::.*(password|token|secret|key)
```

## Checklist by area

| Area | Minimum check | Reference |
|---|---|---|
| Errors | `Result` in APIs, `?` with context, typed error, boundary without leaks | `references/error-handling.md` |
| Memory and `unsafe` | `forbid(unsafe_code)`, `// SAFETY:`, no mutable aliasing, `zeroize` | `references/unsafe-memory.md` |
| Async and concurrency | No blocking in async, locks outside `await`, bounded channels, timeouts | `references/async-concurrency.md` |
| Crypto, secrets, auth | Argon2id, CSPRNG, constant-time comparison, secure cookies, revocable sessions | `references/crypto-auth.md` |
| Web services | Body limits, timeouts, headers, CSRF, SSRF, parameterized SQL, multi-tenant | `references/web-services.md` |
| Dependencies and CI | Lockfile, `cargo deny`, `cargo audit`, lints, MSRV, minimal features | `references/dependencies-ci.md` |

## Proportionality rule

The standard applies according to real risk. An internal CLI with no network does not need the
same ceremony as a multi-tenant service exposed to the Internet. What is **not** optional in any
case: explicit `Result`, no panics in production, justified `unsafe`, secrets out of code, and
audited dependencies. Every exception is documented with its reason, the assumed risk, and a
removal plan.
