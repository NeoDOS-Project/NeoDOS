description: Show test coverage gaps and verify all tests pass

## Steps

1. Check existing test coverage in the modified subsystem (e.g., `src/ob/`, `src/mm/`, etc.)
2. Count test groups in `src/testing.rs` or subsystem test files
3. Check if the changed feature has test cases
4. Verify all tests pass: `neodev test`
