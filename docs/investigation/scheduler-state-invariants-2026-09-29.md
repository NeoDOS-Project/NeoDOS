# Scheduler state-invariant audit — 2026-09-29

Read-only forensic audit of the NeoDOS scheduler state model, motivated by
[#338](https://github.com/NeoDOS-Project/neodos/issues/338). No code was changed.

Repository: `develop` @ `e8a89fc` (`docs: 2026-09 code-health audit and doc sync`).
The #338 fix branch `fix/338-ring0-ready-frame` (`fd9d37b`, `794d6cb`) was **not**
merged into `develop` at the time of this audit.

> **Post-audit update (2026-09-30):** #338 is now **merged** into `develop` via
> PR #351 (squash commit `7fa14e3`). The F-01/F-03 gate is therefore present in
> `develop`; the F-01 and follow-up notes below are kept as the audit-time
> record.

## Scope

Audited transitions:

```text
Running → Ready      Ready → Running     Running → Blocked
Blocked → Ready      Running → Terminated
Blocked → Terminated Ready → Terminated
Running → Sleeping   Sleeping → Ready
```

Files audited:

- `neodos-kernel/src/scheduler/{types,thread,queue,schedule,mod,lifecycle,wake,smp,aging,stack}.rs`
- `neodos-kernel/src/arch/x64/{idt,smp,cpu_local}.rs`
- `neodos-kernel/src/syscall/{resched,handlers,mod}.rs`, `syscall/ob/wait.rs`
- `neodos-kernel/src/{kwait/mod.rs,apc/mod.rs,irp/mod.rs,usermode.rs,net/mod.rs}`
- `neodos-kernel/src/services/manager.rs`
- `docs/architecture/source-of-truth.md` §6, `docs/scheduler/scheduler.md`

Out of scope: scheduling policy, priority values, accounting, runqueue
performance.

## State model

`ThreadState` (`scheduler/types.rs:155`):

| State | Meaning | Writers | Consumers | In runqueue | Can be current | Context required |
| --- | --- | --- | --- | --- | --- | --- |
| `Ready` | Runnable, waiting for a CPU | `make_thread_ready`, `on_timer_tick`, `syscall_try_resched`, idt preempt, creation/activation | `schedule_with` (fast/steal/scan), idle fallback | For non-idle: yes (`enqueue_to_cpu_run_queue`); boot/idle excluded | No (transiently is still `KPRCB.current_thread` during switch-out = I-RUNREADY window) | Dispatchable frame for the consumer: Ring-3 if a Ring-3 consumer may pick it (`rule 6.1.4`); Ring-0 only for kernel/idle threads |
| `Running` | Executing on some CPU | dispatch paths, `resume_current_after_rejected_dispatch`, idle/AP idle init, `wait_for_process` | timer, syscall, exception | No (removed before commit) | Yes (exactly one CPU) | Live CPU context; `KPRCB.current_thread == k` |
| `Blocked { waiting_for }` | Waiting on a KWait magic | `kwait_block`, `handler_read`, `block_current_alertable`, `ob/wait.rs` | wake scans | No | No | Valid saved `rsp` for re-dispatch (Ring-3 for user threads) |
| `Suspended` | Created but not yet activated | `add_ring3_process`, `add_ring3_process_with_stack`, `add_thread_to_process`, `spawn_kthread_named` | `make_thread_ready` activation (ObWait / Service Manager) | No | No | Initial frame from `init_ring3_frame` / `init_ring0_frame` |
| `Terminated` | Exited / killed, awaiting reap | `terminate_current`, `kill_pid`, exception fallback | reaper / never selectable | No | No after switch-out | None |

There is **no** `Sleeping`/`Zombie`/`Dead`/`Reaped` state. `handler_sleep_ex`
and `yield_current_thread` only set `yield_requested`; they produce `Running →
Ready`, not a sleep state. `Terminated` is the only "dead" state; zombies are
tracked out-of-band in `ZOMBIE_PIDS` (`lifecycle.rs`).

Frame layout used by every checker (`frame_is_ring3`, `prepare_timer_return`,
`syscall_try_resched`): `k.rsp + 120 = RIP`, `k.rsp + 128 = CS`, `+136 RFLAGS`,
`+144 user RSP`, `+152 SS`. `frame_is_ring3` = `(CS & 3) == 3`
(`scheduler/schedule.rs:44`). This is correct for both the syscall frame
(`int 0x80`, 15 pushed GPRs + 5 CPU words) and the timer frame (15 pushed GPRs +
3 or 5 CPU words).

## Transition inventory

| Transition | Site (file:line) | Actor | Runqueue op | current_tid/KPRCB | Saved context |
| --- | --- | --- | --- | --- | --- |
| Running → Ready | `scheduler/schedule.rs:719-729` (`on_timer_tick`) | timer (any CPU) | `enqueue_to_cpu_run_queue` (skip boot/idle) | not updated here | `k.rsp = current_rsp` |
| Running → Ready | `arch/x64/idt.rs:1077-1088` (user-preempt) | timer, Ring-3 interrupt | `make_thread_ready` | caller updates | `k.rsp = current_rsp` |
| Running → Ready | `arch/x64/idt.rs:1329-1343` (kernel-preempt) | timer, Ring-0 interrupt | `make_thread_ready` | caller updates | `k.rsp = current_rsp` |
| Running → Ready | `syscall/resched.rs:152-168` | syscall return | `make_thread_ready` | schedule_with updates | `k.rsp = current_rsp` |
| Running → Ready (intent) | `scheduler/mod.rs:615` (`yield_current_thread`), `handlers.rs:98,499` | yield/sleep syscall | none (flag only) | none | consumed later |
| Ready → Running | `schedule.rs:438-485` (fast), `488-525` (steal), `547-621` (scan) | scheduler | dequeue / remove | set | validated when `require_ring3` |
| Ready → Running | `schedule.rs:629-662` (idle fallback) | scheduler | `remove_from_run_queue(idle)` | set | idle Ring-0 |
| Ready → Running | `resched.rs:258-289` (fallback scan), `309-334` (idle) | syscall return | remove | set | Ring-3 checked by scan |
| Ready → Running | `idt.rs:592-639` (`exception_do_resched`) | exception | `schedule_with(true)` | set | validated |
| Ready → Running | `usermode.rs:286-305` (`wait_for_process`) | boot | none | set | target initial Ring-3 |
| Running → Blocked | `kwait/mod.rs:113-114`, `handlers.rs:151`, `ob/wait.rs:109-112`, `apc/mod.rs:331-333` | blocking syscall | `remove_from_run_queue` (no-op while Running) | unchanged until switch | saved on syscall return |
| Blocked → Ready | `wake.rs:15,33`, `kwait/mod.rs:133`, `irp/mod.rs:251`, `apc/mod.rs:114`, `lifecycle.rs:453,517,731,742`, `services/manager.rs:452`, `ob/wait.rs:97` | wake | `make_thread_ready` | unchanged | last saved Ring-3 (user) |
| Suspended → Ready | `lifecycle.rs:453,517`, `services/manager.rs:452`, `ob/wait.rs:97` | activation | `make_thread_ready` | unchanged | initial frame |
| * → Terminated | `lifecycle.rs:586,688,692`, `idt.rs:581` | exit/kill/exception | `remove_from_run_queue` | not cleared until switch | n/a |

## Proven invariants (from code)

1. **I-RUNQUEUE** (`schedule.rs:212` `validate_runqueue_invariants`): a Ready
   non-idle/non-boot thread is in exactly one runqueue; non-Ready threads have
   zero entries; a TID appears at most once. `enqueue_to_cpu_run_queue` checks
   `rq.contains` before pushing (`queue.rs:14-21`).
2. **I-ONE-RUNNING** (`schedule.rs:239-244`, `160-172`): at most one thread per
   CPU is `Running`; `schedule_with` removes the candidate from the runqueue
   before committing `Running`.
3. **I-CURRENT** (`schedule.rs:219-230`): `current_tid`/`KPRCB.current_thread`
   identify a `Running` thread; per-CPU identity is authoritative (rule 6.1.5).
4. **I-FRAME** (rule 6.1.4, `schedule_with(require_ring3)`): a candidate
   committed for a Ring-3 return must have `CS & 3 == 3`; otherwise it is
   returned to its runqueue without committing. **This is the invariant #338
   violates at publication time.**
5. **I-OWNER** (`candidate_owned_elsewhere`, `schedule.rs:69`): a Ready thread
   still owned as `KPRCB.current_thread` by another CPU is not dispatched or
   stolen (I-RUNREADY).
6. **I-IDLE-OWNER** (`find_idle_ptr`, `schedule.rs:351`): an idle Kthread is
   only selectable by its own CPU (`k.cpu == cpu`).
7. **I-TERMINATE** (`terminate_current`, `kill_pid`, `recycle_terminated`): a
   terminated thread is removed from the runqueue; reaping waits until no CPU
   runs the pid (`is_pid_running_on_any_cpu`).

## Findings

| ID | Transition | Location | Classification | Evidence | Issue |
| --- | --- | --- | --- | --- | --- |
| F-01 | Running → Ready (user thread preempted in a syscall) | `scheduler/schedule.rs:717-729`; `arch/x64/idt.rs:1329-1343`; `1077-1088` | DUPLICATE (fixed) | Publishes `Ready` with `k.rsp = current_rsp` where `CS == 0x08`; `schedule_with(true)` then rejects the frame forever | #338 (merged `7fa14e3`) |
| F-02 | Blocked/Terminated → Ready → Running | `syscall/resched.rs:258-289` | SUSPICIOUS | Fallback scan commits a Ring-3 Ready thread without `candidate_owned_elsewhere` (I-OWNER) and without re-homing `k.cpu = this_cpu` | none |
| F-03 | Running → Ready (user, in-kernel yield) | `arch/x64/idt.rs:1326-1344` | SUSPICIOUS (same invariant as #338) | The #338 fix leaves this branch ungated; it fires on `yield_requested` even for a user thread in Ring 0 | #338 (related) |
| F-04 | — | `docs/scheduler/scheduler.md:48-51,151-152,238` vs `queue.rs:57-73`; `schedule_count` (`mod.rs:45-49,339`) | SAFE / DOC DRIFT | Docs say `on_timer_tick` does not enqueue and that an idle backstop exists; code enqueues and never uses `schedule_count` for a backstop | none |
| F-05 | Terminated → Running (resume) | `syscall/resched.rs:130-137` | SUSPICIOUS | `!has_non_idle_threads()` early-return can hand `current_rsp` back to a Terminated current when every other thread is Suspended/Terminated | none |

### F-01 — details (duplicate of #338)

`on_timer_tick` (`schedule.rs:717-729`) sets `state = Ready`, copies the
interrupted frame into `k.rsp`, re-homes `k.cpu = this_cpu` and enqueues
**without checking whether `current_rsp` is a Ring-3 frame**. When a user thread
is preempted while running on its Ring-0 kernel stack inside a syscall
(`CS == 0x08`), the published dispatch frame is Ring 0.

The Ring-3 consumers then reject it: `schedule_with(true)` (`schedule.rs:444`)
skips it, re-enqueues it (`481`), and the global scan (`552`) also skips it.
The thread is permanently `Ready` but non-dispatchable. The same frame is
re-saved by the idt preempt branches (`idt.rs:1079`, `1331`).

The #338 fix branch adds the `thread_dispatch_frame_is_ring3` gate at the
publication sites; it was not in `develop` at audit time. Classification:
**DUPLICATE**. Fixed by PR #351 (merged as `7fa14e3`); the gate is now present.

### F-02 — details

When the current thread is `Blocked`/`Terminated` and `schedule_with(true)`
returns a non-Ring-3 candidate (the idle fallback), `syscall_try_resched`
scans the thread table itself (`resched.rs:258-276`) and commits the first
`Ready` thread with a Ring-3 frame:

```text
remove_from_run_queue(chosen); chosen.state = Running;
current_tid = chosen_tid; KPRCB = chosen; ...
```

Unlike `schedule_with`, this scan does not call `candidate_owned_elsewhere`
and does not set `chosen.cpu = this_cpu`. On SMP > 1, a Ready Ring-3 thread
that is still `KPRCB.current_thread` of another CPU (the I-RUNREADY window)
would be committed here on a second CPU. This is the same class as #293/#346.
It is **not proven** because it requires the switch-out race window; it is
recorded as a latent gap, not an issue.

### F-03 — details

The #338 fix deliberately does not gate the kernel-preempt branch
(`idt.rs:1326`, added comment in `fd9d37b`). That branch handles kernel
threads (netd) and fires on `current_yield`:

> **Post-audit note (2026-09-30):** the merged #338 fix (PR #351) keeps this
> branch ungated on purpose: it serves kernel/idle threads (Ring-0 by design),
> and gating it by `pid == 0` starved `netd`. The residual user-thread-yield
> window described here is **not** closed by #338; it remains a latent,
> unproven gap as stated.

```text
should_preempt = (current_state == Ready) || current_yield
```

A user thread that sets `yield_requested` (`handler_yield`, `handler_sleep_ex`,
`handler_waitpid` wildcard) and then re-enables interrupts before
`syscall_try_resched` consumes the flag could still be published `Ready` with
its Ring-0 frame here. In the deterministic #338 repro (`on_timer_tick`), the
flag is not involved, so this window is not demonstrated. Same invariant as
issue #338; no separate issue.

### F-05 — details

`syscall_try_resched` returns `current_rsp` immediately when
`!has_non_idle_threads()` (`resched.rs:130-137`). `has_non_idle_threads`
excludes `Terminated`/`Suspended`/idle, so a `Terminated` current thread with
every other thread `Suspended`/`Terminated` would resume instead of switching
away. Reaching it requires an otherwise-empty system; on exit the
`exit_to_kernel` path is normally taken instead. Not proven, no issue.

## #338 relationship

`#338` establishes the invariant:

> A user (Ring-3) thread in `Ready` must always have a Ring-3 dispatch frame
> (`CS & 3 == 3`); otherwise the Ring-3 return consumers
> (`schedule_with(require_ring3=true)`) can never select it and it becomes
> permanently undispatchable.

This audit re-derived that invariant from the code (rule 6.1.4 in
`source-of-truth.md:236-243`) and searched for other publications that break
it. The only publication sites that can produce a Ring-0 frame for a user
thread are the timer preemption sites F-01/F-03; F-01 is exactly #338. All
wake (`Blocked → Ready`) and activation (`Suspended → Ready`) paths preserve
the Ring-3 frame saved by `syscall_try_resched:154`, because user threads can
only block through a syscall. Therefore **no independent new invariant break
in the `Ready` frame contract was proven**.

## Existing issues

Verified state at audit time:

| Issue | State | Relationship |
| --- | --- | --- |
| #338 | CLOSED | Root cause: Ring-0 Ready frame; F-01 is this bug. Fixed by PR #351 (`7fa14e3`). |
| #246 | CLOSED | Running→Ready enqueue (P0-1). Present: `on_timer_tick:728` enqueues. |
| #247 | CLOSED | Blocked→Ready + runqueue invariants. Present. |
| #248 | CLOSED | Stale/duplicate runqueue entries. `queue.rs:14-21` present. |
| #249 | CLOSED | `current_tid`/Running consistency. `resume_current_after_rejected_dispatch` present. |
| #250-#255 | CLOSED | P0 scheduler/syscall fixes (P0-1..P0-6). |
| #254 | CLOSED | Syscall return frame validation. `resched.rs:344-384` present. |
| #293 | CLOSED | SMP4 GPF: cross-CPU idle selection. `find_idle_ptr` per-CPU present. |
| #331 | OPEN | SMP>1 hang; signature 2 fixed by #344, signature 1 → #343. |
| #343 | CLOSED | PAGE_CACHE/BLOCK_DEVICES lock order (commit `3d00eb9`). |
| #346 | CLOSED | Idle frame corruption enabled by #338; KEEP_CURRENT recovery present (`resume_current_after_rejected_dispatch`). |
| #348 | CLOSED | Idle canary size; `kernel_stack_canary_addr`/`kernel_stack_size` present. |
| #340 | OPEN | netd scheduling / sys_yield window; not a state-invariant defect. |
| #302 | OPEN | Child-exit wake; cross-ref says root cause fixed in #344. |
| #345 | OPEN | SMP1 heap panic; unrelated to state model. |

## New issues

**None.** No PROVEN BUG was demonstrated that is not already represented by
issue #338. Per the task rules, no issue is created for F-02/F-03/F-05
(suspicious, not proven).

## Non-findings

Investigated and dismissed; recorded so they are not re-investigated:

- `make_thread_ready` (`queue.rs:57`) does not reject `Terminated`. **Safe**:
  every caller guards on `Blocked` (`wake.rs`, `kwait_wake`, `irp_wake_waiter`,
  `queue_user_apc`) or on `Suspended` activation; no caller passes a
  `Terminated` thread.
- `make_thread_ready` enqueues to `k.cpu`, not the waker's CPU. **Intentional**
  (remote wake + IPI); `enqueue_to_cpu_run_queue` deduplicates.
- `on_timer_tick` sets `Ready` for boot/idle without enqueueing. **Intentional**
  special case; boot/idle are excluded from `validate_runqueue_invariants`.
- `retry` of F-02's missing `k.cpu` update: re-homing is performed by
  `schedule_with`'s global scan (`schedule.rs:574-584`) and `steal_and_migrate`;
  the raw scan is the only un-rehomed path.
- Stale runqueue entry after `kill_pid` of a non-running pid: the entry is
  dropped lazily by `schedule_with`/`steal_and_migrate` when `find_kthread`
  returns `None`. **Safe** (lazy cleanup, no dangling dispatch).
- `Terminated` threads are never selected: `schedule_with` requires
  `state == Ready`; the idle fallback checks `state != Terminated`; the resched
  scan requires `Ready`.
- `recycle_terminated` drops kernel stacks only when no CPU runs the pid
  (`is_pid_running_on_any_cpu`). **Safe**.
- Cross-CPU idle reuse: `find_idle_ptr` and the resched idle selector both
  require `k.cpu == this_cpu` (#293 fix). **Safe**.
- `steal_and_migrate` drops non-Ready entries from a victim queue. **Safe**:
  runs under the scheduler lock; non-Ready in a runqueue is already invalid.
- `resume_current_after_rejected_dispatch` does not reset the idle to `Ready`;
  the caller already did at `resched.rs:217-220` before recovery (verified).
  **Safe** (no double-Running).

## Evidence commands used

```bash
git status --short
git branch --show-current
git log -1 --oneline
git diff develop..fix/338-ring0-ready-frame -- neodos-kernel/src
rg "ThreadState::" neodos-kernel/src
rg "make_thread_ready|enqueue_to_cpu_run_queue|remove_from_run_queue" neodos-kernel/src
rg "current_tid|current_thread|sync_per_cpu_current" neodos-kernel/src
rg "current_rsp|saved_rsp|kernel_rsp|rsp\s*=" neodos-kernel/src
gh issue list --search "<scheduler|sched|Running Ready|Blocked Ready|state|runqueue|current_tid|KPRCB>"
```

## Follow-up work

1. Land the `fix/338-ring0-ready-frame` gate (#338) — it removes F-01 and F-03
   and, transitively, the KEEP_CURRENT/#346 enabling condition.
   **Done:** merged via PR #351 (`7fa14e3`).
2. If desired, harden F-02: route the resched fallback through
   `schedule_with(true)` or add `candidate_owned_elsewhere` + `k.cpu` re-home.
   That is a separate, explicitly authorized task.
3. Reconcile `docs/scheduler/scheduler.md` with the code (F-04): either remove
   the "not re-enqueued on expiry" statement or restore the described idle
   backstop.
