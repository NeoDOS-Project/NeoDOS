# Testing

## When to use
Writing new kernel tests, debugging a test failure, or modifying the test framework.

## Goal
Add reliable kernel tests that exercise the target subsystem and integrate with the existing test runner.

## Steps

1. **Locate the test file**
   All kernel tests live in `neodos-kernel/src/testing.rs`.
   Find the relevant test group (e.g., `mod scheduler_tests`, `mod ob_tests`).

2. **Add a test function**
   ```rust
   pub fn test_my_feature() -> TestResult {
       // Arrange
       // Act
       // Assert
       TestResult::Passed  // or TestResult::Failed("reason")
   }
   ```
   `TestResult` is the return type used by the test runner.

3. **Register the test**
   Find the `register_*_tests()` function for your group and add:
   ```rust
   test_group.register(TestSpec::new("my_feature", test_my_feature));
   ```
   The `TestSpec` takes a name (used by the `test` shell command) and the function pointer.

4. **Test patterns**
   - **Success path**: Create objects, perform operations, verify results.
   - **Error path**: Supply invalid handles, null pointers, out-of-range values — verify proper `Status::*` return.
   - **Stress**: Repeated create/destroy, high allocation counts.
   - **Concurrency** (if SMP): Spawn threads that operate on shared objects.

5. **Assertion helpers**
   Use existing helpers in `testing.rs`:
   ```rust
   assert!(condition, "message");           // Fail if false
   assert_eq!(a, b, "message");             // Fail if a != b
   ```
   These return `TestResult::Failed` rather than panicking (panic kills the kernel).

6. **Run tests**
   ```bash
   cargo build && python3 scripts/auto_test.py
   ```
   Or in QEMU shell: use the `test` command to run individual groups or all tests.

7. **Debug a failing test**
   - Check the test's assertion message for details.
   - Add temporary debug output via the kernel logging facility (not printk).
   - Run the failing test in isolation via the `test` command in QEMU (e.g., `test ob_tests`).

## Best practices
- One test per logical behavior — don't cram multiple scenarios into a single test.
- Name tests descriptively: `test_create_and_query_event`, `test_destroy_invalid_handle`.
- Clean up all resources in the test (destroy objects, free memory).
- Tests run in kernel context at IRQL PASSIVE_LEVEL — don't block or sleep.
- Keep tests independent — no shared mutable state between tests.

## Common mistakes
- Tests that pass but leave resources allocated (handle leak, memory leak).
- Tests that depend on global state from a previous test (ordering dependency).
- Using `assert!` instead of the test framework's `assert!` — panicking in kernel space crashes the system.
- Testing only the happy path and ignoring error conditions.
- Adding tests that take too long (>1 second) — tests run sequentially, slow tests add up.

## Final checklist
- [ ] Test registered in the correct `register_*_tests()` function
- [ ] Success and error paths covered
- [ ] No resource leaks (handles, memory, frames)
- [ ] No ordering dependencies on other tests
- [ ] `cargo build` succeeds
- [ ] `python3 scripts/auto_test.py` — all tests pass (including new ones)
