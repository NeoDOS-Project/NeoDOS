# Investigation — #348 `check_kernel_stack_canary` is unsound for the 4 KiB idle stack

**Date:** 2026-09-29
**Branch:** `fix/348-idle-stack-canary`
**Base commit:** `0e4d176` (`develop`, includes the #346 fix / PR #349)
**Environment:** QEMU 10.2.2, TCG, `q35`, 1/2/4 vCPU, user-mode NIC,
kernel `RUSTUP_TOOLCHAIN=nightly`.

---

## Baseline

| Item | Value |
|------|-------|
| `develop` HEAD | `0e4d176` (`git status --short` clean) |
| `neodev test` (pre-fix) | **754/754** kernel, Command + Shell `PASSED` |
| `neodev test` (post-fix) | **757/757** kernel (+3 regressions), Command + Shell `PASSED` |
| #346 | closed/merged (`0e4d176`); **not modified here** |
| #338 | open; **not touched here** |

---

## Defect

Two independent mismatches between the canary checker's assumption (a 16 KiB
stack) and the idle threads' actual stacks.

### 1. BSP idle: 4 KiB stack, checker reads 12 KiB below it

```text
types.rs:11        KERNEL_STACK_SIZE = 16384
types.rs:12        IDLE_STACK_SIZE   = 4096        (private)
stack.rs:38        static mut IDLE_STACK: [u8; 4096]
stack.rs:27        bottom = ks_top.saturating_sub(KERNEL_STACK_SIZE)   // 16384

actual IMAGE-v2 symbol (nm): IDLE_STACK = 0x4462d79
old ks_top = (IDLE_STACK + 4096) & !0xF = 0x4463d70
actual owned idle stack range           = 0x4462d79 .. 0x4463d79
canary SHOULD be at                     = 0x4462d79  (owned bottom)
checker read                            = 0x445fd70  (ks_top - 16384)
                                         0x4462d79 - 0x445fd70 = 12297 bytes
                                         BELOW the owned idle stack
```

So the checker inspects **memory outside the owned idle stack** and reports a
false `KERNEL STACK CANARY CORRUPTED FOR TID=1`.

Two extra facts the original issue did not capture:

- `ks_top = (base + 4096) & !0xF` is **16-aligned**, so for an unaligned BSS
  symbol it does not even equal `base + size`; the owned range is
  `[IDLE_STACK, IDLE_STACK + 4096)`, not `[ks_top - 4096, ks_top)`.
- The AP idle (`register_ap_idle`) sets `kernel_stack_top = stack_top - 4096`,
  where `stack_top` is the top of the AP's 16 KiB region. Its owned span is
  12 KiB, and `ks_top - KERNEL_STACK_SIZE` points another 4 KiB below the AP
  region.

### 2. The idle stacks never received a canary at all

Only `AlignedKStack::try_new_boxed` (heap stacks) writes `STACK_CANARY`, at
`stack.0.as_ptr()` (the box bottom). The static `IDLE_STACK` and the AP idle
region were **never initialized** with a canary. A corrected address alone would
therefore still report corruption.

---

## Actual invariant

```text
check_kernel_stack_canary() must inspect memory owned by the Kthread's actual kernel stack
  normal Kthread (AlignedKStack) -> 16 KiB -> canary at base
  BSP idle  (IDLE_STACK)          ->  4 KiB -> canary at IDLE_STACK
  AP idle   (AP region below frame)-> span  -> canary at ap_region_base

canary address = kernel_stack_top - kernel_stack_size   (the owned size)
and that address must actually contain STACK_CANARY.
```

The size distinction already existed in the code (`IDLE_STACK_SIZE`); it simply
was not represented on the `Kthread`, so the checker could not see it.

---

## Fix

Minimal; no scheduler refactor, no policy change.

