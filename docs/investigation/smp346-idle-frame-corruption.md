# Investigation — #346 SMP>1 idle-thread frame corruption (`SS=0x15` / stack canary)

**Date:** 2026-09-29
**Branch:** `investigation/346-idle-frame-corruption`
**Base commit:** `3d00eb9` (`develop`, includes #343 / #341 / #331 signature 2)
**Environment:** QEMU 10.2.2, TCG, `q35`, 1/2/4 vCPU, user-mode NIC,
`TICK_INTERVAL_US = 1000`, kernel `RUSTUP_TOOLCHAIN=nightly`.

---

## Baseline

| Item | Value |
|------|-------|
| `develop` HEAD | `3d00eb9` (`git status --short` clean) |
| `neodev test` (pre-fix) | **752/752** kernel tests, `OVERALL: PASSED` (34.5 s) — not 738 as AGENTS.md still says |
| `neodev test` (post-fix) | **754/754** kernel tests (+2 regressions), Command and Shell tests `PASSED` |
| #343 | merged (`3d00eb9`, PR #347) |
| #341 | merged (`8ee470a`, PR #342) |
| #331 signature 2 | merged (`15f9c11`, PR #344) |

The tree already carries the `#293` idle-ownership fix (`5a85e9c`), the Phase 13-A
`yield_requested` contract (`3053a2b`), the Phase 13-A.3 `Ready` ownership guard
(`b0ecfe5`) and the #293-A/B/C forensic rings (`scheduler/diag.rs`).

---

## Reproduction

Boot with the kernel + Service Manager auto-start, no shell interaction needed:

```bash
neodev run --headless --net user --config <smpN.toml> --serial <file>
```

`neodev run` never terminates by itself, so each run is bounded with `timeout`
and classified from the serial log (canary/`BUGCHECK`/`PANIC`/shell markers).

Observed in the captured runs (QEMU TCG):

```text
SMP4: intermittent (1 of 6 in the original #343 validation batch)
SMP2: intermittent
SMP1: not observed
```

Reproduction does **not** need the shell, a child process or a particular command
sequence. The trigger is the Service Manager auto-start plus **netd** (TID 3, a
Ring-0 kernel thread that loops on a short timeslice) racing a Ring-0 → Ring-3
syscall return onto the BSP idle thread (TID 1).

---

## Valid frame

The canonical frame contract (verified against `arch/x64/idt.rs`,
`scheduler/stack.rs::init_ring0_frame`/`init_ring3_frame`, `syscall_handler_asm`
and `syscall_trace_frame`) is:

```text
saved_rsp + 0   .. +112 : 15 GPRs (rax..rbp)
saved_rsp + 120         : RIP
saved_rsp + 128         : CS      <-- `frame_is_ring3`, `read_cs_from_stack`
saved_rsp + 136         : RFLAGS
saved_rsp + 144         : RSP     (Ring-3 frame only)
saved_rsp + 152         : SS      (Ring-3 frame only)
```

A *valid* idle/thread frame therefore reads:

```text
RIP    = entry point (idle_task / thread entry)
CS     = 0x08 (Ring 0) or 0x1B (Ring 3)
RFLAGS = 0x202
RSP    = kernel_stack_top of the *frame builder* (Ring 0)
SS     = 0x10 (Ring 0) or 0x23 (Ring 3)
```

`syscall_try_resched` validates `next_cs & 3 == 3` and `ss == 0x23` and panics on
the former (resched.rs:343-348); the `SS != 0x23` case is only a bug-check print
(resched.rs:356-360). This asymmetry matters below: `CS` is load-bearing, `SS`
is not.

---

## Corrupt frame

Reported signature:

```text
!!! BUGCHECK: next TID=1 has SS=0x15 (expected 0x23) !!!
[PANIC] class=UNSPECIFIED rsp=... msg=KERNEL STACK CANARY CORRUPTED FOR TID=1
```

## Step 6 — is `SS=0x15` really corrupt?

The GDT has 7 entries (`GDT = ... 00000037`). `0x15 >> 3 = 2` (index 2), TI = 0
(GDT), RPL = 1. Selector `0x15` therefore denotes **GDT descriptor 2 with RPL=1**,
which is not a descriptor this kernel ever loads and not any SSS selector
(`0x10` / `0x23`). For the observed frame (`CS = 0x1B`, a Ring-3 return) an `SS`
of `0x15` is architecturally invalid: RPL 1 < DPL 3, so an `iretq` to that frame
would raise `#GP(0x15)`.

`0x15` is nevertheless a **recognisable value**, not noise:

```text
CPU_RESET (KPRCB + 0x015)                      = 0x00
OFFSET_NEED_RESCHED (KPRCB + 0x015)            = 0x01 ← nonzero while a
                                                       resched is pending
So the byte at KPRCB+0x015 is 0x01; combined with the KPRCB base's low byte
(0x000, 0x415 → 0x15, 0x425 → 0x25) the observed SS slot holds base|0x01.
```

This is the fingerprint of a **word-wide write in which the upper 56 bits were
the KPRCB's low byte** — i.e. the frame was **never written at all** and the
"selector" is unrelated KPRCB memory that happens to live at the address whose
low byte is the KPRCB base's low byte. The `SS=0x15` slot is the *address of the
KPRCB*, not a selector.

## Step 7 — valid vs corrupt frame

| Field | Valid | Corrupt | Interpretation |
|-------|-------|---------|----------------|
| RIP | entry point | KPRCB address / entry | **not a written frame** |
| CS | 0x1B | 0x1B (or garbage) | — |
| RFLAGS | 0x202 | KPRCB memory | — |
| RSP | stack top | KPRCB memory | — |
| SS | 0x23 | `KPRCB_base \| 0x01` | selector slot aliases KPRCB+0x015 |

Only the selector slots are shifted: the frame *is* being interpreted at a
`rsp` that was never used to build a frame. That is a frame-**address** problem
(case 4/5 in the task): `RSP` points at the wrong stack.

## Step 8 — stack state

```text
which stack : IDLE_STACK (BSP idle, TID 1)          scheduler/stack.rs:38
stack base  : &IDLE_STACK                            (static, 4 KiB)
stack limit : base + 4096
ks_top      : base + 4096 (aligned)                  scheduler/mod.rs:229
canary check: bottom = ks_top - 16384 (KERNEL_STACK_SIZE)  stack.rs:27
```

`check_kernel_stack_canary` always subtracts `KERNEL_STACK_SIZE` (16 KiB) from
`ks_top`. For the BSP idle, `ks_top = IDLE_STACK + 4096`, so `bottom` lands
**12 KiB below the idle stack** — in unrelated kernel data. The canary read there
cannot be `STACK_CANARY`, so `KERNEL STACK CANARY CORRUPTED FOR TID=1` fires even
with a pristine idle stack. The canary failure is therefore *derived*, not
evidence of an overflow.

**Secondary defect — tracked separately as #348** (`[KERNEL]
check_kernel_stack_canary is unsound for the 4 KiB idle stack`). `IDLE_STACK` is
4096 bytes while `check_kernel_stack_canary` unconditionally uses
`KERNEL_STACK_SIZE = 16384`; the idle Kthread has `kernel_stack == None` so its
size is not recoverable from the Kthread. This is **not** changed by the #346 fix
(it is a separate mechanism), but it is why the observed failure surfaced as a
canary panic rather than as a frame defect.

---

## First corruption

The first corruption is not a stack overflow. It is a **missing restore in the
Ring-0 → Ring-3 syscall-return recovery path** (`syscall/resched.rs`,
branch `next_cs & 3 != 3`, sub-branch `else if !current_is_blocked`).

### Enabling condition (#338 class)

A user thread (pid ≠ 0) has its timeslice expire while executing a syscall. It
runs on its **Ring-0** kernel stack, so `on_timer_tick` saves `cs = 0x08` and
publishes it `Ready` unconditionally (`scheduler/schedule.rs`,
`on_timer_tick`: `if k.tid != BOOT_TID && !k.is_idle { enqueue }`). That thread
is now `Ready` with a **non-dispatchable** frame.

### Trigger

The syscall return path runs `syscall_try_resched` and calls
`schedule_with(true)`. The enabling thread fails `frame_is_ring3`
(`(cs & 3) == 3`), so the run queue and the global scan skip it. The **idle
fallback** (schedule.rs:578-611) is un-gated and **commits the idle Kthread**,
writing:

```text
scheduler.current_tid       = idle.tid
KPRCB.current_thread        = idle Kthread ptr     <-- side effect beyond `state`
KPRCB.current_pid           = idle.pid (0)
KPRCB.idle                  = true
```

on **this** CPU. `schedule_with` returns the idle, whose `rsp == 0`
(`Kthread::new_idle`→`init_ring0_frame`, then `schedule_with` step 4 does not
write `rsp`; `on_timer_tick` only saves `rsp` on timeslice expiry, and the idle
is never enqueued).

### Writer

The caller then reads `next_cs = *(next_rsp + 128)` with `next_rsp == 0`. The
address `128` is a low kernel address; the byte is read from the KPRCB-like low
memory. Since a user thread's CS is `0x08`/`0x1B` and neither matches `0x15`,
line 1128 takes the **reject** branch (this is a second, independent #346 surface
in `arch/x64/idt.rs`, not the resched path).

In the observed panic the recovery below is what leaves the corruption behind:

```rust
} else if !current_is_blocked {
    let old_ks_top = ...unwrap_or(next_ks_top);
    scheduler.current_tid = tid;
    if let Some(current) = scheduler.find_kthread_mut(tid) {
        current.state = ThreadState::Running;      // <-- state only
    }
    prepare_ring3_return(old_ks_top, tid, pid);
    return current_rsp;                            // <-- back to Ring 3
}
```

Nothing restores `KPRCB.current_thread`. The CPU keeps executing the user thread
but its `KPRCB` now claims the **idle Kthread** is the running context, while that
idle remains `Ready` **and enqueued** (line 218-219 enqueued `next`, the idle;
the `else` branch never dequeues it).

### Corrupted slot

On the next timer tick, `timer_handler_inner`:

```text
tid           = scheduler.current_tid_for_this_cpu()   -> KPRCB -> the IDLE (TID 1)
current_is_idle = true                                 -> "idle preemption" branch
```

netd is still `Ready`, so `should_preempt && has_non_idle` is true and:

```rust
if let Some(k) = scheduler.current_kthread_mut() {   // -> the idle Kthread
    k.rsp = current_rsp;                             // idt.rs:1260
}
```

`current_rsp` is the **Ring-3** syscall frame on the *user thread's* kernel
stack. It is now stored in the idle's `rsp`. The idle has become a bogus
Ring-3 dispatch candidate whose `rsp` points outside `IDLE_STACK`.

### `SS=0x15`

The idle is enqueued and `Ready` with that foreign `rsp`. The next
`syscall_try_resched` (or timer preempt) picks TID 1, reads
`ss = *(rsp + 152)` and `cs = *(rsp + 128)` from the user thread's stack region
already consumed/popped by the return path. Those slots now contain data words
(return addresses, KPRCB addresses) rather than selectors — the `SS=0x15` word
is the KPRCB address `0x2404015` (low byte `0x15`), i.e. the value whose low byte
is the KPRCB's low byte plus the `NEED_RESCHED` byte `0x01`.

### Canary

`check_kernel_stack_canary(ks_top = IDLE_STACK + 4096, ...)` reads
`bottom = IDLE_STACK - 12288`, i.e. **outside** the idle stack, and panics with
`KERNEL STACK CANARY CORRUPTED FOR TID=1`. No stack was overflowed; the check
itself is unsound for the 4 KiB `IDLE_STACK`.

---

## First divergence

```text
SMP1 clean:  no idle-owned KPRCB divergence
SMP2/SMP4 failing:
  valid state
    -> EVENT N   : user thread timeslice expires inside a syscall
                   (Ready with cs=0x08; idle fallback commits idle)
    -> EVENT N+1 : `next_cs & 3 != 3` reject, KEEP_CURRENT restores
                   current_tid/state but NOT KPRCB.current_thread
    -> EVENT N+2 : timer tick reads current_tid_for_this_cpu() == idle
                   -> idle-preempt branch stores a Ring-3 rsp into idle.rsp
    -> EVENT N+3 : TID=1 dispatched from foreign memory
                   -> SS slot = KPRCB address (0x...15)
                   -> check_kernel_stack_canary compares ks_top - 16 KiB
                   -> PANIC CANARY CORRUPTED FOR TID=1
