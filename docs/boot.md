# Boot Flow

## UEFI Bootloader

The bootloader resides in `neodos-bootloader/` (target `x86_64-unknown-uefi`).
UEFI firmware (OVMF for QEMU) parses the GPT, finds the ESP (FAT32), and loads
`/EFI/BOOT/BOOTX64.EFI`.

### Bootloader Steps

1. **Initialize UEFI services** — call `uefi::helpers::init()`, set up logging
2. **Initialize GOP** — open `GraphicsOutput` protocol via `GetProtocol` (shared
   open; avoids crash on ThinkPad X270). Enumerate all GOP handles if first
   handle fails. Extract framebuffer address, width, height, stride
3. **Load kernel ELF** — read `\EFI\NeoDOS\kernel.elf` from ESP via
   `SimpleFileSystem`. Parse ELF header: locate PT_LOAD segments, compute
   address range, allocate pages via `AllocateType::Address` (fallback to
   `AnyPages` if exact address fails), copy segment data, zero BSS
4. **Load NeoDOS FS image** — read `\EFI\NeoDOS\neodos.fs` from ESP, allocate
   pages via `AllocateType::AnyPages`, copy image into allocated memory
5. **Locate ACPI RSDP** — scan UEFI configuration tables for ACPI 2.0 GUID
   (preferred), fallback to ACPI 1.0 GUID
6. **ExitBootServices** — 2-second stall, then `exit_boot_services()` captures
   the final UEFI memory map (buffer leaked via `core::mem::forget`)
7. **Jump to kernel** — `cli`, transmute entry point to
   `extern "sysv64" fn(&BootInfo) -> !`, call with BootInfo reference

### BootInfo Struct

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

## Kernel Boot Phases

Sequence from `src/main.rs` `rust_start()`:

