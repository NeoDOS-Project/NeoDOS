---
name: neodev
description: Use the NeoDev development tool for build, image, run, test, and deps
---

# NeoDev

NeoDev is the unified build/image/run/test tool for the NeoDOS ecosystem, an
independent project at <https://github.com/NeoDOS-Project/NeoDev>.

## Install

```bash
cargo install --git https://github.com/NeoDOS-Project/NeoDev.git
```

Run from the project root (or pass `--neodos-path` / set `NEODOS_PATH`).

## Commands

| Command | Purpose |
| --------- | --------- |
| `neodev build` | Build components (`--kernel`, `--bootloader`, `--userbin`, `--nxl`, `--nem`, `--all`, `--quick`, `--image`) |
| `neodev image` | Create disk images (NE2, ESP, GPT) |
| `neodev run` | Run in a VM (`--kvm`, `--gdb`, `--storage`, `--net`, `--headless`, `--serial`, `--bdm`) |
| `neodev test` | Run automated kernel tests (`--kvm`, `--storage`, `--timeout`, `--iterations`) |
| `neodev check-deps` | Check cross-subsystem dependencies (alias `deps`) |
| `neodev dhcp` | Run the DHCP integration test |
| `neodev nxp` | Build NXP packages (`--all`, or a binary name) |
| `neodev shell` | Send commands to the NeoDOS shell (automation) |
| `neodev vm` | Manage NeoDOS virtual machines |
| `neodev list` | List discovered projects |
| `neodev config` | Show configuration |
| `neodev clean` | Clean build artifacts |

## Common flows

```bash
# Fast iteration (kernel + bootloader + image)
neodev build --quick --image

# Full build (user binaries, NXL, NEM) + image
neodev build --image

# Run and test
neodev run
neodev test

# Dependency and Markdown checks
neodev check-deps
npx markdownlint '**/*.md' --config .markdownlint.json
```

## Common mistakes

- Running NeoDev outside the project root (use `--neodos-path` or `NEODOS_PATH`).
- Forgetting `--image` when you need a bootable disk (build alone only compiles).
- Not using `--quick` for fast kernel-only iteration.
- Running tests against a stale/dirty image — rebuild with `--quick --image` first.

## Final checklist

- [ ] NeoDev installed
- [ ] `neodev build --image` succeeds
- [ ] `neodev test` passes
- [ ] `neodev check-deps` passes
- [ ] QEMU boots to the shell (`neodev run`), if applicable
