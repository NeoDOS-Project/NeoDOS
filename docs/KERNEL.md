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
4. **CPU structures** — GDT, IDT (exception + IRQ handlers + INT 0x80 trampoline), MSI subsystem init, PIC remap
5. **Timers** — HPET init (ACPI RSDP → HPET table), APIC timer calibration → APIC timer active
6. **Input** — init PS/2 controller, init USB HID keyboard
7. **Physical memory** — parse UEFI memory map, init buddy frame allocator (`memory::init`)
8. **Kernel heap** — init slab allocator + `linked_list_allocator` fallback (`allocator::init`)
9. **SMP** — INIT-SIPI-SIPI sequence, per-CPU KPRCB via GS base
10. **IPI** — cross-CPU reschedule, TLB shootdown, call-function
11. **I/O APIC** — detect from MADT, disable legacy PIC, route ISA IRQs 0/1/4/12 to vectors 32/33/36/44
12. **Enable interrupts** — STI (timer IRQ0 can fire immediately, via APIC timer LVT)
13. **Custom page tables** — 4 GiB identity-map via 2 MB huge pages, user window, framebuffer UC-, framebuffer >4 GiB
14. **PCIe ECAM** — read MCFG from ACPI, map ECAM MMIO region as UC-, activate ECAM (PIO fallback if no MCFG)
15. **Heap demand paging** — split all 16 × 2 MB heap huge pages → 4 KB PTEs
16. **Mmap demand paging** — split mmap region → 4 KB PTEs
17. **Storage — ATA boot stub** — init ATA PIO boot stub (`BootAta`), probe AHCI controller, probe NVMe controller. NEM v3 standalone ATA driver (DMA+PIO) loaded later at PHASE 3.85.
18. **Storage — GPT** — scan GPT on disk 0 (and disk 1), find NeoDOS partition, set `base_lba` on block device driver
19. **Storage — NeoDOS FS** — init block cache and page cache, read superblock from LBA 0 (relative), mount NeoDOS FS on `C:` via VFS
20. **Storage — FAT32** — init FAT32 driver from ESP partition (absolute LBA), mount on `A:` via VFS
21. **Driver Bootstrap** — init Driver Runtime, register built-in drivers (null, echo, timer_listener) (PHASE 3.75), load BOOT + SYSTEM NEM v3 drivers via boot loader (PHASE 3.85), reclaim AHCI port after NEM AHCI overwrites HBA PORT_CLB/PORT_FB (PHASE 3.86)
22. **NXL region init** — split NXL region (`0x1e000000..0x1e200000`) into 4 KB page tables, load `libneodos.nxl` at slot 0 (PHASE 3.87)
23. **Syscall ABI + freeze validation** — validate syscall dispatch table coverage, verify frozen event/capability/IOAPIC ABIs at boot
24. **Shell** — set all keyboard LEDs ON, register kernel tests (512), launch NeoInit (PID 1, Ring 3)

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

The shell runs entirely in Ring 3 as `neoshell.nxe` (spawned by NeoInit, PID 1). All interactive commands are `.NXE` binaries dispatched via PATH resolution. The kernel has no Ring 0 shell.

Commands available via `neoshell` (built-in): `CWD`, `SET`, `EXIT`, `POWEROFF`, `CALL`. External commands (`.NXE` resolved from `C:\Programs\`): `CD`, `DIR`, `HELP`, `CPUINFO`, `MEM`, `VOL`, `TYPE`, `CLS`, `ECHO`, `COPY`, `DEL`, `REN`, `MD`, `RD`, `TREE`, `VER`, `DATETIME`, `DRIVES`, `PS`, `KILL`, `PRI`, `KEYB`, `KOBJ`, `LABEL`, `FSCK`, `NDREG`, `LOADNEM`.

All commands are implemented as individual Rust projects under `userbin/`.

## Scheduler / Timer

The kernel includes a **priority scheduler** (A2) with 4 levels and dynamic time-slicing.

- **Priority levels**: HIGH (400 ticks), ABOVE_NORMAL (200 ticks), NORMAL (100 ticks, default), IDLE (50 ticks)
- **Algorithm**: `schedule()` scans by priority level (HIGH→IDLE), round-robin within the same level
- **Preemption**: `timer_handler_inner` detects Ring 3 (CS=0x1B), saves RSP, calls `schedule()`, updates TSS.RSP0
- **Aging**: every 100 ticks, processes Ready for >= 1000 ticks receive a priority boost (prevents starvation)
- **sys_yield** (RAX=2): Running→Ready + reset time slice + force reschedule
- An idle process (PID 0) is always present

See `docs/SCHEDULER.md`.

## Asynchronous Procedure Calls (APC)

The APC engine (A4.5) provides per-thread queues for kernel and user-mode APCs,
modelled after NT's `KeInitializeApc` / `KeInsertQueueApc`.

- **APC structure**: `ApcEntry { function: ApcFn, context: *mut u8, kernel: bool }`
- **Kernel APCs**: Queued per-thread (max 64), dispatched at PASSIVE_LEVEL
  from `apc_dispatch_on_syscall_return()` (called before IRETQ in the syscall handler).
  Used for IRP completion cleanup and deferred work.
- **User APCs**: Queued per-thread (max 64), dispatched one-at-a-time in Ring 3
  context before returning to user-mode. An alertable wait (`sys_wait_alertable`,
  RAX=40) blocks the thread but wakes immediately if a user APC is queued.
- **IRP→APC bridge**: `irp_complete_with_apc()` extracts the IRP callback and
  delivers it as a user APC. The completion flow is: DIRQL → DPC → APC.
- **Syscalls**: `sys_wait_alertable` (RAX=40) and `sys_sleep_ex` (RAX=41).
- **Integration**: `apc_dispatch_on_syscall_return()` runs in the syscall
  assembly handler (`idt.rs`) before IRETQ, dispatching kernel APCs then one
  user APC per syscall return.
- **File**: `src/apc/mod.rs`
- **Tests**: 5 APC-specific tests (kernel dispatch, alertable wait, queue overflow,
  IRP→APC completion, stress 100 concurrent IRPs).
