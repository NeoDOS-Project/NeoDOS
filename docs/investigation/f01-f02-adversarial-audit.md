# F-01 / F-02 Adversarial Audit — SMP current identity, zombie reap, stack UAF

**Branch:** `feat/phase13-ap-scheduling`
**Base:** `develop` @ `60a68e2` (v0.50.4)
**Kernel:** v0.50.4, QEMU q35, TCG, `-smp 2` (neodev default)
**Scope:** static + code-path audit of the SMP scheduler hazards named in the
Phase 13 handoff: *per-CPU current identity*, *zombie reap*, and *kernel-stack
use-after-free*.

> Context: this audit is carried out with AP scheduling still disabled (APs
> only `hlt` in `ap_entry`). Findings marked **FIXED** are safe independent
> fixes. Findings marked **PROPOSED** are real hazards whose fix is a core
> scheduler semantic change and is intentionally left for the AP-scheduling
> branch (see `phase13-ap-scheduling-design.md`), to be validated with APs
> actually dispatching.

---

## 1. F-01 — Per-CPU current identity

Rule 6.1.5 states the authoritative identity of the thread running on a CPU is
`KPRCB.current_thread` (per-CPU), and the global `Scheduler.current_tid` is
bookkeeping only. The helpers that implement this are:

- `cpu_local::this_cpu_current_thread()`, `this_cpu_current_tid()`,
  `this_cpu_current_pid()` (GS-relative reads).
- `cpu_local::sync_per_cpu_current(ptr, pid)` (GS-relative writes).
- `Scheduler::kprcb_thread_in_self()` — true only when `GS != 0` and the KPRCB
  thread pointer belongs to this `Scheduler` instance (distinguishes the global
  `SCHEDULER` from local test schedulers).
- `Scheduler::current_tid_for_this_cpu()` / `current_pid()` — prefer KPRCB,
  fall back to the global field.

### F-01-A — `this_cpu_set_current_pid` wrote 8 bytes over a 4-byte field **(FIXED)**

**Severity: High.** `Kprcb` layout (enforced by `const _: () = assert!(...)`):

```text
0x008 current_thread : *mut Kthread  (8 bytes)
0x010 current_pid    : u32           (4 bytes)
0x014 idle           : bool          (1 byte)
0x015 need_resched   : bool          (1 byte)
0x016 current_irql   : u8            (1 byte)
0x017 _pad0          : u8
```

`arch/x64/cpu_local.rs::this_cpu_set_current_pid` used
`gs_write_u64(OFFSET_CURRENT_PID, pid as u64)`. `pid` is a `u32`, so the value
is zero-extended and the 8-byte store writes `0x014..0x018` with zeroes. Every
call therefore silently cleared:

- `idle` (0x014),
- `need_resched` (0x015),
- `current_irql` (0x016).

`this_cpu_set_current_pid` is called on every context-switch commit
(`schedule_with` steps 1/2/3, idle fallback, timer paths, `syscall_try_resched`,
`exception_do_resched`). `sync_per_cpu_current` masked the `idle` clobber by
writing `idle` *after* the pid, but `need_resched` and `current_irql` were
**not** restored. Consequences:

- A cross-CPU wake (`IPI_RESCHEDULE` → `this_cpu_set_need_resched(true)`) that
  races a commit on the target CPU can be lost, delaying/starving a woken
  thread. This is exactly the class of bug that becomes observable once APs
  schedule.
- `page_fault_handler`'s `this_cpu_irql() >= DISPATCH_LEVEL` bugcheck can be
  bypassed because the counter was reset to 0 mid-dispatch.

**Fix:** new `raw_gs_write_u32` (HAL) + `gs_write_u32` (cpu_local), and
`this_cpu_set_current_pid` now stores exactly 4 bytes. Regression coverage is
folded into `cpu_local_offset_sanity`, which sets a sentinel pid and asserts
`idle`/`need_resched` are preserved (keeps the suite at 716).

Files: `hal/raw/cpu.rs`, `arch/x64/cpu_local.rs`.

