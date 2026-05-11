# NeoDOS Kernel

The NeoDOS kernel is a `#![no_std]` x86_64 bare-metal executable loaded by the UEFI bootloader from `kernel.elf`.

## Entry

Kernel entry is:

```rust
pub unsafe extern "sysv64" fn _start(boot_info: &BootInfo) -> !
```

The bootloader calls it using the System V AMD64 ABI, so `boot_info` is passed in `RDI`.

## Boot Sequence (Current)

The kernel boot flow in `neodos-kernel/src/main.rs` is roughly:

1. Initialize graphics (GOP framebuffer renderer)
2. Initialize serial output and print banner
3. Initialize VGA text mode as a fallback output
4. Initialize CPU tables:
   - GDT
   - IDT (exception handlers + IRQ handlers)
   - PIC remap/enable
5. Initialize physical memory stats from the UEFI memory map (`memory::init`)
6. Initialize storage stack:
   - ATA driver
   - Parse GPT (`drivers::gpt::find_neodos_partition`) to locate NeoDOS partition
   - Set `base_lba` on ATA driver so NeoDOS FS sees relative LBAs
   - Initialize block cache
   - Mount NeoDOS filesystem (reads superblock at `base_lba + 0`)
   - Initialize FAT32 driver (reads absolute LBA 0/2048 for boot sector)
7. Initialize custom page tables (currently identity-maps 4 GiB)
8. Start the DOS-like shell

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
- `TSR` / `DEVICES` (TSR registry integration)

Commands are implemented as one file per command under `src/shell/commands/`.

## Scheduler / Timer

The kernel includes a simple round-robin scheduler and a timer interrupt handler.

- An idle process is always present.
- Context switching is performed from the timer ISR once non-idle processes exist.

See `docs/SCHEDULER.md`.

