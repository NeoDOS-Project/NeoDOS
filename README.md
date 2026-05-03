# NeoDOS - Minimal x86-64 OS in Rust

A minimal operating system kernel built in Rust with a UEFI bootloader and comprehensive debugging support.

## Quick Start

### Build

```bash
cd neodos
bash scripts/build.sh
```

This compiles:
- UEFI bootloader (x86_64-unknown-uefi)
- 64-bit kernel (x86_64-unknown-none)
- FAT32 disk image with ESP

### Run with Debugging

**Terminal 1: Start QEMU**
```bash
bash scripts/qemu-debug.sh
```

**Terminal 2: Connect GDB**
```bash
gdb -x .gdbinit
```

**Terminal 3: View VNC**
```bash
vncviewer localhost:5900
```

## Project Structure

```
neodos/
├── neodos-bootloader/          # UEFI bootloader
│   ├── src/
│   │   ├── main.rs             # efi_main() entry
│   │   ├── memory.rs           # Memory queries
│   │   ├── file.rs             # Kernel loader
│   │   └── panic.rs            # Error handling
│   └── Cargo.toml
│
├── neodos-kernel/              # 64-bit kernel
│   ├── src/
│   │   ├── main.rs             # _start() entry
│   │   ├── arch/               # CPU intrinsics
│   │   ├── vga.rs              # VGA text output
│   │   └── cpu.rs              # CPUID info
│   ├── kernel.ld               # Linker script
│   ├── .cargo/config.toml      # Build config
│   └── Cargo.toml
│
├── scripts/
│   ├── build.sh                # Compile & create disk image
│   └── qemu-debug.sh           # Run QEMU with GDB support
│
├── .gdbinit                    # GDB configuration
│
├── PHASE1.md                   # Phase 1 overview
├── BOOTLOADER.md               # Bootloader detailed spec
├── KERNEL.md                   # Kernel detailed spec
├── DEBUG.md                    # Complete debug guide
├── ARCHITECTURE.md             # System architecture
└── README.md                   # This file
```

## Features

✅ **UEFI Bootloader**
- Loads kernel binary from ESP
- Displays memory information
- Transitions to 64-bit mode

✅ **64-bit Kernel**
- VGA text output (80×25 characters)
- CPU identification (CPUID)
- Paging state display (CR3, CR4)

✅ **Debugging**
- GDB breakpoints and inspection
- QEMU Monitor for machine state
- VNC GUI output
- Serial console logging

✅ **Automated Build**
- Single command build: `bash scripts/build.sh`
- Booteable FAT32 disk image
- Cross-platform support (Linux)

## Documentation

- [PHASE1.md](PHASE1.md) - Project overview
- [BOOTLOADER.md](BOOTLOADER.md) - Bootloader specs
- [KERNEL.md](KERNEL.md) - Kernel specs
- [DEBUG.md](DEBUG.md) - Debugging guide
- [ARCHITECTURE.md](ARCHITECTURE.md) - System architecture

## System Requirements

### Software

```bash
# Install dependencies
sudo apt update
sudo apt install \
  build-essential \
  rustup \
  qemu-system-x86 \
  ovmf \
  gdb \
  mtools \
  dosfstools

# Add Rust targets
rustup target add x86_64-unknown-uefi
rustup target add x86_64-unknown-none
```

### Hardware

- x86-64 CPU (64-bit)
- 512 MB+ RAM (for QEMU)
- ~1 GB disk space

## Build Output

After `bash scripts/build.sh`:

```
bootloader.efi     - UEFI application
kernel.bin         - Kernel binary with magic header
disk_image.img     - FAT32 booteable disk
qemu_output.log    - Debug log (after running)
```

## Expected Output

### Bootloader (Serial Console)
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

### Kernel (VGA Screen)
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

## Architecture

**Boot Flow:**
```
UEFI Firmware (OVMF)
    ↓
Bootloader @ 0x100000
    ├─ Init UEFI services
    ├─ Get memory info
    ├─ Load kernel binary
    └─ Jump to 0x200000
         ↓
Kernel @ 0x200000
    ├─ Setup VGA
    ├─ Print CPU info
    └─ Halt system
```

**Memory Layout:**
```
0x200000 - 0x3FFFFF    Kernel code/data (2MB)
0x100000 - 0x101FFF    Bootloader
0xB8000  - 0xC0000     VGA text buffer
```

## Debugging

### GDB Breakpoints

```gdb
(gdb) target remote localhost:1234
(gdb) break *0x200000        # Kernel entry
(gdb) continue
(gdb) x /32i 0x200000        # Disassemble
(gdb) p /x $cr3              # Page table root
(gdb) p /x $cr4              # Paging features
```

### QEMU Monitor

```
(monitor) info registers     # All CPU state
(monitor) x /16x 0x200000    # Memory dump
(monitor) info tlb           # TLB entries
```

## Troubleshooting

**Build fails: "target not found"**
```bash
rustup target add x86_64-unknown-uefi x86_64-unknown-none
```

**QEMU fails: "No OVMF firmware"**
```bash
sudo apt install ovmf
```

**No output from kernel**
- Check VNC connection (port 5900)
- Check GDB is connected (port 1234)
- Check QEMU Monitor for CPU state

See [DEBUG.md](DEBUG.md) for detailed troubleshooting.

## Phases

**Phase 1** (Current)
- UEFI bootloader
- Kernel entry point
- VGA output + CPU info
- Debugging infrastructure

**Phase 2** (Planned)
- Interrupt Descriptor Table (IDT)
- Exception handlers
- Timer interrupts

**Phase 3+**
- Memory allocator
- Page table management
- Multi-core support

## References

- [UEFI Specification](https://uefi.org/)
- [uefi-rs](https://github.com/rust-osdev/uefi-rs)
- [Rust Embedded](https://rust-embedded.org/)
- [OSDev Wiki](https://wiki.osdev.org/)

## License

NeoDOS is experimental software. Use at your own risk.

## Author

Developed as a learning project for x86-64 OS development in Rust.

---

**Next:** Read [PHASE1.md](PHASE1.md) for detailed documentation.
