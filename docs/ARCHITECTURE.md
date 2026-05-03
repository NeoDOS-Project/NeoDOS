# NeoDOS Architecture

## Boot Flow Diagram

```
┌─────────────────────────────────────────────────────────────┐
│ QEMU/PC Hardware                                            │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ UEFI Firmware (OVMF)                                 │  │
│  │ - Initializes hardware                              │  │
│  │ - Sets up paging (2MB pages)                        │  │
│  │ - Creates GDT, IDT, TSS                            │  │
│  └──────┬───────────────────────────────────────────────┘  │
│         │                                                   │
│         ▼                                                   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ NeoDOS Bootloader (UEFI Application)               │  │
│  │ @ 0x100000                                          │  │
│  │                                                      │  │
│  │ efi_main():                                         │  │
│  │  1. uefi_services::init()                          │  │
│  │  2. Get memory map (GetMemoryMap)                  │  │
│  │  3. Load kernel (SimpleFileSystem)                 │  │
│  │  4. Verify magic (0xNEODKRN)                       │  │
│  │  5. Copy kernel to 0x200000                        │  │
│  │  6. exit_boot_services()                           │  │
│  │  7. jmp 0x200000                                   │  │
│  └──────┬───────────────────────────────────────────────┘  │
│         │                                                   │
│         │ Bootloader exits UEFI, owns system              │
│         │ RSI = &RuntimeServices, RBX = 0x200000          │
│         │                                                   │
│         ▼                                                   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ NeoDOS Kernel (64-bit bare metal)                  │  │
│  │ @ 0x200000                                          │  │
│  │                                                      │  │
│  │ _start():                                           │  │
│  │  1. vga::init()                                    │  │
│  │  2. Print banner & CPU info                        │  │
│  │  3. Print paging state (CR3, CR4)                  │  │
│  │  4. Print stack pointer                            │  │
│  │  5. arch::halt()                                   │  │
│  └──────┬───────────────────────────────────────────────┘  │
│         │                                                   │
│         ▼                                                   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ System Halted (ready for Phase 2)                  │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## Memory Map

```
┌────────────────────────────────────────────────────┐
│ Physical Memory Layout (x86-64)                   │
├────────────────────────────────────────────────────┤
│ 0xFFFFFFFF80000000 - 0xFFFFFFFFFFFFFFFF (kernel) │
├────────────────────────────────────────────────────┤
│ 0xFEC00000 - 0xFEC01000: Local APIC MMIO         │
├────────────────────────────────────────────────────┤
│ 0xFEE00000 - 0xFEE01000: I/O APIC MMIO           │
├────────────────────────────────────────────────────┤
│ 0x80000000 - ...         : Stack (descending)     │
├────────────────────────────────────────────────────┤
│ 0x400000 - 0x7FFFFFFF:  Heap (unused Phase 1)   │
├────────────────────────────────────────────────────┤
│ 0x300000 - 0x3FFFFF:    Kernel data (1MB)       │
├────────────────────────────────────────────────────┤
│ 0x200000 - 0x2FFFFF:    Kernel code (1MB)  ◄───│─ Kernel load addr
├────────────────────────────────────────────────────┤
│ 0xB8000  - 0xC0000:     VGA text buffer (64KB)   │
├────────────────────────────────────────────────────┤
│ 0x100000 - 0x101FFF:    Bootloader (UEFI)        │
├────────────────────────────────────────────────────┤
│ 0x00000 - 0x00FFF:      IVT/BIOS data            │
└────────────────────────────────────────────────────┘
```

## Module Dependencies

```
main.rs
  ├─→ arch/mod.rs
  │    └─→ CPU intrinsics (CR3, CR4, RSP)
  │
  ├─→ vga.rs
  │    └─→ VGA text buffer @ 0xB8000
  │
  ├─→ cpu.rs
  │    └─→ CPUID instruction
  │
  └─→ Panic handler
       └─→ vga::println!()
