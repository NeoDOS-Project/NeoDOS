# Hardware Abstraction Layer

## Architecture

The HAL follows the raw/safe split established in ABI v0.4. All inline assembly
is strictly confined to `src/hal/raw/`. No `asm!()` calls exist outside this
directory — verified by the audit constraint below.

### Directory Structure

```text
src/hal/
  mod.rs        - HAL re-exports: pub use x64::*, pub use safe::read_cr2;
                  also provides has_rdrand(), rdrand() (retry loop)
  raw/          - Bare unsafe asm primitives (no safety checks)
    cpu.rs      - STI/CLI, HLT, CPUID, RDTSC/RDTSCP, CR0/2/3/4, INVPCID,
                  RDRAND, GDT/IDT loading, segment regs, GS read/write,
                  debug port, REP STOSD, GPR reads (RAX..R15)
    io.rs       - IN/OUT to port space: inb/outb, inw/outw, inl/outl
    msr.rs      - raw_read_msr(msr: u32) -> u64, raw_write_msr(msr: u32, val: u64)
  safe/         - Type-safe wrappers around raw primitives
    msr.rs      - Msr trait, read_msr<T>()/write_msr<T>(), typed MSR constants
                  with IS_SAFE bool flag; GsBase, KernelGsBase, FsBase,
                  ApicBase, Efer, MiscEnable, Sysenter*, TscAux
    mod.rs      - read_cr2() re-export
  pci.rs        - PCIe ECAM MMIO config space access
  x64/          - extern "C" ABI surface, delegates to hal/raw
    cpu.rs      - 12 primitives (enable/disable_interrupts, halt, poweroff, reboot,
                  read_cr2/3, write_cr3, flush_tlb, interrupts_enabled,
                  hlt_once, cpu_info)
    io.rs       - 6 primitives (inb/outb, inw/outw, inl/outl)
    irq.rs      - register_irq, ack_irq (APIC EOI + legacy PIC EOI)
    irql.rs     - IRQL subsystem: PASSIVE(0)/APC(1)/DISPATCH(2)/DIRQL(3-11)/
                  HIGH(15); raise_irql/lower_irql, IrqMutex, at_dispatch()
    mem.rs      - alloc_page, free_page, map_page, unmap_page, walk_ptes_4k,
                  memory_barrier, PAGE_SIZE
    time.rs     - get_ticks, increment_ticks, sleep_hint, get_tick_rate,
                  init_system_timer, TIMER_TICKS AtomicU64
    mod.rs      - without_interrupts(|| {}) closure helper
  tests.rs      - HAL test harness registration
```

## 26 Primitives (extern "C")

| Category | Primitives | Source |
| ---------- | ----------- | -------- |
| CPU Control | `enable_interrupts()`, `disable_interrupts()`, `halt() -> !`, `poweroff() -> !`, `reboot() -> !`, `read_cr2()`, `read_cr3()`, `write_cr3(val)`, `flush_tlb(virt)`, `interrupts_enabled()`, `hlt_once()`, `read_cr0()`, `read_cr4()` | `x64/cpu.rs` |
| Port I/O | `inb(port)`, `inw(port)`, `inl(port)`, `outb(port, val)`, `outw(port, val)`, `outl(port, val)` | `x64/io.rs` |
| Page Memory | `alloc_page() -> *mut u8`, `free_page(ptr)`, `map_page(phys, virt, flags)`, `unmap_page(virt)`, `walk_ptes_4k(virt)` | `x64/mem.rs` |
| Interrupt Management | `register_irq(vector, handler)`, `ack_irq(vector)` | `x64/irq.rs` |
| Timing | `get_ticks()`, `increment_ticks()`, `sleep_hint(us)` | `x64/time.rs` |

### Non-ABI Helpers

| Helper | Location | Purpose |
| -------- | ---------- | --------- |
| `without_interrupts(` \| \| `{})` | `x64/mod.rs:21` | Save RFLAGS.IF, CLI, execute closure, restore |
| `walk_ptes_4k(virt)` | `x64/mem.rs:11` | Walk x86-64 page tables, return `&mut PageTableEntry` or `None` if huge page |
| `cpu_info()` | `x64/cpu.rs:36` | Returns `CpuInfo` struct from CPUID |
| `has_rdrand()` | `mod.rs:11` | Check RDRAND support via CPUID.01h:ECX bit 30 |
| `rdrand()` | `mod.rs:17` | Retry loop (×10), returns `Option<u64>` |
| `memory_barrier()` | `x64/mem.rs:93` | `fence(Ordering::SeqCst)` |

