---
name: neodev
description: Use the NeoDev development tool for build, run, test, and image management
---

# NeoDev

NeoDev is now an independent project at [https://github.com/NeoDOS-Project/NeoDev](https://github.com/NeoDOS-Project/NeoDev).

Install it first, then use:

## Steps

1. **Build kernel + bootloader + image**
   ```bash
   neodev build --quick --image
   ```

2. **Full build with user binaries**
   ```bash
   neodev build --image
   ```

3. **Run in QEMU**
   ```bash
   neodev run
   neodev run --kvm   # with KVM
   ```

4. **Run tests**
   ```bash
   neodev test
   ```

5. **List projects**
   ```bash
   neodev list
   ```

6. **Clean artifacts**
   ```bash
   neodev clean
   ```

## Common mistakes

- Running neodev outside the NeoDOS project directory (use `--neodos-path` or `NEODOS_PATH`)
- Forgetting `--image` flag when building (only compiles, doesn't create disk image)
- Not using `--quick` for fast kernel-only iteration (skips user binaries)

## Final checklist

- [ ] NeoDev installed (`cargo install --git https://github.com/NeoDOS-Project/NeoDev.git`)
- [ ] Build succeeds: `neodev build --quick`
- [ ] Tests pass (if applicable): `neodev test`
- [ ] QEMU boots to shell (if running): `neodev run`
