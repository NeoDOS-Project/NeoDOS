---
name: neodev
description: Use the NeoDev development tool for build, run, test, and image management
---

# NeoDev

## When to use

You are asked to build, run, test, or manage the NeoDOS development environment using the neodev tool.

## Goal

Use neodev correctly for building the kernel, creating disk images, running in QEMU, and executing tests.

## Steps

1. **Build kernel + bootloader + image**
   ```bash
   cargo run --manifest-path tools/neodev/Cargo.toml -- build --quick --image
   ```

2. **Full build with user binaries**
   ```bash
   cargo run --manifest-path tools/neodev/Cargo.toml -- build --image
   ```

3. **Run in QEMU**
   ```bash
   cargo run --manifest-path tools/neodev/Cargo.toml -- run
   cargo run --manifest-path tools/neodev/Cargo.toml -- run --kvm   # with KVM
   ```

4. **Run tests**
   ```bash
   cargo run --manifest-path tools/neodev/Cargo.toml -- test
   ```

5. **List projects**
   ```bash
   cargo run --manifest-path tools/neodev/Cargo.toml -- list
   ```

6. **Clean artifacts**
   ```bash
   cargo run --manifest-path tools/neodev/Cargo.toml -- clean
   ```

## Common mistakes

- Running neodev outside the project root directory
- Forgetting `--image` flag when building (only compiles, doesn't create disk image)
- Not using `--quick` for fast kernel-only iteration (skips user binaries)
- Using the legacy `bash scripts/build.sh` instead of neodev

## Final checklist

- [x] Use `cargo run --manifest-path tools/neodev/Cargo.toml` from project root
- [ ] Build succeeds
- [ ] Tests pass (if applicable)
- [ ] QEMU boots to shell (if running)
