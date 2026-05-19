# HAL ABI v0.2 — Binary Compliance Report

**Date**: 2026-05-19  
**Binary**: `neodos-kernel/target/x86_64-unknown-none/debug/neodos_kernel`  
**ABI Spec**: `docs/HAL_ABI.md`  
**Validator**: `nm`, `objdump`, `readelf`

---

## 1. COMPLIANCE STATUS: **FAIL** ❌

The compiled HAL implementation is **NOT** conformant to ABI v0.2.

---

## 2. VIOLATIONS LIST

### V001 — Missing global symbol export (CRITICAL)

| Property | Expected (ABI v0.2) | Actual |
|----------|---------------------|--------|
| Symbol visibility | `GLOBAL` / `T` | `LOCL` / `t` (5 functions) or **absent** (10 functions) |
| Link-time resolution | All 15 symbols must be linkable | No HAL symbol is externally visible |

**Evidence**: `readelf` shows zero `GLOBAL FUNC` entries for HAL functions. `nm` shows all 5 present symbols as lowercase `t` (local). The 10 unused functions are eliminated entirely.

**Severity**: CRITICAL — breaks binary contract. External code cannot link against any HAL symbol.

---

### V002 — 10 of 15 ABI functions dead-code eliminated (CRITICAL)

| Required symbol | Status | Reason |
|----------------|--------|--------|
| `enable_interrupts` | Local `t` | Present but not exportable |
| `disable_interrupts` | Local `t` | Present but not exportable |
| `halt` | Local `t` | Present but not exportable |
| `poweroff` | Local `t` | Present but not exportable |
| `ack_irq` | Local `t` | Present but not exportable |
| `inb` | **ABSENT** | Dead code eliminated |
| `outb` | **ABSENT** | Dead code eliminated |
| `alloc_page` | **ABSENT** | Dead code eliminated |
| `free_page` | **ABSENT** | Dead code eliminated |
| `map_page` | **ABSENT** | Dead code eliminated |
| `unmap_page` | **ABSENT** | Dead code eliminated |
| `register_irq` | **ABSENT** | Dead code eliminated |
| `get_ticks` | **ABSENT** | Dead code eliminated |
| `memory_barrier` | **ABSENT** | Dead code eliminated |
| `sleep_hint` | **ABSENT** | Dead code eliminated |
| `cpu_info` | **ABSENT** | Dead code eliminated |

**Evidence**: `nm target/.../neodos_kernel | grep hal` returns exactly 1 line:
```
t _RNvNtNtNtC...neodos_kernel3hal3x643cpu8poweroff
```
All others found via full symbol table scan of mangled names:
```
t _RNvNtNtNtC...neodos_kernel3hal3x643cpu17enable_interrupts
t _RNvNtNtNtC...neodos_kernel3hal3x643cpu18disable_interrupts
t _RNvNtNtNtC...neodos_kernel3hal3x643cpu4halt
t _RNvNtNtNtC...neodos_kernel3hal3x643cpu8poweroff
t _RNvNtNtNtC...neodos_kernel3hal3x643irq7ack_irq
```
10 remaining HAL functions produce no symbol of any kind.

**Root cause**: `opt-level=3` + `lto=true` in release profile. The compiler inlines or discards functions that are only called within the same crate. Because HAL lives inside the kernel crate (not a separate crate), there is no external call site to preserve the symbols.

**Severity**: CRITICAL — the binary does not contain a complete ABI surface.

---

### V003 — Forbidden dependency: `get_ticks` reads scheduler global (MAJOR)

| Property | Expected (ABI v0.2) | Actual |
|----------|---------------------|--------|
| Dependency isolation | HAL must not depend on scheduler | `crate::scheduler::TIMER_TICKS.load(Ordering::Relaxed)` |

**Source**: `hal/x64/time.rs:4`

```rust
pub extern "C" fn get_ticks() -> u64 {
    crate::scheduler::TIMER_TICKS.load(Ordering::Relaxed)
}
```

The ABI spec §5.2 explicitly forbids: "Calling the scheduler" and "Process state manipulation."

**Severity**: MAJOR — the function reads a scheduler variable instead of accessing a HAL-owned hardware counter (e.g., PIT/APIC tick register directly).

---

### V004 — `register_irq` stub may mask unsafe future use (MINOR)

| Property | Expected (ABI v0.2) | Actual |
|----------|---------------------|--------|
| `register_irq` | May be a stub returning `-1` | Returns `-1` unconditionally |

**Source**: `hal/x64/irq.rs:5`

The spec §5.4 requires this function to be exported for future compatibility. It is **absent** (dead-code eliminated), so it cannot serve as a forward-compatibility placeholder.

**Severity**: MINOR — the stub contract is correct in source, but the symbol does not survive compilation.

---

### V005 — `cpu_info` is not `extern "C"` but is in the ABI module (MINOR)

| Property | Expected (ABI v0.2) | Actual |
|----------|---------------------|--------|
| Calling convention | `extern "C"` | Missing `extern "C"` — uses Rust ABI |
| Return type | N/A (kernel-internal) | `CpuInfo` struct (not FFI-safe) |

**Source**: `hal/x64/cpu.rs:31`

```rust
pub fn cpu_info() -> crate::cpu::CpuInfo {
```

Not `extern "C"`, not FFI-safe. The spec correctly documents this as kernel-internal only. No violation from the spec perspective, but it creates ambiguity about which functions in the HAL module constitute the ABI boundary.

**Severity**: MINOR — the spec already flags this, but source-level inconsistency may cause confusion.

---

## 3. ABI MATCH TABLE

