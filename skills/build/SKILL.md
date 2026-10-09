---
name: build
description: Compile kernel, bootloader, build the disk image, run in QEMU, execute tests
---

# Build

## When to use

You are asked to build, run, or test the system; you hit a compilation failure; or you need a fresh bootable disk image.

## Goal

Compile the kernel, bootloader, and optional user binaries; produce a bootable image; run it in QEMU; execute the kernel test suite.

## Toolchain

- **NeoDev** is the unified build/image/run/test tool (independent repo:
  <https://github.com/NeoDOS-Project/NeoDev>). Install with
  `cargo install --git https://github.com/NeoDOS-Project/NeoDev.git`.
- The kernel is built with **nightly Rust**: `neodos-kernel/rust-toolchain.toml`
  pins `channel = "nightly"`. NeoDev selects the toolchain automatically; a bare
  `cargo build` needs it on PATH.
- Linker flags live in `neodos/.cargo/config.toml` (`rust-lld`,
  `relocation-model=static`, `-melf_x86_64`).

## Steps

1. **Fast feedback — compile the kernel only**
   Run `cargo build` in `neodos-kernel/` and fix compilation errors first.
   This does not produce an image.

2. **Build everything + disk image** (preferred before validating)

   ```bash
   neodev build --image
   ```

   Quick iteration (kernel + bootloader only):

   ```bash
   neodev build --quick --image
   ```

   Component selectors: `--kernel`, `--bootloader`, `--userbin`, `--nxl`,
   `--nem`, `--all` (default). Image size: `--neodos-size <MB>` (default 100).

3. **Run in QEMU**

   ```bash
   neodev run
   neodev run --kvm                     # KVM acceleration
   neodev run --gdb                     # GDB server on :1234
   neodev run --storage ahci            # ahci | ata | nvme | virtio
   neodev run --net user                # user | tap | bridge (default: bridge)
   neodev run --headless --serial qemu_output.log
   ```

4. **Run the kernel test suite**

   ```bash
   neodev test
   neodev test --kvm --storage ahci --timeout 180 --iterations 1
   ```

   All tests must pass. On failure, read the serial output, find the suite, and
   inspect its `register_*_tests()` module.

5. **Verify cross-subsystem dependencies**

   ```bash
   neodev check-deps
   ```

   Fix any violations against `check-deps-baseline.txt`.

6. **Lint Markdown** (required when docs changed)

   ```bash
   npx markdownlint '**/*.md' --config .markdownlint.json
   ```

7. **Debug with GDB**

   ```bash
   neodev run --gdb
   gdb neodos-kernel/target/x86_64-unknown-none/debug/neodos-kernel \
       -ex 'target remote localhost:1234'
   ```

   See `docs/development/debugging.md`.

## Best practices

- Build before committing every change, no exceptions.
- Iterate with `neodev build --quick --image`; validate with `neodev build --image`.
- Rebuild the image before measuring FS tests — an interactive boot can leave
  `disk_image.img` dirty and produce false failures.
- `cargo build` in `neodos-kernel/` is the fastest compile check.

## Common mistakes

- Editing the bootloader or linker scripts but only rebuilding the kernel — run
  `neodev build --image` (or `--quick --image`).
- Running tests against a stale or dirty image.
- Forgetting `neodev check-deps` after adding cross-module imports.
- Running NeoDev outside the project root (use `--neodos-path` or `NEODOS_PATH`).
- Skipping `markdownlint` when docs changed.

## Final checklist

- [ ] `cargo build` in `neodos-kernel/` succeeds
- [ ] `neodev build --image` succeeds
- [ ] `neodev test` passes
- [ ] `neodev check-deps` passes
- [ ] `npx markdownlint '**/*.md' --config .markdownlint.json` passes
- [ ] QEMU boots to the shell if the image changed