```

## Bootloader Architecture

```
┌─────────────────────────────────────────────┐
│ UEFI Bootloader (uefi-rs)                   │
├─────────────────────────────────────────────┤
│ main.rs                                     │
│  ├─ efi_main() entry point                 │
│  └─ Boot sequence orchestration            │
├─────────────────────────────────────────────┤
│ memory.rs                                   │
│  └─ get_memory_info()                      │
│     ├─ GetMemoryMap protocol               │
│     └─ Calculate RAM statistics            │
├─────────────────────────────────────────────┤
│ file.rs                                     │
│  └─ load_kernel_binary()                   │
│     ├─ SimpleFileSystem protocol           │
│     ├─ Open /EFI/NeoDOS/kernel.bin        │
│     └─ Return Vec<u8> with kernel data    │
├─────────────────────────────────────────────┤
│ panic.rs                                    │
│  └─ #[panic_handler]                       │
│     ├─ Print to ConOut                     │
│     └─ Halt                                 │
└─────────────────────────────────────────────┘
```

## Kernel Architecture

```
┌────────────────────────────────────────────────┐
│ Bare-Metal Kernel (x86_64-unknown-none)      │
├────────────────────────────────────────────────┤
│ main.rs                                        │
│  ├─ _start() entry point                     │
│  └─ Boot sequence orchestration              │
├────────────────────────────────────────────────┤
│ arch/mod.rs                                    │
│  ├─ read_cr3()                               │
│  ├─ read_cr4()                               │
│  ├─ read_rsp()                               │
│  ├─ halt()                                    │
│  ├─ enable_interrupts()                      │
│  └─ disable_interrupts()                     │
├────────────────────────────────────────────────┤
│ vga.rs                                         │
│  ├─ VGA text buffer driver                   │
│  ├─ print() / println!() macros              │
│  └─ Writer struct (fmt::Write impl)          │
├────────────────────────────────────────────────┤
│ cpu.rs                                         │
│  ├─ cpuid() instruction                      │
│  ├─ get_cpu_info()                           │
│  └─ CpuVendor enum                           │
└────────────────────────────────────────────────┘
```

## System Call Convention (x86-64 System V AMD64 ABI)

Used by bootloader to call kernel:

```
Register Mapping:
┌────┬──────────────────────┐
│ Reg│ Purpose              │
├────┼──────────────────────┤
│ RAX│ Return value (kernel)│
│ RBX│ Kernel load addr     │
│ RCX│ (undefined)          │
│ RDX│ (undefined)          │
│ RSI│ RuntimeServices*     │
│ RDI│ (undefined)          │
│ RSP│ Stack pointer        │
│ RBP│ Base pointer         │
└────┴──────────────────────┘

Callee-saved:
  RBX, RSP, RBP, R12-R15

Caller-saved:
  RAX, RCX, RDX, RSI, RDI, R8-R11

Return value: RAX (or RDX:RAX for 128-bit)
Return address: Pushed on stack
```

Bootloader uses inline ASM:
```rust
core::arch::asm!(
    "jmp {kernel_addr}",
    kernel_addr = in(reg) KERNEL_LOAD_ADDR,
    in("rsi") &runtime_services as *const _ as u64,
    in("rbx") KERNEL_LOAD_ADDR,
    options(noreturn)
);
```

## VGA Text Mode Layout

```
Memory @ 0xB8000 (4KB buffer):

┌──────────────────────────────────────────┐
│ Row 0: Characters 0-79                   │
│ Row 1: Characters 80-159                 │
│ ...                                      │
│ Row 24: Characters 1920-1999             │
└──────────────────────────────────────────┘

Each character (2 bytes):
┌─────────────┬──────────────────┐
│ Byte 0      │ Byte 1           │
├─────────────┼──────────────────┤
│ ASCII code  │ Color attributes │
│ (0-255)     │ FG (0-15) BG (0-7)
└─────────────┴──────────────────┘

Example: Space with white on black
  0x0720 = 0x07 (white) << 8 | 0x20 (space)

Color palette:
  0x0 = Black    0x8 = Dark Gray
  0x1 = Blue     0x9 = Light Blue
  0x2 = Green    0xA = Light Green
  0x3 = Cyan     0xB = Light Cyan
  0x4 = Red      0xC = Light Red
  0x5 = Magenta  0xD = Light Magenta
  0x6 = Brown    0xE = Yellow
  0x7 = White    0xF = Bright White
```

## CPUID Call Sequence

```
CPUID Leaf 0x00 (Vendor):
  EAX = max cpuid leaf
  EBX = first 4 bytes of vendor
  EDX = next 4 bytes
  ECX = last 4 bytes

  Result string (little-endian):
    "GenuineIntel" → Intel
    "AuthenticAMD" → AMD

