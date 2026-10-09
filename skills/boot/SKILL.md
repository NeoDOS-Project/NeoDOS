---
name: boot
description: Modify the bootloader, kernel boot sequence, boot phases, or BootInfo ABI
---

# Boot Flow

## When to use

Modifying the UEFI bootloader, the kernel boot sequence (`src/boot/mod.rs`
`init()` / `src/main.rs` `rust_start()`), the `BootInfo` struct, GPT layout, RAM
disk loading, or boot-time initialization order.

## Goal

Modify the boot process without breaking phase ordering, BootInfo ABI
compatibility, or critical invariants (memory map, framebuffer, filesystem image).

## References

- `docs/boot/boot-flow.md` — boot flow documentation
- `neodos-bootloader/` — UEFI bootloader source (target `x86_64-unknown-uefi`)
- `src/boot/mod.rs` — `init(boot_info)` boot sequence
- `src/main.rs` — `rust_start()`, `KERNEL_VERSION_CODE`, version check
- `src/drivers/storage/gpt.rs` — GPT parsing
- `src/drivers/storage/block.rs` — RAM disk registration (`set_ram_disk`)
- `src/object/namespace/` — Ob namespace root (`\`) and standard directories

## Boot ABI (BootInfo)

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
| `KERNEL_VERSION_CODE` | `(10 << 8) \| 5` = `0x0A05` | Stale comment (v0.10.5); project version is v0.51.5 (tracked as `CH-11`) |
| `BOOT_VERSION` | `(10 << 8) \| 5` | Must match `KERNEL_VERSION_CODE` (currently identical → no warning) |

The bootloader writes `boot_info.version`; the kernel compares it to
`KERNEL_VERSION_CODE` at entry. A mismatch is non-fatal but logs a kernel warning.

## Bootloader steps (`neodos-bootloader/`)

1. **UEFI init** — `uefi::helpers::init()`, logging.
2. **GOP init** — open `GraphicsOutput`, extract framebuffer (enumerate handles
   if the first fails).
3. **Load kernel ELF** — read `\EFI\NeoDOS\kernel.elf`, parse `PT_LOAD`, allocate
   pages, copy segments, zero BSS.
4. **Load FS image** — read `\EFI\NeoDOS\neodos.fs`, allocate + copy.
5. **Locate ACPI RSDP** — scan UEFI config tables (ACPI 2.0, fallback 1.0).
6. **ExitBootServices** — 2 s stall, capture the final memory map.
7. **Jump to the kernel** — `cli`, call `entry(&BootInfo) -> !`.

## Kernel boot phases (`src/boot/mod.rs`)

| Phase | Description |
| ----- | ----------- |
| 0 | Verify boot info magic + version (halt on bad magic) |
| 1 | Graphics init, RAM disk setup, serial init, benchmark init |
| 2 | GDT (5 selectors + TSS), IDT (exceptions + IRQs + INT 0x80), MSI, PIC remap |
| 3 | HPET init + APIC timer calibration, PS/2 + USB HID init |
| 2.5 | Physical memory: parse UEFI map, buddy allocator, crash dump area (16 MB @ 0x0F000000), watchdog |
| 2.75 | Kernel heap: slab + linked-list allocator fallback |
| 2.759 | Object Manager init |
| 2.7595 | Timer Manager init (64 slots) |
| 2.76 | Ob namespace: `\`, `\Global`, `\Device`, `\Registry`, `\Ob\Process`, `\Security`, `\Global\Info\` |
| 2.77 | Security init (default admin/user tokens) |
| 2.8 | SMP: per-CPU KPRCB, INIT-SIPI-SIPI |
| 2.9 | IPI: reschedule, TLB shootdown, call-function |
| 2.91 | I/O APIC: detect from MADT, disable PIC, route ISA IRQs |
| 3 | **STI**; 4 GiB identity map via 2 MB huge pages |
| 3.0 | Demand paging: split heap + mmap huge pages into 4 KB PTEs |
| 3.1 | TEB page at 0x7000 (`USER_ACCESSIBLE`) for SEH |
| 3.2 | PCIe ECAM: read MCFG, map MMIO, activate |
| 3.3 | Storage init: ATA → AHCI → NVMe → VirtIO probe (priority NVMe > VirtIO > AHCI > ATA) |
| 3.4 | GPT scan, IoStack creation, Block Cache, Page Cache (128 × 4 KB) |
| 3.4b | NeoDOS FS mount → C: |
| 3.4c | FAT32 ESP mount → A: |
| 3.5 | Input manager init (VT subsystem, keyboard) |
| 3.80 | Driver Isolation Layer: 16 × 1 MB slots @ 0x30000000 |
| 3.85 | Boot driver loader: BOOT → SYSTEM `.nem` (dependency-sorted) |
| 3.86 | AHCI port reclaim |
| 3.87 | NEM bridges (RTC), NXL region init, hot-reload, NXL loader |
| 3.88 | Networking init: ARP cache, `\Device\Tcp`/`\Device\Udp`, NICs via NEM |
| 3.881 | Registry init (Cm): create `\Registry` tree, mount SYSTEM hive |
| 3.881b | Default registry values |
| 3.9 | ABI freeze validation (`syscall::validate_abi()`, frozen-ABI verifier) |
| 4 | Kernel self-tests, `netpump` kernel thread, benchmarks, NeoInit (PID 1) |

## GPT layout

| Part | Filesystem | LBA | Mount | GPT Type GUID |
| ---- | ---------- | --- | ----- | ------------- |
| 1 | FAT32 (ESP) | 2048-206847 | A: | C12A7328-F81F-11D2-BA4B-00A0C93EC93B |
| 2 | NeoDOS FS | 206848-227327 | C: | EBD0A0A2-B9E5-4433-87C0-68B6B72699C7 |

The GPT is parsed by `src/drivers/storage/gpt.rs` on the primary block device.

## Steps

### 1. Modify a boot phase

Add the step in `src/boot/mod.rs` (or `rust_start()`), ordered by dependency.
Early init functions may be annotated `#[link_section = ".init"]` so their memory
is reclaimed after boot — never reference them later.

### 2. Add a field to `BootInfo`

1. Add the field to the struct in the bootloader **and** the kernel (both must
   match).
2. Set it in the bootloader before `ExitBootServices`.
3. Read it in the kernel (`rust_start()` / `src/boot/mod.rs`).
4. Bump `BOOT_VERSION` / `KERNEL_VERSION_CODE` if the change breaks ABI.

### 3. Modify the GPT layout

Partition geometry is produced by NeoDev (`neodev/src/image.rs`) and configured
via `neodev.toml` / `neodev build --neodos-size`. Update `src/drivers/storage/gpt.rs`
if partition numbers or types change.

### 4. Modify RAM disk loading

The bootloader passes `neodos.fs` via `BootInfo::fs_image_addr/size`; the kernel
registers it:

```rust
drivers::storage::block::set_ram_disk(fs_image_addr, fs_image_size);
```

### 5. Add an init step to NeoInit (PID 1)

NeoInit source: `userbin/neoinit/src/main.rs`. The kernel launches it at Phase 4
after tests complete.

## Best practices

- Phases are dependency-ordered — never initialize a subsystem before its
  prerequisites.
- `BootInfo` is ABI between bootloader and kernel — both sides MUST match.
- Kernel image loads at physical address `0x4000000`; don't hardcode others.
- Prefer fractional phase numbering (2.761, 3.881) over renumbering.
- Rebuild the image (`neodev build --image`) after any GPT or boot change.

## Common mistakes

- Forgetting to update `BOOT_VERSION`/`KERNEL_VERSION_CODE` on a BootInfo change.
- Modifying `BootInfo` in only one side (silent corruption).
- Adding heap allocation before the heap phase, or namespace ops before Phase 2.76.
- Enabling interrupts before Phase 3 (STI).
- Editing GPT geometry without updating the image builder / `gpt.rs`.
- Referencing a `#[link_section = ".init"]` function after boot.

## Final checklist

- [ ] `BootInfo` synchronized between bootloader and kernel
- [ ] New phase placed after all dependency phases
- [ ] `BOOT_VERSION` / `KERNEL_VERSION_CODE` bumped if the ABI changed
- [ ] GPT changes reflected in the image builder and `gpt.rs`
- [ ] RAM disk loads and mounts correctly
- [ ] `neodev build --image` succeeds; `neodev test` passes
- [ ] QEMU boots to the shell
- [ ] `docs/boot/boot-flow.md` updated
- [ ] `neodev check-deps` passes