| Phase | Description | Key Code |
|-------|-------------|----------|
| 0 | Verify boot info magic + version. Halt on bad magic | `rust_start():106` |
| 1 | Graphics init, Caps Lock LED on, RAM disk setup, serial init, boot benchmark init | `:113` |
| 2 | GDT (5 selectors + TSS), IDT (exception + IRQ + INT 0x80 trampoline), MSI init, PIC remap (master 0x20, slave 0xA0) | `:148-158` |
| 3 | HPET init (ACPI RSDP -> HPET table), APIC timer calibration -> APIC timer active (1 KHz bus), PS/2 controller + USB HID init | `:160-179` |
| 2.5 | Physical memory: parse UEFI mem map, init buddy frame allocator, reserve crash dump area (16 MB @ 0x0F000000), init watchdog | `:187-194` |
| 2.75 | Kernel heap: init slab allocator + linked_list_allocator fallback | `:199` |
| 2.759 | Object Manager init: `ObObjectTable` root (Mutex<BTreeMap<ObId, ObObject>>) | `:205` |
| 2.7595 | Timer Manager init (64 slots, integrated with timer tick) | `:210` |
| 2.76 | Ob namespace init: create root `\` and standard directories (`\Global`, `\Device`, `\Registry`, `\Ob\Process`, `\Security`). Create virtual info objects in `\Global\Info\` | `:216-253` |
| 2.77 | Security subsystem init: default admin/user tokens | `:260` |
| 2.8 | SMP: per-CPU KPRCB data structures, INIT-SIPI-SIPI sequence via local APIC ICR | `:266` |
| 2.9 | IPI infrastructure: cross-CPU reschedule, TLB shootdown, call-function | `:272` |
| 2.91 | I/O APIC: detect from MADT, disable legacy PIC, route ISA IRQs | `:278` |
| 3 | **STI** (enable interrupts), then custom page tables: 4 GiB identity-map via 2 MB huge pages | `:285-292` |
| 3.0 | Demand paging: split heap 16x2 MB huge pages -> 4 KB PTEs; split mmap region -> 4 KB PTEs | `:295-297` |
| 3.1 | TEB page mapping at 0x7000 (USER_ACCESSIBLE) for SEH exception handling | `:305` |
| 3.2 | PCIe ECAM: read MCFG, map MMIO as UC-, activate ECAM | `:311` |
| 3.3 | Storage init: ATA boot stub, AHCI probe, NVMe probe, VirtIO probe. Priority NVMe > VirtIO > AHCI > ATA | `:321` |
| 3.4 | GPT scan on primary disk, create IoStacks for NeoDOS + ESP partitions, Block Cache init, Page Cache init (128x4 KB = 512 KB, hash + LRU) | `:332-371` |
| 3.4b | NeoDOS FS mount on `\Device\NeoDosVolume0` -> C: | `:396-406` |
| 3.4c | FAT32 ESP mount on `\Device\EspVolume0` -> A: | `:420-432` |
| 3.5 | Input manager init (VT subsystem, keyboard buffer) | `:441` |
| 3.80 | X4 Driver Isolation Layer: 16x1 MB slots @ 0x30000000 for NEM driver loading | `:449` |
| 3.85 | Boot Driver Loader: auto-scan and load BOOT .nem drivers first, then SYSTEM .nem drivers (dependency-sorted) | `:457` |
| 3.86 | AHCI port reclaim (BootAhci DMA register fix after NEM AHCI init) | `:464` |
| 3.87 | NEM bridges (RTC), NXL region init, hot-reload, NXL loader | `:469-472` |
| 3.88 | Networking init: e1000 NIC probe, ARP cache, `\Device\Tcp` + `\Device\Udp` namespace objects | `:480` |
| 3.881 | Registry init (Cm): create `\Registry` namespace tree, mount SYSTEM hive | `:488` |
| 3.881b | Default registry values: `CurrentControlSet\Services\NeoInit\DefaultShell`, `Network\Interfaces\0\DHCPEnabled`, `Control\WaitForNetwork` | `:497` |
| 3.9 | ABI freeze validation: `syscall::validate_abi()` asserts SSDT completeness, `abi_freeze::verify_all_frozen_abis()` checks frozen ABI structs | `:503-507` |
| 4 | Kernel self-tests (646+ tests), cmdtest.nxe load + execute, NeoInit launch (PID 1, Ring 3) | `:513-663` |

## Unified GPT Disk Layout

| Partition | Filesystem | LBA Range | Mount Point | GPT Type GUID |
|-----------|-----------|-----------|-------------|---------------|
| 1 | FAT32 (ESP) | 2048-206847 | A: | C12A7328-F81F-11D2-BA4B-00A0C93EC93B |
| 2 | NeoDOS FS | 206848-227327 | C: | EBD0A0A2-B9E5-4433-87C0-68B6B72699C7 |

The GPT is parsed by `drivers/gpt.rs` which searches for both partition types
on the primary block device. The ESP is always partition 1; the NeoDOS FS is
partition 2.

## Boot ABI

| Constant | Value | Description |
|----------|-------|-------------|
| `BOOTINFO_MAGIC` | `0x4E444F53` | "NDOS" magic in BootInfo |
| `KERNEL_VERSION_CODE` | `(10 << 8) \| 5` = `0x0A05` | v0.10.5 |
| `BOOT_VERSION` | `(10 << 8) \| 5` | Must match kernel version code |

The bootloader version (`boot_info.version`) is compared against
`KERNEL_VERSION_CODE` at kernel entry. A mismatch is non-fatal but produces
a warning line via serial. Both values are generated by `bash scripts/build.sh`
which builds `bootloader.efi` and links it into the GPT image.

## RAM Disk

If `neodos.fs` is found on the ESP, the bootloader allocates pages and copies
the filesystem image into memory, passing the address/size in `BootInfo`. The
kernel calls `drivers::block::set_ram_disk()` to register it as the `RAMDISK`
block device. If no FS image is found, the kernel expects a real disk with
a NeoDOS partition.
