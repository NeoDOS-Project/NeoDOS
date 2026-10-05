# #488 — Fix: boot-thread `IRETQ_BAD_FRAME` false positive

**Branch:** `fix/488-boot-iretq-audit-exempt` (from `develop` @ `2dc0db4`).
**Issue:** #488. **Scope:** diagnostic contract only.

## Root cause (confirmed)

`neodos-kernel/src/scheduler/mod.rs:271` sets the boot thread's
`kernel_stack_top = crate::hal::bootstrap_stack_top()`, which is
`raw_read_rsp()` at the moment `Scheduler::new()` runs
(`hal/x64/cpu.rs:40`) — a **snapshot** of the initial RSP, not the true top of
the bootstrap stack. The boot thread runs on the whole bootstrap stack, so
frames during later execution sit **above** the recorded value.

`neodos-kernel/src/scheduler/diag/iretq.rs::audit_core` exempted only
`ks_top == 0`, so the boot thread's legitimate dispatch frame was reported as
`FRAME_OUTSIDE_KSTACK`. Observed ~1/14 (QEMU `smb4`, VBox SMP2 churn 3/12) as:

```
[IRETQ_BAD_FRAME] kind=FRAME_OUTSIDE_KSTACK site=timer_preempt cpu=0 tid=0
  pid=0 name=boot ... next_rsp=0x1fffcfd0 ks_top=0x1fffce40
```

The ownership guards `save_live_rsp_checked` and `on_timer_tick` already exempt
`BOOT_TID`; the IRETQ audit did not.

## Fix

Exempt `BOOT_TID` in `audit_core` (documented). The general Ring-3 frame check
is unchanged; this is not a weakening — the boot thread has no scheduler-managed
kernel stack, so the invariant does not apply to it.

## Validation

* `neodev build --quick --image`: OK; `neodev test`: **825/825**.
* VBox SMP2 ×6: `IRETQ_BAD_FRAME = 0` (was 3/12 before the fix), `RSP_OWNER_MISMATCH = 0`, `RSP_FOREIGN = 0`, `panic = 0`.
* QEMU SMP4 ×3 (the environment where the false positive was first seen): `IRETQ_BAD_FRAME = 0`, `panic = 0`.

## Verdict

```text
FIXED — boot-thread IRETQ_BAD_FRAME false positive removed; general Ring-3 check unchanged.
```
