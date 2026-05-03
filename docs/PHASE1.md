# NeoDOS Phase 1: UEFI Bootloader + Debug Kernel

## Overview

Phase 1 of the NeoDOS project establishes a bootable x86-64 system with:
- **UEFI bootloader** (Rust + `uefi-rs`) that loads a kernel binary
- **Minimal kernel** (Rust) with VGA text output and CPU debugging info
- **Automated build pipeline** with QEMU debugging support
- **Complete debugging workflow** via GDB + QEMU Monitor

## Architecture

### Boot Flow

```
BIOS/UEFI Firmware (OVMF)
    ↓
Bootloader @ 0x100000 (UEFI entry point)
    ├─ Init UEFI services
    ├─ Print memory info
    ├─ Load kernel from ESP
    └─ Jump to kernel @ 0x200000
         ↓
Kernel @ 0x200000 (_start entry point)
    ├─ Setup VGA text buffer
    ├─ Print banner & CPU info
    ├─ Display page tables (CR3/CR4)
    └─ Halt with `hlt` instruction
```

### Memory Layout

```
0x00000000 - 0x000FFFFF   | UEFI/BIOS reserved
0x00100000 - 0x001FFFFF   | Bootloader code/data
0x00200000 - 0x003FFFFF   | Kernel code/data (2MB reserved)
0x00B8000  - 0x00C0000    | VGA text buffer (80x25)
0xFEC00000 + (variable)   | Local APIC MMIO
```

## Project Structure

```
neodos/
├── neodos-bootloader/        # UEFI bootloader (Cargo binary)
│   ├── src/
│   │   ├── main.rs           # Entry point: efi_main()
│   │   ├── memory.rs         # GetMemoryMap wrapper
│   │   ├── file.rs           # Kernel binary loader
│   │   └── panic.rs          # UEFI panic handler
│   ├── Cargo.toml            # uefi + uefi-services deps
│   └── target/               # Build output
│
├── neodos-kernel/            # Bare-metal kernel (Cargo binary)
│   ├── src/
│   │   ├── main.rs           # Entry point: _start()
│   │   ├── arch/
│   │   │   └── mod.rs        # CPU intrinsics (CR3, CR4, RSP)
│   │   ├── vga.rs            # VGA text buffer (0xB8000)
│   │   ├── cpu.rs            # CPUID information
│   │   └── panic.rs (in main) # Panic handler
│   ├── kernel.ld             # Linker script
│   ├── .cargo/config.toml    # Target + rustflags
│   └── target/               # Build output
│
├── scripts/
│   ├── build.sh              # Build bootloader + kernel + disk image
│   └── qemu-debug.sh         # Run QEMU with debug support
│
├── .gdbinit                  # GDB initialization for kernel debugging
├── PHASE1.md                 # This file
├── BOOTLOADER.md             # Bootloader specs
├── KERNEL.md                 # Kernel specs
├── DEBUG.md                  # Debugging guide
└── ARCHITECTURE.md           # Boot flow diagrams
```

## Building

### Prerequisites

- Rust toolchain (rustup)
- x86_64-unknown-uefi target
- x86_64-unknown-none target
- QEMU with UEFI support (ovmf.bin)
- GDB for debugging
- Optional: mtools (for disk image)

### Build Steps

```bash
cd neodos
bash scripts/build.sh
```

This will:
1. Compile bootloader to `bootloader.efi`
2. Compile kernel to `kernel.bin` (with magic header)
3. Create FAT32 disk image `disk_image.img`
4. Copy both to ESP partition (/EFI/NeoDOS/)

Output files:
- `bootloader.efi` - UEFI PE binary
- `kernel.bin` - Kernel with 0xNEODKRN magic header
- `disk_image.img` - Booteable FAT32 image

## Running

### With QEMU Debug GUI

```bash
bash scripts/qemu-debug.sh
```

Then in another terminal:
```bash
# Connect GDB
gdb -x .gdbinit

# Or inspect QEMU Monitor
telnet localhost 4444
```

### VNC Viewer

```bash
vncviewer localhost:5900
```

You should see:
- Bootloader messages on serial console (terminal)
- Kernel boot messages on VGA screen (VNC)

## Expected Output

### Bootloader (serial console)
```
========================================
NeoDOS Bootloader v0.1
========================================

[+] Fetching memory map...
    Total memory: 512.00 MB
    Available:    491.00 MB

[+] Loading kernel binary...
    Kernel loaded: 4096 bytes
    Kernel magic: 0xneodkrn
    Magic verified ✓

[+] Copying kernel to 0x200000...
    Kernel copied ✓

[+] Exiting boot services...
[+] Jumping to kernel @ 0x200000...
```

### Kernel (VGA screen)
```
========================================
NeoDOS Kernel v0.1
========================================

[+] Kernel Entry Point
    Load address:     0x200000
    Magic:            0xneodkrn

[+] CPU Information
    Vendor:           Intel
    Brand:            Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz

[+] Paging & CPU State
    CR3 (Page root):  0x0000000000000000
    CR4 (Features):   0x00000000000b0671

[+] Stack Information
    RSP:              0x0000000080000000

========================================
NeoDOS Kernel Ready
========================================
```

## Debugging Workflow

### Setting Breakpoints in GDB

```gdb
(gdb) break *0x200000     # Kernel entry
(gdb) continue

(gdb) x /32i 0x200000     # Disassemble kernel
(gdb) p /x $cr3           # Page table root
(gdb) p /x $cr4           # Control register state

(gdb) watch $rsp          # Alert on stack changes
(gdb) bt                  # Stack trace
```

### QEMU Monitor Commands

```
(monitor) info registers   # CPU state
(monitor) info tlb        # TLB entries (paging)
(monitor) x /16x 0x200000 # Memory dump
(monitor) info irq        # Interrupt status
```

## Troubleshooting

### Build fails with "target not found"

```bash
rustup target add x86_64-unknown-uefi
rustup target add x86_64-unknown-none
```

### QEMU fails to start

```bash
# Check OVMF installation
ls /usr/share/OVMF/
# Install if missing: sudo apt install ovmf
```

### Bootloader doesn't find kernel

- Ensure `scripts/build.sh` completed successfully
- Check disk image has ESP partition with /EFI/NeoDOS/kernel.bin
- Use `mdir -i disk_image.img` to verify

### Kernel doesn't print to VGA

- Check VNC connection (localhost:5900)
- Verify bootloader jumped correctly (GDB breakpoint at 0x200000)
- Check for page table setup issues (CR3 value)

## Phase 2 Preview

Phase 2 will add:
- Interrupt Descriptor Table (IDT) setup
- Page table management (custom paging)
- Memory allocator (bump allocator)
- LAPIC/IOAPIC interrupt controller setup
- Timer interrupt handling
- Multi-core CPU startup

## References

- [UEFI Specification](https://uefi.org/sites/default/files/resources/UEFI_Spec_2_10_Aug29.pdf)
- [uefi-rs Documentation](https://github.com/rust-osdev/uefi-rs)
- [x86-64 ISA Reference](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [OSDev Wiki](https://wiki.osdev.org/)

## License

NeoDOS Phase 1 is experimental software. Use at your own risk.
