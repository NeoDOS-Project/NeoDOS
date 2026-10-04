# FPU / SSE Context Save — Design Document

> **Version:** v0.1-draft
> **Status:** Design
> **Target Release:** v0.52+
> **Issue:** [#471](https://github.com/NeoDOS-Project/NeoDOS/issues/471)
> **Related:** [#119](https://github.com/NeoDOS-Project/NeoDOS/issues/119)
> **ABI Impact:** None (internal context-switch change)

---

## 1. Problem Analysis

### 1.1 Current Limitation

The context switch saves general-purpose registers via the interrupt frame and
the saved stack pointer, but **does not save or restore x87/MMX/XMM/YMM
state**. A search of `neodos-kernel/src` and the (nonexistent) `.s`/`.asm`
files finds no `fxsave`/`fxrstor`/`xsave`/`xrstor` and no FPU-related `CR0`/
`CR4` setup.

Consequences:

- Two threads using floating point / SSE clobber each other's registers across
  a preemption.
- `MXCSR` (rounding mode, exception masks/flags) leaks between threads.
- Any userland runtime that emits SSE for `memcpy`, formatting, or math is
  subtly corrupted even if the application logic is integer-only.

### 1.2 Why It Matters

This is a latent cross-cutting bug, not only a Doom concern. A Rust Doom port
uses mostly fixed-point, but the compiler may emit SSE2 (baseline on
`x86_64-unknown-none`) for moves and initialization. Any FP/SIMD userland is
unsafe until this is fixed.

### 1.3 Scope

Save/restore per-thread FP/SIMD state on context switch, plus the required
`CR0`/`CR4`/`XCR0` configuration. Not scope: FP exception reporting to
userland, or lazy-switch performance tuning (kept as a follow-up).

---

## 2. Design

### 2.1 State Area

Add an FP/SIMD area to each thread's kernel context:

```rust
#[repr(C, align(64))]
pub struct FpuArea {
    pub data: [u8; 4096], // large enough for XSAVE (AVX); 512 used by FXSAVE
}
```

- `FXSAVE` needs 512 bytes, 16-byte aligned.
- `XSAVE` needs a size returned by CPUID `0x0D`, 64-byte aligned; 4 KB is a
  safe cap for AVX (not AVX-512).
- Allocation: the thread struct already lives in kernel memory; add the area
  inline or allocate it at thread creation with the correct alignment. Prefer
  a dedicated aligned allocation to avoid bloating every `Eprocess`.
- Initial state: zeroed `FXSAVE` image with `FCW = 0x037F`, `MXCSR = 0x1F80`
  (x87 extended precision, all SIMD exceptions masked, round-to-nearest).

### 2.2 CPU Feature Detection and Enablement

At boot (Phase 2, after CPUID):

1. Detect `FXSR` (CPUID.1:EDX bit 24) and `XSAVE`/`OSXSAVE`
   (CPUID.1:ECX bits 26/27), plus `OSXSAVE`.
2. If AVX (CPUID.7:EBX bit 28) and we choose XSAVE:
   - `XCR0 = x87 | SSE | AVX` via `xsetbv`.
3. Configure control registers:
   - `CR0.EM = 0` (no CPU emulation of FP).
   - `CR0.MP = 1` (monitor coprocessor).
   - `CR0.TS  = 0` for eager switching (see 2.4).
   - `CR4.OSFXSR = 1`, `CR4.OSXMMEXCPT = 1`.
4. Choose the save instruction: `XSAVE`/`XRSTOR` if AVX present, else
   `FXSAVE`/`FXRSTOR`.

If neither FXSR nor XSAVE is available, the kernel panics at boot (x86-64
guarantees SSE2, so this is a hardware sanity check).

### 2.3 Context Switch Integration

The scheduler switch path (`scheduler` + `arch/x64/idt` save/restore) gains:

```text
save_current:
    fxsave  [current->fpu_area]      ; or xsave
    ...existing GPR/RSP save...
    switch
restore_next:
    ...existing GPR/RSP restore...
    fxrstor [next->fpu_area]         ; or xrstor
    iretq
```

- Save happens after switch-out is decided and the outgoing thread is known;
  restore happens immediately before returning to the incoming thread.
- The idt path handles both timer preemption and explicit reschedule; the
  save/restore must be applied consistently in all of them (mirroring the
  existing "update TSS.RSP0 on switch" pattern).
- Kernel threads that never use FP still get a default area; the cost is one
  512-byte save/restore per switch.

### 2.4 Eager vs Lazy

- **Eager (v1, proposed):** always `fxsave`/`fxrstor`. Simple, correct, small
  fixed cost.
- **Lazy (future):** set `CR0.TS` on switch; `#NM` (device-not-available)
  exception saves/restores on first FP use. Faster for FP-light workloads but
  adds a trap path; deferred.

### 2.5 SIMD in the Kernel

The kernel itself compiles without SIMD today, but `memcpy`/`memset` emitted by
LLVM may use XMM. Correct FPU save/restore also protects the kernel from
clobbering (and being clobbered by) userland SIMD across syscalls and
preemption.

### 2.6 Interaction with Userland

- No ABI change. Threads created via `ob_create(Thread)` inherit the default
  FPU area.
- `SIGFPE`-style reporting is out of scope; unmasked FP exceptions would
  fault. Since `MXCSR` masks are set to all-masked by default, no surprise
  faults.

---

## 3. Alternatives Considered

- **Do nothing while userland avoids SIMD**: not enforceable (compiler affects
  it) and incorrect on preemption regardless. Rejected.
- **Save only XMM via `movdqu`**: incomplete (loses x87, MXCSR, and AVX
  upper halves). Rejected.
- **Lazy `CR0.TS` from the start**: more moving parts (interruptible
  `#NM` path) for a first fix. Deferred to 2.4.

---

## 4. Affected Components

| Subsystem | Change | Impact |
|-----------|--------|--------|
| `arch/x64/cpu.rs` / `cpuid` | Feature detection, `CR0`/`CR4`/`XCR0` setup | Low |
| `arch/x64/idt` | Save/restore in preemption path | Moderate |
| `scheduler` | `FpuArea` per thread, switch hooks | Moderate |
| `hal/raw` | `fxsave`/`fxrstor`/`xsave`/`xrstor` asm | Low |
| New thread creation | Initialize FPU area | Low |
| `docs/scheduler/scheduler.md` | Document FP context | Low |

---

## 5. Contracts

| Contract | Value |
|----------|-------|
| Per-thread state | `FpuArea`, aligned 64 |
| Default `FCW`/`MXCSR` | `0x037F` / `0x1F80` |
| Save instruction | `XSAVE` if AVX, else `FXSAVE` |
| `CR0`/`CR4` | `EM=0, MP=1, TS=0`; `OSFXSR=1, OSXMMEXCPT=1` |
| Missing support | Boot panic (hardware sanity) |

---

## 6. Test Plan

| Test | Expected |
|------|----------|
| `fpu_default_state` | New thread has `FCW=0x037F`, `MXCSR=0x1F80` |
| `fpu_xmm_isolation` | Two threads write distinct XMM values; each sees its own after preemption |
| `fpu_mxcsr_isolation` | Thread A sets rounding mode; thread B keeps default |
| `fpu_fxsave_roundtrip` | `fxsave` then modify then `fxrstor` restores values |
| `fpu_kernel_memcpy_safe` | Kernel `memcpy` during user FP loop does not corrupt user XMM |
| `fpu_cr4_flags` | `OSFXSR`/`OSXMMEXCPT` set at boot |
| `fpu_avx_xsave` | On AVX CPUs, `XSAVE` preserves YMM upper halves |
| `fpu_no_alloc_in_switch` | Switch creates no heap allocations (INV-2) |

---

## 7. Files and Modules

### Modified / new

| Path | Change |
|------|--------|
| `neodos-kernel/src/arch/x64/cpu.rs` (or `cpuid`) | Feature detection + CR0/CR4/XCR0 |
| `neodos-kernel/src/hal/raw` (or `arch/x64`) | `fxsave`/`fxrstor`/`xsave`/`xrstor` asm |
| `neodos-kernel/src/scheduler/types.rs` | `FpuArea` in the thread struct |
| `neodos-kernel/src/scheduler/*`, `arch/x64/idt/mod.rs` | Save/restore on switch |
| `neodos-kernel/src/scheduler` thread creation | Initialize FPU area |
| `docs/scheduler/scheduler.md` | FP context section |

---

## 8. Implementation Plan

1. Add FPU feature detection and `CR0`/`CR4`/`XCR0` setup at boot.
2. Add the asm primitives and the `FpuArea` type.
3. Attach an `FpuArea` to each thread and initialize it.
4. Save/restore in every reschedule path (timer, syscall return, explicit
   reschedule).
5. Add the isolation and round-trip tests.
6. Docs + `neodev test`.

---

## 9. Open Questions

1. XSAVE from the start, or FXSAVE-only until AVX is actually used?
   (Proposed: XSAVE when AVX is present, else FXSAVE.)
2. Inline `FpuArea` in `Eprocess` vs separate allocation? (Proposed: separate
   aligned allocation to keep the process struct small.)
3. When is lazy switching (`CR0.TS`) worth it? (Follow-up after eager lands.)
4. Should `xsave`/`xrstor` be Exposed via the HAL surface (26-fn ABI) or kept
   private in `arch`?

---

## 10. Dependencies

- `arch/x64` (CPUID, CR0/CR4, XCR0, asm).
- `scheduler` (thread context).
- No VFS/block-driver dependency (INV-1).
- Interacts with the context-switch work in `#119` (scheduler lock-free).
