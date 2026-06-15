# NeoDOS — Minimal x86-64 OS in Rust

A minimal operating system written in Rust with a UEFI bootloader, NeoDOS filesystem, DOS-like shell, demand paging, user-mode processes, driver framework, and QEMU/GDB debugging.

**Version**: [v0.16.7](CHANGELOG.md) — 196 kernel tests + 4 user-mode binaries

## Quick Start

```bash
cd neodos
bash scripts/build.sh              # bootloader + kernel + GPT disk image
bash scripts/build.sh --neodos-image   # + NeoDOS FS image + user binaries
bash scripts/qemu-debug.sh         # QEMU + OVMF, serial to stdout, GDB :1234
gdb -x .gdbinit                    # from neodos/, connects to QEMU
python3 scripts/auto_test.py       # Automated headless test runner
```

```bash
QEMU_ACCEL=kvm bash scripts/qemu-debug.sh   # KVM acceleration (default: TCG)
QEMU_ACCEL=kvm python3 scripts/auto_test.py
```

## Project Structure

```
neodos/
├── neodos-bootloader/          # UEFI bootloader (x86_64-unknown-uefi)
│   └── src/main.rs             # efi_main(), loads kernel ELF, passes BootInfo
├── neodos-kernel/              # 64-bit kernel (x86_64-unknown-none)
│   ├── src/
│   │   ├── main.rs             # _start() entry, boot sequence (6 phases)
│   │   ├── arch/x64/           # GDT, IDT, PIC, paging, entry, serial
│   │   ├── hal/                # HAL ABI v0.3 (26 primitives: CPU, port I/O, mem, IRQ, time)
│   │   ├── scheduler.rs        # Round-robin scheduler with Blocked states
│   │   ├── syscall.rs          # INT 0x80 dispatch (21 syscalls)
│   │   ├── usermode.rs         # Ring 3 trampoline (IRETQ), exit-to-kernel
│   │   ├── memory.rs           # Frame allocator (bitmap, 4 GiB)
│   │   ├── elf.rs              # ELF64 loader (PT_LOAD, .bss)
│   │   ├── pipe.rs             # IPC pipes (16 static buffers, refcounted, blocking reads)
│   │   ├── vfs.rs              # Virtual filesystem layer
│   │   ├── input.rs            # Lock-free PS/2 ring buffer (1024 bytes)
│   │   ├── eventbus/           # Event Bus v1 (SPSC ring buffer, 12 event types)
│   │   ├── devices/            # Device model + HAL binding layer (32-slot registry)
│   │   ├── drivers/            # Driver runtime, certification pipeline, boot loader
│   │   ├── nem/                # NEM v3 driver format parser
│   │   ├── fs/                 # NeoDOS filesystem (inode-based)
│   │   ├── shell/              # DOS-like shell (commands, handler, history, autocomplete)
│   │   ├── tsr/                # TSR (Terminate-and-Stay-Resident) modules
│   │   ├── testing.rs          # In-kernel test framework (24 suites, 245+ tests)
│   │   └── ...
│   ├── kernel.ld               # Linker script (entry @ 0x200000)
│   └── .cargo/config.toml      # Build config (static relocation, rust-lld)
├── libneodos/                  # no_std user-mode library for Ring 3 Rust programs
│   └── src/                    # syscall wrappers, IO, FS, mem, macros
├── drivers/                    # NEM driver sources (ps2kbd, serial, reference drivers)
├── scripts/
│   ├── build.sh                # Compile bootloader + kernel + GPT disk image
│   ├── qemu-debug.sh           # Run QEMU with GDB support
│   ├── auto_test.py            # Headless automated test runner
│   ├── create_neodos_image.py  # Build NeoDOS FS image
│   └── create_gpt_image.py     # Combine ESP + NeoDOS into GPT disk
├── userbin/                    # User-mode test binaries (hello, systest, filetest, alltest)
├── docs/                       # Design notes + specifications
│   ├── ARCHITECTURE.md         # System architecture
│   ├── ARCHITECTURE_SOURCE_OF_TRUTH.md  # Architecture invariants & source of truth
│   ├── KERNEL.md               # Kernel spec
│   ├── BOOTLOADER.md           # Bootloader spec
│   ├── HAL_ABI.md              # Hardware Abstraction Layer spec
│   ├── SCHEDULER.md            # Scheduler design
│   ├── SYSCALLS.md             # Syscall reference
│   ├── DEBUG.md                # Debugging guide
│   └── IMPROVEMENTS.md         # Ordered improvement roadmap
├── CHANGELOG.md                # Version history
├── AGENTS.md                   # Development guide
├── .gdbinit                    # GDB configuration
└── README.md                   # This file
```

## Features