## Safe MSR Access

The `Msr` trait gates MSR operations by type. Each MSR constant carries a
compile-time `IS_SAFE` flag. Unsafe write is required for unsafe MSRs (EFER,
MISC_ENABLE, IA32_FEATURE_CONTROL); safe MSRs permit direct use.

```rust
use crate::hal::safe::{read_msr, write_msr, GsBase, GS_BASE};

// Safe read (IS_SAFE = true)
let gs_base: u64 = read_msr(&GS_BASE);

// Unsafe write — caller must ensure no side effects
unsafe { write_msr(&GS_BASE, new_base); }

// Convenience methods
let base = GsBase::read();
unsafe { GsBase::write(new_base); }

// ApicBase provides typed helpers
let enabled = ApicBase::is_enabled();
let bsp = ApicBase::is_bsp();
let phys_base = ApicBase::read();
```

### Typed MSR Constants

| Constant | Address | Type | IS_SAFE |
| ---------- | --------- | ------ | --------- |
| `GS_BASE` | `0xC0000101` | `GsBase` | true |
| `KERNEL_GS_BASE` | `0xC0000102` | `KernelGsBase` | true |
| `FS_BASE` | `0xC0000100` | `FsBase` | true |
| `APIC_BASE_MSR` | `0x0000001B` | `ApicBaseMsr` | true |
| `EFER` | `0xC0000080` | `Efer` | false |
| `MISC_ENABLE` | `0x000001A0` | `MiscEnable` | false |
| `SYSENTER_CS` | `0x00000174` | `SysenterCs` | true |
| `SYSENTER_ESP` | `0x00000175` | `SysenterEsp` | true |
| `SYSENTER_EIP` | `0x00000176` | `SysenterEip` | true |
| `TSC_AUX` | `0xC0000103` | `TscAux` | true |
| `IA32_FEATURE_CONTROL` | `0x0000003A` | `Ia32FeatureControl` | false |

## PCIe ECAM (`src/hal/pci.rs`)

The Enhanced Configuration Access Mechanism (ECAM) provides memory-mapped PCI
config space access via the MCFG ACPI table. The implementation lives in
`src/hal/pci.rs`.

### Core Functions

| Function | Signature | Purpose |
| ---------- | ----------- | --------- |
| `set_ecam_base(base)` | `fn(u64)` | Set ECAM base from MCFG, activates ECAM mode |
| `ecam_base()` | `fn() -> u64` | Return current ECAM base (0 if unset) |
| `ecam_is_active()` | `fn() -> bool` | Check if ECAM mode is active |
| `ecam_deactivate()` | `fn()` | Fall back to legacy PIO (0xCF8/0xCFC) |
| `ecam_read_config_dword(bus, dev, func, offset)` | `fn(u8,u8,u8,u8) -> u32` | MMIO 32-bit read (unsafe) |
| `ecam_read_config_word(bus, dev, func, offset)` | `fn(u8,u8,u8,u8) -> u16` | 16-bit read via dword + shift |
| `ecam_read_config_byte(bus, dev, func, offset)` | `fn(u8,u8,u8,u8) -> u8` | 8-bit read via dword + shift |
| `ecam_write_config_dword(bus, dev, func, offset, val)` | `fn(u8,u8,u8,u8,u32)` | MMIO 32-bit write (unsafe) |
| `ecam_write_config_word(bus, dev, func, offset, val)` | `fn(u8,u8,u8,u8,u16)` | Read-modify-write 16-bit |
| `ecam_write_config_byte(bus, dev, func, offset, val)` | `fn(u8,u8,u8,u8,u8)` | Read-modify-write 8-bit |

### ECAM Addressing

```text
addr = ECAM_BASE | (bus << 20) | (dev << 15) | (func << 12) | offset
```

- bus range: 0..255 (8 bits)
- dev range: 0..31 (5 bits)
- func range: 0..7 (3 bits)
- offset: 0..4095 (12 bits), only dword-aligned values for dword access

### Auto-Select Logic

