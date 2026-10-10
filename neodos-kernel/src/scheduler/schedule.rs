//! Scheduler core schedule — extracted from mod.rs
use alloc::boxed::Box;
use alloc::vec::Vec;
use crate::log::LogSubsys;
use crate::scheduler::types::{Kthread, ThreadState, BOOT_TID, PRIORITY_COUNT, IDLE_TIME_SLICE, TIME_SLICES, AGING_INTERVAL_TICKS, MAX_STARVATION_TICKS};
use crate::arch::x64::cpu_local::MAX_CPUS;
use crate::scheduler::Scheduler;
use crate::scheduler::lifecycle::reap_pending_zombies;

pub(crate) static SCHEDULE_CALLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

// ── Phase 8 forensic: centralized consistency check (WARN + dump, no panic) ──
// Disabled by default so normal boot timing is unaffected; enabled after the
// boot test-suite via `sched_forensic_enable(true)`. Allocation-free.
static CHECK_LAST_TICK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static CHECK_LAST_STATE: [core::sync::atomic::AtomicU8; 64] =
    [const { core::sync::atomic::AtomicU8::new(0xFF) }; 64];
static CHECK_WARN_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static SCHED_FORENSIC: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static SCHED_FORENSIC_VERBOSE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Enable/disable the Phase 8 forensic consistency checker.
pub fn sched_forensic_enable(v: bool) {
    SCHED_FORENSIC.store(v, core::sync::atomic::Ordering::Relaxed);
}

/// Enable/disable verbose scheduler state-transition traces (TID2/TID5).
pub fn sched_forensic_verbose_enable(v: bool) {
    SCHED_FORENSIC_VERBOSE.store(v, core::sync::atomic::Ordering::Relaxed);
}

#[inline]
pub fn sched_forensic_verbose() -> bool {
    SCHED_FORENSIC_VERBOSE.load(core::sync::atomic::Ordering::Relaxed)
}

fn state_name(s: u8) -> &'static str {
    match s { 0 => "READY", 1 => "RUNNING", 2 => "BLOCKED", 3 => "SUSP", 4 => "TERM", _ => "?" }
}

/// Phase 9: is `k`'s saved context dispatchable back to Ring 3?
/// Used so `schedule_with(require_ring3=true)` never commits a candidate whose
/// frame cannot be returned through a Ring-3 interrupt/syscall frame.
#[inline]
pub(crate) fn frame_is_ring3(k: &Kthread) -> bool {
    if k.rsp < 0x1000 { return false; }
    let cs = unsafe { *((k.rsp + 128) as *const u64) };
    (cs & 3) == 3
}

/// #338 invariant guard: a user thread's saved dispatch frame must be Ring 3.
///
/// A user thread interrupted inside a syscall runs on its Ring-0 kernel stack;
/// the timer would otherwise save that Ring-0 frame (`cs == 0x08`) as the
/// thread's authoritative dispatch frame and mark it `Ready`. It would then be
/// permanently undispatchable, because `schedule_with(require_ring3=true)`
/// rejects every non-Ring-3 frame. Callers use this predicate to distinguish
/// "the live map/rsp was updated for accounting" from "the context published
/// for Ring-3 dispatch is valid".
///
/// `is_kernel_thread` must be `true` for idle threads and for kernel threads
/// created without a user image (e.g. `netd` via `spawn_kthread_named`). Those
/// run in Ring 0 **by design** and are only ever dispatched through
/// `schedule_with(require_ring3=false)` (Ring-0/idle timer preemption), so
/// their Ring-0 frame is legitimate and must not be rejected. Using `pid == 0`
/// as the exemption was wrong: `spawn_kthread_named` assigns kernel threads a
/// real pid, so `netd` (pid != 0) would have been stranded.
#[inline]
pub(crate) fn thread_dispatch_frame_is_ring3(k: &Kthread, is_kernel_thread: bool) -> bool {
    is_kernel_thread || frame_is_ring3(k)
}

/// #474: gate for the timer's Ring-0 preemption branch.
///
/// A context observed in Ring 0 (`cs & 3 != 3`) may only be published Ready
/// with its live `rsp` when that `rsp` is a valid dispatch frame for it:
///
/// - genuine kernel/idle threads run in Ring 0 by design and are dispatched
///   through `schedule(require_ring3=false)`, so their Ring-0 frame is valid;
/// - a *user* thread running in Ring 0 is inside a syscall. Its live `rsp` is
///   a transient kernel call frame, not a dispatch frame. Publishing it Ready
///   would make a later `mov rsp,next_rsp; pop 15; iretq` read RIP/CS from
///   arbitrary stack contents (wild RIP → INVALID_OPCODE). It must be deferred
///   to its syscall-return path, which saves the real Ring-3 frame.
///
/// `interrupted_cs` is the CS of the frame the timer interrupted.
#[inline]
pub(crate) fn ring0_publish_is_dispatchable(interrupted_cs: u64, is_kernel_thread: bool) -> bool {
    (interrupted_cs & 3) == 3 || is_kernel_thread
}

// ── Phase 13-A.3: Ready publication ownership guard ──────────────────────
// I-RUNREADY: a `Ready` KTHREAD must not be the live `KPRCB.current_thread` of
// any *other* CPU. A wake landing in the `Blocked → switch-out` window publishes
// a still-executing thread `Ready` with a stale `rsp`; without this guard a
// different CPU could dispatch it and run the same KTHREAD on two CPUs sharing
// one kernel stack (the Phase 13 `iretq` #GP class). The selecting CPU may still
// re-select its own current thread (the legitimate switch-out path).
pub(crate) static SCHED_CANDIDATE_REJECTED: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);
pub(crate) static READY_WHILE_RUNNING: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);
pub(crate) static STALE_RSP_DISPATCH: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);
pub(crate) static STACK_OWNERSHIP_CONFLICT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

