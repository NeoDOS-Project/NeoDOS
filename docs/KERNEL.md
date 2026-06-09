# NeoDOS Kernel

The NeoDOS kernel is a `#![no_std]` x86_64 bare-metal executable loaded by the UEFI bootloader from `kernel.elf`.

## Entry

Kernel entry is:

```rust
pub unsafe extern "sysv64" fn _start(boot_info: &BootInfo) -> !
```

The bootloader calls it using the System V AMD64 ABI, so `boot_info` is passed in `RDI`.

## Boot Sequence (Current)

The kernel boot flow in `neodos-kernel/src/main.rs` is:

0. **Verify boot info** — check magic (`0x4E444F53` = "NDOS") and bootloader version
1. **Graphics + RAM disk** — init GOP framebuffer renderer, set Caps Lock LED, store RAM disk base/size from bootloader
2. **Serial** — init serial output, print bootloader version (compatibility check)
3. **Console** — init VGA text mode fallback, set Caps+Num Lock LEDs, print kernel banner
4. **CPU structures** — GDT, IDT (exception + IRQ handlers + INT 0x80 trampoline), PIC remap/enable
5. **Input** — init PS/2 controller, init USB HID keyboard
6. **Physical memory** — parse UEFI memory map, init frame allocator bitmap (`memory::init`)
7. **Kernel heap** — init `linked_list_allocator` (`allocator::init`) — enables `Box`/`Vec`/`String`
8. **Enable interrupts** — STI (timer IRQ0 can fire immediately)
9. **Storage — ATA boot stub** — init ATA PIO boot stub (`BootAta`), probe AHCI controller, probe NVMe controller. NEM v3 standalone ATA driver (DMA+PIO) loaded later at PHASE 3.85.
10. **Storage — GPT** — scan GPT on disk 0 (and disk 1), find NeoDOS partition, set `base_lba` on block device driver
11. **Storage — NeoDOS FS** — init block cache and page cache, read superblock from LBA 0 (relative), mount NeoDOS FS on `C:` via VFS
12. **Storage — FAT32** — init FAT32 driver from ESP partition (absolute LBA), mount on `A:` via VFS
13. **Page tables** — init custom 4-level page tables: 4 GiB identity-map via 2 MB huge pages, user window (`0x400000..0x800000`) marked `USER_ACCESSIBLE`, framebuffer marked uncacheable (`NO_CACHE`), map framebuffer >4 GiB if needed
14. **Heap demand paging** — split all 16 × 2 MB heap huge pages (`0x10000000..0x12000000`) into 4 KB page tables via `init_heap_demand_paging()`
15. **Mmap demand paging** — split mmap region (`0x20000000..0x22000000`) into 4 KB page tables via `init_mmap_demand_paging()`
16. **Driver Bootstrap** — init Driver Runtime, register built-in drivers (null, echo, timer_listener) (PHASE 3.75), load BOOT + SYSTEM NEM v3 drivers via boot loader (PHASE 3.85), reclaim AHCI port after NEM AHCI overwrites HBA PORT_CLB/PORT_FB (PHASE 3.86)
17. **NXL region init** — split NXL region (`0x1e000000..0x1e200000`) into 4 KB page tables, load `libneodos.nxl` at slot 0 (PHASE 3.87)
18. **Syscall ABI validation** — validate syscall dispatch table coverage at boot
19. **Shell** — set all keyboard LEDs ON, register kernel tests (301), create and run `DosShell`

### GPT Layout (single disk)

```
sudo parted disk_image.img unit s print
Model:  (file)
Disk disk_image.img: 249856s
Partition table: gpt

Number  Start    End      Size     Type     Name
 1      2048s    206847s  204800s  fat32    EFI System (ESP)
 2      206848s  227327s  20480s   msftdata NeoDOS Filesystem
```

The kernel's GPT parser (`drivers/gpt.rs`) scans the partition table on the master drive
for type GUID `EBD0A0A2-B9E5-4433-87C0-68B6B72699C7` and returns the partition's start LBA.
This is set as `base_lba` in the block device driver (BootAta or NEM ATA), so all NeoDOS FS
sector reads/writes are transparently offset to the correct partition location.

## Shell

The shell provides DOS-like commands backed by the NeoDOS filesystem. Built-ins include:

- `HELP`, `DIR`, `TYPE`, `COPY`, `MD`, `CD`, `VOL`, `DRIVES`, `SYNC`, `CALL`, `RD`, `DEL`, `REN`
- `CPUINFO` (CPUID vendor/brand)
- `MEM` (memory stats derived from UEFI memory map)
- `PS` (list processes with PID, state, priority, handles)
- `PRI <pid> <level>` (set process priority 0-3)
- `KOBJ` (list kernel objects tracked by KOBJ manager)
- `NDREG LIST|SHOW|QUERY|RUNTIME|HEALTH|DEBUG|LOAD` (driver registry CLI)
- `LOADLIB <path>` (load shared library NXL)
- `FSCK [drive:] [/F]` (filesystem integrity check)
- `SHUTDOWN` / `POWEROFF` / `EXIT` (system shutdown via ACPI)
- `KILL <pid>` (terminate process)
- `KEYB US|SP` (switch keyboard layout)
- `DATE`, `TIME`, `VER`
- `test` (run 301 kernel self-tests + 4 user-mode binaries)

Commands are implemented as one file per command under `src/shell/commands/`.

## Scheduler / Timer

The kernel includes a **priority scheduler** (A2) with 4 levels and dynamic time-slicing.

- **Priority levels**: HIGH (400 ticks), ABOVE_NORMAL (200 ticks), NORMAL (100 ticks, default), IDLE (50 ticks)
- **Algorithm**: `schedule()` scans by priority level (HIGH→IDLE), round-robin within the same level
- **Preemption**: `timer_handler_inner` detects Ring 3 (CS=0x1B), saves RSP, calls `schedule()`, updates TSS.RSP0
- **Aging**: every 100 ticks, processes Ready for >= 1000 ticks receive a priority boost (prevents starvation)
- **sys_yield** (RAX=2): Running→Ready + reset time slice + force reschedule
- An idle process (PID 0) is always present

See `docs/SCHEDULER.md`.