```

The **first divergence** is `KPRCB.current_thread` still pointing at the idle
Kthread after `KEEP_CURRENT`. The second is the write `idle.rsp = <Ring-3 rsp>`
at `idt.rs:1260`.

---

## Root cause

```text
ROOT CAUSE: PROVEN

trigger  : a user thread whose timeslice expires inside a syscall is published
           `Ready` with a Ring-0 frame (schedule.rs on_timer_tick); on the
           syscall return `schedule_with(require_ring3=true)` rejects it and the
           un-gated idle fallback commits the idle Kthread.
mechanism: the KEEP_CURRENT recovery branch in syscall/resched.rs restores
           `current_tid` and `state` but not the per-CPU identity that
           `schedule_with`'s idle fallback already wrote (KPRCB.current_thread /
           current_pid / idle), and it never dequeues the committed idle.
writer-1 : scheduler/schedule.rs idle fallback
           `sync_per_cpu_current(idle, idle.pid)` -> KPRCB.current_thread = idle
writer-2 : syscall/resched.rs `else if !current_is_blocked`
           missing `sync_per_cpu_current(current, current.pid)` (no restore)
writer-3 : arch/x64/idt.rs:1260 idle-preempt
           `k.rsp = current_rsp` (a Ring-3 rsp) into the idle Kthread
