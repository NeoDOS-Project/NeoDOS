description: Full verification — compile kernel, run tests, check deps, lint

## Steps

1. Build kernel:
   ```bash
   cargo build --manifest-path neodos-kernel/Cargo.toml
   ```

2. Quick build via neodev:
   ```bash
   neodev build --quick
   ```

3. Run tests:
   ```bash
   neodev test
   ```

4. Check deps:
   ```bash
   scripts/check_deps.py
   ```

5. Markdown lint:
   ```bash
   npx markdownlint '**/*.md' --config .markdownlint.json
   ```
