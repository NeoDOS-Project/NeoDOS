# HAL ABI v0.4 — raw/safe split

> **Status**: Active. This document describes HAL ABI v0.4.
> ABI v0.3 binary interface is preserved (same 26 extern "C" primitives).
> Internal restructuring adds `hal/raw/` (bare asm) and `hal/safe/` (type-safe wrappers)
> to isolate all inline assembly from the rest of the kernel.
>
> **Audit constraint**: `grep -rn 'asm!(' src/ --exclude-dir=hal/` MUST return 0.
>
> **Source of truth**: `src/hal/`. This document is derivative — it formalises
> what already exists; it does not define new behaviour.

---

## 1. ABI v0.3

The following 23 extern "C" functions constitute the HAL ABI v0.3, exported from `src/hal/`.
Additional non-ABI helpers (`cpu_info`, `walk_ptes_4k`, `without_interrupts`) are part of the
HAL module but are NOT `extern "C"` and are therefore **outside** the binary ABI contract.

### 1.1 CPU Control — `hal/x64/cpu.rs`

```rust
pub extern "C" fn enable_interrupts();
pub extern "C" fn disable_interrupts();
pub extern "C" fn halt() -> !;
pub extern "C" fn poweroff() -> !;
pub extern "C" fn read_cr2() -> u64;
pub extern "C" fn read_cr3() -> u64;
pub extern "C" fn write_cr3(val: u64);
pub extern "C" fn flush_tlb(virt: u64);
pub extern "C" fn interrupts_enabled() -> bool;
pub extern "C" fn hlt_once();
pub fn cpu_info() -> CpuInfo;                    // NOT extern "C" — kernel-internal only
```

### 1.2 Port I/O — `hal/x64/io.rs`

```rust
pub extern "C" fn inb(port: u16) -> u8;
pub extern "C" fn outb(port: u16, val: u8);
pub extern "C" fn inw(port: u16) -> u16;
pub extern "C" fn outw(port: u16, val: u16);
pub extern "C" fn inl(port: u16) -> u32;
pub extern "C" fn outl(port: u16, val: u32);
```

### 1.3 Page Memory — `hal/x64/mem.rs`

```rust
pub const PAGE_SIZE: u64 = 4096;

pub extern "C" fn alloc_page() -> *mut u8;
pub extern "C" fn free_page(ptr: *mut u8);
pub extern "C" fn map_page(phys: u64, virt: u64, flags: u64) -> i32;
pub extern "C" fn unmap_page(virt: u64) -> i32;
pub extern "C" fn memory_barrier();
pub fn walk_ptes_4k(virt: u64) -> Option<&'static mut PageTableEntry>;  // NOT extern "C"
```

### 1.4 Interrupt Management — `hal/x64/irq.rs`

```rust
pub type IrqHandler = extern "C" fn();

pub extern "C" fn register_irq(vector: u8, handler: IrqHandler) -> i32;
pub extern "C" fn ack_irq(vector: u8);
```

### 1.5 Timing — `hal/x64/time.rs`

```rust
pub extern "C" fn get_ticks() -> u64;
pub extern "C" fn increment_ticks();
pub extern "C" fn sleep_hint(us: u32);
```

### 1.6 Convenience — `hal/x64/mod.rs`

```rust
pub fn without_interrupts<F, R>(f: F) -> R;     // NOT extern "C" — generic closure helper
```

---

### 1.7 Internal Structure — `hal/raw/` and `hal/safe/`

Since HAL v0.4, the HAL module is split into three layers to audit inline asm:

| Layer | Description |
|-------|-------------|
| `hal/raw/` | Bare inline asm primitives (`raw_read_msr`, `raw_cpuid`, `raw_sti`, `raw_pause`, GS access, I/O ports, etc.) |
| `hal/safe/` | Type-safe `Msr` trait: `read_msr<T: Msr>()`, `write_msr<T: Msr>()`, named MSR constants with `IS_SAFE` flag |
| `hal/x64/` | Extern "C" ABI surface — delegates all asm calls to `hal/raw` |