### F-01-B — `Scheduler.current_tid` is still written/read as a fallback

`current_tid_for_this_cpu()` returns `KPRCB.current_tid` only when
`kprcb_thread_in_self()`; otherwise it returns the global `self.current_tid`.
Every commit branch still writes `self.current_tid = tid` unconditionally. On a
multi-CPU run this is a single global cell written by whichever CPU holds the
scheduler lock. That is acceptable only because the KPRCB is the *read*
authority; but the following paths can observe the global as identity:

- `usermode.rs::wait_for_process` uses `s.current_tid` directly (boot path, BSP).
- `syscall/resched.rs` fallback branch sets `scheduler.current_tid = ...` and
  then relies on `this_cpu_set_current_thread/pid` (safe), but the global is
  left pointing at a thread that may be running on another CPU.
- `Scheduler::validate_runqueue_invariants` computes `effective_tid` from KPRCB
  when possible, but tests/local schedulers use the global.

**Assessment:** not a live bug while APs are passive. It is a prerequisite for
A: once APs dispatch, every *read* of current identity in a dispatch path must
go through KPRCB, and `self.current_tid` should be demoted to "last committed
tid" (diagnostics only). Design note tracked in the Phase 13 design doc.

### F-01-C — `try_per_cpu_pid()` returns `Some(0)` with an un-synced KPRCB

When `GS != 0` but `current_thread == null`, `try_per_cpu_pid()` falls through to
`this_cpu_current_pid()` and returns `Some(0)`. The free function
`scheduler::current_pid()` returns that `0` instead of the scheduler's tracked
pid during early boot. `Scheduler::current_pid()` itself is guarded by
`kprcb_thread_in_self()` and is correct. **Severity: Low** (boot-only window,
pid 0 is also the fallback). Recommend `try_per_cpu_pid()` return `None` when
`current_thread` is null unless the raw `current_pid` is explicitly desired.

---

## 2. F-02 — Zombie reap and kernel-stack lifetime

The scheduler defers reclaiming an exited `EPROCESS` behind a bounded zombie
queue (`ZOMBIE_PIDS`, max 64, `defer_reap`/`defer_reap_with_scheduler`).
`reap_pending_zombies(sched, current_pid)` may be called from inside
`schedule_with` (4 sites) and from `spawn_usermode` backpressure handling. It
frees a pid only when `!is_pid_running_on_any_cpu(pid)`.

`is_pid_running_on_any_cpu(pid)` scans `KPRCB_PAGES[cpu] + OFFSET_CURRENT_PID`
for every CPU. Reclaiming an eligible pid drops the `Kthread`, which drops
`kernel_stack: Option<Box<AlignedKStack>>` (16 KB).

### F-02-A — Reap excludes the *new* pid, not the stack we are running on **(PROPOSED)**

**Severity: High (latent under UP, exploitable under SMP).**

`schedule_with` commits the new thread **and calls `sync_per_cpu_current`
first**, then calls `reap_pending_zombies(self, new_pid)`. Because the KPRCB has
already been repointed to the new pid, `is_pid_running_on_any_cpu(old_pid)`
returns false for the thread whose kernel stack is *still under the CPU* until
the caller performs the actual `mov rsp`/`iretq`. The functions are not
reentrant on the old stack between reap and the RSP switch.

Concrete reachable path (exception termination):

```text
exception handler (running on faulting thread's kernel stack)
  -> terminate_user_process()
       -> terminate_current(-1)      // sets Terminated, defer_reap(last pid)
       -> exception_do_resched()
            -> schedule_with(true)
                 -> commit new thread + sync_per_cpu_current(new pid)
                 -> reap_pending_zombies(self, new_pid)
                      -> is_pid_running_on_any_cpu(old_pid) == false   // KPRCB moved
                      -> recycle_terminated(old_pid)                   // drops Box<AlignedKStack>
            -> schedule_with returns to exception_do_resched
            -> asm: mov rsp, next_rsp ; pop GPRs ; iretq   // still on freed stack
```

