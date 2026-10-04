# Fix / mitigation — #476 foreign `rsp` stored into a thread's saved context

**Date:** 2026-10-04
**Branch:** `fix/476-kprcb-revert-desync` (from `investigation/476-iretq-frame` =
develop + #474/#476 diagnostics; #477 untouched).
**Detector that proved it:** `docs/investigation/issue-476-iretq-frame-audit.md`.

---

## 1. What was demonstrated (and what was not)

**Demonstrated:**
- `[IRETQ_BAD_FRAME]` reproducible on VBox SMP2 churn: the idle's saved `rsp`
  lies outside its `IDLE_STACK`.
- `RSP_TRACE` / `KSTACK_RING` show the writer is a `k.rsp = current_rsp` save
  running while `KPRCB.current_thread` is the *next* thread but the CPU is still
  on the *previous* thread's stack (site `timeslice`/`idt_idle`/`resched`).
- A corrupt frame (`cs=0x1e471820`) is then `iretq`'d → #GP/#PF/#UD.

**Disproven:**
- The `IF=1` window hypothesis. The syscall IDT entry is an interrupt gate
  (`idt[0x80].disable_interrupts(true)`); the syscall path runs with `IF=0`.

**Not isolated:**
- The exact code path that leaves `KPRCB.current_thread` pointing at the next
  thread while the CPU continues on the previous stack. The timer Ring-3-preempt
  revert *did* miss a `KPRCB` restore (a real coherence bug, fixed below), but a
  campaign showed the corruption persisted without the invariant guard, so the
  revert was **not** the only writer.

## 2. Changes

1. **Coherence bug fixed** (`arch/x64/idt/mod.rs`, timer Ring-3-preempt revert):
   after `current.state = Running`, restore the per-CPU identity with
   `sync_per_cpu_current(current…)`, mirroring the syscall revert. Previously the
   revert restored only the global `scheduler.current_tid`.
2. **Invariant enforced at every save site** (`scheduler/stack.rs`
   `save_live_rsp_checked`): a `k.rsp = current_rsp` only stores when
   `current_rsp` lies on `k`'s own kernel stack (boot exempt); otherwise it logs
   `[RSP_FOREIGN]` and skips the store. Wired into all five save sites:
   `on_timer_tick` (production-only), `idt_user`, `idt_idle`, `idt_kernel`,
   `resched`.

No ABI / scheduler-design / context-switch/lock changes.

## 3. Regression

`scheduler/tests.rs::n476_on_timer_tick_rsp_ownership` — pure helper
`rsp_in_kernel_stack`: inside / at-bottom / at-top / below / other-kstack /
unknown.

## 4. Validation

<!-- VALIDATION -->

## 5. Classification

Root cause **partially demonstrated**: the corruption mechanism and the broken
invariant are proven, and the fix restores the invariant (survived the campaign).
The exact `KPRCB` desync writer remains unidentified. **#476 stays OPEN** (do not
close on symptom disappearance).
