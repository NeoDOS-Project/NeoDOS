# NeoDOS Kernel Specification

## Overview

The NeoDOS kernel is a minimal 64-bit bare-metal application that:
1. Sets up VGA text mode output
2. Displays system information (CPU, memory, paging)
3. Demonstrates successful boot
4. Halts cleanly for debugging

## Entry Point: `_start()`

```rust
#[no_mangle]
pub unsafe extern "sysv64" fn _start() -> !
```

**Calling convention:** x86-64 System V AMD64 ABI (used by bootloader)

**Behavior:**
- Initializes VGA text buffer
- Displays kernel banner
- Prints CPU information via CPUID
- Prints paging configuration (CR3, CR4)
- Prints stack pointer location
- Halts indefinitely

**Never returns** (marked with `!` type)

## Register State at Entry

From bootloader:
```
RSI   = &RuntimeServices (from bootloader)
RBX   = 0x200000 (kernel load address)
RAX   = (undefined)
RDX   = (undefined)
RSP   = UEFI boot stack (~256MB)

CR0   = paging enabled, protected mode
CR3   = UEFI page table root
CR4   = paging features enabled
```

## Boot Flow

### 1. Initialize VGA Text Buffer

**Function:** `vga::init()`

Sets up:
- VGA memory base address: `0xB8000`
- Character buffer dimensions: 80 columns × 25 rows
- Text mode color (white on black)
- Internal row/column tracking

### 2. Clear Screen

**Function:** `vga::clear_screen()`

Fills entire VGA buffer with spaces (0x0720 = space with default colors).

### 3. Print Kernel Banner

```
========================================
NeoDOS Kernel v0.1
========================================
```

Uses `vga::println!()` macro to output text.

### 4. Print Entry Information

```
[+] Kernel Entry Point
    Load address:     0x200000
    Magic:            0xneodkrn
```

Demonstrates bootloader successfully handed control.

### 5. Query CPU Information

**Function:** `cpu::get_cpu_info()`

Uses CPUID instruction (leaf 0x00) to get:
- **Vendor ID:** "GenuineIntel", "AuthenticAMD", or unknown
- **Brand string:** Uses CPUID leaves 0x80000002-0x80000004

**Output:**
```
[+] CPU Information
    Vendor:           Intel
    Brand:            Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz
```

### 6. Print Paging State

**Function:** `arch::read_cr3()`, `arch::read_cr4()`

Uses inline assembly to read control registers:
```
CR3 = Page table base address
CR4 = Paging features (PSE, PAE, PGE, etc.)
```

**Output:**
```
[+] Paging & CPU State
    CR3 (Page root):  0x0000000000000000
    CR4 (Features):   0x00000000000b0671
```

### 7. Print Stack Pointer

**Function:** `arch::read_rsp()`

Reads RSP register to show current stack location.

**Output:**
```
[+] Stack Information
    RSP:              0x0000000080000000
```

### 8. Display Ready Message

```
========================================
NeoDOS Kernel Ready
========================================
```

### 9. Halt System

**Function:** `arch::halt()`

Infinite loop with HLT instructions:
```rust
loop {
    unsafe { core::arch::asm!("hlt"); }
}
```

The HLT instruction:
- Halts CPU execution
- Waits for next interrupt
- Reduces CPU power consumption
- Allows debugger to inspect state

## Module Structure

### `main.rs`

Core kernel logic:
- `_start()` entry point
- Panic handler
- Orchestrates boot sequence

**Lines:** ~70
**Dependencies:** arch, vga, cpu modules

### `arch/mod.rs`

Low-level CPU intrinsics:
- `read_cr3()` - Get page table root
- `read_cr4()` - Get paging features
- `read_rsp()` - Get stack pointer
- `halt()` - Infinite loop with HLT
- `enable_interrupts()` - STI instruction
- `disable_interrupts()` - CLI instruction

**Implementation:**
```rust
#[inline]
pub fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value);
    }
    value
}
```

### `vga.rs`

VGA text buffer output:
- `init()` - Initialize VGA driver
- `clear_screen()` - Clear text buffer
- `print()` - Write string to VGA
- `println!()` - Write with newline
- `Writer` struct - Implements `fmt::Write`

**Key data:**
```rust
const VGA_BUFFER: u64 = 0xB8000;
const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;
```

**Character format:**
```
Byte 0: ASCII character
Byte 1: Color (foreground in lower 4 bits, background in upper 4 bits)
```

Example: `0x0720` = white space on black background

### `cpu.rs`

CPU identification via CPUID:
- `get_cpu_info()` - Query vendor and brand
- `cpuid()` - Execute CPUID instruction
- `CpuVendor` enum - Intel/AMD/Other
- `CpuInfo` struct - Vendor + brand string

**CPUID leaves used:**
- `0x00` - Vendor ID
- `0x80000002`, `0x80000003`, `0x80000004` - Brand string

