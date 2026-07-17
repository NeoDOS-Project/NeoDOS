---
name: boot
description: Modify bootloader, kernel boot sequence, boot phases, or BootInfo ABI
---

# Boot Flow

## When to use

Modifying the UEFI bootloader, the kernel boot sequence (`rust_start` phases 0–4), the `BootInfo` struct, GPT layout, RAM disk loading, or boot-time initialization order.

## Goal

Correctly modify the boot process without breaking phase ordering, BootInfo ABI compatibility, or critical invariants (memory map, framebuffer, filesystem image).

## References

- `docs/boot.md` — boot flow documentation
- `neodos-bootloader/` — UEFI bootloader source
- `src/main.rs` — `rust_start()` with phases 0–4
- `drivers/gpt.rs` — GPT partition parsing
- `drivers/block.rs` — RAM disk registration (`set_ram_disk`)
- `src/object/namespace.rs` — Ob namespace root (`\`) and standard directories

## Architecture

### Boot ABI (BootInfo)

```rust
#[repr(C)]
pub struct BootInfo {
    pub magic: u32,                    // 0x4E444F53 ("NDOS")
    pub version: u32,                  // bootloader version code
    pub fb_info: FramebufferInfo,      // base_address, size, width, height, stride
    pub memory_map_addr: u64,          // physical address of UEFI memory map
    pub memory_map_size: u64,          // total size in bytes
    pub memory_map_desc_size: u64,     // size of each descriptor
    pub memory_map_desc_version: u32,  // descriptor format version
    pub fs_image_addr: u64,            // physical address of FS image (RAM disk)
    pub fs_image_size: u64,            // size in bytes
    pub acpi_rsdp_addr: u64,           // ACPI RSDP (0 if not found)
}
```

| Constant | Value | Description |
| ---------- | ------- | ------------- |
| `BOOTINFO_MAGIC` | `0x4E444F53` | "NDOS" magic |
| `KERNEL_VERSION_CODE` | `(49 << 8) \| 0` = `0x3100` | v0.49.0 |
| `BOOT_VERSION` | `(10 << 8) \| 5` | Must match kernel version |

### Bootloader Steps (`neodos-bootloader/`)

1. **UEFI init** — `uefi::helpers::init()`, set up logging
2. **GOP init** — open `GraphicsOutput` protocol, extract framebuffer
3. **Load kernel ELF** — read `\EFI\NeoDOS\kernel.elf`, parse PT_LOAD, allocate pages, copy + zero BSS
4. **Load FS image** — read `\EFI\NeoDOS\neodos.fs`, allocate pages, copy to memory
5. **Locate ACPI RSDP** — scan UEFI config tables for ACPI 2.0 GUID (fallback 1.0)
6. **ExitBootServices** — 2s stall, capture final UEFI memory map
7. **Jump to kernel** — call `entry(&BootInfo) -> !`

### Kernel Boot Phases (`src/main.rs`)

| Phase | Description | Key Line |
| ----- | ----------- | -------- |
| 0 | Verify boot info magic + version | `:106` |
| 1 | Graphics, RAM disk, serial init, benchmark | `:113` |
| 2 | GDT (5 selectors + TSS), IDT (exceptions + IRQs + INT 0x80), MSI, PIC remap | `:148-158` |
| 3 | HPET init, APIC timer calibration, PS/2 + USB HID init | `:160-179` |
| 2.5 | Physical memory: parse UEFI mem map, buddy allocator, crash dump area | `:187-194` |
| 2.75 | Kernel heap: slab + linked_list_allocator | `:199` |
| 2.759 | Object Manager init | `:205` |
| 2.7595 | Timer Manager init | `:210` |
| 2.76 | Ob namespace: `\`, `\Global`, `\Device`, `\Registry`, `\Ob\Process`, `\Security`, `\Global\Info\` | `:216-253` |
| 2.77 | Security subsystem init (default admin/user tokens) | `:260` |
| 2.8 | SMP: per-CPU KPRCB, INIT-SIPI-SIPI | `:266` |
| 2.9 | IPI: cross-CPU resched, TLB shootdown, call-function | `:272` |
| 2.91 | I/O APIC: detect from MADT, disable PIC, route ISA IRQs | `:278` |
| 3 | **STI**, custom 4 GiB identity map via 2 MB huge pages | `:285-292` |
| 3.0 | Demand paging: split heap + mmap into 4 KB PTEs | `:295-297` |
| 3.1 | TEB page at 0x7000 (USER_ACCESSIBLE) for SEH | `:305` |
| 3.2 | PCIe ECAM: read MCFG, map MMIO, activate | `:311` |
| 3.3 | Storage init: ATA→AHCI→NVMe→VirtIO probe priority | `:321` |
| 3.4 | GPT scan, IoStack creation, Block Cache, Page Cache | `:332-371` |
| 3.4b | NeoDOS FS mount → C: | `:396-406` |
| 3.4c | FAT32 ESP mount → A: | `:420-432` |
| 3.5 | Input manager init (VT subsystem, keyboard) | `:441` |
| 3.80 | Driver Isolation Layer: 16×1 MB slots @ 0x30000000 | `:449` |
| 3.85 | Boot driver loader: BOOT → SYSTEM .nem (dep-sorted) | `:457` |
| 3.86 | AHCI port reclaim | `:464` |
| 3.87 | NEM bridges (RTC), NXL region, hot-reload | `:469-472` |
| 3.88 | Networking: e1000, ARP, TCP/UDP namespace objects | `:480` |
| 3.881 | Registry init (Cm): mount SYSTEM hive | `:488` |
| 3.881b | Default registry values | `:497` |
| 3.9 | ABI freeze validation | `:503-507` |
| 4 | Kernel self-tests, cmdtest.nxe, NeoInit launch (PID 1) | `:513-663` |

### GPT Layout

| Part | Filesystem | LBA | Mount | GPT Type GUID |
| ---- | ---------- | --- | ----- | ------------- |
| 1 | FAT32 (ESP) | 2048–206847 | A: | C12A7328-F81F-11D2-BA4B-00A0C93EC93B |
| 2 | NeoDOS FS | 206848–227327 | C: | EBD0A0A2-B9E5-4433-87C0-68B6B72699C7 |

## Steps

### 1. Modify a boot phase

```rust
// In rust_start(), add a new phase between existing ones
// Phase 2.761 — My new init
#[link_section = ".init"]
unsafe fn init_my_subsystem(boot_info: &BootInfo) {
    // ... initialization ...
    serial_println!("[MY] Subsystem initialized");
}
```

Always annotate early init functions with `#[link_section = ".init"]` so the memory can be reclaimed after boot.

### 2. Add a field to BootInfo

1. Add field to `BootInfo` struct in the bootloader AND kernel (both must match).
2. Set the field in the bootloader before ExitBootServices.
3. Read it in the kernel's `rust_start()`.
4. Bump `BOOT_VERSION` if the change breaks ABI compatibility.

### 3. Modify the GPT layout

Edit `scripts/gpt.json` or the partition constants in the image builder. Update `drivers/gpt.rs` partition tables if partition numbers or sizes change.

### 4. Add a new boot phase

- Add between existing phases to maintain ordinal numbering (e.g., 2.761 after 2.76).
- Ensure all dependencies (heap, Ob namespace, etc.) are initialized before your phase.
- Mark the function `unsafe` with `#[link_section = ".init"]`.

### 5. Modify RAM disk loading

The RAM disk (`neodos.fs`) is loaded by the bootloader and passed via `BootInfo::fs_image_addr/size`. The kernel registers it:

```rust
drivers::block::set_ram_disk(fs_image_addr, fs_image_size);
```

### 6. Add a new init step to NeoInit (PID 1)

NeoInit source is in `userbin/neoinit/src/main.rs`. The kernel launches it at phase 4 after tests complete.

## Best practices

- Phases are ordered by dependency — never initialize a subsystem before its prerequisites.
- `#[link_section = ".init"]` functions are unmapped after boot — don't reference them later.
- `BootInfo` is ABI between bootloader and kernel — both sides MUST match.
- Use `without_interrupts` for critical sections during early boot (pre-phase 3).
- The kernel runs at physical address `0x4000000` — don't hardcode other addresses.
- Boot version mismatch is non-fatal but warns — avoid unnecessary version bumps.
- Add new phases with fractional numbering (2.761, 3.881) rather than renumbering.
- Always test GPT changes by rebuilding the image (`--image` flag).

## Common mistakes

- Forgetting `#[link_section = ".init"]` on early init functions — memory not reclaimable.
- Modifying `BootInfo` layout in kernel only (not bootloader) — silent corruption.
- Adding heap allocation before phase 2.75 (slab init) — page fault.
- Adding Ob namespace operations before phase 2.76 — namespace root doesn't exist.
- Enabling interrupts before phase 3 (STI) — handler not ready.
- Breaking the boot version compatibility check — kernel panics or warns incorrectly.
- Adding a new partition without updating the GPT image builder script.

## Final checklist

- [ ] BootInfo changes synchronized between bootloader and kernel
- [ ] New phase placed after all its dependency phases
- [ ] `#[link_section = ".init"]` used for early init functions
- [ ] `BOOT_VERSION` bumped if BootInfo ABI changed
- [ ] GPT layout changes reflected in image builder and `drivers/gpt.rs`
- [ ] RAM disk loads and mounts correctly
- [ ] Tests pass: `neodev test`
- [ ] Image builds: `neodev build --image`
- [ ] `docs/boot.md` updated with new phase descriptions
- [ ] `scripts/check_deps.py` passes
