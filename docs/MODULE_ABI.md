# NeoDOS Module ABI — v0.10.5

## Overview

NeoDOS supports two kinds of loadable modules:

1. **User-mode modules** (`.ndm`) — Ring 3 processes that interact via the INT 0x80 syscall interface. Loaded by the `LOAD` command. They run as scheduled processes with their own user slot and heap.

2. **TSR modules** — Terminate-and-Stay-Resident programs loaded into kernel memory (`0x430000+`). Dispatched via interrupt vectors. Currently loaded from NeoDOS FS, not yet via `.ndm` header format.

This document defines the binary contract for both.

---

## 1. `.ndm` File Format v1

All `.ndm` files start with a 64-byte header:

```
Offset  Size  Field           Description
──────────────────────────────────────────────────────
 0       4    magic           "NDM\x00"
 4       4    version         ABI version = 1
 8       1    module_type     0=driver, 1=filesystem, 2=shell_ext
 9       1    reserved
10       2    header_size     64 (bytes)
12       4    entry_offset    File offset of entry point (relative to file start)
16       4    code_offset     File offset of code section
20       4    code_size       Size of code section (bytes)
24       4    data_offset     File offset of data section
28       4    data_size       Size of data section (bytes)
32       4    api_version     Module API version (must match kernel)
36       4    _reserved2
40      16    name            NUL-padded module name (max 15 chars + NUL)
56       1    compat_flags    Bit 0: requires_smp, Bit 1: needs_timer
57       7    _padding
```

Total header: 64 bytes.

### Module types

| Type | Value | Description |
|------|-------|-------------|
| Driver | 0 | Hardware driver (registered via sys_register_device) |
| Filesystem | 1 | Filesystem driver (registered via VFS) |
| Shell extension | 2 | Adds shell commands |

### Bytecode format

The code section is raw x86-64 machine code, position-independent, with the entry point at `code_offset + entry_offset` relative to the file start. The module is loaded at `BASE` (0x400000 for user-mode modules).

### Calling convention

User-mode modules use the standard INT 0x80 syscall interface:

| Register | Purpose |
|----------|---------|
| RAX | Syscall number |
| RBX | arg0 |
| RCX | arg1 |
| RDX | arg2 |
| Return | RAX |

Syscall table is defined in `AGENTS.md` (see Syscall Table section).

### TSR modules

TSR modules run in Ring 0. They are loaded into kernel memory and called via function pointers:

```rust
// TSR entry point signature (called from dispatch_interrupt)
extern "C" fn tsr_entry();
```

TSR modules access kernel services via the KERNEL_SERVICE_TABLE (see §3).

---

## 2. Module API Table (exported symbols)

Every `.ndm` module can optionally export an API table. The table is defined as a `repr(C)` struct at a known offset in the data section:

```c
#define NDM_API_EXPORTED 0x4E444D01  // "NDM\1"

struct ndm_api_v1 {
    uint32_t    magic;           // NDM_API_EXPORTED
    uint32_t    version;         // 1
    uint32_t    api_table_size;  // bytes
    uint32_t    name_offset;     // offset in data section of name string
    
    // Function pointers (offsets in code section, 0 = unsupported)
    uint32_t    fn_init;         // Module init (called once after load)
    uint32_t    fn_fini;         // Module deinit (called before unload)
    uint32_t    fn_ioctl;        // extern "C" fn(u32 cmd, void* buf) -> i32
    uint32_t    fn_irq;          // extern "C" fn(u8 irq_num) (called from IRQ handler)
    uint32_t    fn_poll;         // extern "C" fn() -> bool (poll for events)
    uint32_t    _reserved[7];
};
```

The kernel scans the data section for the `NDM_API_EXPORTED` magic to locate the API table.

---

## 3. Kernel Service Table (imported symbols)

TSR modules (Ring 0) call kernel services through a function pointer table stored at a well-known address:

```c
#define KERNEL_SERVICE_TABLE_ADDR  0x4FFFF00

struct kernel_services_v1 {
    uint32_t    magic;       // KERNEL_SERVICE_MAGIC
    uint32_t    version;     // 1
    
    // Console / debug output
    void (*print_str)(const char* s);
    void (*serial_print)(const char* s);
    
    // Memory (must be called with interrupts disabled)
    uint64_t (*alloc_frame)(void);    // returns phys addr or 0
    void     (*free_frame)(uint64_t phys);
    
    // I/O (x86_64 port I/O)
    uint8_t  (*inb)(uint16_t port);
    void     (*outb)(uint16_t port, uint8_t val);
    uint16_t (*inw)(uint16_t port);
    void     (*outw)(uint16_t port, uint16_t val);
    uint32_t (*inl)(uint16_t port);
    void     (*outl)(uint16_t port, uint32_t val);
    
    // Block device access (from init-time BlockDevice)
    int (*read_blocks)(uint64_t lba, uint8_t count, void* buf);
    int (*write_blocks)(uint64_t lba, uint8_t count, const void* buf);
    
    uint64_t _reserved[8];
};
```