| # | Function | Source signature | Binary symbol | Global? | Calling convention | ABI v0.2 |
|---|----------|-----------------|---------------|---------|-------------------|----------|
| 1 | `enable_interrupts` | `extern "C" fn()` | Local `t` | ❌ | System V ✓ | ❌ |
| 2 | `disable_interrupts` | `extern "C" fn()` | Local `t` | ❌ | System V ✓ | ❌ |
| 3 | `halt` | `extern "C" fn() -> !` | Local `t` | ❌ | System V ✓ | ❌ |
| 4 | `poweroff` | `extern "C" fn() -> !` | Local `t` | ❌ | System V ✓ | ❌ |
| 5 | `inb` | `extern "C" fn(u16) -> u8` | **absent** | N/A | N/A | ❌ |
| 6 | `outb` | `extern "C" fn(u16, u8)` | **absent** | N/A | N/A | ❌ |
| 7 | `alloc_page` | `extern "C" fn() -> *mut u8` | **absent** | N/A | N/A | ❌ |
| 8 | `free_page` | `extern "C" fn(*mut u8)` | **absent** | N/A | N/A | ❌ |
| 9 | `map_page` | `extern "C" fn(u64,u64,u64) -> i32` | **absent** | N/A | N/A | ❌ |
| 10 | `unmap_page` | `extern "C" fn(u64) -> i32` | **absent** | N/A | N/A | ❌ |
| 11 | `register_irq` | `extern "C" fn(u8, fn()) -> i32` | **absent** | N/A | N/A | ❌ |
| 12 | `ack_irq` | `extern "C" fn(u8)` | Local `t` | ❌ | System V ✓ | ❌ |
| 13 | `get_ticks` | `extern "C" fn() -> u64` | **absent** | N/A | N/A | ❌ |
| 14 | `memory_barrier` | `extern "C" fn()` | **absent** | N/A | N/A | ❌ |
| 15 | `sleep_hint` | `extern "C" fn(u32)` | **absent** | N/A | N/A | ❌ |

**Result**: 0 of 15 functions pass all checks. **0% compliance.**

---

## 4. DISASSEMBLY VERIFICATION (present functions only)

| Function | Bytes | Prologue | Register usage | Return | Verdict |
|----------|-------|----------|---------------|--------|---------|
| `enable_interrupts` | 2 | none | none | `ret` | ✓ System V |
| `disable_interrupts` | 2 | none | none | `ret` | ✓ System V |
| `halt` | 4 | none | none | loop+`ret` | ✓ System V |
| `poweroff` | 119 | `sub $0x28,%rsp` | `%rdi`, `%dx`, `%ax` | `int3` | ✓ System V |
| `ack_irq` | 47+ | `sub $0x2,%rsp` | `%dil→%al`, `%dx` | `ret` | ✓ System V |

Calling convention is correct for all 5 present functions. Parameter arriving in `%rdi` (or `%dil`), port I/O via `%dx`/`%ax`. No callee-saved register corruption.

---

## 5. FORBIDDEN BEHAVIOR DETECTION

| Check | Verdict | Detail |
|-------|---------|--------|
| Filesystem access | ✅ PASS | No fs calls in HAL path |
| Module loading (NDM) | ✅ PASS | NDM removed |
| Process management | ✅ PASS | No process calls |
| Scheduler interaction | ❌ **FAIL** (V003) | `get_ticks` → `TIMER_TICKS` |
| Dynamic dispatch | ✅ PASS | No trait objects in HAL |
| Panic across ABI boundary | ✅ PASS | All functions use `-> !` or return values |
| `extern "C"` ABI in source | ✅ PASS | All 14 spec functions use `extern "C"` |

---

## 6. BINARY STABILITY CHECKS

| Check | Verdict | Detail |
|-------|---------|--------|
| No unexpected exported symbols | ✅ PASS | All `GLOBAL` symbols are expected kernel exports |
| Debug-only exports in release | ✅ PASS | Same symbol set in release |
| Inline ABI-breaking optimizations | ❌ **FAIL** | 10 functions eliminated entirely by LTO |
| Dead code affecting ABI surface | ❌ **FAIL** | Dead code elimination removes required ABI symbols |

---

## 7. ROOT CAUSE ANALYSIS

The HAL v0.2 as designed is **architecturally correct** (source code), but **compilation-invalid** (binary):

1. **Same-crate inlining**: Because HAL lives inside `neodos_kernel` (same crate), the compiler treats all HAL functions as eligible for inlining and dead-code elimination. Functions only survive if called from within the crate, and even then only as local symbols.

2. **No forced export**: Rust `extern "C"` does not force global symbol emission when the function can be internalized by LTO. Without `#[used]` or `#[no_mangle]` with explicit `#[export_name]`, the linker sees no external reference and discards unreferenced code.

3. **No separate compilation unit**: For a binary ABI contract, the HAL must either be:
   - A separate crate (`.rlib`/`.so`) with its own compilation boundary, OR
   - Have explicit retention annotations (`#[used]`, `#[no_mangle]`)
   - Be linked separately (e.g., static library + kernel)

4. **`get_ticks` coupling**: Even if export were fixed, `get_ticks` couples the HAL to the scheduler, which violates the "no scheduler dependency" rule. The function should read a HAL-owned hardware timer, not a kernel global.

---

## 8. SUMMARY

```
┌─────────────────────────────────────────────────────────────┐
│ HAL ABI v0.2 BINARY COMPLIANCE                              │
├─────────────────────────────────────────────────────────────┤
│  CRITICAL violations: 2  (V001, V002)                       │
│  MAJOR violations:     1  (V003)                            │
│  MINOR violations:     2  (V004, V005)                      │
│                                                             │
│  RESULT: FAIL ❌                                             │
│                                                             │
│  The kernel binary does NOT conform to HAL ABI v0.2.        │
│  It MUST NOT be booted as a HAL-compliant system.           │
└─────────────────────────────────────────────────────────────┘
```