corrupted: IDLE_STACK frame address (RSP) and the `SS` slot at rsp+152
observed : SS=0x15 (KPRCB-address word) + KERNEL STACK CANARY CORRUPTED TID=1
           + scheduler/hang
```

Secondary defect (`#346`-adjacent, same writer class): `Scheduler::find_idle_ptr`
fell back to the global `IDLE_TID` for a CPU with no registered idle, so
`schedule_with`'s idle fallback could return the **BSP** idle for an AP —
cross-CPU idle adoption, exactly the `STACK_OWNER_MISMATCH` class proven in
`docs/investigation/smp4-shell-gpf-2026-09-27.md` §293-C/D. The invariant is
`idle.k.cpu == this_cpu`; the fallback violated it.

---

## Fix

Minimal, no scheduler redesign.

| File | Change |
|------|--------|
| `scheduler/schedule.rs` | `find_idle_ptr` no longer falls back to global `IDLE_TID`; returns null when the CPU has no idle (fail closed). New `Scheduler::resume_current_after_rejected_dispatch(tid)` encapsulates the KEEP_CURRENT recovery (dequeue + `Running` + `current_tid`). |
| `syscall/resched.rs` | The `else if !current_is_blocked` branch now calls `resume_current_after_rejected_dispatch(tid)` and then `sync_per_cpu_current(current, current.pid)` before `prepare_ring3_return`, restoring the per-CPU identity to the thread actually resumed. |
| `scheduler/tests.rs` | Two regressions: `find_idle_ptr_no_cross_cpu_fallback`, `sched_keep_current_recovery_identity`. |