### Kernel Core
- **UEFI Bootloader** — loads kernel ELF from ESP, exits boot services, passes memory map
- **64-bit Kernel** — nightly Rust, custom linker, static reloc model, rust-lld
- **HAL ABI v0.3** — 26 primitives for CPU, port I/O, page memory, IRQ, timing; architecture backend in `hal/x64/`
- **Demand Paging** — 4 KB page granularity for user heap (`0x10000000`), lazy allocation on page fault
- **mmap Lazy** — anonymous + file-backed mappings in dedicated region (`0x20000000`), per-process VMA list
- **Heap** — 16 MB (`0x1000000`) with `linked_list_allocator`; `Box`, `Vec`, `String` available

### Interrupts & Scheduling
- **IDT** — exception handlers, IRQ handlers (PIT timer, PS/2 keyboard, COM1 serial)
- **PIC** — master/slave remapping (IRQ0→32)
- **Round-robin Scheduler** — 16 process slots, Blocked states (pipe wait), `NEED_RESCHED` flag
- **Event Bus v1** — lock-free SPSC 64-slot ring buffer, 12 event types, dispatched from idle loop

### User Mode
- **Ring 3 Processes** — IRETQ trampoline, ELF64 + flat binary loading
- **21 Syscalls** (INT 0x80) — `exit`, `write`, `read`, `open`, `readfile`, `writefile`, `close`, `yield`, `getpid`, `waitpid`, `pipe`, `dup2`, `brk`, `mmap`, `munmap`
- **`libneodos`** — `no_std` Rust standard library for user-mode: syscall wrappers, IO (`print!`/`eprintln!`), filesystem (`File::open/read/write`), memory (`brk`/`mmap`)
- **IPC Pipes** — 16 static 4 KB buffers, refcounted, blocking reads, `dup2` redirection

### Driver Framework
- **NEM v3 Format** — NeoDOS Driver Model: 80-byte header with 4 sections (text/rodata/data/bss), relocation entries, symbol table, string table, ABI validation, categories (Boot/System/Demand)
- **Certification Pipeline** — 7-state lifecycle: Loaded → Initialized → Registered → Bound → Active → Faulted/Unloaded
- **Boot Driver Loader** — automatic loading from `C:\SYSTEM\DRIVERS\BOOT\` and `C:\SYSTEM\DRIVERS\SYSTEM\`
- **PS/2 Keyboard Driver** (NEM v3) — layout switching (US/SP), compose keys, Event Bus integration
- **Serial Driver** (NEM v3) — COM1 IRQ4, Event Bus serial data, loopback
- **Reference Drivers** — storage, framebuffer, PS/2 keyboard (for validation)
- **NDREG CLI** — driver registry management: LIST, SHOW, QUERY, RUNTIME, HEALTH, DEBUG, LOAD

### Filesystem
- **NeoDOS FS** — inode-based, 256-byte inodes, 12 direct block pointers, 4 KB blocks
- **VFS Layer** — unified `FileSystem` trait, `remove_file()`, `remove_dir()`, `rename()`
- **FSCK** — integrity checking with optional repair (`/F`)
- **GPT Disk** — ESP (FAT32) + NeoDOS FS in single GPT image; kernel parses GPT at boot

### Shell (DOS-like)
- Commands: `DIR`, `TYPE`, `ECHO`, `RUN`, `DEL`, `REN`, `RD`, `FSCK`, `NDREG`, `KEYB`, `KILL`, `DATE`, `TIME`, `CLS`, `VER`, `HELP`, `CPUINFO`, `MEM`, `TEST`, `DEVICES`, `TSR`
- TAB autocomplete (commands + file paths)
- Command history (32 entries, ↑/↓)
- Pipeline support (`CMD1 | CMD2`) via pipes + dup2

### Storage
- **AHCI** — DMA polling, ATA READ/WRITE DMA EXT, ATAPI PACKET, multi-sector (up to 8 sectors)
- **ATA Bus-Master DMA** — PCI IDE controller scan, PRDT scatter-gather, polling-based
- **FAT32** — read-only from absolute LBAs

### Testing
- **245+ kernel tests** in 24 suites (NeoFS, NEM, ELF, Event Bus, pipes, mmap, FSCK, driver lifecycle, boot loader, reference drivers, stress, etc.)
- **8 user-mode binaries** — `HELLO.NXE`, `SYSTEST.NXE`, `FILETEST.NXE`, `ALLTEST.NXE`, `CPUTEST.NXE`, `TEST.NXE`, `CPUINFO.NXE`, `DIR.NXE`
- **Automated runner** — `python3 scripts/auto_test.py`

## Documentation

| Document | Description |
|----------|-------------|
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | System architecture |
| [`docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`](docs/ARCHITECTURE_SOURCE_OF_TRUTH.md) | Architecture invariants & source of truth |
| [`docs/KERNEL.md`](docs/KERNEL.md) | Kernel specification |
| [`docs/BOOTLOADER.md`](docs/BOOTLOADER.md) | Bootloader specification |
| [`docs/HAL_ABI.md`](docs/HAL_ABI.md) | Hardware Abstraction Layer ABI v0.3 |
| [`docs/SCHEDULER.md`](docs/SCHEDULER.md) | Scheduler design |
| [`docs/SYSCALLS.md`](docs/SYSCALLS.md) | Complete syscall reference |
| [`docs/DEBUG.md`](docs/DEBUG.md) | Debugging guide |
| [`docs/IMPROVEMENTS.md`](docs/IMPROVEMENTS.md) | Ordered improvement roadmap |
| [`CHANGELOG.md`](CHANGELOG.md) | Version history |
| [`AGENTS.md`](AGENTS.md) | Development guide |

## System Requirements

### Software

```bash
sudo apt install build-essential rustup qemu-system-x86 ovmf gdb mtools dosfstools util-linux
rustup target add x86_64-unknown-uefi x86_64-unknown-none
```

### Hardware

- x86-64 CPU
- 512 MB+ RAM (for QEMU)
- ~1 GB disk space

## Build Output

After `bash scripts/build.sh`:

```
bootloader.efi          — UEFI application
kernel.elf              — Kernel ELF (entry @ 0x200000)
disk_image.img          — GPT disk image (ESP FAT32 + NeoDOS FS)
qemu_output.log         — Debug log (after running QEMU)
```

## Expected Output

### Bootloader (Serial Console)
```
========================================
NeoDOS Bootloader v0.10
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
NeoDOS Kernel v0.16
========================================

