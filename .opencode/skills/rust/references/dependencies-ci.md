# Dependencies, lints, and CI

## Cargo hygiene

- Commit `Cargo.lock` for binaries and services; for libraries, follow the project's policy but keep
  reproducible CI.
- Pin production dependencies to compatible ranges and rely on the lockfile; no floating `*`.
- Declare `rust-version` (MSRV) and test it in CI.
- `default-features = false` when it reduces surface; enable only the features actually used.
- Workspace-level dependency versions and shared lints; avoid divergent versions of the same crate.
- Remove unused dependencies (`cargo machete`, `cargo udeps` on nightly).
- Review before adding: maintenance, authorship, license, vulnerability history, feature surface,
  and `unsafe` usage.

```toml
[package]
rust-version = "1.8x"

[dependencies]
serde = { version = "1", default-features = false, features = ["derive"] }
```

## Lint configuration

```toml
[lints.rust]
unsafe_code = "forbid"
missing_debug_implementations = "warn"
unreachable_pub = "warn"

[lints.clippy]
all = "deny"
pedantic = "warn"
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "deny"
unimplemented = "deny"
await_holding_lock = "deny"
indexing_slicing = "warn"
arithmetic_side_effects = "warn"
cast_possible_truncation = "warn"
cast_sign_loss = "warn"
```

Notes:

- `unwrap_used`, `expect_used`, `panic`, and `indexing_slicing` are restriction lints, allowed by
  default; enabling them is a deliberate policy decision.
- Tests need relief; do it per test module instead of weakening the whole crate:

```rust
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    // ...
}
```

- Do not blanket-allow lints to make CI green. Fix the code or document the exception at the
  narrowest scope.
- Keep `cargo fmt` enforced; no style debates in review.

## `cargo deny`

```toml
# deny.toml
[advisories]
yanked = "deny"
ignore = [] # only with a documented reason and expiry

[licenses]
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0"]
confidence-threshold = 0.9

[bans]
multiple-versions = "warn"
wildcards = "deny"
deny = [
    # crates with no maintenance, known issues, or incompatible licenses
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

- `cargo audit` (RustSec) for vulnerabilities; `cargo deny check advisories` also covers them.
- Run both: `cargo deny` for policy (licenses, sources, bans) and `cargo audit` for the advisory DB.
- Any `ignore` entry needs an owner, a justification, and a review date.
- Audit `unsafe` in new dependencies: `cargo geiger`, `cargo tree -i <crate>`, and a quick read of
  the unsafe code.

## CI pipeline

```yaml
# Illustrative; adapt to the project's CI system.
steps:
  - cargo fmt --all -- --check
  - cargo clippy --all-targets --all-features -- -D warnings
  - cargo test --all-features --locked
  - cargo test --doc --all-features --locked
  - cargo deny check
  - cargo audit
  - cargo build --release --locked
```

Optional, by risk:

- `cargo +nightly miri test` for crates with `unsafe`.
- `cargo llvm-cov` for coverage thresholds on critical modules.
- `cargo hack --feature-powerset` to catch feature-combination breakage.
- `cargo semver-checks` for public libraries.
- Fuzzing (`cargo fuzz`) for parsers and protocol handling.
- `cargo geiger` report reviewed on dependency changes.

## Supply chain

- CI uses pinned toolchain (`rust-toolchain.toml`) and pinned action versions.
- Build artifacts reproducible where feasible; sign releases if distributed.
- Do not run `curl | sh` in CI; download and verify checksums.
- Secrets in CI come from the secret store, never from repository variables echoed in logs.
- Dependency updates: automated PRs (Dependabot/Renovate) + review; security updates prioritized.

## Test expectations

- Unit tests for domain logic and error mapping.
- Integration tests for adapters with realistic failures (timeout, 5xx, malformed payload).
- Negative/security tests for every control: unauthorized, expired token, other tenant, size limit,
  malformed input, replay, double use.
- Concurrency tests where shared state exists.
- Property-based tests (`proptest`) for parsers, canonicalization, and arithmetic.