| File | Change |
|------|--------|
| `scheduler/types.rs` | `IDLE_STACK_SIZE` made `pub`. New `Kthread.kernel_stack_size: usize` (the owned span at `kernel_stack_top`). |
| `scheduler/stack.rs` | New pure `kernel_stack_canary_addr(ks_top, size) -> Option<u64>`; new `check_kernel_stack_canary_sized(ks_top, size, pid, tid, rsp)`; `check_kernel_stack_canary` kept as a 16 KiB wrapper; new `init_raw_stack_canary` / `init_idle_stack_canary`. |
| `scheduler/mod.rs` | BSP idle: initialize `IDLE_STACK`'s canary and set the idle `kernel_stack_size = IDLE_STACK_SIZE`; the exact `ks_top` is now `IDLE_STACK + IDLE_STACK_SIZE` (no `& !0xF`, so `ks_top - size == IDLE_STACK`). |
| `scheduler/schedule.rs` | `register_ap_idle`: record the AP idle's true span and initialize its canary at the region base. |
| `arch/x64/smp.rs` | `AP_STACK_SIZE` made `pub` so the AP idle span can be derived. |
| `scheduler/thread.rs` | `new_idle` → `IDLE_STACK_SIZE`; `new_idle_bare` → `IDLE_STACK_SIZE` default; `new_ring3_with_stack` → `KERNEL_STACK_SIZE`. |
| `scheduler/mod.rs` (boot thread) / `lifecycle.rs` | `kernel_stack_size = KERNEL_STACK_SIZE`; the lifecycle canary capture uses the thread's own size. |
| `syscall/resched.rs` | Both canary call sites now call `check_kernel_stack_canary_sized` with the owning thread's `kernel_stack_size`. |

Design options considered (issue §6): A (pass size) and B (Kthread metadata) were
both viable; **B** was chosen because the size is an already-existing property of
the stack that the scheduler must carry anyway, and `init_*` requires the base
independent of the call sites. C (a bounds helper) is realised by
`kernel_stack_canary_addr`. D (skip idle) was rejected — it would lose coverage.

`KERNEL_STACK_SIZE` was **not** changed, and no stack size was altered to make
the old checker work.

---

## Tests

Deterministic, in `scheduler/tests.rs` (+3):

| Test | Invariant |
|------|-----------|
| `stack_canary_bounds_model` | Canary address = `ks_top - actual_size`; 16 KiB and 4 KiB give their own bases; the idle address is *not* `ks_top - 16384`; zero top/size yield `None`. |
| `stack_canary_initialized_for_both_sizes` | Both a 16 KiB `AlignedKStack` and a 4 KiB raw buffer actually contain `STACK_CANARY` at the address the sized checker computes. |
| `stack_canary_detects_real_corruption` | The sized checker accepts an intact canary and a corrupted word changes the inspected address (real detection preserved). |

No test couples to #346 or checks `SS == 0x15`.

---

## Validation

Kernel suite: `neodev test` → **757/757**, Command + Shell `PASSED`.

Boot matrix (bounded with `timeout`; `neodev run` never self-terminates):

<!-- SMP_TABLE -->
| Config | Runs | Shell reached | SS=0x15 | Canary | GPF | PF | PANIC | SCHED_WARN | STACK_OWNER_MISMATCH |
|--------|------|---------------|---------|--------|-----|----|-------|------------|----------------------|
| SMP1 | 3 | 3/3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| SMP2 | 3 | 0/3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| SMP4 | 3 | 3/3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

`[READY_GUARD_STATS]` on SMP2/SMP4:
`rejected=0 ready_while_running=0 stale_rsp_dispatch=0 stack_ownership_conflict=0`.

The three SMP2 runs did not reach the prompt (they stopped in
`NEOINIT_RING3` / `SERVICE_MANAGER_DONE` / after DHCP respectively, with CPU
`hlt`/Ring-3 in the live captures) — the same independent, pre-existing SMP>1
service-start boot-gap class recorded in `smp343-service-lock-order-deadlock.md`
and `smp346-idle-frame-corruption.md`, **not** a canary or #348 symptom (no
`STACK CANARY`, no panic, no guard violation).

Diagnostics to confirm no scheduler regression (`READY_WHILE_RUNNING`,
`STALE_RSP_DISPATCH`, `STACK_OWNERSHIP_CONFLICT` from `[READY_GUARD_STATS]`)
and no new `PANIC`/`GPF`/`PF`/`STACK CANARY`/`SCHED_WARN`.

Because the defect was a *false* canary panic that only fired on the #346 corrupt
state, and #346 is already fixed, the canary path is validated primarily by the
unit tests plus the SMP smoke matrix below.

---

## Related issues

- **#346** — SMP>1 idle-thread frame corruption. Closed/merged (`0e4d176`). The
  canary false-positive was *discovered* during its investigation but the #346
  fix does not depend on #348. **Not modified here.**
- **#348** — this issue.
- **#338** — Ring-0 `Ready` frame publication. Open and intentionally untouched.
- **#293** — idle fallback cross-CPU ownership (closed); its regression test is
  retained.