// ── #355: bounded anti-starvation hand-off for Ring-0 kernel threads ─────
// On a CPU whose current context is Ring-3, `schedule_with(true)` only commits
// Ring-3 frames, so a Ready Ring-0 kernel thread (netd/netpump/boot) can be
// starved forever. When one is starved past the threshold, the CPU hands off to
// its own idle thread for one turn; the following selection runs from a Ring-0
// context (`require_ring3 = false`) and can dispatch the kernel thread. The
// per-CPU flag lets the callers (`resched`, timer user-preempt) accept the idle
// dispatch instead of reverting a non-Ring-3 `next`.
pub(crate) static KERNEL_HANDOFF: [core::sync::atomic::AtomicBool; MAX_CPUS] =
    [const { core::sync::atomic::AtomicBool::new(false) }; MAX_CPUS];

/// True when `kptr` is `KPRCB.current_thread` of a CPU other than `self_cpu`.
/// The rejection is counted; the serial note is forensic-verbose only.
#[inline]
pub(crate) fn candidate_owned_elsewhere(kptr: *const Kthread, self_cpu: u32) -> bool {
    match crate::arch::x64::cpu_local::kthread_current_cpu(kptr) {
        Some(owner) if owner != self_cpu => {
            SCHED_CANDIDATE_REJECTED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if sched_forensic_verbose() {
                crate::serial_println!(
                    "[READY_GUARD] tid={} state=Ready current_cpu={} self_cpu={} action=defer",
                    unsafe { (*kptr).tid }, owner, self_cpu);
            }
            true
        }
        _ => false,
    }
}

/// Decide whether a non-Ring-3 `next` selected by the syscall-return or timer
/// user-preempt path may be dispatched.
///
/// `handoff` is the #355 anti-starvation signal (the target is this CPU's idle).
/// Otherwise only a genuine, non-idle kernel thread is accepted: it runs in
/// Ring 0 by design and its saved context is a valid Ring-0 dispatch frame, so
/// it may be resumed directly from a Ring-3 context. A *user* thread interrupted
/// inside a syscall is never accepted here (`is_kernel == false`).
///
/// Shared by both return paths so their acceptance policy cannot diverge.
#[inline]
pub(crate) fn accept_non_ring3_dispatch(handoff: bool, next: *const Kthread) -> bool {
    if handoff {
        return true;
    }
    if next.is_null() {
        return false;
    }
    unsafe { (*next).is_kernel && !(*next).is_idle }
}

/// Fallback Ring-3 selection used by the syscall-return path when the current
/// thread is `Blocked`/`Terminated` (i.e. the normal scheduler picked a
/// non-Ring-3 candidate). Returns the index into `self.kthreads` of the
/// highest-priority `Ready` Ring-3 thread **not owned by another CPU**, or
/// `None`. It deliberately enforces the same I-RUNREADY guard
/// ([`candidate_owned_elsewhere`], #293/#346) as every other dispatch site
/// (#354); the caller commits the state.
impl Scheduler {
    pub fn select_fallback_ring3(&self, self_cpu: u32) -> Option<usize> {
        for prio in 0..PRIORITY_COUNT {
            for (idx, k_opt) in self.kthreads.iter().enumerate() {
                if let Some(k) = k_opt {
                    if k.state == ThreadState::Ready
                        && k.priority == prio
                        && k.rsp != 0
                        && !candidate_owned_elsewhere(&**k as *const Kthread, self_cpu)
                    {
                        let cs_val = unsafe { *((k.rsp + 15 * 8 + 8) as *const u64) };
                        if (cs_val & 3) == 3 {
                            return Some(idx);
                        }
                    }
                }
            }
        }
        None
    }

    /// #355: is a Ring-0 kernel thread on `cpu` Ready and starved past the
    /// threshold? Such a thread cannot be committed from a Ring-3 selection.
    pub(crate) fn kernel_thread_starved(&self, cpu: u32) -> bool {
        self.kthreads.iter().flatten().any(|k| {
            !k.is_idle
                && k.state == ThreadState::Ready
                && k.cpu == cpu
                && k.ticks_since_scheduled >= MAX_STARVATION_TICKS
                && self.is_kernel_thread(k)
        })
    }

    /// Minimum priority among `Ready`, non-idle threads. `PRIORITY_COUNT` when
    /// none. The per-CPU run queue is a FIFO, not priority-ordered, so the fast
    /// path must consult this before committing its popped candidate: committing
    /// a lower-priority candidate while a higher-priority thread is `Ready`
    /// bypasses the priority scan and starves that thread (#382).
    pub(crate) fn highest_ready_priority(&self) -> u8 {
        let mut p = PRIORITY_COUNT;
        for cpu in 0..MAX_CPUS {
            let bitmap = crate::arch::x64::cpu_local::read_active_bitmap(cpu);
            if bitmap != 0 {
                let lowest_bit = bitmap.trailing_zeros() as u8;
                if lowest_bit < p {
                    p = lowest_bit;
                }
            }
        }
        p
    }

    /// #355: consume a pending idle hand-off for `cpu`.
    pub(crate) fn take_kernel_handoff(cpu: u32) -> bool {
        let idx = cpu as usize;
        if idx < MAX_CPUS {
            KERNEL_HANDOFF[idx].swap(false, core::sync::atomic::Ordering::Relaxed)
        } else {
            false
        }
    }