```rust
// Safe MSR access example (type-safe):
use crate::hal::safe::{read_msr, write_msr, GsBase, GS_BASE};

let gs_base: u64 = read_msr(&GS_BASE);           // type-safe, no inline asm
unsafe { write_msr(&GS_BASE, new_base); }
```

The `Msr` trait marks dangerous MSRs (EFER, IA32_FEATURE_CONTROL) with `IS_SAFE = false`,
requiring explicit `unsafe` to write.

---

## 2. Formalisation Layer

### 2.1 Type Sizes (Fixed for v0.3)

| Rust type      | ABI type   | Size (bits) | Alignment |
|----------------|------------|-------------|-----------|
| `u8`           | `uint8_t`  | 8           | 1         |
| `u16`          | `uint16_t` | 16          | 2         |
| `u32`          | `uint32_t` | 32          | 4         |
| `u64`          | `uint64_t` | 64          | 8         |
| `i32`          | `int32_t`  | 32          | 4         |
| `*mut u8`      | `void*`    | 64          | 8         |
| `usize`        | `uintptr_t`| 64          | 8         |
| `fn()` pointer | `void(*)()`| 64          | 8         |
| `bool`         | `uint8_t`  | 8           | 1         |

### 2.2 Calling Convention: `extern "C"` (System V AMD64)

| Property          | Rule |
|-------------------|------|
| ABI               | System V AMD64 ABI (psABI) |
| Return address    | Stack (`call`/`ret`) |
| Integer args 1–6  | `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9` |
| Return value      | `rax` |
| Stack alignment   | 16 bytes before `call` |
| Stack cleanup     | Caller — standard `extern "C"` |
| Preserved regs    | `rbx`, `rbp`, `r12`–`r15` |
| Scratch regs      | `rax`, `rcx`, `rdx`, `rdi`, `rsi`, `r8`–`r11` |
| Direction flag    | Clear (`cld`) on entry/exit |
| Red zone          | Not used (kernel mode, `rsp` may be volatile) |

### 2.3 `IrqHandler` type

`IrqHandler` is `extern "C" fn()`. Its signature must match:

```c
typedef void (*irq_handler_t)(void);
```

- No arguments.
- No return value.
- Must not fault; must not longjmp.
- Executes with interrupts in an architecture-defined state (masked or unmasked — see §5.5).

### 2.4 `map_page` flags encoding

The `flags` argument is a bitmask. Bits **not listed** are reserved and must be zero.

| Bit | Value  | Name             | x86_64 mapping |
|-----|--------|------------------|----------------|
| 0   | `0x01` | —                | Always set by implementation (PRESENT) |
| 1   | `0x02` | PAGE_WRITABLE    | PageTableFlags::WRITABLE |
| 2   | `0x04` | PAGE_USER        | PageTableFlags::USER_ACCESSIBLE |
| 3   | `0x08` | PAGE_WRITE_THROUGH | PageTableFlags::WRITE_THROUGH |
| 4   | `0x10` | PAGE_NO_CACHE    | PageTableFlags::NO_CACHE |
| 5–63| —      | Reserved         | Must be zero |

On failure, `map_page` returns `-1`. The only defined failure mode is: the virtual address
is covered by a 2 MB huge page that cannot be split (implementation-specific).

---

## 3. Function Table

