# Testing (NeoDOS Kernel)

## Test Framework

- `test_case!` macro in neodos-kernel/src/testing.rs
- Run: `cargo run --manifest-path tools/neodev/Cargo.toml -- test`
- Groups: ob, syscall, mm, scheduler, vfs, hal, driver, registry, security, ipc, boot
- Individual: `cargo run --manifest-path tools/neodev/Cargo.toml -- test <group>`

## What to Test

- Every syscall (happy path + all error codes)
- Every ObType (create, open, close, query, set)
- Every InfoClass variant
- Edge cases: null handles, max sizes, empty paths, exhaustion
- Concurrent access (same ObHandle from multiple threads)
- Security: verify SeAccessCheck denies unauthorized access

## Test-Driven Development

Mandatory for new features:
1. Write test_case! first (RED)
2. Run test — verify it fails
3. Write minimal kernel implementation (GREEN)
4. Run test — verify it passes
5. Refactor

## Coverage Goals

- 100% of syscalls have at least one test
- 100% of documented error codes are tested
- 100% of ObInfoClass variants are tested
- Edge cases for every public API function
