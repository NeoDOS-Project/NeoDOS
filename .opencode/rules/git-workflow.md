# Git Workflow (NeoDOS)

## Commit Message Format

```
<type>: <description>
```

Types: feat, fix, refactor, docs, test, chore, perf, ci

## Pre-Commit Checklist

1. Build kernel: `cargo build` in neodos-kernel/
2. Build + test: `cargo run --manifest-path tools/neodev/Cargo.toml -- build --quick && cargo run --manifest-path tools/neodev/Cargo.toml -- test`
3. Check deps: `scripts/check_deps.py`
4. Lint docs: `npx markdownlint '**/*.md' --config .markdownlint.json`
5. Commit: `git add -A && git commit -m "feat|fix|refactor: ..." && git push`
6. Update CHANGELOG.md and docs/IMPROVEMENTS.md