| #  | ABI name            | Inputs                                          | Return    | Binary constraints |
|----|---------------------|-------------------------------------------------|-----------|--------------------|
| 1  | `enable_interrupts` | —                                               | `void`    | No stack frame needed |
| 2  | `disable_interrupts`| —                                               | `void`    | No stack frame needed |
| 3  | `halt`              | —                                               | `noreturn`| Infinite loop       |
| 4  | `poweroff`          | —                                               | `noreturn`| Sequence of port writes, then halt |
| 5  | `inb`               | `rdi`=`port: u16` (zero-extended to 64 bit)     | `rax`=`u8`| 8-bit port read |
| 6  | `outb`              | `rdi`=`port: u16`, `rsi`=`val: u8`              | `void`    | 8-bit port write |
| 7  | `inw`               | `rdi`=`port: u16`                               | `rax`=`u16`| 16-bit port read |
| 8  | `outw`              | `rdi`=`port: u16`, `rsi`=`val: u16`              | `void`    | 16-bit port write |
| 9  | `inl`               | `rdi`=`port: u16`                               | `rax`=`u32`| 32-bit port read |
| 10 | `outl`              | `rdi`=`port: u16`, `rsi`=`val: u32`              | `void`    | 32-bit port write |
| 11 | `read_cr2`          | —                                               | `rax`=`u64`| Page-fault linear address |
| 12 | `read_cr3`          | —                                               | `rax`=`u64`| Current PML4 physical address |
| 13 | `write_cr3`         | `rdi`=`val: u64`                                | `void`    | Reloads page tables, flushes TLB |
| 14 | `flush_tlb`         | `rdi`=`virt: u64`                               | `void`    | `invlpg` instruction |
| 15 | `interrupts_enabled`| —                                               | `rax`=`bool`| Reads RFLAGS.IF via pushfq |
| 16 | `hlt_once`          | —                                               | `void`    | Single HLT, returns after next IRQ |
| 17 | `alloc_page`        | —                                               | `rax`=`*mut u8` | Null on OOM |
| 18 | `free_page`         | `rdi`=`ptr: *mut u8`                            | `void`    | Undefined if `ptr` not from `alloc_page` |
| 19 | `map_page`          | `rdi`=`phys: u64`, `rsi`=`virt: u64`, `rdx`=`flags: u64` | `rax`=`i32` | 0=ok, -1=fail |
| 20 | `unmap_page`        | `rdi`=`virt: u64`                               | `rax`=`i32` | 0=ok, -1=fail |
| 21 | `register_irq`      | `rdi`=`vector: u8`, `rsi`=`handler: IrqHandler`  | `rax`=`i32` | Always returns -1 (stub — see §5.4) |
| 22 | `ack_irq`           | `rdi`=`vector: u8`                              | `void`    | Port writes to PIC. Safe for vectors 32–47 |
| 23 | `get_ticks`         | —                                               | `rax`=`u64` | Atomic relaxed load |
| 24 | `increment_ticks`   | —                                               | `void`    | Atomic relaxed increment |
| 25 | `memory_barrier`    | —                                               | `void`    | `atomic_thread_fence(seq_cst)` |
| 26 | `sleep_hint`        | `rdi`=`us: u32`                                 | `void`    | Busy-wait: ~1 port-0x80 stall per unit |

### 3.1 ABI identity

The ABI is identified by the **function symbol name** at link time. There is no dispatch
table in v0.3. A future v0.4 may introduce a dispatch table; the symbol-based ABI
remains valid.

---

## 4. Memory Model (Clarification)

### 4.1 Pointer representation

| Property        | Value     |
|-----------------|-----------|
| Pointer width   | 64 bits   |
| Address space   | Flat, single 64-bit virtual address space |
| Null pointer    | `0x0000000000000000` (`*mut u8` = null) |
| Canonical       | On x86_64, addresses must be canonical (bits 48–63 equal bit 47). Violations are undefined. |

### 4.2 Page size

| Property       | Value  |
|----------------|--------|
| Minimum page   | 4096 bytes |
| Alignment      | 4096 bytes for all page operations |
| `PAGE_SIZE`    | 4096 (constant, exported) |

`alloc_page` returns a 4096-byte-aligned physical address cast to `*mut u8`.
`map_page` and `unmap_page` require 4096-byte-aligned virtual addresses.

### 4.3 Frame allocator guarantees