On a uniprocessor the freed 16 KB block is simply not reused before the RSP
switch, so the bug is masked. On SMP another CPU can allocate a `Box`/stack in
that window and observe/write stack memory of a "dead" thread — the classic UAF
the handoff warns about.

**Proposed fix (minimal, no new state):** reap must exclude the pid of the
context being switched *away from*, not the one being switched *to*. At reap
time the new pid is always `Running` (`thread_count > 0`), so it can never be in
the zombie queue; the only meaningful exclusion is the old pid. Change the four
call sites in `scheduler/schedule.rs` to pass `prev`'s pid:

```rust
// after commit + sync_per_cpu_current:
let prev_pid = self.find_kthread(prev).map(|t| t.pid).unwrap_or(0);
reap_pending_zombies(self, prev_pid);
```

`reap_pending_zombies`' parameter should be renamed `exclude_pid`. The
terminating pid is then reclaimed on a *later* schedule (guaranteed by the timer)
when no CPU is executing on its stack. Bounded-queue behavior is unchanged; no
leak (the pid stays queued and stays eligible).

### F-02-B — `cleanup_terminated_process` / `kill_pid` bypass the liveness guard **(PROPOSED)**

**Severity: High under SMP.**

`cleanup_terminated_process(pid)` (`scheduler/mod.rs`) calls
`recycle_terminated(pid)` directly, with no `is_pid_running_on_any_cpu` check.
Callers include `handler_waitpid` (both branches) and `syscall/ob/wait.rs`.
Likewise `Scheduler::kill_pid` (services manager, `syscall/ob/set.rs`) frees all
threads of a pid immediately (`self.kthreads[i] = None`).

If the target pid still has a thread executing on another CPU (or is between
`terminate` and its RSP switch), its kernel stack is freed while in use. Under
the current single-user boot model the child has fully exited before
`waitpid` reaps, so this is masked; with APs dispatching it becomes reachable.

**Proposed fix:** route both through the zombie path (or add the
`is_pid_running_on_any_cpu` + defer guard) so reclaim only happens when no CPU
holds the pid. Alternative: assert `thread_count == 0 &&
!is_pid_running_on_any_cpu(pid)` and defer otherwise.

### F-02-C — Liveness check covers `Running` only (assessment: OK)

`is_pid_running_on_any_cpu` only sees `KPRCB.current_pid`. A thread that is
`Ready` but not yet dispatched would not be seen — however, `recycle_terminated`
only ever runs for a pid with `thread_count == 0`, i.e. every thread is
`Terminated` and already removed from its run queue. No Ready-but-unreaped
thread can belong to a reap-eligible pid. **Not a bug today**; it becomes
load-bearing for A and should be stated as an invariant.

### F-02-D — `Rule 6.3.2` `pid_gen` is not implemented (informational)

`next_pid` is monotonically increasing and never reused, so stale-PID UAF
cannot occur via reuse today. The documented `pid_gen` guard is absent; if PID
reuse is ever introduced, the zombie queue must be generation-tagged. No action
now.

---

## 3. Summary

| ID | Severity | Status | Action |
|----|----------|--------|--------|
| F-01-A | High | **FIXED** | 4-byte pid store + regression test |
| F-01-B | Medium | Design | KPRCB-only identity in dispatch paths (Phase 13-A) |
| F-01-C | Low | Open | `try_per_cpu_pid` should return `None` un-synced |
| F-02-A | High | **PROPOSED** | reap excludes previous pid |
| F-02-B | High | **PROPOSED** | guard `cleanup_terminated_process`/`kill_pid` |
| F-02-C | — | OK | invariant to record |
| F-02-D | Low | Open | pid_gen if reuse ever added |

**Validation of the fixes applied here:** `neodev test` SMP2 → 716/716 PASS
(including the widened `cpu_local_offset_sanity`). The PROPOSED items are
deferred to the AP-scheduling work so they can be validated with real AP
dispatch (evidence: `thief=cpu>0`), per the Phase 13 plan.