[+] Starting NeoDOS Shell...
NeoDOS v0.16 - FS Started
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
    ├─ Load kernel ELF (0x200000)
    ├─ Exit boot services + capture memory map
    └─ Jump to 0x200000
         ↓
Kernel @ 0x200000
    ├─ Phase 1: CPU tables (GDT/IDT/PIC)
    ├─ Phase 2: Serial + framebuffer
    ├─ Phase 3: Memory map + frame allocator + paging + heap
    ├─ Phase 3.5: Page table split (4K user heap + mmap regions)
    ├─ Phase 3.75: Device model + HAL binding
    ├─ Phase 3.85: Boot driver loader
    ├─ Phase 4: GPT parse + NeoDOS FS mount
    ├─ Phase 5: Scheduler + syscall init
    └─ Phase 6: Shell
```

**Memory Layout:**
```
0x00000000 — 0x000FFFFF  Zero page + IVT + BDA + EBDA
0x00100000 — 0x001FFFFF  Bootloader
0x00200000 — 0x00FFFFFF  Kernel (.text, .rodata, .data, .bss)
0x01000000 — 0x01FFFFFF  Kernel heap (16 MB, linked_list_allocator)
0x02000000 — 0x0FFFFFFF  Identity-mapped (page tables, ACPI, framebuffer, etc.)
0x10000000 — 0x11FFFFFF  User heap (demand-paged 4K, 32 MB, per-process)
0x20000000 — 0x21FFFFFF  mmap region (demand-paged 4K, 32 MB, per-process)
0x00400000 — 0x007FFFFF  User code + stack (flat binary / ELF loaded here)
```

**Subsystem Dependency Rules:**

16 explicit subsystems with controlled dependencies. See [`docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`](docs/ARCHITECTURE_SOURCE_OF_TRUTH.md).

## Shell Commands

| Command | Description |
|---------|-------------|
| `HELP` | List built-in commands |
| `DIR [path]` | List directory contents |
| `TYPE <file>` | Display file contents |
| `ECHO <text>` | Print text (supports `>>` redirection) |
| `RUN <binary>` | Execute user-mode binary |
| `DEL <file>` | Delete file |
| `REN <old> <new>` | Rename file |
| `RD <dir>` | Remove empty directory |
| `FSCK [drive:] [/F]` | Check filesystem integrity (with optional repair) |
| `NDREG <subcommand>` | Driver registry management (LIST, SHOW, QUERY, RUNTIME, HEALTH, DEBUG, LOAD) |
| `KEYB <US\|SP>` | Switch keyboard layout |
| `KILL <pid>` | Terminate process |
| `DATE` / `TIME` | Show system date/time |
| `CLS` | Clear screen |
| `VER` | Show version |
| `CPUINFO` | Show CPU vendor/brand |
| `MEM` | Show memory stats |
| `TEST` | Run in-kernel test suite + user-mode binaries |
| `DEVICES` | Show device model table |
| `TSR` | List TSR modules |

## License

NeoDOS is experimental software. Use at your own risk.
