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
   - Block cache
   - Mount NeoDOS filesystem
7. Initialize custom page tables (currently identity-maps 4 GiB)
8. Start the DOS-like shell

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