    fn set_kernel_handoff(cpu: u32) {
        let idx = cpu as usize;
        if idx < MAX_CPUS {
            KERNEL_HANDOFF[idx].store(true, core::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Post-commit detector: increment the violation counters if, immediately after
/// a candidate was committed `Running`, it is still owned by another CPU. The
/// guard should make this unreachable; a non-zero count is direct evidence of a
/// check→commit race (I-RUNREADY escaped).
#[inline]
pub(crate) fn note_dispatch_owner_check(kptr: *const Kthread, self_cpu: u32) {
    if let Some(owner) = crate::arch::x64::cpu_local::kthread_current_cpu(kptr) {
        if owner != self_cpu {
            READY_WHILE_RUNNING.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            STALE_RSP_DISPATCH.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            STACK_OWNERSHIP_CONFLICT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if sched_forensic_verbose() {
                crate::serial_println!(
                    "[READY_GUARD] tid={} current_cpu={} self_cpu={} action=dispatch_conflict",
                    unsafe { (*kptr).tid }, owner, self_cpu);
            }
        }
    }
}

/// Phase 13-A.3 forensic counters (rejected, ready_while_running,
/// stale_rsp_dispatch, stack_ownership_conflict).
pub fn sched_ready_guard_stats() -> (u64, u64, u64, u64) {
    use core::sync::atomic::Ordering::Relaxed;
    (
        SCHED_CANDIDATE_REJECTED.load(Relaxed),
        READY_WHILE_RUNNING.load(Relaxed),
        STALE_RSP_DISPATCH.load(Relaxed),
        STACK_OWNERSHIP_CONFLICT.load(Relaxed),
    )
}

fn rq_len(cpu: usize) -> usize {
    let kprcb = unsafe { crate::arch::x64::cpu_local::KPRCB_PAGES[cpu] };
    if kprcb == 0 { return 0; }
    match crate::arch::x64::cpu_local::RUNQUEUE_LOCKS[cpu].try_lock() {
        Some(_g) => {
            let rq = unsafe {
                &*((kprcb + crate::arch::x64::cpu_local::OFFSET_RUN_QUEUE as u64)
                    as *const crate::arch::x64::cpu_local::CpuRunQueue)
            };
            rq.count as usize
        }
        None => usize::MAX,
    }
}

impl Scheduler {
    /// Phase 8: detect the first invariant violation that leaves a thread
    /// `Running` without being the dispatched context. WARN + dump (never panic,
    /// never mutate). Must be called with the scheduler lock held.
    pub fn consistency_check(&self, tag: &str) {
        if !SCHED_FORENSIC.load(core::sync::atomic::Ordering::Relaxed) { return; }
        let now = crate::hal::get_ticks();
        let last = CHECK_LAST_TICK.load(core::sync::atomic::Ordering::Relaxed);
        if now == last { return; }
        CHECK_LAST_TICK.store(now, core::sync::atomic::Ordering::Relaxed);

        let mut running_tids = [0u32; 8];
        let mut running_cpus = [0u32; 8];
        let mut nrunning = 0usize;
        for k in self.kthreads.iter().flatten() {
            let idx = k.tid as usize;
            if idx < 64 {
                let cur = k.state.to_u8();
                let prev = CHECK_LAST_STATE[idx].swap(cur, core::sync::atomic::Ordering::Relaxed);
                if prev != 0xFF && prev != cur && (k.tid == 2 || k.tid == 5)
                    && sched_forensic_verbose()
                {
                    crate::serial_println!(
                        "[SCHED_STATE] tid={} cpu={} {}->{} tag={} sched.current={} rq0={} rq1={} kprcb_tid={:?}",
                        k.tid, k.cpu, state_name(prev), state_name(cur), tag, self.current_tid,
                        rq_len(0), rq_len(1),
                        crate::arch::x64::cpu_local::try_per_cpu_tid());
                }
            }
            if k.state == ThreadState::Running && nrunning < 8 {
                running_tids[nrunning] = k.tid;
                running_cpus[nrunning] = k.cpu;
                nrunning += 1;
            }
        }

        let mut dup_cpu: Option<u32> = None;
        for i in 0..nrunning {
            for j in (i + 1)..nrunning {
                if running_cpus[i] == running_cpus[j] { dup_cpu = Some(running_cpus[i]); }
            }
        }
        if let Some(cpu) = dup_cpu {
            let c = CHECK_WARN_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            // Phase 293-B: record the DOUBLE_RUNNING with the two offenders.
            {
                let mut a: Option<&crate::scheduler::Kthread> = None;
                let mut b: Option<&crate::scheduler::Kthread> = None;
                for k in self.kthreads.iter().flatten() {
                    if k.state == ThreadState::Running && k.cpu == cpu {
                        if a.is_none() { a = Some(k); } else if b.is_none() { b = Some(k); }
                    }
                }
                if let (Some(a), Some(b)) = (a, b) {
                    let kprcb_tid = crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(0);
                    crate::scheduler::diag::dr_ev(cpu, a, b, self.current_tid, kprcb_tid);
                }
            }
            if c < 12 {
                crate::serial_println!(
                    "[SCHED_WARN] tag={} TWO+ Running on cpu={} tids={:?}/{:?} sched.current={} kprcb_tid={:?}",
                    tag, cpu, running_tids, running_cpus, self.current_tid,
                    crate::arch::x64::cpu_local::try_per_cpu_tid());
                for k in self.kthreads.iter().flatten() {
                    crate::serial_println!(
                        "[SCHED_WARN]   tid={} pid={} name={} state={} cpu={} wait={:?}",
                        k.tid, k.pid, k.name(), state_name(k.state.to_u8()), k.cpu, k.waiting_for);
                }
            }
            // #345: dump the transition ring once, at the first occurrence.
            crate::scheduler::diag::run_dump_first();
        }

        // Phase 293-B: detect the same Kthread being KPRCB.current_thread of
        // more than one CPU (two CPUs on one kernel stack). Observe only.
        crate::scheduler::diag::stack_owner_scan();
    }

    /// Validate run queue invariants.
    /// Invariant: for each thread,
    ///   Ready    => exactly one entry in its CPU's run queue
    ///   !Ready   => zero entries in its CPU's run queue
    /// Returns Ok(count) on success, Err(message) on violation.
    pub fn validate_runqueue_invariants(&self) -> Result<usize, &'static str> {
        // F-01: prefer per-CPU tid only for global scheduler (KPRCB thread in self)
        let effective_tid = if self.kprcb_thread_in_self() {
            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
        } else {
            self.current_tid
        };
        let current = self.find_kthread(effective_tid)
            .ok_or("current_tid does not identify a thread")?;
        if current.state != ThreadState::Running {
            return Err("current_tid does not identify a Running thread");
        }
        // Only enforce cpu match when KPRCB is initialized and this is global
        if self.kprcb_thread_in_self() {
            let current_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
            if current.cpu != current_cpu {
                return Err("current thread belongs to another CPU");
            }
        }

        let mut thread_tids = Vec::new();
        let mut running_cpus = Vec::new();
        for k in self.kthreads.iter().flatten() {
            if thread_tids.contains(&k.tid) {
                return Err("duplicate TID in scheduler thread table");
            }
            thread_tids.push(k.tid);
            if k.state == ThreadState::Running {
                if running_cpus.contains(&k.cpu) {
                    return Err("more than one Running thread on a CPU");
                }
                running_cpus.push(k.cpu);
            }
        }

        let mut queue_tids = Vec::new();
        let mut total_entries = 0usize;
        for cpu in 0..crate::arch::x64::cpu_local::MAX_CPUS {
            // SMP-safe: lock each queue while reading
            let queue_entries = crate::arch::x64::cpu_local::with_runqueue(cpu, |rq| {
                rq.entries_vec()
            });

            for tid in queue_entries {
                if queue_tids.contains(&tid) {
                    return Err("duplicate TID in run queue");
                }
                let k = self.find_kthread(tid)
                    .ok_or("orphan TID in run queue")?;
                if tid == BOOT_TID || k.is_idle {
                    return Err("boot or idle thread found in run queue");
                }
                if k.cpu as usize != cpu {
                    return Err("run queue entry belongs to another CPU");
                }
                if k.state != ThreadState::Ready {
                    return Err("non-Ready thread found in run queue");
                }
                queue_tids.push(tid);
                total_entries += 1;
            }
        }

        for k in self.kthreads.iter().flatten() {
            let count = queue_tids.iter().filter(|&&tid| tid == k.tid).count();
            if k.is_idle || k.tid == BOOT_TID {
                if count != 0 {
                    return Err("special thread found in run queue");
                }
                continue;
            }
            if k.state == ThreadState::Ready && count != 1 {
                return Err("Ready thread not in run queue exactly once");
            }
            if k.state != ThreadState::Ready && count != 0 {
                return Err("Non-Ready thread found in run queue");
            }
        }
        Ok(total_entries)
    }


    /// Find a thread slot by TID, returning a raw pointer to the Kthread Box allocation (stable).
    fn find_kthread_ptr(&self, tid: u32) -> *mut Kthread {
        for th in self.kthreads.iter() {
            if let Some(k) = th {
                if k.tid == tid {
                    return &**k as *const Kthread as *mut Kthread;
                }
            }
        }
        core::ptr::null_mut()
    }

    /// #346: undo a committed non-dispatchable candidate and keep `tid` Running.
    ///
    /// `schedule_with(require_ring3=true)` is allowed to fall back to the idle
    /// Kthread (a Ring-0 frame). The syscall-return path then rejects that
    /// candidate and resumes the original Ring-3 thread. This helper restores
    /// the scheduler-visible identity of that thread: it removes `tid` from its
    /// run queue, makes it the scheduler's current thread and marks it
    /// `Running`. Returns `(ptr, pid, kernel_stack_top)` so the caller can also
    /// restore the per-CPU `KPRCB.current_thread/current_pid/idle` and
    /// `TSS.RSP0`. Omitting the `KPRCB` restore is what let the timer later
    /// treat the idle Kthread as the running context and store a Ring-3 frame
    /// into its `rsp` (#346).
    pub fn resume_current_after_rejected_dispatch(
        &mut self,
        tid: u32,
    ) -> Option<(*mut Kthread, u32, u64)> {
        let (pid, ks_top) = {
            let k = self.find_kthread(tid)?;
            (k.pid, k.kernel_stack_top)
        };
        let ptr = self.find_kthread_ptr(tid);
        self.current_tid = tid;
        if let Some(current) = self.find_kthread_mut(tid) {
            Self::remove_from_run_queue(current);
            crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_RESUME_REJECT, current);
            current.state = ThreadState::Running;
        }
        Some((ptr, pid, ks_top))
    }

    /// Find the idle Kthread for a specific CPU (0 = BSP idle / IDLE_TID).
    ///
    /// #346 / #293 stack-ownership invariant: an idle Kthread may only be
    /// returned for the CPU that owns it (`k.cpu == cpu`). The former global
    /// fallback to `IDLE_TID` returned the BSP idle for any CPU that had not
    /// registered its own idle yet; on SMP that let two CPUs adopt one
    /// `Kthread` and execute on a single kernel stack (STACK_OWNER_MISMATCH ->
    /// frame/canary corruption). Returning null here lets the caller fail
    /// closed instead of silently crossing CPUs.
    pub fn find_idle_ptr(&self, cpu: u32) -> *mut Kthread {
        for th in self.kthreads.iter() {
            if let Some(k) = th {
                if k.is_idle && k.cpu == cpu {
                    return &**k as *const Kthread as *mut Kthread;
                }
            }
        }
        core::ptr::null_mut()
    }

    /// Register the per-CPU idle thread for an AP. Called by the AP itself once
    /// it has adopted the final address space. Returns `(idle_tid, idle_ptr)`.
    ///
    /// The idle Kthread owns no Box stack: `kernel_stack_top` is the
    /// pre-allocated AP stack and `rsp` is captured by the first timer tick.
    /// Must be called with interrupts disabled.
    pub fn register_ap_idle(&mut self, cpu: u32, stack_top: u64) -> Option<(u32, *mut Kthread)> {
        // Need room for the fabricated frame 4 KB below the top.
        if stack_top < 4096 {
            return None;
        }
        if let Some(k) = self.kthreads.iter().flatten().find(|k| k.is_idle && k.cpu == cpu) {
            return Some((k.tid, &**k as *const Kthread as *mut Kthread));
        }
        let th_slot = self.alloc_kthread_slot()?;
        let tid = self.next_tid;
        self.next_tid += 1;
        // Fabricate a Ring0 frame 4 KB below the AP stack top (ap_entry's live
        // frame sits near the top). The AP iretqs into `idle_task` exactly like
        // CPU0's idle, so every later timer switch has a valid saved frame.
        let frame_top = stack_top.saturating_sub(4096);
        let entry = crate::scheduler::stack::idle_task as *const () as u64;
        let mut idle = Kthread::new_idle(tid, 0, entry, frame_top);
        // #348: the AP idle owns the region below its fabricated frame. Its
        // canary belongs at the bottom of that region, so record the exact span
        // and initialize the canary there (previously the canary was never
        // written for AP idle stacks).
        let ap_stack_base = stack_top.saturating_sub(
            crate::arch::x64::smp::AP_STACK_SIZE as u64,
        );
        let ap_idle_span = frame_top.saturating_sub(ap_stack_base).max(1) as usize;
        idle.kernel_stack_size = ap_idle_span;
        unsafe {
            crate::scheduler::stack::init_raw_stack_canary(
                ap_stack_base as *mut u8,
                ap_idle_span,
            );
        }
        idle.cpu = cpu;
        crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_AP_IDLE, &idle);
        idle.state = ThreadState::Running;
        {
            // Phase 14-A: per-CPU idle name ("idle/<cpu>"), bounded.
            let mut n = crate::scheduler::types::KernelName::from_str("idle/");
            n.push_u32(cpu);
            idle.name = n;
        }
        self.put_kthread(th_slot, Box::new(idle));
        let ptr = &**self.kthreads[th_slot].as_ref()? as *const Kthread as *mut Kthread;
        Some((tid, ptr))
    }


    /// Dispatch this CPU's idle thread (Ring-0) for one turn. Returns the idle
    /// Kthread pointer, or null when no idle for `this_cpu` is available.
    /// Used by the #355 anti-starvation hand-off and by the final idle fallback.
    /// Must be called under the scheduler lock.
    fn dispatch_idle(&mut self, this_cpu: u32) -> *mut Kthread {
        let ptr = self.find_idle_ptr(this_cpu);
        if ptr.is_null() {
            return core::ptr::null_mut();
        }
        unsafe {
            let idle = &mut *ptr;
            if idle.state == ThreadState::Terminated {
                return core::ptr::null_mut();
            }
            let idle_tid = idle.tid;
            Scheduler::remove_from_run_queue(idle);
            let prev = if self.kprcb_thread_in_self() {
                crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
            } else { self.current_tid };
            let prev_state = self.find_kthread(prev).map(|t| t.state.to_u8()).unwrap_or(255);
            self.current_tid = idle_tid;
            if self.kprcb_thread_in_self() {
                crate::arch::x64::cpu_local::sync_per_cpu_current(ptr, (*ptr).pid);
            }
            crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_DISPATCH_IDLE, idle);
            idle.state = ThreadState::Running;
            idle.time_slice_remaining = IDLE_TIME_SLICE;
            kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=idle_fallback",
                prev, idle_tid);
            crate::trace_cswitch!(prev as u64, idle_tid as u64);
            crate::trace_sched_switch!(prev, prev_state, idle_tid, idle.state.to_u8());
            let prev_pid = self.find_kthread(prev).map(|t| t.pid).unwrap_or(0);
            reap_pending_zombies(self, prev_pid);
        }
        ptr
    }

    /// Schedule the next thread.  Tries per-CPU run queue first, falls back
    /// to global priority scan.  Returns a `*mut Kthread` for RSP/stack access.
    /// Phase 9: select+commit with an explicit dispatchability contract.
    /// `require_ring3=true` means the caller will iretq to the returned thread's
    /// saved frame from a Ring-3 context, so only candidates whose frame is
    /// Ring-3 may be committed. Invalid candidates are returned to the runqueue
    /// WITHOUT touching current_tid/KPRCB/state (SELECT -> VALIDATE -> COMMIT).
    /// `schedule()` keeps the historical behavior (require_ring3=false).
    pub fn schedule(&mut self) -> *mut Kthread {
        self.schedule_with(false)
    }

    pub fn schedule_with(&mut self, require_ring3: bool) -> *mut Kthread {
        self.schedule_with_handoff(require_ring3, false)
    }

    /// Like [`schedule_with`], but `allow_handoff` additionally permits the
    /// #355 anti-starvation idle hand-off. Only callers that correctly accept a
    /// non-Ring-3 `next` (syscall return, timer user-preempt) pass `true`; the
    /// exception path passes `false`.
    pub fn schedule_with_handoff(&mut self, require_ring3: bool, allow_handoff: bool) -> *mut Kthread {
        ktrace!(LogSubsys::Sched, "schedule entry");
        SCHEDULE_CALLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // Count every schedule decision, not just global-scan fallbacks.
        self.schedule_count += 1;

        // Phase 13-A.3: CPU performing this selection. A `Ready` candidate that
        // is still the live `KPRCB.current_thread` of a *different* CPU is
        // deferred (I-RUNREADY); see `candidate_owned_elsewhere`.
        let self_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };

        // #355: anti-starvation hand-off, now a fallback. Ring-0 kernel threads
        // are normally selectable from a Ring-3 context (see the frame gate in
        // steps 1-3). This branch only fires if such a thread is still Ready
        // past the starvation threshold (e.g. it is being outranked by an even
        // higher-priority thread); it runs this CPU's idle for one turn so the
        // next Ring-0 selection can dispatch the kernel thread.
        if require_ring3 && allow_handoff && self.kernel_thread_starved(self_cpu) {
            let ptr = self.dispatch_idle(self_cpu);
            if !ptr.is_null() {
                Self::set_kernel_handoff(self_cpu);
                return ptr;
            }
        }

        // 1. Try per-CPU local run queue (fast path)
        if let Some(tid) = Self::try_dequeue_local() {
            let ptr = self.find_kthread_ptr(tid);
            if !ptr.is_null() {
                unsafe {
                    let k = &mut *ptr;
                    if k.state == ThreadState::Ready
                        && (!require_ring3 || k.is_kernel || frame_is_ring3(k))
                        && !candidate_owned_elsewhere(ptr, self_cpu)
                        // #382: never commit a lower-priority candidate while a
                        // higher-priority thread is Ready — fall through to the
                        // priority scan instead of starving it.
                        && k.priority <= self.highest_ready_priority()
                    {
                        let prev = if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
                        } else { self.current_tid };
                        let prev_state = self.find_kthread(prev).map(|t| t.state.to_u8()).unwrap_or(255);
                        self.current_tid = tid;
                        // F-01: sync per-CPU KPRCB only for global scheduler (tests use local schedulers)
                        if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::sync_per_cpu_current(ptr, k.pid);
                        }
                        crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_FAST, k);
                        k.state = ThreadState::Running;
                        note_dispatch_owner_check(ptr, self_cpu);
                        Self::account_dispatch(k);
                        crate::scheduler::diag::ev(
                            crate::scheduler::diag::EV_DISPATCH_RQ, self_cpu, tid, k.rsp,
                            prev as u64);
                        if (tid == 5 || prev == 5) && sched_forensic_verbose() {
                            crate::serial_println!("[T5_SCHED] step=1 prev={} new={} current={} rq0={} kprcb={:?}",
                                prev, tid, self.current_tid, rq_len(0),
                                crate::arch::x64::cpu_local::try_per_cpu_tid());
                        }
                        kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=runqueue",
                            prev, tid);
                        crate::trace_cswitch!(prev as u64, tid as u64);
                        crate::trace_sched_switch!(prev, prev_state, tid, k.state.to_u8());
                        // F-02-A: reap must exclude the context we are switching AWAY
                        // from (its kernel stack is still under us), not the new one.
                        // The new pid is Running, so it can never be in the zombie queue.
                        let prev_pid = self.find_kthread(prev).map(|t| t.pid).unwrap_or(0);
                        reap_pending_zombies(self, prev_pid);
                        return ptr;
                    } else if k.state == ThreadState::Ready {
                        // Phase 9: valid Ready candidate but frame not dispatchable
                        // from this context. Return it to the runqueue and do NOT
                        // commit any state (no current_tid/KPRCB/state change).
                        Scheduler::enqueue_to_cpu_run_queue(k);
                    }
                }
            }
        }

        // 2. Try work stealing from another CPU
        if let Some(tid) = self.try_work_steal() {
            let ptr = self.find_kthread_ptr(tid);
            if !ptr.is_null() {
                unsafe {
                    let k = &mut *ptr;
                    if k.state == ThreadState::Ready
                        && (!require_ring3 || k.is_kernel || frame_is_ring3(k))
                        && !candidate_owned_elsewhere(ptr, self_cpu)
                        // #382: never commit a lower-priority candidate while a
                        // higher-priority thread is Ready — fall through to the
                        // priority scan instead of starving it.
                        && k.priority <= self.highest_ready_priority()
                    {
                        let prev = if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
                        } else { self.current_tid };
                        let prev_state = self.find_kthread(prev).map(|t| t.state.to_u8()).unwrap_or(255);
                        self.current_tid = tid;
                        if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::sync_per_cpu_current(ptr, k.pid);
                        }
                        crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_STEAL, k);
                        k.state = ThreadState::Running;
                        note_dispatch_owner_check(ptr, self_cpu);
                        Self::account_dispatch(k);
                        crate::scheduler::diag::ev(
                            crate::scheduler::diag::EV_DISPATCH_STEAL, self_cpu, tid, k.rsp,
                            prev as u64);
                        kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=steal",
                            prev, tid);
                        crate::trace_cswitch!(prev as u64, tid as u64);
                        crate::trace_sched_switch!(prev, prev_state, tid, k.state.to_u8());
                        // F-02-A: exclude the previous context's pid (stack in use).
                        let prev_pid = self.find_kthread(prev).map(|t| t.pid).unwrap_or(0);
                        reap_pending_zombies(self, prev_pid);
                        return ptr;
                    } else if k.state == ThreadState::Ready {
                        // Phase 9: not dispatchable here; return to its CPU queue.
                        Scheduler::enqueue_to_cpu_run_queue(k);
                    }
                }
            }
        }

        // 3. Fallback: global priority scan (existing algorithm)
        let effective_tid = if self.kprcb_thread_in_self() {
            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
        } else { self.current_tid };
        let start = (effective_tid + 1) % self.next_tid.max(1);

        let mut picked_ptr: *mut Kthread = core::ptr::null_mut();
        let mut picked_pid: u32 = 0;
        let mut picked_tid: u32 = 0;
        let mut picked_prio: u8 = 0;
        let mut picked_prev: u32 = 0;
        let mut picked_prev_state: u8 = 0;
        // F-01: compute prev outside the mutable iterator to avoid borrow conflict
        let scan_prev = if self.kprcb_thread_in_self() {
            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
        } else { self.current_tid };
        // Cached outside the mutable kthread scan (borrow checker) and used to
        // re-home a global-scan candidate onto this CPU.
        let is_global_sched = self.kprcb_thread_in_self();
        let scan_cpu = self_cpu;
        'scan: for priority in 0..PRIORITY_COUNT {
            for offset in 0..self.next_tid {
                let check_tid = (start + offset) % self.next_tid.max(1);
                for k in self.kthreads.iter_mut().flatten() {
                    if k.tid == check_tid && k.state == ThreadState::Ready && k.priority == priority
                        && (!require_ring3 || k.is_kernel || frame_is_ring3(k))
                        // Never pick an idle thread here: idles are CPU-bound and
                        // selected by the per-CPU idle fallback.
                        && !k.is_idle
                        // Keep the boot thread pinned to CPU0; migrating it would
                        // strand the BSP boot flow on an AP.
                        && !(k.tid == BOOT_TID && scan_cpu != 0)
                        // Phase 13-A.3 (I-RUNREADY): do not commit a candidate a
                        // different CPU still owns as its live current thread.
                        && !candidate_owned_elsewhere(&**k as *const Kthread, scan_cpu)
                    {
                        // P0-3 FIX: Remove from runqueue BEFORE setting state to Running.
                        Scheduler::remove_from_run_queue(&**k);
                        let prev = scan_prev;
                        let prev_state = k.state.to_u8();
                        self.current_tid = check_tid;
                        // Phase 13: a global-scan candidate may have been migrated
                        // to another CPU's queue (Kthread.cpu) before it became
                        // Ready here. It now runs on THIS CPU, so its affinity
                        // must follow, otherwise two threads can appear Running
                        // on the old CPU. Only for the global scheduler so local
                        // test schedulers keep their synthetic cpu values.
                        if is_global_sched {
                            let old_cpu = k.cpu;
                            crate::scheduler::diag::kcpu_ev(
                                crate::scheduler::diag::SITE_KCPU_SCAN, k, scan_cpu);
                            k.cpu = scan_cpu;
                            if old_cpu != scan_cpu && sched_forensic_verbose() {
                                crate::serial_println!(
                                    "[KCPU] scan tid={} old_cpu={} new_cpu={} prev={}",
                                    check_tid, old_cpu, scan_cpu, scan_prev);
                            }
                        }
                        crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_SCAN, &**k);
                        k.state = ThreadState::Running;
                        note_dispatch_owner_check(&**k as *const Kthread, scan_cpu);
                        Self::account_dispatch(k);
                        crate::scheduler::diag::ev(
                            crate::scheduler::diag::EV_DISPATCH_SCAN, scan_cpu, check_tid, k.rsp,
                            prev as u64);
                        picked_ptr = &mut **k as *mut Kthread;
                        picked_pid = k.pid;
                        picked_tid = k.tid;
                        picked_prio = priority;
                        picked_prev = prev;
                        picked_prev_state = prev_state;
                        break 'scan;
                    }
                }
            }
        }
        if !picked_ptr.is_null() {
            if (picked_tid == 5 || picked_prev == 5) && sched_forensic_verbose() {
                crate::serial_println!("[T5_SCHED] step=3 prev={} new={} current={} rq0={} kprcb={:?}",
                    picked_prev, picked_tid, self.current_tid, rq_len(0),
                    crate::arch::x64::cpu_local::try_per_cpu_tid());
            }
            kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=priority_scan prio={}",
                picked_prev, picked_tid, picked_prio);
            crate::trace_cswitch!(picked_prev as u64, picked_tid as u64);
            // Need to get state again for trace (already Running)
            let new_state = unsafe { (*picked_ptr).state.to_u8() };
            crate::trace_sched_switch!(picked_prev, picked_prev_state, picked_tid, new_state);
            if self.kprcb_thread_in_self() {
                unsafe { crate::arch::x64::cpu_local::sync_per_cpu_current(picked_ptr, picked_pid); }
            }
            // F-02-A: exclude the pid being switched away from (stack still in use).
            let prev_pid = self.find_kthread(picked_prev).map(|t| t.pid).unwrap_or(0);
            reap_pending_zombies(self, prev_pid);
            return picked_ptr;
        }

        // Fallback to this CPU's idle thread (see `dispatch_idle`).
        // By design the idle thread is never enqueued; it runs only when no
        // other thread is ready (or for the #355 hand-off).
        {
            if !self.has_non_idle_threads() {
                kdebug!(LogSubsys::Sched, "[SCHED] idle_fallback: has_non_idle_threads=false (only idle or Suspended threads)");
            }
            let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
            let ptr = self.dispatch_idle(this_cpu);
            if !ptr.is_null() {
                return ptr;
            }
        }
        panic!("No ready threads and idle is unavailable");
    }

    // ── Timer tick ──

    /// Phase 15-A.1: account a thread that is about to be committed `Running`.
    ///
    /// Called at every successful dispatch (run-queue fast path, work steal and
    /// global scan). Terminated threads are skipped — a dead thread cannot
    /// execute. Idle threads are not armed here (they are dispatched by the
    /// idle-fallback path); their execution is never attributed to a process.
    /// Must be called with the scheduler lock held. No `&self` so it can be used
    /// while a mutable borrow of a Kthread from `self.kthreads` is live.
    #[inline]
    pub(crate) fn account_dispatch(k: &mut Kthread) {
        // A dispatched thread is, by definition, no longer starved: reset the
        // starvation counter and restore the base priority. Aging then only
        // boosts a thread while it remains Ready-but-unrun; the boost is
        // temporary and cannot invert fairness permanently.
        k.ticks_since_scheduled = 0;
        if k.priority < k.base_priority {
            k.priority = k.base_priority;
        }
        if k.is_idle || k.state == ThreadState::Terminated {
            k.cpu_time_base = Kthread::CPU_TIME_UNSET;
            return;
        }
        // Grant a fresh timeslice at dispatch. The timer preemption paths
        // already did this; doing it here makes the syscall-return dispatch
        // path consistent, so an accepted thread can never be committed with
        // `time_slice_remaining == 0` (which would republish it after one tick).
        let idx = (k.priority as usize).min(PRIORITY_COUNT as usize - 1);
        k.time_slice_remaining = TIME_SLICES[idx];
        crate::scheduler::accounting::mark_dispatch(k);
    }

    /// Is `pid` a thread without a user (Ring-3) image?
    ///
    /// Idle threads and kernel threads created by `spawn_kthread_named` (e.g.
    /// `netd`) run in Ring 0 by design. Their dispatch frame is legitimately
    /// Ring 0, so the #338 Ring-3 publication gate must not apply to them.
    /// A thread is a user thread iff its `Eprocess` owns a user address-space
    /// slot (`user_slot.is_some()`, set only by `Eprocess::new_ring3`).
    #[inline]
    pub(crate) fn is_kernel_thread(&self, k: &Kthread) -> bool {
        if k.is_kernel || k.pid == 0 || k.is_idle {
            return true;
        }
        match self.find_eprocess(k.pid) {
            Some(ep) => ep.user_slot.is_none(),
            // Threads with no Eprocess (bootstrap/boot) are kernel threads.
            None => true,
        }
    }

    /// Timer tick accounting and timeslice expiry.
    ///
    /// `interrupted_cs` is the CS of the frame the timer interrupted (the timer
    /// handler decodes it from `current_rsp + 128`: `cs & 3 == 3` means Ring 3).
    /// It decides whether an expired **user** thread may be published `Ready`:
    /// a user thread whose timeslice expires while it is inside a syscall
    /// (Ring 0, `cs == 0x08`) must not be re-enqueued with a non-Ring-3 dispatch
    /// frame (#338). Kernel/idle threads run in Ring 0 by design and are exempt
    /// (`is_kernel_thread`); they keep the historical behaviour. Unit tests pass
    /// the CS of the context they simulate.
    pub fn on_timer_tick(&mut self, current_rsp: u64, interrupted_cs: u64) {
        if crate::scheduler::SCHED_TEST_MODE.load(core::sync::atomic::Ordering::Relaxed) {
            return;
        }
        self.timer_ticks += 1;

        if self.timer_ticks.is_multiple_of(AGING_INTERVAL_TICKS) {
            self.apply_aging();
        }

        let _tid = if self.kprcb_thread_in_self() {
            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
        } else { self.current_tid };

        let mut needs_resched = false;
        let mut expired_priority: u8 = 0;
        let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        // #338: compute the exemption *before* the mutable borrow below (the
        // helper reads the eprocess table, so it needs an immutable borrow).
        // Kernel/idle threads run in Ring 0 by design; the Ring-3 publication
        // gate must not apply to them (netd has a non-zero pid).
        let current_is_kernel_thread = self
            .find_kthread(_tid)
            .map(|k| self.is_kernel_thread(k))
            .unwrap_or(true);
        // #476: the ownership guard below only applies to the live per-CPU
        // scheduler (production). Local schedulers used by unit tests pass
        // synthetic `current_rsp` values and must keep the historical save.
        let kprcb_in_self = self.kprcb_thread_in_self();
        if let Some(k) = self.current_kthread_mut() {
            let state_before = k.state.to_u8();
            if k.state == ThreadState::Running {
                k.cpu_ticks += 1;
                // Phase 15-A.1: charge the elapsed interval to the thread that
                // actually executed it. Idle threads are excluded so idle CPU
                // is never attributed to a process.
                if !k.is_idle {
                    crate::scheduler::accounting::account_tick(k);
                }

                if k.time_slice_remaining > 0 {
                    k.time_slice_remaining -= 1;
                }

                if k.time_slice_remaining == 0 {
                    expired_priority = k.priority;
                    k.yield_requested = false;
                    crate::scheduler::diag::rsp_ev(crate::scheduler::diag::SITE_RSP_TIMESLICE, k, current_rsp);
                    // #476: only store the live rsp when it lies on this
                    // thread's own kernel stack. A mismatch means `KPRCB` is
                    // desynchronised from the live context; storing it would
                    // corrupt the thread's dispatch frame. The boot thread is
                    // exempt (it runs on the bootstrap stack, not `boot_ks_top`).
                    if kprcb_in_self {
                        crate::scheduler::stack::save_live_rsp_checked(k, current_rsp, "timeslice");
                    } else {
                        k.rsp = current_rsp;
                    }
                    // Re-home to the CPU that actually ran it before enqueueing.
                    crate::scheduler::diag::kcpu_ev(
                        crate::scheduler::diag::SITE_KCPU_TIMESLICE, k, this_cpu);
                    k.cpu = this_cpu;
                    // #338: only publish `Ready`/enqueue when the interrupted
                    // context was Ring 3. A user thread whose timeslice expires
                    // inside a syscall was interrupted in Ring 0 (`cs == 0x08`);
                    // enqueueing it would strand it, because
                    // `schedule_with(true)` rejects every non-Ring-3 frame. The
                    // thread is left `Running`; its syscall return path saves the
                    // real Ring-3 frame and re-enqueues it. The switch-out sites
                    // (idt.rs user/kernel branches) also refuse to publish a
                    // Ring-0 frame, so nothing resurrects it.
                    //
                    // Kernel/idle threads (netd, boot) run in Ring 0 by design
                    // and are dispatched through `schedule_with(require_ring3=
                    // false)`; the gate must not apply to them or netd would be
                    // starved (it always interrupts in Ring 0).
                    let expose_to_ring3 =
                        (interrupted_cs & 3) == 3 || current_is_kernel_thread;
                    if crate::scheduler::preempt_disabled() {
                        // #376: the current thread is inside a kernel spinlock
                        // critical section. Do not deschedule it — a held lock
                        // whose owner becomes an undispatchable Ready Ring-0
                        // frame deadlocks every waiter. Grant a fresh slice and
                        // continue; the switch happens once the lock is dropped.
                        let idx = (k.priority as usize).min(PRIORITY_COUNT as usize - 1);
                        k.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
                    } else if expose_to_ring3 {
                        k.state = ThreadState::Ready;
                        if k.tid != BOOT_TID && !k.is_idle {
                            Self::enqueue_to_cpu_run_queue(k);
                        }
                        needs_resched = true;
                        crate::trace_sched_state!(k.tid, state_before, k.state.to_u8(), 2u8); // TIMESLICE_EXPIRED
                    } else {
                        // User thread preempted inside a syscall: keep Running
                        // and grant a fresh slice; do not set NEED_RESCHED. Its
                        // syscall return path saves the real Ring-3 frame and
                        // re-enqueues it. (Kernel/idle threads never reach this
                        // branch — `current_is_kernel_thread` is true for them.)
                        let idx = (k.priority as usize).min(PRIORITY_COUNT as usize - 1);
                        k.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
                    }
                }
            }
        }

        if needs_resched {
            kdebug!(LogSubsys::Sched, "[SCHED] TIMESLICE_EXPIRED tid={} priority={}",
                _tid, expired_priority);
            crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
        }
    }
}