System code uses `pci_config_read_dword()` / `pci_config_write_dword()` in
`drivers/pci.rs` which automatically routes through ECAM if active, else falls
back to legacy PIO (0xCF8/0xCFC).

### Capability and BAR Helpers

- `find_capability(bus, dev, func, cap_id)` — walks PCI capability list
- `read_bar(bus, dev, func, bar_idx)` — reads BAR register (I/O or MMIO)
- `map_bar_mmio(bus, dev, func, bar_idx)` — maps BAR MMIO region with UC-
  caching, returns virtual address

### ECAM Init (Phase 2.3)

Called after custom page tables are active:

1. Read MCFG ACPI table (parsed during ACPI init) for ECAM base address
2. Map the ECAM MMIO region as uncacheable (UC-) via custom page tables
3. Call `set_ecam_base(base)` to activate ECAM

```rust
// In drivers/pci.rs init_ecam():
let mcfg_base = acpi_find_mcfg();
let ecam_phys = mcfg_base.read_base_address();
map_page_uc(ecam_phys, ecam_virt);  // UC- mapping
hal::pci::set_ecam_base(ecam_virt);
```

## IRQL Subsystem (`src/hal/x64/irql.rs`)

IRQL is a per-CPU interrupt priority mechanism stored in KPRCB (GS-segment
offset 0x016). Five levels control interrupt masking:

```text
PASSIVE  (0)  — normal kernel/user code, all interrupts enabled
APC      (1)  — APC delivery, most device interrupts enabled
DISPATCH (2)  — DPC + scheduler, timer/device IRQs masked (CLI)
DIRQL    (3-11) — device interrupt handlers (mapped to vectors 32–40)
HIGH     (15) — NMI, machine check
```

- `raise_irql(level)` — set new IRQL, CLI if >= DISPATCH, returns old level
- `lower_irql(old_level)` — restore IRQL, STI if crossing DISPATCH threshold
- `IrqMutex<T>` — spin::Mutex wrapper that automatically raises to DISPATCH on
  lock and lowers on drop, satisfying the invariant that spinlock holding
  implies IRQL >= DISPATCH
- `at_dispatch(|| {})` — execute closure at DISPATCH_LEVEL

## Audit Constraint

All inline assembly is strictly confined to `src/hal/raw/`. Run this after any
code change:

```bash
# No asm! outside hal/ — MUST return 0 matches
grep -rn 'asm!(' src/ --include='*.rs' | grep -v 'hal/' || echo "CLEAN"

# All asm! calls are in hal/raw/
grep -rn 'asm!(' src/hal/ --include='*.rs'
```

## Backend Abstraction

The `x64/` backend implements the full extern "C" ABI surface for x86_64.
Future backends (e.g., `aarch64/`) would provide an identical API using
architecture-specific instructions, enabling cross-platform kernel builds
without changing callers.

---

## ABI Reference

> **Status**: Active. HAL ABI v0.4 with raw/safe split. ABI v0.3 binary interface is preserved
> (26 extern "C" primitives). Internal restructuring adds `hal/raw/` (bare asm) and `hal/safe/`
> (type-safe wrappers) to isolate all inline assembly from the rest of the kernel.
>
> **Source of truth**: `src/hal/`. This document is derivative — it formalises
> what already exists; it does not define new behaviour.

### ABI v0.3 — 23 extern "C" Functions

