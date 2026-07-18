# Git Workflow Rules

## Before Commit

1. Check `git status` and `git diff` for unintended changes
2. Build + test: `cargo build --manifest-path neodos-kernel/Cargo.toml && neodev build --quick && neodev test`
3. Check deps: `scripts/check_deps.py`
4. Check markdown: `npx markdownlint '**/*.md' --config .markdownlint.json`