- `alloc_page` returns the physical address of a 4096-byte frame.
- The frame is zeroed only if the backing implementation is a bitmap allocator that treats
  newly-freed frames as dirty. **No zeroing guarantee is made** — the caller must zero
  sensitive data.
- `free_page` accepts any pointer previously returned by `alloc_page` and has not been
  freed already. Passing any other pointer is undefined behaviour.

### 4.4 Page table manipulation

- `map_page` operates on the **active** page tables (those pointed to by `CR3`).
- If the target virtual address is covered by a 2 MB huge page, the implementation must
  split it first. If splitting fails, `map_page` returns `-1`.
- `unmap_page` clears the leaf PTE and flushes the TLB entry via `invlpg`.
- Both functions implicitly issue a TLB flush for the target virtual address.

---

## 5. Validation Rules

### 5.1 Conformant implementation checklist

A binary that claims HAL ABI v0.2 compliance MUST:

| Rule | Description |
|------|-------------|
| R01  | Export every function listed in §3 with the exact symbol name. |
| R02  | Every exported function uses `extern "C"` (System V AMD64) calling convention. |
| R03  | No function accepts more parameters than declared in §3. |
| R04  | No function returns additional values (no hidden struct returns). |
| R05  | `halt()` never returns. |
| R06  | `poweroff()` never returns (or falls through to `halt()`). |
| R07  | `alloc_page` returns null on OOM, never faults. |
| R08  | `free_page` is a no-op for null pointer. |
| R09  | `map_page` preserves reserved flag bits (§2.4) as zero. |
| R10  | `ack_irq` does not fault for any `u8` value (may be a no-op for undefined vectors). |
| R11  | `memory_barrier` emits at least a compiler fence; a full `mfence` is recommended. |
| R12  | `get_ticks` never blocks, never faults. |

### 5.2 Forbidden in HAL v0.2

| Forbidden | Rationale |
|-----------|-----------|
| Calling the scheduler | HAL is below scheduler |
| Allocating from the kernel heap (`Box`, `Vec`, `String`) | HAL does not depend on the heap allocator |
| Filesystem or VFS access | HAL is hardware-only |
| Process state manipulation | HAL is not aware of processes |
| Dynamic dispatch or trait objects | Binary contract requires fixed symbols |
| `panic!` | HAL must not panic — failure is signalled via return value |

### 5.3 Symbol naming

Symbol names follow the Rust `extern "C"` naming rules: for `#[no_mangle] pub extern "C" fn foo()`,
the linker symbol is exactly `foo`.

| Symbol               | Mandatory | Since |
|----------------------|-----------|-------|
| `enable_interrupts`  | Yes       | v0.2  |
| `disable_interrupts` | Yes       | v0.2  |
| `halt`               | Yes       | v0.2  |
| `poweroff`           | Yes       | v0.2  |
| `inb`                | Yes       | v0.2  |
| `outb`               | Yes       | v0.2  |
| `inw`                | Yes       | v0.3  |
| `outw`               | Yes       | v0.3  |
| `inl`                | Yes       | v0.3  |
| `outl`               | Yes       | v0.3  |
| `read_cr2`           | Yes       | v0.3  |
| `read_cr3`           | Yes       | v0.3  |
| `write_cr3`          | Yes       | v0.3  |
| `flush_tlb`          | Yes       | v0.3  |
| `interrupts_enabled` | Yes       | v0.3  |
| `hlt_once`           | Yes       | v0.3  |
| `alloc_page`         | Yes       | v0.2  |
| `free_page`          | Yes       | v0.2  |
| `map_page`           | Yes       | v0.2  |
| `unmap_page`         | Yes       | v0.2  |
| `register_irq`       | Yes (stub)| v0.2  |
| `ack_irq`            | Yes       | v0.2  |
| `get_ticks`          | Yes       | v0.2  |
| `increment_ticks`    | Yes       | v0.3  |
| `memory_barrier`     | Yes       | v0.2  |
| `sleep_hint`         | Yes       | v0.2  |