### Memory layout for user-mode modules

```
0x400000   Code + data (up to 64 KB)
0x410000   Stack (64 KB, grows down from 0x41FFFF)
0x420000   Next slot
  ...
0x7FFFFF   Last slot end

0x10000000 Heap slot 0 (2 MB)
0x10200000 Heap slot 1 (2 MB)
  ...
0x11E00000 Heap slot 15 (2 MB)
```

---

## 4. Compatibility and Validation

On module load, the kernel checks:

1. **Magic** — must be `"NDM\x00"`
2. **Version** — must match kernel's supported ABI version
3. **Header size** — must be exactly 64
4. **Code range** — `code_offset + code_size` must not exceed file size
5. **Data range** — `data_offset + data_size` must not exceed file size
6. **Overlap** — code and data sections must not overlap
7. **Entry point** — `entry_offset` must be within code section
8. **Name** — must not be empty

### Version negotiation

The kernel's supported ABI version is stored as a global:

```rust
const NDM_ABI_VERSION: u32 = 1;
```

If `module.api_version != NDM_ABI_VERSION`, the kernel refuses to load the module and prints a diagnostic.

---

## 5. Module Lifecycle

```
 Load ─→ validate header ─→ allocate slot ─→ copy code/data
                                         ↓
                                    call fn_init()
                                         ↓
                              Module is Running (Ring 3)
                                         ↓
                                    call fn_fini()
                                         ↓
                              free slot ─→ Unloaded
```

### Init order

1. Alloc user slot + heap slot
2. Copy `.ndm` binary to slot (code + data)
3. Patch data section addresses (relocation)
4. Set entry point to `code_base + entry_offset`
5. Spawn process via `spawn_usermode()`
6. `wait_for_process()` enters Ring 3
7. Module entry point runs (calls `fn_init` internally)

### Shutdown

1. Module calls `sys_exit(0)`
2. Kernel frees user slot + heap
3. `wait_for_process()` returns to caller

---

## 6. TSR Dispatch Model

TSR modules are registered to specific interrupt vectors. The dispatch flow:

```
 hardware IRQ
     ↓
 IDT handler (timer_handler_inner, etc.)
     ↓
 tsr::dispatch_interrupt(irq_num)
     ↓
 for each registered TSR for this vector:
     call func()  // Ring 0, interrupts disabled
```

TSRs must:
- Be short and non-blocking (run in IRQ context)
- Not make syscalls (Ring 0, no INT 0x80)
- Not allocate memory (may deadlock with allocator mutex)
- Acknowledge any device interrupts they handle

---

## 7. Example: Minimal Driver `.ndm`

```python
# Build with generate_driver.py
# Structure:
#   [NDM header (64 bytes)]
#   [code: sys_write init msg]
#   [code: sys_register_device(0)]
#   [code: loop: sys_ioctl poll → process command]
#   [data: string constants]
```

Header generation:

```python
import struct

def build_header(name, entry_offset, code_size, data_size):
    hdr = b'NDM\x00'                           # magic
    hdr += struct.pack('<I', 1)                 # version
    hdr += struct.pack('<B', 0)                 # module_type = driver
    hdr += struct.pack('<B', 0)                 # reserved
    hdr += struct.pack('<H', 64)                # header_size
    hdr += struct.pack('<I', entry_offset)      # entry_offset
    hdr += struct.pack('<I', 64)                # code_offset (right after header)
    hdr += struct.pack('<I', code_size)         # code_size
    hdr += struct.pack('<I', 64 + code_size)    # data_offset
    hdr += struct.pack('<I', data_size)         # data_size
    hdr += struct.pack('<I', 1)                 # api_version
    hdr += struct.pack('<I', 0)                 # _reserved2
    name_bytes = name.encode('ascii').ljust(16, b'\x00')[:16]
    hdr += name_bytes                           # name
    hdr += struct.pack('<B', 0)                 # compat_flags
    hdr += b'\x00' * 7                         # _padding
    return hdr
```
