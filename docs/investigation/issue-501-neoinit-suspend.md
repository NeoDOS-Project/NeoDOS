# #501 — NeoInit left `SUSP` at the boot hand-off; NeoShell never starts

**Branch:** `fix/501-neoinit-suspend-handoff` (from `develop` @ `9d04417`).
**Issue:** #501. **Scope:** bootstrap hand-off state transition only. No
scheduler policy, ABI, KStack or diagnostic change.

---

## 1. Symptom

`NeoInit` reaches the Ring-3 bootstrap hand-off but is left `SUSP`; it never
executes its first instruction and NeoShell is never spawned:

```text
[BOOT_PROGRESS] NEOINIT_RING3
[USERMODE] wait_for_process pid=2
[USERMODE] blocking TID 0, current_tid=0 activating pid=2
[USERMODE] activated TID=4
[SCHED_WARN] tag=timer TWO+ Running on cpu=0 tids=[0, 3, 5, …]/[0, 1, 0, …] sched.current=3 kprcb_tid=Some(5)
[SCHED_WARN]   tid=4 pid=2 name=neoinit state=SUSP cpu=0 wait=None
```

`NeoInit v…` and `[neoinit] entering spawn loop…` never appear, so the shell
never starts.

## 2. Reproduction

```bash
neodev build --quick --image
neodev run --headless --net user --serial /tmp/serial.txt
# grep: no "NeoInit v"; [SCHED_WARN] neoinit state=SUSP; "TWO+ Running"
```

