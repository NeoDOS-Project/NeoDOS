# Memory, `unsafe`, and sensitive types

## `unsafe` policy

By default, in every crate:

```rust
// lib.rs or main.rs
#![forbid(unsafe_code)]
```

`forbid` also blocks a local `#[allow(unsafe_code)]`. If a crate genuinely needs `unsafe`, downgrade
to `#![deny(unsafe_code)]` and document every exception.

Every `unsafe` block requires:

```rust
// SAFETY: `ptr` comes from `Vec::as_ptr()` and the Vec is not reallocated while the slice lives;
// the `&self` borrow guarantees exclusivity and the length was checked on line 42.
let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
```

Additional requirements:

- Invariants written in the comment: what must hold, who guarantees it, and what happens if not.
- A test covering the safe path and, where possible, Miri (`cargo +nightly miri test`) or sanitizers
  in CI for the affected module.
- A safe alternative rejected for a measurable reason (performance, FFI interoperability).
- Explicit review by another person or, failing that, a dedicated PR section.
- `unsafe` audit of dependencies: `cargo geiger`, `cargo tree`, crate review.

Never use `unsafe` to:

- Avoid a minor `clone` without measurement.
- Bypass the borrow checker for convenience.
- Implement cryptography, parsing of untrusted formats, or custom serialization.
- Convert FFI pointers without validating nullability, alignment, size, and lifetime.

## FFI

- Validate null pointers and alignment before dereferencing.
- Clearly define memory ownership on both sides (`Box::from_raw` exactly once).
- Use `#[repr(C)]` and fixed-size types.
- Wrap the API in a safe module with a minimal surface and tests for double free / use after free.

## Strong types for sensitive data

```rust
use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TenantId(uuid::Uuid);

impl TenantId {
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        uuid::Uuid::parse_str(raw)
            .map(Self)
            .map_err(|_| DomainError::InvalidInput { field: "tenant_id" })
    }
}

impl fmt::Debug for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TenantId(***)")
    }
}
```

- Do not derive `Serialize`/`Deserialize` on types wrapping secrets. If needed, implement explicit
  serialization only at the boundary.
- Implement `TryFrom<&str>`/`FromStr` with canonical validation (trim, lowercase, Unicode
  normalization) before comparing.
- For permissions, use typed enums or bitsets, not bare `String` or `u32`.

## Secrets in memory

```rust
use secrecy::{ExposeSecret, SecretString};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct RawKey([u8; 32]);

pub struct Config {
    api_key: SecretString,
}

fn use_key(cfg: &Config) {
    let key = cfg.api_key.expose_secret(); // explicit, scoped exposure
    // ...
}
```

- `SecretString`/`SecretBox` redact `Debug` and `Display`.
- Avoid copying secrets into `String`, `Vec<u8>`, or logs. If unavoidable, `zeroize` afterwards.
- Do not derive `Clone` on types holding sensitive material unless truly needed; every copy is
  another leak surface.
- Compare tokens and MACs with `subtle::ConstantTimeEq` (or
  `ring::constant_time::verify_slices_are_equal`), never with `==` on bytes.
- The compiler may optimize intermediate copies; `zeroize` reduces risk, it does not eliminate it.
  Do not promise guaranteed RAM erasure.

## Constant-time comparison

```rust
use subtle::ConstantTimeEq;

fn tokens_equal(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}
```

- Always evaluate compatible lengths; `ct_eq` returns `false` when they differ, but avoid early
  branches in business logic.
- Apply to session tokens, API keys, HMAC signatures, and challenge responses.

## Memory and arithmetic lints

```toml
[profile.release]
overflow-checks = true   # in services; measure the cost on hot paths

[lints.clippy]
indexing_slicing = "warn"
arithmetic_side_effects = "warn"
cast_possible_truncation = "warn"
cast_sign_loss = "warn"
cast_possible_wrap = "warn"
```

- Prefer `u32::try_from(n)` over `n as u32`.
- Use `checked_*`, `saturating_*`, or `wrapping_*` explicitly according to the intended semantics.
- When parsing lengths and offsets, validate `checked_add`/`checked_mul` before allocating.

## Cancellation and `Drop`

- A cancelled future does not run the rest of its body: if it mutates state, cancellation can leave
  invariants half-applied. Document cancellation safety or use `tokio::select!` with explicit
  cleanup (a guard that reverts, `CancellationToken`, and `defer`).
- Do not rely on `Drop` for asynchronous operations; `Drop` is synchronous.
- `std` locks are poisoned on panic; decide consciously whether to propagate the panic or recover
  the guard (`PoisonError::into_inner`).
