---
name: drivers
description: Develop NEM drivers, modify the driver runtime, ABI negotiation, lifecycle
---

# Drivers

## When to use

Developing a new NEM driver, modifying the driver runtime, changing ABI
negotiation, updating capability flags, or altering the driver lifecycle.

## Goal

Create or modify a NEM driver correctly — from standalone `.o` through
`nem-pack`, ABI negotiation, capability declaration, isolation, and lifecycle.

## References

- `docs/drivers/overview.md`, `docs/drivers/nem-spec.md`,
  `docs/drivers/driver-migration.md`, `docs/drivers/kcr-compliance.md`
- Kernel format/loader: `src/drivers/nem/format.rs`,
  `src/drivers/nem/loader/` (`v3loader.rs`, `net_bridge.rs`)
- Runtime/certification: `src/drivers/nem/runtime/`
- Management: `src/drivers/nem/management/` (`abi/mod.rs`, `caps.rs`,
  `isolation.rs`, `dependency/mod.rs`, `boot_loader/mod.rs`, manager, hot-reload)
- Hardware drivers: `src/drivers/hw/` (`pci`, `ata`, `ahci`, `nvme`,
  `virtio_blk`, `ps2`, `rtc`); storage: `src/drivers/storage/`
- Example driver: `drivers/e1000/` (`src/lib.rs`, `build_nem.py`)
- Packer: `tools/nem-pack.py`

## NEM v3 format (80-byte header)

Source: `src/drivers/nem/format.rs` (re-exported as `crate::nem`).
Magic `"NEM3"` = `0x334D454E`, `header_size = 80`, `version = 3`.

Key fields: `abi_min`, `abi_target`, `abi_max` (u16), `driver_type`,
`category` (0=BOOT, 1=SYSTEM, 2=DEMAND), section sizes
(`text/rodata/data/bss/total_mem_size`), entry offsets (`entry_init`,
`entry_event`, `entry_fini`), relocation table, symbol/string tables, and the
driver name offset. ABI constants: `ABI_MIN_VALID = 1`, `ABI_TARGET = 1`,
`ABI_MAX_VALID = 2`.

## Steps

### 1. Author the driver as a standalone `no_std` library

Place it in `drivers/<name>/src/lib.rs`:

```rust
#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

#[no_mangle]
pub extern "C" fn driver_init() -> i32 { /* probe + register */ 0 }

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 { 0 }

#[no_mangle]
pub unsafe extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 { 0 }

#[no_mangle]
pub extern "C" fn driver_fini() { /* release resources */ }
```

Drivers call host services through imported `hst_*` functions (`hst_inb/outb`,
`hst_log`, `hst_push_event`, `hst_ecam_read_dword`,
`hst_register_network_device`, `hst_virt_to_phys`, …). See `drivers/e1000/`
for a complete example.

### 2. Build and pack the driver

Each driver carries a `build_nem.py` that compiles the Rust sources to an object
and runs the packer. At the project level use `neodev build --nem`, or:

```bash
python3 tools/nem-pack.py <input.o> <output.nem> \
    --name <name> --type <0-5> --category <0-2> \
    --abi-min 1 --abi-target 1 --abi-max 2
```

### 3. Choose the category

- `BOOT(0)` — loaded during boot before the system is fully up.
- `SYSTEM(1)` — core drivers loaded by the boot driver loader.
- `DEMAND(2)` — on-demand drivers.

### 4. Declare capabilities (`management/caps.rs`)

Request the minimum needed. 13 frozen v0.42 bits (0-12):

`CAP_IRQ(0)`, `CAP_DMA(1)`, `CAP_MMIO(2)`, `CAP_PORTIO(3)`,
`CAP_ALLOC_PAGE(4)`, `CAP_BLOCK_DEVICE(5)`, `CAP_EVENT_BUS(6)`, `CAP_INPUT(7)`,
`CAP_LOG(8)`, `CAP_TIMING(9)`, `CAP_MEMORY(10)`, `CAP_ISOLATION(11)`,
`CAP_NS_WRITE(12)`. Each `hst_*` export calls `check_cap()` first. DEMAND drivers
cannot escalate (hard boundary); SYSTEM drivers may escalate via
`EVENT_CAP_ESCALATION`.

### 5. Lifecycle (8 states)

`Loaded → Initialized → Registered → Bound → Active → Faulted → Unloading →
Unloaded`. `certify_and_activate()` only reaches `Active` when the state is
`Bound`, `last_error == ERR_NONE`, and the driver is not `Faulted`. Handle
`Faulted → Unloaded` and the unload path carefully.

### 6. ABI negotiation (`management/abi/mod.rs`)

The kernel compares the driver's `[abi_min, abi_max]` against `ABI_TARGET`; empty
intersection = `Incompatible`. Update `ABI_TARGET` in
`src/drivers/nem/format.rs` only on a breaking NEM ABI change (and bump the ABI in
`AGENTS.md`).

### 7. Isolation (`management/isolation.rs`)

`CAP_ISOLATION` runs the driver in one of 16 × 1 MB slots at `DRIVER_ISO_BASE`
(`0x30000000`). Modes: `None`, `Basic` (page-isolated, validated exports),
`Sandbox` (faults outside the region → `FAULTED`). `validate_driver_ptr()`
accepts only known regions. Make the driver fault-tolerant when isolated.

### 8. Dependencies (`management/dependency/mod.rs`)

Declare `__dep_DRIVERNAME` symbols in the NEM symbol table. The resolver computes
a topological order (max 32 deps/driver, max 16 drivers) and rejects cycles.

### 9. Boot loading (`management/boot_loader/mod.rs`, Phase 3.85)

BOOT drivers load first, then SYSTEM drivers (dependency-sorted). A failing BOOT
driver is marked `FAULTED` and logged; boot continues.

### 10. Build and test

```bash
neodev build --nem
neodev build --image && neodev test
neodev check-deps
```

Driver state is observable through the Object Manager:
`ob_open("\Global\Info\Drivers")` + `ob_query_info(Drivers)`.

## Best practices

- Request exactly the capabilities the driver needs; over-privilege is a risk.
- Only go through HAL host services (`hst_*`) — never touch hardware directly.
- Keep `driver_init`/`driver_fini` symmetric (release every IRQ/MMIO/DMA).
- Set `abi_max` honestly — too high risks loading on an incompatible kernel.
- Test the isolated mode if `CAP_ISOLATION` is set.

## Common mistakes

- Forgetting to release IRQs/MMIO on `driver_fini` (double-free later).
- Declaring `abi_max` above what the kernel supports.
- Requesting `CAP_ISOLATION` / escalation without testing (DEMAND cannot escalate).
- Assuming the old `nem_driver!` macro exists — drivers are standalone libs with
  `#[no_mangle]` entry points, packed by `nem-pack.py`.
- Bumping `ABI_TARGET` without updating `AGENTS.md` docs.

## Final checklist

- [ ] NEM v3 header valid (magic, checksum, version fields)
- [ ] ABI `[min, target, max]` intersects `ABI_TARGET`
- [ ] Capabilities declared (minimum set)
- [ ] Category correct (BOOT/SYSTEM/DEMAND)
- [ ] Lifecycle handled: init, activate, event, fini, fault recovery
- [ ] Dependencies declared and acyclic (`__dep_*`)
- [ ] Isolation behavior correct if `CAP_ISOLATION` set
- [ ] Builds via `neodev build --nem` and `neodev test` passes
- [ ] `docs/drivers/overview.md` updated if ABI or lifecycle changed