Reproduced on QEMU SMP1/2/4 and VirtualBox SMP2, and on unmodified `develop`
(no #501 changes). The `[SCHED_WARN]` line is timing-dependent, but the missing
`NeoInit v` / `[neoinit]` output is the invariant symptom.

## 3. Root cause

```text
Root cause:
wait_for_process() marks the hand-off target thread Running via
current_kthread_mut(), which resolves the thread through the per-CPU
KPRCB.current_thread. Since #482 deferred the KPRCB publication to just before
the Ring-3 iretq, that resolver still identifies the bootstrap thread (TID 0)
at that point, so the target (NeoInit, TID 4) was never transitioned
Suspended -> Running and stayed Suspended.

Trigger:
NeoInit is created Suspended (add_ring3_process_with_stack) and its only
activation is the kernel bootstrap hand-off in wait_for_process() (usermode.rs);
it is not activated through activate_suspended_process().

Why NeoInit becomes SUSP:
It is born Suspended; the hand-off failed to set it Running because
current_kthread_mut() resolved to boot instead of the target. Nothing re-marks
it, so it stays Suspended.

Why sched.current != KPRCB.current_thread:
sched.current_tid is set to the target (4) inside the without_interrupts block;
the per-CPU KPRCB.current_thread is deliberately still boot until the deferred
#482 publication, immediately before the iretq. The [SCHED_WARN] snapshot is
taken by a timer during that window (IF restored after the block, before
disable_interrupts()).

Why the shell never starts:
schedule() only commits ThreadState::Ready candidates. NeoInit is Suspended, so
it is never selected again; it never executes its first syscall and NeoShell is
never spawned.

Why existing guards/tests did not catch it:
The suite passed (826/826); no test asserted the hand-off target's state
transition. [SCHED_WARN] TWO+ Running is a warning, not a failure, and shell
startup is not part of the deterministic suite.

Not the cause:
- The deferred KPRCB publication itself (#476/#482) is correct; the defect is
  the resolver used for the target's state transition.
- schedule() never selects a Suspended thread (verified).
- No #PF/heap fault on the failing boots (#490 is unrelated).
- netd/ntpd scheduling (#340): the clock path is independent.
```

### Evidence — why this is a #482 regression

`git show 2dc0db4^:neodos-kernel/src/usermode.rs` shows the pre-#482 order:

```rust
// F-01: sync per-CPU KPRCB (BSP)
if !target_ptr.is_null() {
    sync_per_cpu_current(target_ptr, target_pid);   // KPRCB = target
}
...
if let Some(k) = s.current_kthread_mut() {          // resolves target
    k.state = ThreadState::Running;
}
```

`#482` (`2dc0db4`) moved `sync_per_cpu_current` *after* `disable_interrupts()`
and after the `current_kthread_mut()` block:

```rust
target_ptr_out = target_ptr;                        // capture only
...
if let Some(k) = s.current_kthread_mut() {          // now resolves BOOT
    k.state = ThreadState::Running;                 // boot re-marked Running
}
...
disable_interrupts();
sync_per_cpu_current(target_ptr_out, target_pid_out);
```

So the target was left `Suspended` and the bootstrap thread was re-marked
`Running` (the `TWO+ Running` snapshot), while `sched.current_tid` pointed at
the target. Before #482 the same code was correct only because the KPRCB had
already been published.

## 4. Fix

Mark the target by TID, independent of the KPRCB publication order. The
bookkeeping is extracted into `Scheduler::mark_handoff_target_running`
(`scheduler/lifecycle.rs`) so the contract is testable, and
`wait_for_process` calls it:

```rust
s.mark_handoff_target_running(target_tid);
```

`mark_handoff_target_running` resolves with `find_kthread_mut(target_tid)` (not
`current_kthread_mut()`) and sets `ThreadState::Running`. The deferred #482
publication, the boot-thread block, RSP0 handling and the iretq ordering are
unchanged.

## 5. Regression coverage

Kernel test `handoff_target_marked_running_by_tid` (`scheduler/tests.rs`): with
`sched.current_tid = BOOT_TID` (the deferred-publication window) and a
`Suspended` target, it asserts the target becomes `Running` and is no longer
`Suspended`.

Proven both ways:

- correct method (`find_kthread_mut(target_tid)`): **827/827** PASS;
- broken method (`current_kthread_mut()`): **826 passed, 1 failed**
  (`handoff_target_marked_running_by_tid`).

## 6. Validation

### QEMU

- SMP1 / SMP2 / SMP4 (runtime): `NeoInit v…`, `[neoinit] entering spawn loop…`,
  `[neoinit] Iniciando el intérprete de comandos…`, `neoshell.nxe` spawned,
  `C:\>` prompt. `name=neoinit state=SUSP` = 0, `TWO+ Running` = 0.
- Kernel suite: **827/827**.

### VirtualBox

- SMP2 (runtime + NAT): `NeoInit v…`, spawn loop, `C:\>` prompt,
  `[ntpd] synced`, `name=neoinit state=SUSP` = 0, `TWO+ Running` = 0.

### Scheduler invariants

`[READY_GUARD_STATS]` all zero; `IRETQ_AUDIT … bad=0`; `RSP_FOREIGN` /
`RSP_OWNER_MISMATCH` / `WF_ORDER_VIOLATION` / `IRETQ_BAD_FRAME` not observed on
any fixed run.

### Before / after (QEMU SMP2)

| | `NeoInit v…` | spawn loop | shell | neoinit SUSP | TWO+ |
| --- | --- | --- | --- | --- | --- |
| `develop` (before) | 0 | 0 | no | 12 | 12 |
| fixed | 1 | 1 | `C:\>` | 0 | 0 |

## 7. Ruled-out hypotheses

- **#490 residual `Option::unwrap()` panic**: not present on the failing boots
  and not on the fixed boots; no causal link.
- **#340 netd/ntpd scheduling**: the shell failure reproduces with the clock
  path failing, and the clock path succeeds independently of the shell.
- **KPRCB publication order (#476/#482)**: the deferred publication is correct
  and preserved; the defect was the resolver used for the state transition.
- **`schedule()` selecting a Suspended thread**: it only commits `Ready`
  candidates.
- **Memory/allocator fault**: no fault on the failing boots.

## 8. Relationship to #340 / #490

No causal relationship is claimed. The `[SCHED_WARN] TWO+ Running` observed here
is a *symptom of this bug* (boot re-marked Running while the target stayed
Suspended), not the #340 netd/ntpd scheduling issue. #490 is untouched.

## 9. Follow-up

None required. This is a self-contained hand-off state-transition fix; no
scheduler policy, ABI or other repository changes.
