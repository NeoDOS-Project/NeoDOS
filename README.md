# NeoDOS - Minimal x86-64 OS in Rust

A minimal operating system written in Rust with a UEFI bootloader, a simple filesystem-backed DOS-like shell, and QEMU/GDB debugging support.

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

### Run (QEMU)

```bash
bash scripts/qemu-debug.sh
```

This launches QEMU with serial output redirected to your terminal and saves the full log to `neodos/qemu_output.log`.

### Debug (GDB)

```bash
gdb -x .gdbinit
```

## Project Structure

```
neodos/
├── neodos-bootloader/          # UEFI bootloader
│   ├── src/
│   │   └── main.rs             # efi_main(), loads kernel ELF and passes memory map
│   └── Cargo.toml
│
├── neodos-kernel/              # 64-bit kernel
│   ├── src/
│   │   ├── main.rs             # _start() entry
│   │   ├── arch/               # CPU intrinsics
│   │   ├── vga.rs              # VGA text output
│   │   ├── cpu.rs              # CPUID info (vendor/brand)
│   │   ├── memory.rs           # Physical memory stats (for MEM command)
│   │   └── shell/              # DOS-like shell + commands
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
├── docs/                       # Design notes + specs
│   ├── PHASE1.md               # Phase 1 overview
│   ├── BOOTLOADER.md           # Bootloader spec
│   ├── KERNEL.md               # Kernel spec
│   ├── DEBUG.md                # Debugging guide
│   └── ARCHITECTURE.md         # System architecture
└── README.md                   # This file
```

## Features

✅ **UEFI Bootloader**
- Loads kernel ELF from the ESP
- Exits boot services and passes the final UEFI memory map to the kernel

✅ **64-bit Kernel**
- VGA text output + serial logging
- CPU identification via `CPUID` (`CPUINFO` shell command)
- Basic physical memory accounting from the UEFI memory map (`MEM` shell command)
- DOS-like shell with basic filesystem operations

✅ **Debugging**
- GDB breakpoints and inspection
- QEMU Monitor for machine state
- Serial console logging

✅ **Automated Build**
- Single command build: `bash scripts/build.sh`
- Booteable FAT32 disk image
- Cross-platform support (Linux)

## Documentation

- [`docs/PHASE1.md`](docs/PHASE1.md) - Project overview
- [`docs/BOOTLOADER.md`](docs/BOOTLOADER.md) - Bootloader spec
- [`docs/KERNEL.md`](docs/KERNEL.md) - Kernel spec
- [`docs/DEBUG.md`](docs/DEBUG.md) - Debugging guide
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) - System architecture

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
kernel.bin         - Kernel binary (built from kernel ELF)
disk_image.img     - FAT32 booteable disk
qemu_output.log    - Debug log (after running)
```

## Expected Output

### Bootloader (Serial Console)
```
========================================
NeoDOS Bootloader v0.6
========================================

[+] Initializing GOP...
[✓] Graphics: 1280x800 @ 0x80000000
[+] Loading kernel ELF...
[✓] Kernel loaded. Entry: 0x200000
[+] Exiting boot services...
```

### Kernel (Serial Console)
```
========================================
NeoDOS Kernel v0.5 - Modern ELF Edition
========================================

[+] Starting NeoDOS Shell...
NeoDOS v0.6 - FS Started
Type HELP for a list of commands.

C:\>
```

## Architecture

**Boot Flow:**
```
UEFI Firmware (OVMF)
    ↓
Bootloader @ 0x100000
    ├─ Init UEFI services
    ├─ Load kernel ELF
    ├─ Exit boot services + capture memory map
    └─ Jump to 0x200000
         ↓
Kernel @ 0x200000
    ├─ Init CPU tables (GDT/IDT/PIC)
    ├─ Init memory stats from UEFI memory map
    ├─ Mount NeoDOS FS
    └─ Run shell
```

**Memory Notes:**
```
- Kernel link address starts at 0x200000 (see `neodos-kernel/kernel.ld`).
- Current paging setup identity-maps the first 4 GiB; memory stats reflect that.
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

## Shell Commands

At the `C:\>` prompt:

- `HELP` shows built-in commands.
- `CPUINFO` prints CPU vendor/brand string from `CPUID`.
- `MEM` prints total/usable/free memory derived from the UEFI memory map.

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
- Check GDB is connected (port 1234) if you are debugging
- Check QEMU Monitor for CPU state

See [`docs/DEBUG.md`](docs/DEBUG.md) for detailed troubleshooting.

## Phases

**Phase 1** (Current)
- UEFI bootloader
- Kernel entry point
- Serial/VGA output + CPU info
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

**Next:** Read [`docs/PHASE1.md`](docs/PHASE1.md) for detailed documentation.
