# Contributing to NeoDOS

## Development Setup

```bash
# Prerequisites: Rust nightly, QEMU, OVMF
rustup toolchain install nightly
cargo install --git https://github.com/NeoDOS-Project/NeoDev.git

# Build and run
neodev build --image
neodev run
```

## Quick Start

1. Read [`docs/README.md`](../README.md) to understand the documentation layout
2. Read [`AGENTS.md`](../../AGENTS.md) for permanent rules and architecture overview
3. Read [`docs/architecture/source-of-truth.md`](../architecture/source-of-truth.md) for invariants

## Code Style

- Follow AGENTS.md naming rules: kebab-case for files/dirs, PascalCase for types, snake_case for fns/vars
- All inline assembly confined to `src/hal/raw/`
- New syscalls must be `sys_ob_*` (RAX >= 77)
- No Ring 0 shell commands — user binaries go in `userbin/` as `.NXE`

## Pull Request Process

1. Branch from `develop`: `feat/`, `fix/`, `refactor/`
2. Run validation before commit:
   ```bash
   cargo build
   neodev test
   scripts/check_deps.py
   npx markdownlint '**/*.md' --config .markdownlint.json
   ```
3. Reference issues: `feat: description (#123)`
4. Open PR against `develop`

## Reporting Issues

Report bugs at [github.com/NeoDOS-Project/neodos/issues](https://github.com/NeoDOS-Project/neodos/issues).

## Documentation

Update relevant docs when changing APIs. See `docs/development/debugging.md` for debugging guides,
`docs/development/testing.md` for test writing guidelines.