### 5.4 `register_irq` stub contract

The current implementation of `register_irq` returns `-1` unconditionally. This means:

- The function symbol **must** be exported (for future ABI compatibility).
- The return value signals "not implemented" — callers MUST check the return code.
- A future implementation (v0.4 or later) may make it functional. The ABI call
  pattern `(vector: u8, handler: IrqHandler) -> i32` is locked and must not change.

### 5.5 `ack_irq` vector range

`ack_irq` sends End-Of-Interrupt to the 8259 PIC(s) for vectors in the range 32–47:

| Vector range | Action |
|-------------|--------|
| 32–39       | Master PIC EOI (port `0x20`, value `0x20`) |
| 40–47       | Slave + master EOI (port `0xA0` then `0x20`, value `0x20`) |
| all other   | No-op (no port writes) |

### 5.6 `sleep_hint` contract

`sleep_hint(N)` performs approximately `N` I/O-port delay cycles (one `outb` to port `0x80`
per unit). The exact timing depends on the platform bus speed. It is a **hint**, not a
precise microsecond delay. Callers that require precise timing MUST NOT rely on this
function.

### 5.7 `increment_ticks` contract

`increment_ticks` performs an atomic `fetch_add(1, Relaxed)` on the global tick counter.
It is designed for the timer IRQ handler. Must not block, must not fault.

### 5.8 `without_interrupts` contract (non-ABI helper)

`without_interrupts` saves RFLAGS via `pushfq`, disables interrupts via `disable_interrupts()`,
executes the closure, then restores the saved IF bit via `enable_interrupts()`.
It is a Rust generic function, NOT `extern "C"`, and is not part of the binary ABI.

---

## 6. Future Extensions (Non-Binding)

### 6.1 Proposed additions for later versions

| Proposed function     | Signature                                        | Rationale |
|-----------------------|--------------------------------------------------|-----------|
| `get_phys_from_virt`  | `(virt: u64) -> u64`                             | Walk page tables, return physical address (0 = not mapped) |
| `set_timer_freq`      | `(hz: u32) -> i32`                               | Reprogram PIT/APIC timer frequency |
| `get_cpu_count`       | `() -> u32`                                      | Return number of online CPUs |
| `irq_enable` / `irq_disable` | `(vector: u8) -> i32`                    | Per-IRQ mask/unmask via IMR |
| `increment_ticks`     | `() -> void`                                     | v0.3 added this; listed for completeness |

### 6.2 Proposed dispatch table

A future revision MAY introduce a fixed-address dispatch table for dynamic HAL lookups:

```c
typedef struct {
    uint64_t magic;           // "HALv03\0\0"
    uint32_t version;         // 0x00030003
    uint32_t entry_count;     // number of function pointers
    void*    entries[];       // function pointers indexed by ABI ID (§3)
} hal_dispatch_v03;
```

If implemented, the dispatch table MUST be placed at a well-known fixed address
(reserved by the linker, e.g. `0x4FFFF00`). The symbol-based ABI v0.3 functions
MUST remain present for link-time resolution.

---

## Appendix A: Page Flags Constant Reference (Human-Readable)

```text
HAL_PAGE_PRESENT       = 0x01   // always set by map_page
HAL_PAGE_WRITABLE      = 0x02
HAL_PAGE_USER          = 0x04
HAL_PAGE_WRITE_THROUGH = 0x08
HAL_PAGE_NO_CACHE      = 0x10
```

## Appendix B: Error Return Convention

All functions that return `i32` use:
- `0`  = success
- `-1` = generic failure

No other error codes are defined in v0.3. A future version may assign specific
negative codes to individual functions.

---

*This document is maintained at `docs/HAL_ABI.md`. The source of truth is the
implementation in `src/hal/`. Any discrepancy should be resolved in favour of the
implementation.*
