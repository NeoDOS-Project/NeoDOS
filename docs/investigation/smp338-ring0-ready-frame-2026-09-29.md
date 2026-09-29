# Investigation — #338 Ring-0 `Ready` frame publication (netcfg stalls after one iteration)

**Date:** 2026-09-29
**Branch:** `fix/338-ring0-ready-frame`
**Rebased onto:** `develop` @ `e8a89fc`
**PR:** #351
**Environment:** QEMU 10.2.2, TCG, `q35`, SMP1/2/4, user-mode NIC,
VirtualBox 7.2 (bridged) and QEMU monitor-driven shell.

---

## Baseline

| Item | Value |
|------|-------|
| `develop` HEAD | `e8a89fc` (`git status --short` clean) |
| `neodev test` (pre-fix) | **757/757** kernel, Command + Shell `PASSED` |
| `neodev test` (post-fix) | **762/762** kernel (+5 regressions), Command + Shell `PASSED` |
| Historical attempt | `95a6ab2` on `fix/338-ring0-ready-frame` (never merged) |
| #346 | merged (`0e4d176`); *recovery* path only — not modified |
| #348 | merged (`22a53c9`); idle stack canary — not modified |

---

## Reproduction (original)

`netcfg` (the resident daemon introduced in #320) applies DHCP leases published
by `dhcpd` in the Registry to the runtime NIC. Observed on VirtualBox SMP1,
bridged:

```text
[NETCFG_DBG] daemon start
[NETCFG_DBG] iter
[NETCFG_DBG] read ip=0.0.0.0
[NETCFG_DBG] link=up
[NETCFG_DBG] sleep
```

Then nothing for 45+ s (no second `iter`), while the shell stayed responsive and
`dhcpd` stayed alive. Consequence: `netcfg /status` showed
`registry: ip=10.0.1.57` but `nic: ip=0.0.0.0` (`aplicado: no`); ping 100 % loss.

---

## Root Cause

Exact causal chain:

```text
netcfg (Ring 3) executes one loop iteration
    → enters a syscall (Ring 0, cs = 0x08)
    → a timer IRQ fires while the thread is inside the syscall
    → on_timer_tick / timer switch-out saves k.rsp = current_rsp
      (a Ring-0 frame) and publishes the thread Ready + enqueued
    → the thread is now Ready with a non-dispatchable Ring-0 frame
    → every schedule_with(require_ring3 = true) rejects it (frame_is_ring3 == false)
    → Ready + rqcpu = -1 forever → the daemon never runs again
```

The three publish sites saved `current_rsp` **without** checking that the frame
was dispatchable back to Ring 3:

| File | Site |
|------|------|
| `scheduler/schedule.rs` | `on_timer_tick` timeslice-expiry branch |
| `arch/x64/idt.rs` | user-preempt switch-out save block |
| `arch/x64/idt.rs` | kernel-preempt switch-out save block |

The explicit `if next_cs & 3 != 3` revert logic in the timer protected only the
thread selected as **next**; it did not protect the thread being **saved**.

### Invariant

```text
Ready + saved dispatch frame  =>  cs & 3 == 3     (user threads)
```

---

## Historical Fix (`95a6ab2`)

`95a6ab2` (*not merged*) introduced:

- `thread_dispatch_frame_is_ring3(k)` — predicate with a `pid == 0` exemption,
  intended to exempt kernel threads from the Ring-3 gate;
- `on_timer_tick(current_rsp, interrupted_cs)` — gate Ready publication on the
  interrupted CS;
- gating of **both** `idt.rs` switch-out save blocks with the predicate;
- a `debug_assert` on the syscall-return publish;
- 5 regression tests.

It correctly identified the enabling condition and the three publish sites. It
was based on `51107a5`, i.e. before #346/#348, and was never merged, so it had to
be rebased and reconciled with the later scheduler work.

---

## Current Fix

Rebased `95a6ab2` onto `develop` (clean, no conflicts) and reconciled with the
later scheduler fixes (#346 and #348), then corrected a latent defect in the
historical predicate.

### Historical approach vs current adaptation

```text
Historical approach (95a6ab2):
  thread_dispatch_frame_is_ring3(k) = (k.pid == 0) || frame_is_ring3(k)
  Gate: on_timer_tick(interrupted_cs) + both idt.rs switch-out branches.

Current adaptation:
  The `pid == 0` exemption is WRONG. `spawn_kthread_named` gives kernel
  threads a real pid, so netd has pid == 1 and was NOT exempt. netd always
  runs in Ring 0, so the gate left it Running with a fresh slice and never
  set NEED_RESCHED → netd was starved and never printed
  '[NET] netd running' (a #340-class regression introduced by the rebase).

Reason:
  The correct discriminant is "does this thread have a user image?".
  Eprocess::new_kernel sets user_slot = None; Eprocess::new_ring3 sets
  Some(slot). A thread is a kernel thread iff its Eprocess has no user slot.

Changes made in this adaptation:
  - thread_dispatch_frame_is_ring3(k, is_kernel_thread):
       is_kernel_thread || frame_is_ring3(k)
  - New Scheduler::is_kernel_thread(k): pid == 0 || is_idle ||
    eprocess.user_slot.is_none() (no eprocess => kernel thread).
  - on_timer_tick: gate only user threads; kernel/idle threads keep the
    historical behaviour (publish Ready).
  - idt.rs user-preempt branch: gate with frame_is_ring3 (only user threads
    reach this branch).
  - idt.rs kernel-preempt branch: NO gate — it serves kernel threads (netd),
    which run in Ring 0 by design and are dispatched via
    schedule_with(require_ring3 = false).
  - resched.rs syscall-return debug_assert uses frame_is_ring3 directly
    (that path is Ring-3 only).
```

No scheduler refactor, no policy change, `require_ring3` not weakened, `netcfg`
not modified.

### Behaviour when the frame is Ring 0

The thread is **left `Running`** with a fresh time slice; `NEED_RESCHED` is not
set. Its in-flight syscall continues; on syscall return
(`syscall_try_resched`) the real Ring-3 frame is captured and the thread is
published `Ready` correctly. The timer switch-out sites likewise refuse to
publish a Ring-0 frame for a user thread, so nothing resurrects it prematurely.
This is the existing NeoDOS deferral mechanism, not a new state transition.

---

## Regression

Deterministic, in `scheduler/tests.rs` (+5):

| Test | Invariant |
|------|-----------|
| `n338_ready_frame_must_be_ring3` | Ring-3 frame is dispatchable; a user thread's Ring-0 frame is not; a **kernel** thread (non-zero pid, Ring-0 frame) **is** (netd guard). |
| `n338_ring0_frame_ready_thread_not_dispatched` | `schedule_with(require_ring3 = true)` never commits a Ready thread whose frame is Ring 0. |
| `n338_syscall_preempt_preserves_ring3_progress` | A user thread preempted in a syscall is re-published with a Ring-3 frame on syscall return and becomes schedulable again. |
| `n338_long_lived_yield_loop_stays_dispatchable` | 64 yield/preempt cycles: the thread stays dispatchable every iteration (netcfg/dhcpd pattern). |
| `n338_timer_expiry_ring3_gate` | Timeslice expiry publishes Ready only on a Ring-3 interrupt; a Ring-0 (in-syscall) expiry stays Running. Extended with kernel-vs-user Eprocess coverage. |

No test couples to #346 or checks `SS == 0x15`.

---

## Validation

### Kernel suite

`neodev test` → **762/762**, Command + Shell `PASSED` (baseline 757/757; +5).

### End-to-end (the #338 acceptance criterion)

QEMU SMP1, user-mode NIC, shell driven over the QEMU monitor:

```text
[dhcpd] ACK: IP=10.0.1.80 mask=255.255.255.0 gw=10.0.1.1
[dhcpd] Network configured

# first poll (daemon has not applied yet)
netcfg: estado de la interfaz
  registry: ip=10.0.1.80 mask=255.255.255.0 gw=10.0.1.1 dhcp=on
  nic:      ip=0.0.0.0
netcfg: aplicado: no

    Dirección IPv4 . . . . .: 10.0.1.80      (ipconfig)
    Puerta de enlace . . . .: 10.0.1.1

# later poll — daemon applied the lease
netcfg: estado de la interfaz
  registry: ip=10.0.1.80 mask=255.255.255.0 gw=10.0.1.1 dhcp=on
  nic:      ip=10.0.1.80
netcfg: aplicado: sí
```

The `Netcfg` daemon (pid 3) kept running and applied the lease between the two
polls. Before the fix the NIC stayed `0.0.0.0` (`aplicado: no`); had the daemon
stopped after one iteration it could never have applied it.

### Smoke matrix (QEMU, bounded with `timeout`)

| Config | Runs | ALL_TESTS_COMPLETE | PANIC | GPF | PF | STACK CANARY | READY_WHILE_RUNNING | STALE_RSP_DISPATCH | STACK_OWNERSHIP_CONFLICT |
|--------|------|--------------------|-------|-----|----|--------------|---------------------|--------------------|--------------------------|
| SMP1 | 2 | 2/2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| SMP2 | 3 | 3/3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| SMP4 | 3 | 3/3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

`[NET] netd running` present in every run (SMP1/2/4).

The SMP2/SMP4 runs end in the same independent, pre-existing SMP>1
service-start boot-gap class recorded in `smp343-...md` / `smp346-...md`
(no canary, no panic, no guard violation). They are **not** a #338 symptom.

### Marker comparison vs baseline `develop` (SMP1)

`PANIC`/`GPF`/`PF`/`STACK CANARY`/`SCHED_WARN`/`READY_WHILE_RUNNING`/
`STALE_RSP_DISPATCH`/`STACK_OWNERSHIP_CONFLICT` are 0 in both. The 16
pre-existing `[SYSCALL_CORRUPT]` reports present on `develop` are **gone** with
the fix (frames now consistently Ring 3 across the syscall window).

---

## Relationship to previous scheduler work

- **#293** — idle CPU ownership. Preserved: the idle fallback still selects only
  the current CPU's idle (`find_idle_ptr(cpu)`).
- **#346** — stale `KPRCB.current_thread` after a rejected non-Ring3 dispatch +
  un-gated idle fallback. This is the *recovery* path; #338 is the *enabling
  condition*. #346 remains intact and untouched.
- **#348** — idle stack canary sizing. The canary checks here use the owning
  thread's `kernel_stack_size`; untouched.
- **#331** — SMP>1 hang; distinct (this reproduces at SMP1). The recursive
  `SCHEDULER` lock fix (#344) is preserved.
