# Phase 13 — SMP Scheduling Real: design for AP dispatch

**Status:** implemented behind the non-default `smp-ap-sched` feature. The
post-`netd` AP context-switch GPF was root-caused and fixed in Phase 13-A.1
(§9); APs now dispatch, preempt, context-switch and return through `iretq`
cleanly on SMP2/SMP4. The default build keeps APs idle for deterministic twin
boots and is regression-clean.
**Branch context:** `feat/phase13-ap-scheduling-real`
**Related:** `docs/investigation/smp-bring-up-report.md`,
`docs/investigation/f01-f02-adversarial-audit.md`,
`docs/architecture/source-of-truth.md` (Rules 6.1.4, 6.1.5, 6.1.6).

---

## 0. Implemented pieces (this session)

| Area | Change |
|------|--------|
| AP stacks | `init_smp` now allocates AP stacks with `alloc_frames(2)` (16 KB contiguous). The old single `alloc_page()` made `top` point 12 KB past the frame, so APs clobbered foreign memory. |
| Per-AP stack table | Trampoline loads its stack from `ap_stack_table[apic_id]` (per-AP) instead of one shared `ap_stack_ptr` (all APs shared CPU1's stack). |
| Shared IDT | `idt::kernel_idt_descriptor()` publishes the BSP IDT; APs `lidt` it instead of a zeroed per-CPU page (which would triple fault on the first interrupt). |
| Final PML4 | `paging::publish_paging_final()`/`kernel_pml4()`; APs wait for paging finalization then `write_cr3` the final kernel PML4. |
| Per-CPU idle | `Kthread.is_idle`; `register_ap_idle(cpu, stack_top)` fabricates a Ring0 frame 4 KB below the AP stack top and the AP `iretq`s into `idle_task` (see `ap_enter_idle`). |
| Per-AP LAPIC timer | `timers::apic::init_apic_enable()` + `init_apic_timer_ap()` (vector 32, no HPET calibration / PIC mutation). |
| BSP-only side effects | Timer global tick/cursor/watchdog/DPC/event-bus and idle work-queue draining are BSP-only. |
| Per-CPU invariants | `invariants::IRQ_NESTING` / `IN_TIMER_IRQ` are per-CPU (`IRQ_REENTRANCY` false positive fixed). |
| Identity | `sync_bsp_identity()` pins CPU0's KPRCB to the boot thread; global scan re-homes `Kthread.cpu` to the executing CPU; boot pinned to CPU0; idle excluded from the global scan. |
| Steal safety | `steal_and_migrate` only migrates `Ready` threads (drops invalid/stray entries). |
| Enable gate | `AP_SCHED_ACTIVE` gated by the `smp-ap-sched` cargo feature; AP timer handler early-returns before taking the scheduler lock until enabled. |

---

## 1. Goal

Make Application Processors (APs) actually dispatch threads instead of idling
in `raw_hlt_once()`. Today APs are brought up, get a per-CPU KPRCB/GDT/TSS/IDT,
increment `AP_READY_COUNT`, and then only `hlt`:

```rust
// arch/x64/smp.rs::ap_entry (~452)
loop {
    if cpu_local_mod::this_cpu_need_resched() {
        cpu_local_mod::this_cpu_set_need_resched(false);
        // TODO(smp): wire up local scheduler schedule() — APs spin with HLT but never yield
    }
    unsafe { crate::hal::raw::raw_hlt_once(); }
}
```

Target: an AP enters the scheduler, steals a Ready thread from another CPU's
per-CPU run queue, and runs it. Evidence of success:

- `STEAL_ATTEMPTS`/`STEAL_SUCCESS` with `thief = cpu > 0`.
- A real thread observed Running with `cpu ∈ {1,2,3}` (KPRCB + `SCHED_DUMP`).
- `sched_forensic_enable(true)` produces **zero** `[SCHED_WARN]`.
- `neodev test` 716/716 on SMP1/SMP2/SMP4.

Non-goals for the first cut: per-CPU scheduler locks, load balancing policy,
CPU affinity, and preempting kernel threads on APs while they hold scheduler
locks.

---

## 2. Current state (verified)

| Component | State |
|-----------|-------|
| `Scheduler` | **One global instance** behind `spin::Mutex`. `current_tid` is a single global cell. |
| Per-CPU run queues | Exist: `CpuRunQueue` at `KPRCB + 0x018`, `RUNQUEUE_LOCKS[16]`. |
| Per-CPU identity | `KPRCB.current_thread/pid/idle/need_resched`; `sync_per_cpu_current`; `kprcb_thread_in_self()` gates KPRCB vs global. |
| Idle | **Single** `IDLE_TID = 1` Kthread, static `IDLE_STACK[4096]`, entry `idle_task`. |
| Idle hardcoding | `tid == IDLE_TID` in `schedule.rs`, `idt.rs`, `queue.rs`, `aging.rs`, `resched.rs`, `cpu_local.rs`. |
| Work stealing | `scheduler/smp.rs::try_work_steal` / `steal_and_migrate` already lock both run queues and update `Kthread.cpu`. |
| AP timer | None. `init_apic_timer()` runs once on the BSP. APs never program their LAPIC timer. |
| AP CR3 | Trampoline loads the **bootloader PML4**. APs never adopt the final `PML4` loaded by `init_custom_page_tables`. |
| AP bring-up time | `main.rs` PHASE 2.8 (`init_smp`), **before** `STI` (2.91) and before `init_custom_page_tables`. |
| Dispatch contract | `schedule_with(require_ring3)` (Rule 6.1.4) already prevents committing non-dispatchable frames. |

---

## 3. Blockers and decisions

### B1 — Per-CPU idle representation

The scheduler cannot switch away from an AP unless the AP's execution context
is represented as a `Kthread` with a saved `rsp`. The AP's `ap_entry` stack
(`AP_STACK_PTRS[cpu]`) must become the idle thread's kernel stack.

**Decision:** add `is_idle: bool` to `Kthread` and make all idle checks
`k.is_idle` instead of `tid == IDLE_TID`. This is cleaner than a reserved TID
range and survives TID reuse. Required touch points:

- `types.rs` (+ constructors in `mod.rs`, `thread.rs`, `lifecycle.rs`).
- `schedule.rs`: runqueue validation skips idle; idle fallback resolves the
  **current CPU's** idle (`k.is_idle && k.cpu == this_cpu`).
- `mod.rs::has_non_idle_threads`, `queue.rs::enqueue_to_cpu_run_queue`,
  `aging.rs`, `idt.rs` preemption branch, `resched.rs` idle fallback,
  `cpu_local::sync_per_cpu_current`.
- `Scheduler::new()` keeps TID1 as CPU0's idle; add
  `Scheduler::register_ap_idle(cpu, stack_top) -> tid` allocating a distinct TID.

### B2 — AP must adopt the final kernel PML4

`init_custom_page_tables` loads a static `PML4` (4 GiB identity map + user
window). APs still run the bootloader's tables. Kernel threads may survive on
the identity map, but Ring-3 pages and any map/unmap operations performed after
boot live only in the final `PML4`.

**Decision:** publish the final CR3 in an atomic (`KERNEL_PML4`), and have each
AP load it once before enabling scheduling. Two viable orderings:

- **(preferred) Move AP bring-up later**, after `init_custom_page_tables` and
  `exception::init_teb_paging`, so APs bootstrap directly into the final PML4.
  This also avoids a window where APs run on a table that is later replaced.
- Or keep PHASE 2.8 and have `ap_entry` spin until `KERNEL_PML4 != 0`, then
  `write_cr3(KERNEL_PML4)` (must not be done while the AP is using stale TSS
  mappings; safe because both tables identity-map the kernel).

The first option is cleaner and removes the "AP runs while BSP mutates global
paging" hazard. It also lets APs be started only once the I/O APIC / IDT / IPI
infrastructure exists.

### B3 — Per-AP LAPIC timer

`timers/apic.rs::init_apic_timer` does BSP-only work: HPET calibration, legacy
PIC masking, HPET legacy disable. APs must not repeat those.

**Decision:** add `init_apic_timer_ap()`:

```rust
/// Program this CPU's LAPIC timer using the already-calibrated bus frequency.
/// No HPET calibration, no PIC/HPET legacy mutation.
pub fn init_apic_timer_ap() -> bool {
    if !is_apic_present() { return false; }
    let bus_khz = apic_bus_khz();
    if bus_khz == 0 { return false; }
    // SVR: enable APIC (keep spurious vector 0xFF)
    // LVT timer: periodic, vector 32
    // DIVIDE_16, INIT_COUNT = (bus_khz/16 * TICK_INTERVAL_US) / 1000
    // EOI
}
```

Reusing **vector 32** means APs run the existing `timer_handler_asm` →
`timer_handler_inner` path. That path is already written around
`current_tid_for_this_cpu()`/`current_kthread_mut()`, which prefer KPRCB. The
remaining change is to treat the AP idle as idle (B1) and to skip BSP-only side
effects (watchdog/cache flush/DPC) when `this_cpu_id() != 0` — or accept them as
per-CPU no-ops (the watchdog is global; it should stay BSP-only).

### B4 — Test determinism

The kernel test suite runs **after** `netd` exists and exercises global
scheduler/runqueue state. If APs dispatch concurrently:
`try_work_steal` is paused by `SCHED_TEST_MODE` only inside k18/k19, so APs
could steal threads created by other tests and make the suite nondeterministic.

**Decision:** add an explicit boot gate `AP_SCHED_ACTIVE: AtomicBool` (false by
default). AP timer handler / AP idle wakeups early-return until it is set.
`main.rs` sets it **after** `testing::run_all()` (next to
`sched_forensic_enable(true)`). This is a bring-up control, not a scheduling
workaround: no thread is forced, no runqueue is drained, no extra yield is
injected. The gated default keeps SMP1/SMP2/SMP4 suites deterministic; the E2E
evidence is captured in the post-suite window.

---

## 4. Proposed change list

| File | Change |
|------|--------|
| `scheduler/types.rs` | `Kthread.is_idle: bool` (default false). |
| `scheduler/mod.rs` | `Scheduler` idle lookup by `(is_idle, cpu)`; `register_ap_idle(cpu, stack_top)`; `has_non_idle_threads` uses `!is_idle`; `AP_SCHED_ACTIVE` static + setter. |
| `scheduler/thread.rs` | `new_idle` sets `is_idle = true`; `new_ring3_with_stack` false. |
| `scheduler/lifecycle.rs` | `spawn_kthread` sets `is_idle = false`. |
| `scheduler/schedule.rs` | Idle fallback resolves this CPU's idle; validation/aging skip `is_idle`; reap exclusion per F-02-A. |
| `scheduler/queue.rs` | `enqueue_to_cpu_run_queue` returns early for `is_idle`. |
| `scheduler/aging.rs` | Skip `is_idle`. |
| `scheduler/smp.rs` | Unchanged algorithm; gate already respected via `try_work_steal`. |
| `arch/x64/smp.rs` | `ap_entry`: set GS/KPRCB (existing) → register idle → set `KPRCB.current_thread` to idle → `init_apic_timer_ap()` → `sti` → iretq into idle frame (or call `idle_task`). Idle loop honors `AP_SCHED_ACTIVE`. |
| `arch/x64/idt.rs` | Idle preemption branch recognizes `is_idle` (per-CPU); skip BSP-only timer side effects for `this_cpu_id() != 0`. |
| `arch/x64/paging.rs` | Publish final `KERNEL_PML4` after `write_cr3` (if move-later is not chosen). |
| `main.rs` | Ordering: AP bring-up after paging/exceptions (preferred) or publish CR3; set `AP_SCHED_ACTIVE(true)` after `run_all()`. |
| `syscall/resched.rs` | Idle fallback via `is_idle`; F-01-B KPRCB-only identity. |
| `arch/x64/cpu_local.rs` | `sync_per_cpu_current` uses `is_idle`. |
| `timers/apic.rs` | `init_apic_timer_ap()`. |

---

## 5. AP bootstrap and first dispatch (sketch)

```text
ap_entry(stack_top):
  GS = KPRCB                       (existing)
  AP_READY_COUNT++                 (existing, early)
  gdt::init_ap(cpu); load shared IDT (existing + idt::kernel_idt_descriptor)
  # NEW:
  wait paging_final; write_cr3(kernel_pml4)
  tid = register_ap_idle(cpu, stack_top)  # is_idle=true, Ring0 frame -> idle_task
  init_apic_enable(); init_apic_timer_ap()  # periodic vector 32 on this CPU
  sti
  iretq into idle.rsp              # idle_task loop; timer now preempts it
```

Timer tick on AP (existing `timer_handler_inner`, idle branch):
until `AP_SCHED_ACTIVE` (feature `smp-ap-sched`) the handler early-returns
before taking the scheduler lock. Once enabled: `on_timer_tick` expires the
idle slice → state Ready → idle branch calls `scheduler.schedule()` → step 2
`try_work_steal()` pulls a Ready TID (or step 3 global scan) → commit +
`sync_per_cpu_current` → `timer_handler_asm` switches RSP and `iretq`s into the
thread.

Notes:

- `schedule_with(false)` is correct for `iretq` from the AP's Ring-0 idle
  context; if the stolen thread's saved frame is Ring-3, `prepare_timer_return`
  sets TSS.RSP0 and the `iretq` drops to Ring 3. If it is Ring-0 (kernel
  thread), the interrupt-frame mechanics return to Ring 0 directly.
- The global scheduler mutex serializes BSP/AP; the KPRCB is the dispatch
  authority. `self.current_tid` becomes "last committed" bookkeeping only.

---

## 6. Risks

| Risk | Mitigation |
|------|------------|
| Kernel-stack UAF on reap while switching away | fix F-02-A (reap excludes previous pid) **before** enabling APs. |
| `cleanup_terminated_process`/`kill_pid` freeing a stack in use | fix F-02-B (liveness guard / defer) before enabling APs. |
| AP runs on stale PML4 | move bring-up after `init_custom_page_tables`, or reload published CR3. |
| AP allocates from heap before heap lock is safe | AP idle registration done by the **BSP** before SIPI, or AP allocates only after `AP_READY_COUNT`; prefer BSP-side pre-allocation. |
| `need_resched` lost on commit | fixed by F-01-A (u32 pid store). |
| Test nondeterminism | `AP_SCHED_ACTIVE` gate set after `run_all()`. |
| Deadlock: AP spins on SCHEDULER while BSP holds it with IRQs off | keep critical sections short; AP `on_timer_tick` already early-returns under `SCHED_TEST_MODE`; audit lock order SCHEDULER → RUNQUEUE → USER_MEMORY. |
| LAPIC timer vector 32 shared with BSP side effects | gate watchdog/cache-flush/DPC to BSP only. |
| Only one `IDLE_STACK` for all CPUs | per-CPU idle reuses the pre-allocated AP stack; no shared static. |

---

## 7. Validation plan

1. Land F-01-A (done) and F-02-A/F-02-B audit fixes; `neodev test` 716/716 SMP2.
2. Implement B1 (is_idle) without enabling APs; suite must stay 716/716.
3. Implement B2 (PML4) + B3 (per-AP timer) + `ap_entry` bootstrap; keep
   `AP_SCHED_ACTIVE=false`; suite must stay 716/716.
4. Enable `AP_SCHED_ACTIVE` after `run_all()`; capture:
   - `[STEAL] attempts>0 success>0` with `thief` logged from `cpu>0`,
   - `SCHED_DUMP` showing a thread Running on `cpu=1/2/3`,
   - `[SCHED_WARN]` count == 0.
5. Run `neodev test` with `cpus = 1, 2, 4` (rebuild `disk_image.img` before
   each measurement; `neodev.toml` currently defaults to 2).
6. F-01/F-02 adversarial re-read once APs are live (F-01-B, F-02-C invariants).

---

## 8. Open questions

1. **Bring-up ordering:** move `init_smp` after `init_custom_page_tables`
   (cleaner, bigger boot-flow diff) vs. publish/reload CR3 (smaller PIO, leaves
   a window with two active PML4s). Recommendation: move bring-up later.
2. **Idle thread per CPU vs. per-`is_idle` pool:** distinct TIDs per CPU is
   simplest; alternatively one idle Kthread per CPU but excluded from
   `next_tid` accounting. Recommendation: distinct TIDs, `is_idle` drives logic.
3. **BSP-only side effects in vector 32:** extract a
   `timer_handler_per_cpu()` vs. guard with `this_cpu_id() == 0`. Recommendation:
   guard first, extract if the hot path gets noisy.
4. **Steal policy:** current `try_work_steal` scans victims 0..MAX_CPUS and
   migrates entire queues. Fine for bring-up; revisit (steal half, work
   conserving) after correctness is proven.

---

## 9. Resolution (Phase 13-A.1)

**Status:** root cause found and fixed. With `--features smp-ap-sched` the AP
now dispatches, is preempted, context-switches and returns through `iretq`
without #GP on SMP2 and SMP4 (see §10 for the validation matrix).

### 9.1 Root cause

The GPF was **not** a corrupt `iretq` frame built by one context switch. It was
the symptom of **the same KTHREAD executing concurrently on two CPUs sharing one
kernel stack**. When both CPUs took their timer interrupt on the shared stack,
the AP's 15-GPR save area overlapped the BSP's pending `iretq` frame; the BSP
then executed `iretq` with a clobbered `CS` (`0x7800`, a garbage selector) and
raised `#GP error=0x7800 at rip=<timer_handler_asm iretq>`.

The route that exposed a *live* thread to another CPU was the cooperative-yield
path:

```text
netd (TID 3, Ring 0)                 BSP
  net_tick()
  yield_current_thread()
      make_thread_ready()  ← state = Ready, rsp STILL STALE, enqueued on cpu0
      (netd keeps executing)          ... next timer tick ...
                                      (AP) schedule() global scan sees
                                      TID 3 Ready on cpu0 and commits it
                                      (k.cpu=1, state=Running)
                                      iretq into TID 3's STALE saved frame
  ─────────────────────────────────────────────────────────────────────
  both CPUs now run netd_entry() on the same 16 KiB stack
```

`yield_current_thread()` (and `handler_yield`/`sleep_ex`/`waitpid`, which called
`make_thread_ready` directly) published a **running** thread as `Ready` before
its `rsp` had been saved at the switch-out point. On a uniprocessor the thread
was simply re-selected and the stale `rsp` was fixed on the next tick; on SMP
the global priority scan / work-stealing could dispatch it on another CPU
first, producing two execution contexts on one stack.

A second, smaller SMP defect was found while validating: `usermode.rs::
wait_for_process` decided whether to block the boot thread using the global
`Scheduler::current_tid`, which an AP had already advanced to its own last
committed thread. The BSP therefore failed to block TID 0, leaving two threads
`Running` on CPU0 (`[SCHED_WARN] TWO+ Running`) and a stale dispatchable boot
context.

### 9.2 Fix

Enforce the SMP dispatch invariant *“a `Ready` thread has a saved context and is
not executing on any CPU”* by decoupling *yield intent* from *Ready*:

| File | Change |
|------|--------|
| `scheduler/types.rs` | new `Kthread.yield_requested: bool`. |
| `scheduler/mod.rs`, `syscall/handlers.rs` | `yield_current_thread`, `sys_yield`, `sleep_ex`, `waitpid` record `yield_requested = true`; they no longer mark the running thread Ready/enqueued. |
| `arch/x64/idt.rs` | timer preemption treats `yield_requested` as a preemption request; on switch-out it saves `rsp`, re-homes `k.cpu` to the executing CPU, clears the flag and only then `make_thread_ready()` (enqueue). |
| `syscall/resched.rs` | `syscall_try_resched` saves `rsp` and consumes `yield_requested` before publishing. |
| `scheduler/schedule.rs`, `scheduler/queue.rs` | timeslice expiry and `make_thread_ready` clear `yield_requested`; timeslice expiry re-homes `k.cpu` before enqueue. |
| `usermode.rs` | use `current_tid_for_this_cpu()` instead of the global `current_tid` when blocking the boot thread. |

No scheduling policy was disabled: migration, work-stealing, AP timers and
context switches are unchanged. A `Ready` thread is still fully migratable — it
just becomes `Ready` only once its context has actually been saved.

### 9.3 Forensic evidence

QEMU `-d int` on the pre-fix build:

```text
GPF v=0d e=7800 cpl=0 IP=0008:0000000004010ea7 (timer_handler_asm iretq)
     GS base = 0x2407000 (CPU0 KPRCB)   RSP = 0x24791a8
prev. INT=0x20 on CPU1 GS base = 0x2408000  RSP = 0x24791d8
     (both stacks within 0x30 bytes → same netd kernel stack)
```

`0x4010ea7` is the `iretq`; the `CS` slot had been overwritten by CPU1's GPR
pushes because both CPUs were using netd's single kernel stack. After the fix
the same `-d int` capture reports **zero** `v=0d` and the shell is reached.

### 9.4 Post-fix state-publication audit

Every `Ready` producer was enumerated (see
`phase13-ap-timer-iretq-gpf-forensics.md` §10–§16). The `yield_requested`
contract is honoured by all yield callers, all creation/suspend paths publish
only never-executed threads, and the switch-out paths (`on_timer_tick`,
timer preempt, `syscall_try_resched`) save the live `rsp`, re-home `k.cpu` and
publish under the `SCHEDULER` mutex.

One residual window remained in the post-fix audit: a waker calling
`make_thread_ready` on a `Blocked` thread that is still executing its
`state = Blocked → syscall_try_resched` path. The block sites
(`kwait_block`, `ObWait`, alertable APC wait, `handler_read`,
`wait_for_process`) set `Blocked` without saving `k.rsp`, and the selection
paths (`schedule_with` steps 1–3, `steal_and_migrate`) test only
`state == Ready`. Another CPU could therefore dispatch the woken thread to its
stale frame — the same G2/G4 class as §9.1, triggered by a wake instead of a
yield; it was narrower and not observed in validation, and invisible to
`consistency_check` (a KTHREAD has a single `state`/`cpu`).

**Phase 13-A.3 (implemented):** candidate ownership guard. The authoritative
check is `cpu_local::kthread_current_cpu(kptr)` (pointer comparison against
`KPRCB.current_thread` per CPU, never a dereference of a possibly-stale
pointer). `schedule::candidate_owned_elsewhere(kptr, self_cpu)` defers a
`Ready` candidate owned by *another* CPU (the selecting CPU may still re-select
its own current thread, preserving the switch-out protocol). It is applied to
`schedule_with` step 1 (local queue), step 2 (steal commit), step 3 (global
scan) and to `steal_and_migrate`. A post-commit detector
(`note_dispatch_owner_check`) counts any escaped I-RUNREADY event. Because all
`KPRCB.current_thread` writes already occur under the `SCHEDULER` mutex (and
selection runs under the same mutex), the check cannot go stale before commit.
`yield_requested`, F-02, work stealing and scheduling policy are unchanged; a
guarded candidate is deferred, not dropped, and no pending-wake protocol was
introduced. **Current suite: 723/723**; SMP1/SMP2/SMP4 clean with
`GPF/PF/PANIC/SCHED_WARN/IRQ_REENTRANCY = 0` and `READY_WHILE_RUNNING /
STALE_RSP_DISPATCH / STACK_OWNERSHIP_CONFLICT = 0` (forensic report Part III §22).
A pre-existing `[SCHED_WARN] tag=timer TWO+ Running` bookkeeping warning can
appear during extended interactive shell-read stress; it reproduces on HEAD
without the guard (2/4 path-guaranteed SMP4 runs vs 0/4 guarded) and the guard
defers nothing in those runs, so it is not a Phase 13-A.3 regression.

---

## 10. Phase 13-A.1 validation matrix

| Scenario | Result |
|----------|--------|
| SMP2, `smp-ap-sched` | 722/722 tests; AP online; `[AP_EVIDENCE] cpu=1 current_tid=3 idle=0`; `[STEAL] success>0`; no GPF/panic; zero `[SCHED_WARN]`. |
| SMP4, `smp-ap-sched` | 722/722 tests; APs online and dispatching; no GPF; zero `[SCHED_WARN]`. |
| SMP1 (default) | regression-clean. |
| `neodev test` | 722/722 (default build). |

The `smp-ap-sched` feature remains opt-in so twin boots stay deterministic; the
default kernel is regression-clean and APs idle.
