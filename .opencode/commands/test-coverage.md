---
description: Analyze NeoDOS kernel test coverage and generate missing test_case! entries for uncovered syscalls, Ob types, error paths, and edge cases.
---

# Test Coverage for NeoDOS

Analyze test coverage in the NeoDOS kernel:

1. Review existing tests: search for `test_case!` in neodos-kernel/src/testing.rs

2. Cross-reference with kernel API surface:
   - Check each syscall in docs/syscalls.md — is there a test?
   - Check each ObType in src/object/types.rs — is there a create/close test?
   - Check each InfoClass variant — is query/set tested?

3. For each uncovered area, generate test_case! entries:
   - Happy path (normal operation)
   - Error path (invalid params, missing permissions)
   - Edge cases (null handles, max sizes, empty names)
   - Concurrent access scenarios

4. Verify all tests pass: `cargo run --manifest-path tools/neodev/Cargo.toml -- test`