#### CPU Control — `hal/x64/cpu.rs`
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
pub fn cpu_info() -> CpuInfo;                    // NOT extern "C"
```

#### Port I/O — `hal/x64/io.rs`
```rust
pub extern "C" fn inb(port: u16) -> u8;
pub extern "C" fn outb(port: u16, val: u8);
pub extern "C" fn inw(port: u16) -> u16;
pub extern "C" fn outw(port: u16, val: u16);
pub extern "C" fn inl(port: u16) -> u32;
pub extern "C" fn outl(port: u16, val: u32);
```

#### Page Memory — `hal/x64/mem.rs`
```rust
pub extern "C" fn alloc_page() -> *mut u8;
pub extern "C" fn free_page(ptr: *mut u8);
pub extern "C" fn map_page(phys: u64, virt: u64, flags: u64) -> i32;
pub extern "C" fn unmap_page(virt: u64) -> i32;
pub extern "C" fn memory_barrier();
pub fn walk_ptes_4k(virt: u64) -> Option<&'static mut PageTableEntry>;  // NOT extern "C"
```

#### Interrupt Management — `hal/x64/irq.rs`
```rust
pub type IrqHandler = extern "C" fn();
pub extern "C" fn register_irq(vector: u8, handler: IrqHandler) -> i32;
pub extern "C" fn ack_irq(vector: u8);
```

#### Timing — `hal/x64/time.rs`
```rust
pub extern "C" fn get_ticks() -> u64;
pub extern "C" fn increment_ticks();
pub extern "C" fn sleep_hint(us: u32);
```

### Calling Convention

System V AMD64 ABI, `extern "C"`. Args 1-6: `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`.
Return: `rax`. Stack 16-byte aligned before `call`. Scratch regs: `rax`, `rcx`,
`rdx`, `rdi`, `rsi`, `r8`-`r11`. Preserved: `rbx`, `rbp`, `r12`-`r15`.

### `map_page` Flags

| Bit | Value | Name | x86_64 mapping |
|-----|-------|------|----------------|
| 0 | `0x01` | — | Always set (PRESENT) |
| 1 | `0x02` | PAGE_WRITABLE | PageTableFlags::WRITABLE |
| 2 | `0x04` | PAGE_USER | PageTableFlags::USER_ACCESSIBLE |
| 3 | `0x08` | PAGE_WRITE_THROUGH | PageTableFlags::WRITE_THROUGH |
| 4 | `0x10` | PAGE_NO_CACHE | PageTableFlags::NO_CACHE |
| 5-63 | — | Reserved | Must be zero |

### Validation Rules

- All exported functions use `extern "C"` (System V AMD64)
- `halt()` and `poweroff()` never return
- `alloc_page` returns null on OOM, never faults
- `free_page` is a no-op for null pointer
- HAL must not call the scheduler, heap allocator, filesystem, or process code
- HAL must not panic — failure signalled via return value
- No `asm!()` calls outside `src/hal/raw/`

### Current Extensions (since v0.39.4)

#### PCI Express ECAM (`src/hal/pci.rs`)
ECAM addressing: `ECAM_BASE + (bus<<20) + (dev<<15) + (func<<12) + offset`.
Activated at Phase 2.3 from ACPI MCFG table.

#### I/O APIC (`src/interrupts/ioapic.rs`)
ISA IRQs routed: IRQ0 (timer) → vec32, IRQ1 (keyboard) → vec33,
IRQ4 (serial) → vec36, IRQ12 (PS/2 mouse) → vec44.

#### MSI-X (`src/interrupts/msi.rs`)
`configure_msix_entry` and `configure_msix_entries` for per-entry MSI-X setup.

### `ack_irq` Updated Contract (v0.39.4+)
1. **APIC EOI** (always): Write 0 to Local APIC EOI register for ALL vectors
2. **IOAPIC active**: Return immediately after APIC EOI — legacy PIC disabled
3. **Legacy PIC fallback**: Proper EOI to master/slave PIC for vectors 32-47

### Error Return Convention
- `0` = success
- `-1` = generic failure

---

## GDT & IDT Reference

### Global Descriptor Table (GDT)

NeoDOS implements a 64-bit GDT with Ring 0 (Kernel) and Ring 3 (User) segments.

| Index | Selector | Type | DPL |
|-------|----------|------|-----|
| 0 | `0x00` | Null | - |
| 1 | `0x08` | Kernel Code | 0 |
| 2 | `0x10` | Kernel Data | 0 |
| 3 | `0x18` | User Code | 3 |
| 4 | `0x20` | User Data | 3 |
| 5 | `0x28` | TSS | 0 |

The TSS provides a clean stack for critical exceptions (Double Faults via IST0).

### Interrupt Descriptor Table (IDT)

IDT maps 256 interrupt vectors. Exception vectors (0-31):

| Vector | Name | Type | Handler |
|--------|------|------|---------|
| 0 | Divide Error | Fault | Panic |
| 6 | Invalid Opcode | Fault | Panic |
| 8 | Double Fault | Abort | Panic (IST0) |
| 13 | GPF | Fault | Panic |
| 14 | Page Fault | Fault | Panic |

Hardware IRQs (32-47, PIC remapped):
- **32 (IRQ0)**: System Timer (Context Switch)
- **33 (IRQ1)**: PS/2 Keyboard
