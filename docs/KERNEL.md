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
9. **Storage — ATA** — init primary + secondary ATA drivers, scan PCI for IDE bus-master DMA, probe AHCI controller, set AHCI fallback on ATA driver
10. **Storage — GPT** — scan GPT on disk 0 (and disk 1), find NeoDOS partition, set `base_lba` on ATA driver
11. **Storage — NeoDOS FS** — init block cache and page cache, read superblock from LBA 0 (relative), mount NeoDOS FS on `C:` via VFS
12. **Storage — FAT32** — init FAT32 driver from ESP partition (absolute LBA), mount on `A:` via VFS
13. **Page tables** — init custom 4-level page tables: 4 GiB identity-map via 2 MB huge pages, user window (`0x400000..0x800000`) marked `USER_ACCESSIBLE`, framebuffer marked uncacheable (`NO_CACHE`), map framebuffer >4 GiB if needed
14. **Heap demand paging** — split all 16 × 2 MB heap huge pages (`0x10000000..0x12000000`) into 4 KB page tables via `init_heap_demand_paging()`
15. **Shell** — set all keyboard LEDs ON, register kernel tests (37), create and run `DosShell`

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
This is set as `base_lba` in the ATA driver, so all NeoDOS FS sector reads/writes are
transparently offset to the correct partition location.

## Shell

The shell provides DOS-like commands backed by the NeoDOS filesystem. Built-ins include:

- `HELP`, `DIR`, `TYPE`, `COPY`, `MD`, `CD`, `VOL`, `DRIVES`, `SYNC`, `CALL`
- `CPUINFO` (CPUID vendor/brand)
- `MEM` (memory stats derived from UEFI memory map)

Commands are implemented as one file per command under `src/shell/commands/`.

## Scheduler / Timer

The kernel includes a simple round-robin scheduler and a timer interrupt handler.

- An idle process is always present.
- Context switching is performed from the timer ISR once non-idle processes exist.

See `docs/SCHEDULER.md`.

