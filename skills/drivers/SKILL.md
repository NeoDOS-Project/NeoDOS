---
name: drivers
description: Develop NEM drivers, modify driver runtime, ABI negotiation, lifecycle
---

# Drivers

## When to use

Developing a new NEM driver, modifying the driver runtime, changing the ABI negotiation, updating capability flags, or altering the driver lifecycle.

## Goal

Create or modify a NEM driver correctly — from .nxe module through ABI negotiation, capability declaration, isolation, and lifecycle management.

## Steps

1. **Read `docs/drivers/overview.md`**
   Understand NEM format, driver lifecycle, capabilities, isolation model, and ABI negotiation.

2. **Choose driver category**
   - `BOOT(0)`: Loaded during boot phase 2 before the object manager is fully initialized.
   - `SYSTEM(1)`: Core system drivers loaded by the driver boot loader.
   - `DEMAND(2)`: On-demand drivers loaded when a device or service is requested.

3. **Create the NEM module source**
   Place in `drivers/<name>/src/main.rs`. Structure:

   ```rust
   #![no_std]
   #![no_main]

   use nem::*;

   nem_driver! {
       name: "mydriver",
       category: DEMAND,    // or BOOT, SYSTEM
       abi_min: 5,
       abi_target: 7,
       abi_max: 7,
       caps: CAP_IRQ | CAP_IO,
       init: mydriver_init,
       unload: mydriver_unload,
   }

   fn mydriver_init(ctx: &mut DriverContext) -> Result<(), DriverError> {
       // Register devices, allocate resources
       Ok(())
   }

   fn mydriver_unload(ctx: &mut DriverContext) -> Result<(), DriverError> {
       // Release resources, unregister devices
       Ok(())
   }
   ```

4. **NEM v2 header** (`neodos-kernel/src/nem/`)
   The header is 48 bytes. Fields: magic ("NE"), format_version (2), abi_min/abi_target/abi_max, caps bitmap, entry points, size, checksum.
   The `nem_driver!` macro generates this automatically.

5. **Capability flags** (`src/drivers/caps.rs`)
   Declare only what the driver needs. Review 12 flags:
   - `CAP_IRQ(1)` — interrupt handling
   - `CAP_DMA(2)` — direct memory access
   - `CAP_IO(4)` — port I/O
   - `CAP_MMIO(8)` — memory-mapped I/O
   - `CAP_ISOLATION(2048)` — driver isolation (hardware-enforced)

6. **Driver lifecycle states** (`src/drivers/driver_runtime.rs`)
   8 states: `Loaded → Initialized → Registered → Bound → Active → Faulted → Unloaded → Unloading`.
   Handle each state transition in `mydriver_init()`: typically go from Initialized to Registered.

7. **ABI negotiation** (`src/drivers/abi/`)
   The kernel compares `abi_min..=abi_max` against `KERNEL_ABI_VERSION`. If the intersection is empty, loading fails.
   Update `KERNEL_ABI_VERSION` in `src/nem/mod.rs` when the NEM ABI changes.

8. **Isolation** (`src/drivers/isolation/`)
   If `CAP_ISOLATION` is set, the driver runs in a restricted environment (separate address space, I/O port restrictions).
   Ensure the driver can handle page faults gracefully if isolated.

9. **Add to boot loader** (`src/drivers/boot_loader/`)
   If BOOT or SYSTEM category: register the driver in the boot loader's driver list so it's loaded automatically.

10. **Build and test**

    ```bash
    bash scripts/build.sh --neodos-image
    python3 scripts/auto_test.py
    ```

## Best practices

- Request exactly the capabilities needed — over-privilege is a security risk.
- Handle all state transitions gracefully, especially `Faulted → Unloaded`.
- Use `DriverContext` for all resource registration (IRQs, MMIO, DMA).
- Validate ABI versions at compile time with `static_assert!` if possible.
- Keep init/unload functions idempotent where possible.

## Common mistakes

- Setting `abi_max` too high — the driver may load on a kernel that breaks compatibility.
- Not releasing IRQs in `unload` — causes double-free on the IRQ line.
- Requesting `CAP_ISOLATION` without testing — isolation adds significant complexity.
- Forgetting to update `KERNEL_ABI_VERSION` when the NEM ABI changes.
- Accessing hardware directly without going through HAL abstractions.

## Final checklist

- [ ] NEM v2 header valid (magic, checksum, version fields)
- [ ] ABI version range intersects `KERNEL_ABI_VERSION`
- [ ] Capabilities declared (minimum required set)
- [ ] Category correct (BOOT/SYSTEM/DEMAND)
- [ ] Lifecycle states handled: init, unload, fault recovery
- [ ] Registered in boot loader if BOOT or SYSTEM
- [ ] Resources released on unload (IRQs, MMIO, DMA channels)
- [ ] Isolation model correct if `CAP_ISOLATION` set
- [ ] `cargo build` and `python3 scripts/auto_test.py` pass
- [ ] `docs/drivers/overview.md` updated if ABI or lifecycle changed