CPUID Leaf 0x80000002-4 (Brand):
  EAX, EBX, ECX, EDX = 16 bytes of brand name
  Collect 3 leaves = 48 bytes total

  Example: "Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz"
```

## Register State Transitions

### At UEFI Boot Entry

```
CR0 = 0x80000011  (PG, ET, PE)    ← Paging enabled
CR2 = (undefined)
CR3 = (UEFI page table)
CR4 = 0x000b0671  (PSE, PAE, PGE, …)

RFLAGS = 0x00000246  (IF=1, interrupts enabled)
CS = 0x0008  (64-bit code segment)
DS = 0x0010  (Data segment)

GDT  ← UEFI GDT (valid)
IDT  ← UEFI IDT (valid)
```

### After Bootloader Jump

```
RAX = (undefined, bootloader return value unused)
RBX = 0x200000  (kernel load address)
RSI = &RuntimeServices  (bootloader parameter)
RSP = (UEFI boot stack)
RIP = 0x200000  (_start)

Control registers unchanged:
  CR0, CR2, CR3, CR4 still UEFI values
  Paging still active
  GDT/IDT still installed (to be replaced in Phase 2)
```

## Error Handling Flow

### Bootloader Error

```
efi_main() detects error
  ↓
Prints error to ConOut (UEFI console)
  ↓
Returns Status::LOAD_ERROR
  ↓
UEFI returns to firmware
  ↓
System reboots or hangs
```

### Kernel Error

```
Rust panic! invoked
  ↓
#[panic_handler] called
  ↓
Prints panic info to VGA buffer
  ↓
arch::halt()
  ↓
Infinite loop with HLT instructions
  ↓
Debugger can inspect state via GDB
```

## Phase Evolution

```
Phase 1 (Current): Boot → Halt
  ├─ UEFI bootloader
  ├─ Kernel entry point
  └─ VGA output + CPU info

Phase 2: Interrupts & Exceptions
  ├─ IDT setup
  ├─ Exception handlers
  ├─ Interrupt controller (LAPIC/IOAPIC)
  └─ Timer interrupt

Phase 3: Memory Management
  ├─ Custom page tables
  ├─ Memory allocator
  ├─ Virtual memory
  └─ Kernel heap

Phase 4: Multitasking
  ├─ Process/thread structures
  ├─ Context switching
  ├─ Scheduler
  └─ IPC (Inter-Process Communication)

Phase 5+: Drivers, Filesystems, etc.
```

## Thread/Interrupt Safety (Phase 1)

**Phase 1 has no threading or interrupts enabled**, so:
- No locks needed
- No atomics needed
- Single execution context
- Bootloader fully owns system

Phase 2 will introduce:
- Interrupt handlers (atomic operations)
- Spinlocks (for shared data)
- Memory barriers

## Build System Overview

```
Cargo projects:
  neodos-bootloader/
    ├─ Cargo.toml (uefi, uefi-services, log)
    ├─ src/
    └─ target/x86_64-unknown-uefi/release/
         └─ neodos_bootloader.efi

  neodos-kernel/
    ├─ Cargo.toml (x86_64, log)
    ├─ .cargo/config.toml (custom linker, rustflags)
    ├─ kernel.ld (linker script)
    ├─ src/
    └─ target/x86_64-unknown-none/release/
         └─ neodos_kernel (ELF)

Build pipeline (scripts/build.sh):
  1. cargo build bootloader → bootloader.efi
  2. cargo build kernel → neodos_kernel
  3. objcopy kernel → kernel.bin (raw binary)
  4. Prepend magic 0xNEODKRN
  5. dd + mkfs.fat create disk image
  6. mcopy files to FAT32 partition
```

## References

- [System V x86-64 ABI](https://refspecs.linuxbase.org/elf/x86_64-abi-0.99.pdf)
- [UEFI Specification](https://uefi.org/sites/default/files/resources/UEFI_Spec_2_10_Aug29.pdf)
- [Intel x86-64 ISA](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [OSDev Boot Sequence](https://wiki.osdev.org/Boot_Sequence)
- [VGA Text Mode](https://wiki.osdev.org/Text_Mode_Cursor)

---

**Document version:** 0.1 (Phase 1)
**Last updated:** 2026-04-29
