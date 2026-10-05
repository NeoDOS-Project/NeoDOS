# #482 — Fix: `wait_for_process` publish order (KPRCB vs bootstrap stack)

**Branch:** `fix/482-wait-for-process-publish-order` (from `develop` @ `d522ebf`).
**Issue:** #482.
**Scope:** the process-launch identity handover only. No scheduler policy / ABI /
KStack change.

---

## Root cause (demonstrated)

`neodos-kernel/src/usermode.rs::wait_for_process`:

1. Inside a `without_interrupts` block it called
   `sync_per_cpu_current(target_ptr, target_pid)`, repointing
   `KPRCB.current_thread = target`, while the CPU still executed on the
   **bootstrap stack** (the boot thread's early stack).
2. The block's `without_interrupts` restores IF on exit; the CPU stayed on the
   bootstrap stack until `disable_interrupts()` + `execute_usermode`'s iretq.

Instrumented QEMU SMP2 (`[WF]` markers):

```
[WF] published  if=false kprcb=Some(4) rsp=0x1fffda28   # IF=0, target on bootstrap stack
[WF] post-block if=true  kprcb=Some(4) rsp=0x1fffdb28   # IF=1 window, target on bootstrap stack
```

The permanent #476 assertion (`switch_out_clear_at`) fires at `site=0` (manual
`switch_out_clear`) and `site=3` (timer) with `kprcb_tid=target` and
`live_rsp` on the bootstrap stack.

### Consumer analysis

The async consumer is the timer (`on_timer_tick`). For a ring0-interrupted user
thread (`cs=0x8`, not a kernel thread) the timer takes the no-preempt path:
`ring0_publish_is_dispatchable(0x8, false) == false`, and at slice expiry
`expose_to_ring3 == false` (re-arm slice, keep `Running`) — it never enqueues or
switches based on the wrong `current`. `[WF_TIMER]` (slice expiry with a
mismatch) = **0**. So the window is a **real invariant violation but currently
benign**; it depends on `is_kernel_thread(target) == false` and
`expose_to_ring3 == false` for safety, which is fragile.

## Fix

Capture the target in the block, but **publish `KPRCB.current_thread` only after
`disable_interrupts()`, immediately before the Ring-3 iretq**:

```
without_interrupts { block boot; s.current_tid = target; target.state = Running;
                     target_ptr_out = target; }   # KPRCB stays = boot
disable_interrupts()
sync_per_cpu_current(target_ptr_out, target_pid_out)  # IF=0, atomic with iretq
execute_usermode(entry, user_stack_top)               # iretq to Ring-3
```

While the CPU is on the bootstrap stack, `KPRCB.current_thread` stays `boot`
(the physical stack owner). The identity change happens at IF=0, atomically with
the transition away from the bootstrap stack (analogous to the #476 syscall
fix).

Verified sequence after the fix:

```
[WF] post-block if=true  kprcb=Some(0) rsp=0x1fff...   # boot, matching the stack
[WF] cli        if=true  kprcb=Some(0) rsp=0x1fff...
[WF] published  if=false kprcb=Some(4) rsp=0x1fff...   # IF=0, just before iretq
```

## Regression

* Deterministic boot-time contract check `[WF_ORDER_VIOLATION]`: at the publish
  point `KPRCB.current_thread` must still be `BOOT_TID`; publishing earlier
  (the defect) is detected.
* The permanent #476 assertion `switch_out_clear_at` now reports **0** at all
  sites (previously site 0/3 per launch).

The #478 `save_live_rsp_checked` guard and `switch_out_clear_at` are preserved.

## Validation

* `neodev build --quick --image`: OK.
* `neodev test`: **825/825**.
* QEMU SMP1/2/4 ×2 (6 runs):

| Env | tests | RSP_OWNER_MISMATCH | WF_ORDER_VIOLATION | RSP_FOREIGN | IRETQ_BAD | #PF | #GP | #UD | panic |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| SMP1 | ok | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1* |
| SMP2 | ok | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| SMP4 | ok | 0 | 0 | 0 | 1† | 0 | 0 | 0 | 0 |

\* The SMP1 panic is `Option::unwrap() on None` at `rsp=0x1fffd9e8` occurring
**before** `wait_for_process` starts (`[WF] entry` never logged; the only
`USERMODE` line is `[BOOT_PROGRESS] SPAWN_USERMODE`) — i.e. between
`[BOOT_PROGRESS] SERVICE_MANAGER_DONE` and the `wait_for_process` call, outside
this fix's code path. Flaky (1/6). Recorded separately.
† The SMP4 `IRETQ_BAD_FRAME` is the pre-existing **boot-thread** `ks_top`
false positive (`site=timer_preempt tid=0 name=boot`), unrelated.

Before the fix the same launch produced `RSP_OWNER_MISMATCH` at site 0/3 on
every boot; after the fix it is 0 at all sites.

## Verdict

```text
FIXED — wait_for_process publishes KPRCB.current_thread only after the CPU
        leaves the bootstrap stack (IF=0, immediately before the Ring-3 iretq).
```
