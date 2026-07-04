# Build

## When to use
You are asked to build, run, or test the system; or encountered a build failure.

## Goal
Successfully compile the kernel, bootloader, and optional user binaries; run in QEMU; execute tests.

## Steps

1. **Cargo build kernel**
   Run `cargo build` in `neodos-kernel/`. Fix any compilation errors.
   If uncertain about subsystem dependencies, run `scripts/check_deps.py` first.

2. **Full disk image**
   ```bash
   bash scripts/build.sh                 # bootloader + kernel + GPT image
   bash scripts/build.sh --neodos-image  # + user binaries (.NXE)
   ```

3. **Run in QEMU**
   ```bash
   bash scripts/qemu-debug.sh            # QEMU + OVMF + GDB on :1234
   QEMU_ACCEL=kvm bash scripts/qemu-debug.sh  # with KVM acceleration
   ```

4. **Run kernel tests**
   ```bash
   python3 scripts/auto_test.py
   ```
   All 537+ tests must pass. If a test fails, inspect `neodos-kernel/src/testing.rs` for the test group and fix the failing test or the code it exercises.

5. **Verify dependencies**
   ```bash
   scripts/check_deps.py
   ```
   Fix any cross-subsystem dependency violations.

## Best practices
- Build before committing every change, no exceptions.
- Run `cargo build` in `neodos-kernel/` first (fastest feedback), then `python3 scripts/auto_test.py`.
- Use `QEMU_ACCEL=kvm` for much faster emulation on Linux hosts.
- When debugging, connect GDB with `gdb neodos-kernel/target/x86_64-unknown-none/debug/neodos-kernel` and `target remote :1234`.

## Common mistakes
- Forgetting to build the bootloader after linker script changes (`bash scripts/build.sh` handles both).
- Only building user binaries (`--neodos-image`) when the kernel ABI changed — NEM drivers/dlls need matching kernel.
- Building without `--release` and wondering why QEMU is slow — debug builds have no optimizations.
- Running `auto_test.py` without building first — it doesn't trigger a build.

## Final checklist
- [ ] `cargo build` in `neodos-kernel/` succeeds
- [ ] `python3 scripts/auto_test.py` passes all tests
- [ ] `scripts/check_deps.py` passes
- [ ] QEMU boots to shell if image changed
