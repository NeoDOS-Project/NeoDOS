---
name: testing
description: Write kernel tests with the test_case! harness, debug failures
---

# Testing

## When to use

Writing new kernel tests, debugging a failing test, or extending the in-kernel
test harness.

## Goal

Add reliable in-kernel tests and integrate them with the existing harness.

## Overview

The test framework is compiled into the kernel — there is no external runner.
Global registration lives in `neodos-kernel/src/testing.rs`; each subsystem
exports a `register_*_tests()` function that registers its cases. `neodev test`
boots QEMU headless and parses PASS/FAIL counts from the serial console. See
`docs/development/testing.md`.

## Steps

1. **Write the test** with the harness macros (in the relevant module):

   ```rust
   test_case!("my_feature", {
       // arrange / act / assert
       test_eq!(a, b);
       test_ne!(a, b);
       test_true!(cond);
       test_fail!("message");
   });
   ```

   API: `test_case!(name, { body })` registers a named test (clean completion =
   PASS; an assertion returns `Err` = FAIL); `test_eq!`, `test_ne!`, `test_true!`
   assert; `test_fail!` always returns `Err` (for error paths).

2. **Export a registration function** from that module, one `test_case!` per
   test:

   ```rust
   pub fn register_my_tests() {
       test_case!("my_feature_ok", { /* ... */ });
       test_case!("my_feature_rejects_bad_handle", { /* ... */ });
   }
   ```

3. **Register it centrally** — add `register_my_tests();` to
   `testing::register_tests()` in `neodos-kernel/src/testing.rs`.

4. **Cover the right patterns**: success path, error path (invalid handles,
   null/bad pointers, out-of-range enums → the expected `Status` /
   `SyscallError`), stress (repeated create/destroy, allocation pressure),
   and concurrency for SMP-sensitive code.

5. **Build and run**

   ```bash
   cd neodos-kernel && cargo build
   neodev test
   ```

   The run ends with a summary (`TESTS: X total, Y passed, Z failed`);
   `neodev test` exits 0 only when there are 0 failures.

6. **Debug a failure**: read the assertion message in the serial output; run a
   subset in QEMU with the built-in `test <suite>` command; use the kernel
   logging facility rather than `printk`.

## Best practices

- One behavior per test; use descriptive names
  (`test_create_and_query_event`, `test_destroy_invalid_handle`).
- Clean up every resource (objects, handles, frames, memory) before the test ends.
- Keep tests independent — no ordering dependencies, no shared mutable state.
- Tests run at PASSIVE_LEVEL with interrupts disabled (`hal::without_interrupts`).
- For FS-mutating tests remember the disk image is shared across suites; rebuild
  the image before measuring (`neodev build --quick --image && neodev test`).

## Common mistakes

- Using `assert!` / `panic!` directly — a kernel panic kills the machine. Use the
  harness macros.
- Adding a `test_case!` without wiring `register_*_tests()` into
  `testing::register_tests()`.
- Testing only the happy path.
- Leaving handles, frames, or memory allocated (leaks that break later suites).
- Depending on global state left by a previous test.

## Final checklist

- [ ] Tests use `test_case!` and the harness assertions
- [ ] `register_*_tests()` exported and called from `testing::register_tests()`
- [ ] Success and error paths covered
- [ ] No resource leaks
- [ ] No ordering dependencies
- [ ] `cargo build` succeeds; `neodev test` passes with 0 failures
