---
description: Run complete verification for NeoDOS — build kernel, run tests, check dependencies, and lint docs.
---

# Verify NeoDOS

Run comprehensive verification in this exact order:

1. **Build Kernel**
   cargo build in neodos-kernel/
   If it fails, report errors and STOP

2. **Build Image**
   cargo run --manifest-path tools/neodev/Cargo.toml -- build --quick
   If it fails, report errors and STOP

3. **Run Tests**
   cargo run --manifest-path tools/neodev/Cargo.toml -- test
   Report pass/fail count

4. **Check Dependencies**
   scripts/check_deps.py

5. **Lint Docs**
   npx markdownlint '**/*.md' --config .markdownlint.json

6. **Git Status**
   git status (uncommitted changes)

## Output

```
VERIFICATION: [PASS/FAIL]

Build:     [OK/FAIL]
Image:     [OK/FAIL]
Tests:     [X/Y passed]
Deps:      [OK/FAIL]
Docs:      [OK/X warnings]

Ready for commit: [YES/NO]
```