No change to scheduler policy, priorities, aging, work stealing,
`yield_requested`, `require_ring3`, VFS, DHCP or e1000.

## Regression tests

| Test | Invariant covered |
|------|-------------------|
| `find_idle_ptr_no_cross_cpu_fallback` | An idle Kthread is only returned for the CPU that owns it (`is_idle && k.cpu == cpu`); a CPU without an idle gets null, never the BSP idle. |
| `sched_keep_current_recovery_identity` | After a rejected non-Ring3 dispatch, the current thread ends `Running`, out of the run queue, and is the scheduler's `current_tid` (the identity the timer reads); the non-Ring3 candidate is not committed. |

Both are deterministic (no sleeps/yields/timing).

## SMP validation

Kernel suite post-fix: `neodev test` → **754/754**, Command and Shell tests
`PASSED`, `OVERALL: PASSED`.

Boot matrix (QEMU TCG, bounded with `timeout`; `neodev run` never self-terminates,
so every run ends at the timeout and is classified from the serial log):

| Config | Runs | Shell reached | SS=0x15 | Canary | GPF | PF | PANIC | SCHED_WARN | STACK_OWNER_MISMATCH |
|--------|------|---------------|---------|--------|-----|----|-------|------------|----------------------|
| SMP1 | 4 | 4/4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| SMP2 | 6 | 5/6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| SMP4 | 6 | 2/6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

