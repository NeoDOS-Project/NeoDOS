# Phase 13 — SMP Scheduling Real: design for AP dispatch

**Status:** design (not implemented)
**Branch context:** `feat/phase13-ap-scheduling` (Task C + F-01/F-02 audit landed)
**Related:** `docs/investigation/smp-bring-up-report.md`,
`docs/investigation/f01-f02-adversarial-audit.md`,
`docs/architecture/source-of-truth.md` (Rules 6.1.4, 6.1.5).

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
  gdt::init_ap(cpu); load IDT      (existing)
  # NEW:
  tid = scheduler.register_ap_idle(cpu, stack_top)   # Kthread with is_idle=true,
                                                     # state=Running, entry=idle_task
  KPRCB.current_thread = idle_kthread
  KPRCB.current_pid    = 0
  KPRCB.idle           = 1
  init_apic_timer_ap()             # periodic vector 32 on this CPU
  prepare_ring3_return(idle.kernel_stack_top, tid, 0)  # TSS.RSP0 for this CPU
  AP_SCHED_ACTIVE = true           # global, after run_all()
  sti
  iretq into idle.rsp              # idle_task loop; timer now preempts it
```

Timer tick on AP (existing `timer_handler_inner`, idle branch):
`on_timer_tick` expires the idle slice → state Ready → idle branch calls
`scheduler.schedule()` → step 2 `try_work_steal()` pulls a Ready TID from the
BSP runqueue (or step 3 global scan) → commit + `sync_per_cpu_current` →
`timer_handler_asm` switches RSP and `iretq`s into the thread.

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
