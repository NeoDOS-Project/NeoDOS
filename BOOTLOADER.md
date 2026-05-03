# NeoDOS Bootloader Specification

## Overview

The NeoDOS bootloader is a UEFI application that:
1. Initializes UEFI boot services
2. Queries system memory
3. Locates and loads the kernel binary
4. Transitions to 64-bit protected mode
5. Jumps to kernel entry point

## Entry Point: `efi_main()`

```rust
#[entry]
fn efi_main(image: Handle, mut system_table: SystemTable<Boot>) -> Status
```

**Parameters:**
- `image`: Handle to the bootloader image (for resource loading)
- `system_table`: UEFI System Table with boot services

**Return:** UEFI Status code (success or error)

## Boot Flow

### 1. Initialize UEFI Services

```rust
uefi_services::init(&mut system_table)?;
```

Sets up:
- Logger (for debug output)
- Memory management
- Console I/O

### 2. Display Banner

Output to ConOut (console):
```
========================================
NeoDOS Bootloader v0.1
========================================
```

### 3. Query Memory Map

**Function:** `memory::get_memory_info()`

Uses UEFI `GetMemoryMap` to:
- Enumerate all physical memory regions
- Calculate total RAM
- Calculate available (CONVENTIONAL) RAM
- Display results on screen

**Output:**
```
[+] Fetching memory map...
    Total memory: 512.00 MB
    Available:    491.00 MB
```

### 4. Load Kernel Binary

**Function:** `file::load_kernel_binary()`

Uses UEFI Simple File System protocol to:
- Open root volume (ESP)
- Navigate to `/EFI/NeoDOS/kernel.bin`
- Read entire file into memory (allocated by bootloader)
- Return Vec<u8> containing kernel binary

**Error handling:**
- File not found → print error, return LOAD_ERROR
- Read failure → print error, return LOAD_ERROR

### 5. Verify Kernel Magic

Reads first 4 bytes of kernel binary:
- Expected magic: `0xNEODKRN` (encoded as NEODKRN in ASCII)
- If mismatch: print error, return LOAD_ERROR
- If match: continue

### 6. Copy Kernel to Load Address

```rust
unsafe {
    core::ptr::copy_nonoverlapping(
        kernel_data.as_ptr(),
        KERNEL_LOAD_ADDR as *mut u8,
        kernel_data.len(),
    );
}
```

Copies kernel binary to `0x200000` (2MB physical address).

### 7. Exit Boot Services

```rust
let (runtime_services, _mmap) = 
    system_table.exit_boot_services(image, &mut Default::default());
```

Transitions from UEFI boot mode to OS-owned execution:
- UEFI console becomes unavailable
- Bootloader owns entire system
- Must handle all interrupts (none expected yet)

### 8. Jump to Kernel

```rust
unsafe {
    core::arch::asm!(
        "jmp {kernel_addr}",
        kernel_addr = in(reg) KERNEL_LOAD_ADDR,
        in("rsi") &runtime_services as *const _ as u64,
        in("rbx") KERNEL_LOAD_ADDR,
        options(noreturn)
    );
}
```

Inline assembly that:
- Jumps to `0x200000` (kernel entry point)
- Passes SystemTable pointer in `RSI` register
- Passes kernel load address in `RBX` register
- Never returns (noreturn)

## Module Structure

### `main.rs`

Core bootloader logic:
- `efi_main()` entry point
- Flow control
- Print statements to console

**Lines:** ~120
**Dependencies:** memory, file, panic modules

### `memory.rs`

UEFI memory enumeration:
- `get_memory_info()` - Get total and available memory
- Uses UEFI GetMemoryMap protocol
- Parses MemoryDescriptor array

**Key function:**
```rust
pub fn get_memory_info(
    boot_services: &BootServices,
    stdout: &mut Output,
) -> uefi::Result<(u64, u64)>
```

Returns `(total_bytes, available_bytes)`

### `file.rs`

Kernel binary loader:
- `load_kernel_binary()` - Load kernel from ESP
- Uses UEFI Simple File System protocol
- Opens `/EFI/NeoDOS/kernel.bin`
- Reads entire file into Vec<u8>

**Key function:**
```rust
pub fn load_kernel_binary(
    image: Handle,
    boot_services: &BootServices,
    stdout: &mut Output,
) -> uefi::Result<Vec<u8>>
```

### `panic.rs`

UEFI panic handler:
- Catches Rust panics in bootloader
- Prints to console
- Halts system

## Register State at Kernel Entry

When kernel receives control from bootloader:

```
RAX   = (undefined)
RBX   = 0x200000 (kernel load address)
RCX   = (undefined)
RDX   = (undefined)
RSI   = &RuntimeServices (passed by bootloader)
RDI   = (undefined)
RSP   = (UEFI boot stack, ~256MB)
RBP   = (undefined)
```

**Important:**
- Paging is already active (UEFI sets it up)
- GDT/IDT are installed (UEFI sets them up)
- Interrupts are enabled (kernel should disable if needed)
- Everything is 64-bit mode

## Kernel Binary Format

**File:** `/EFI/NeoDOS/kernel.bin`

Structure:
```
Offset  Size  Content
0x00    4     Magic: 0xNEODKRN (verified by bootloader)
0x04    ...   Kernel code/data
```

**Magic verification:**
```rust
let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
if magic != 0xNEODKRN { return Status::LOAD_ERROR; }
```

## Debugging Bootloader

### Serial Console Output

All bootloader output goes to UEFI ConOut, which is redirected to:
- QEMU serial port → displayed in terminal
- Physical serial port → displayed via serial cable

### Breakpoints (GDB)

Since bootloader is UEFI application, debugging is limited:
- Can set breakpoints in main() before exit_boot_services()
- Cannot debug after exiting boot services (no UEFI console)

### QEMU Monitor

```
(monitor) info registers    # CPU state at any time
(monitor) x /16x 0x100000   # Bootloader code dump
(monitor) x /16x 0x200000   # Kernel code location
```

## Error Codes

Bootloader can return:

| Code | Meaning |
|------|---------|
| SUCCESS | Kernel loaded and jumped to successfully |
| LOAD_ERROR | Kernel file not found or read failed |
| INVALID_PARAMETER | UEFI service call failed |

## Dependencies

**Cargo.toml:**
```toml
uefi = "0.28"
uefi-services = "0.29"
log = "0.4"
```

**Rust target:** `x86_64-unknown-uefi`

## Compilation

```bash
cargo build --target x86_64-unknown-uefi --release
```

Output: `target/x86_64-unknown-uefi/release/neodos_bootloader.efi`

This EFI binary is copied to disk image at `/EFI/NeoDOS/bootloader.efi`.

## Known Limitations

1. **Single-file kernel:** Only loads `/EFI/NeoDOS/kernel.bin`
   - Phase 2 could support multiple modules
2. **No error recovery:** Panics on any error
   - Phase 2 could implement fallback bootloader
3. **UEFI-only:** Cannot boot on legacy BIOS
   - Phase 2 could add MBR bootloader
4. **No compression:** Kernel must fit in available memory
   - Phase 2 could add kernel decompression

## Next Steps (Phase 2)

- [ ] Interrupt handling (IDT setup)
- [ ] Custom paging (replace UEFI page tables)
- [ ] Serial port output (bootloader + kernel)
- [ ] Module loading (multiple binaries)
- [ ] Multicore support (AP startup)