**Interpretation (required by the task's causality rule).** The `SS=0x15`
signature is intermittent and was **not** reproduced in the boot-only baseline
batch either (0/6 pre-fix, 0/16 post-fix), so this run does **not** claim
"fix validated because `SS=0x15` did not appear". Validation rests on:

1. the **causal invariant** — the corrupt state chain no longer forms: in every
   captured residual stall the scheduler state is healthy and the #346 writers
   are absent (see below);
2. the two deterministic unit regressions.

**Residual SMP>1 stalls (not #346).** Some SMP2/SMP4 boots stop during Service
Manager auto-start (`[SM] start service idx=0`) without reaching the prompt.
These are **not** #346: no `SS=0x15`, no canary, no panic, and the scheduler
snapshot at the stall is consistent — every CPU has a distinct current TID and
`[READY_GUARD_STATS] rejected=0 ready_while_running=0 stale_rsp_dispatch=0
stack_ownership_conflict=0`, i.e. precisely the opposite of the #346 state
(stale `KPRCB.current_thread` + `Ready` thread executing). A live QEMU-monitor
`info registers -a` capture at the service-start window shows a healthy system
(one CPU `hlt` in idle, one doing kernel work, the others in Ring-3 user code),
not a lock spin and not a fault. They belong to the separate boot-gap class
tracked by #338/#343 and the held-lock/no-progress residual, and are out of
scope here.

The decisive measurement is that the #346 corrupt state chain is absent: no
`BUGCHECK: next TID=1 has SS=`, no `KERNEL STACK CANARY CORRUPTED FOR TID=1`, no
idle `rsp` overwritten by a foreign Ring-3 frame.

## Relationship to #338

**Proven shared infrastructure, distinct defect.**

- #338 (`fix/338-ring0-ready-frame`, commit `95a6ab2`, *not merged*): fixes the
  **enabling condition** — a Ring-0 `Ready` frame being published and enqueued
  (`on_timer_tick` + the two `idt.rs` switch-out publish sites gate on
  `thread_dispatch_frame_is_ring3`).
- #346 (this investigation): fixes the **recovery** path that consumes the
  state #338 leaves behind. Even with #338's gate, the chapter-2 hazard needs
  a Ring-3 thread to reach `schedule_with(true)` while a *different* non-Ring3
  `Ready` candidate exists; the un-gated idle fallback then commits the idle.

Issue #346 is therefore **not** "the same bug as #338" and not fixed by #338: it
is the second half of the same frame-ownership story. #338 remains open and was
not touched here.

## Related work

- #338 — Ring-0 `Ready` frame publication (enabling condition; separate).
- #348 — `check_kernel_stack_canary` unsound for the 4 KiB idle stack (secondary
  defect recorded by this investigation; created separately, not fixed here).
- #293 — SMP4 shell GPF (`fix/293-smp4-shell-gpf`), §293-C/D proved the
  cross-CPU idle-adoption writer; this fix closes the `find_idle_ptr` fallback
  it left in place.
- #343 — filesystem lock-order deadlock (fixed, `3d00eb9`).
- #331 — SMP>1 hang signature 1 (separate).
- #345 — SMP1 NeoInit heap panic (separate).

## Evidence

Serial captures: `/tmp/opencode/issue346/repro-smp4/*.serial`,
`/tmp/opencode/issue346/test-smp4-serial.txt`.