**Output format:**
```
Vendor: Intel
Brand: Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz
```

## Panic Handler

```rust
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    vga::println!("");
    vga::println!("!!! KERNEL PANIC !!!");
    if let Some(location) = info.location() {
        vga::println!("Location: {}:{}", location.file(), location.line());
    }
    if let Some(msg) = info.message().as_str() {
        vga::println!("Message: {}", msg);
    }
    arch::halt();
}
```

If kernel panics:
1. Prints location (file + line number)
2. Prints panic message
3. Halts system

This allows debugging via GDB (paused at HLT).

## Linker Script: `kernel.ld`

Configures memory layout:
```linker
ENTRY(_start)
SECTIONS {
    . = 0x200000;      # Load address
    
    .text : {
        *(.text*)
        *(.rodata*)
    }
    
    .data : {
        *(.data*)
    }
    
    .bss : {
        *(.bss*)
        *(COMMON)
    }
}
```

**Key points:**
- Entry symbol: `_start` (bootloader jumps here)
- Load address: `0x200000` (2MB)
- Sections: text (code), data, BSS (zero-initialized)

## Cargo Configuration: `.cargo/config.toml`

```toml
[build]
target = "x86_64-unknown-none"

[target.x86_64-unknown-none]
rustflags = [
    "-C", "link-arg=-m64",
    "-C", "link-arg=-Tkernel.ld",
    "-C", "link-arg=-no-pie",
    "-C", "relocation-model=static",
]
```

**Flags:**
- `-m64` - 64-bit binary
- `-Tkernel.ld` - Custom linker script
- `-no-pie` - Position-independent code disabled
- `-relocation-model=static` - No relocations

## Compilation

```bash
cargo build --target x86_64-unknown-none --release
```

**Output:** `target/x86_64-unknown-none/release/neodos_kernel`

This ELF binary is:
1. Extracted to raw binary: `objcopy -O binary`
2. Prefixed with magic (0xNEODKRN)
3. Copied to disk image

## Dependencies

**Cargo.toml:**
```toml
x86_64 = "0.14"   # x86-64 intrinsics and utilities
log = "0.4"       # Logging facade (optional)
```

**Rust features:**
- `#![no_std]` - No standard library
- `#![no_main]` - Custom entry point
- `#![feature(asm_const)]` - Inline assembly constants

## Memory Layout

```
0x200000 - 0x200FFF   | Kernel entry + first page
0x201000 - 0x2FFFFF   | Kernel code/data (rest of 1MB)
0x300000 - 0x3FFFFF   | Kernel data section (reserved, 1MB)
0x400000 - ...        | Heap (not used in Phase 1)
0xB8000  - 0xC0000    | VGA text buffer (64KB)
```

## Debugging Kernel

### GDB Workflow

1. **Start QEMU with debug server:**
   ```bash
   bash scripts/qemu-debug.sh
   ```

2. **Connect GDB in another terminal:**
   ```bash
   gdb -x .gdbinit
   ```

3. **Inspect kernel state:**
   ```gdb
   (gdb) break *0x200000      # Breakpoint at entry
   (gdb) c                     # Continue
   (gdb) x /32i $rip           # Disassemble
   (gdb) p /x $cr3             # Print page table root
   (gdb) p /x $cr4             # Print paging features
   (gdb) p /x $rsp             # Print stack pointer
   (gdb) kernel_state          # Custom command in .gdbinit
   ```

### QEMU Monitor

```
(monitor) info registers      # All CPU registers
(monitor) x /16x 0x200000     # Memory dump at kernel entry
(monitor) p $cr3              # Page table root
(monitor) p $cr4              # Paging config
```

### VNC Viewer

View kernel output on virtual screen:
```bash
vncviewer localhost:5900
```

You should see the kernel banner and debug information.

## Known Limitations

1. **No interrupts:** IDT/ISR not installed
2. **No malloc:** Kernel uses only stack
3. **Single-core:** Only bootstrap processor runs
4. **No serial output:** Only VGA text mode output
5. **No exception handling:** Panics halt immediately

## Next Steps (Phase 2)

- [ ] Interrupt Descriptor Table (IDT) setup
- [ ] Exception handlers (#GP, #PF, #DF)
- [ ] Page table management (custom paging)
- [ ] Memory allocator (bump/slab allocator)
- [ ] Keyboard input (scan codes)
- [ ] Timer interrupt (PIT or APIC timer)
- [ ] Multi-core startup (AP bootstrap)

## References

- [CPUID Instruction Reference](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
- [x86-64 ISA](https://www.amd.com/en/support/amd-pseries)
- [VGA Text Mode](https://wiki.osdev.org/Text_Mode_Cursor)
- [OSDev Bare Metal](https://wiki.osdev.org/Bare_Bones)
