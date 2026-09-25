//! Scheduler core schedule — extracted from mod.rs
use alloc::vec::Vec;
use crate::log::LogSubsys;
use crate::scheduler::types::{Kthread, ThreadState, BOOT_TID, IDLE_TID, PRIORITY_COUNT, IDLE_TIME_SLICE, AGING_INTERVAL_TICKS};
use crate::scheduler::Scheduler;
use crate::scheduler::lifecycle::{defer_reap, reap_pending_zombies};

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
fn frame_is_ring3(k: &Kthread) -> bool {
    if k.rsp < 0x1000 { return false; }
    let cs = unsafe { *((k.rsp + 128) as *const u64) };
    (cs & 3) == 3
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
            if c < 12 {
                crate::serial_println!(
                    "[SCHED_WARN] tag={} TWO+ Running on cpu={} tids={:?}/{:?} sched.current={} kprcb_tid={:?}",
                    tag, cpu, running_tids, running_cpus, self.current_tid,
                    crate::arch::x64::cpu_local::try_per_cpu_tid());
                for k in self.kthreads.iter().flatten() {
                    crate::serial_println!(
                        "[SCHED_WARN]   tid={} pid={} state={} cpu={} wait={:?}",
                        k.tid, k.pid, state_name(k.state.to_u8()), k.cpu, k.waiting_for);
                }
            }
        }
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
                let mut entries = Vec::new();
                let cap = rq.entries.len();
                let mut idx = rq.head_idx as usize;
                for _ in 0..rq.count {
                    entries.push(rq.entries[idx]);
                    idx = (idx + 1) % cap;
                }
                entries
            });

            for tid in queue_entries {
                if queue_tids.contains(&tid) {
                    return Err("duplicate TID in run queue");
                }
                let k = self.find_kthread(tid)
                    .ok_or("orphan TID in run queue")?;
                if tid == BOOT_TID || tid == IDLE_TID {
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
            if k.tid == IDLE_TID || k.tid == BOOT_TID {
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
        ktrace!(LogSubsys::Sched, "schedule entry");
        SCHEDULE_CALLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // Count every schedule decision, not just global-scan fallbacks.
        self.schedule_count += 1;

        // 1. Try per-CPU local run queue (fast path)
        if let Some(tid) = Self::try_dequeue_local() {
            let ptr = self.find_kthread_ptr(tid);
            if !ptr.is_null() {
                unsafe {
                    let k = &mut *ptr;
                    if k.state == ThreadState::Ready && (!require_ring3 || frame_is_ring3(k)) {
                        let prev = if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
                        } else { self.current_tid };
                        let prev_state = self.find_kthread(prev).map(|t| t.state.to_u8()).unwrap_or(255);
                        self.current_tid = tid;
                        // F-01: sync per-CPU KPRCB only for global scheduler (tests use local schedulers)
                        if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::sync_per_cpu_current(ptr, k.pid);
                        }
                        k.state = ThreadState::Running;
                        if (tid == 5 || prev == 5) && sched_forensic_verbose() {
                            crate::serial_println!("[T5_SCHED] step=1 prev={} new={} current={} rq0={} kprcb={:?}",
                                prev, tid, self.current_tid, rq_len(0),
                                crate::arch::x64::cpu_local::try_per_cpu_tid());
                        }
                        kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=runqueue",
                            prev, tid);
                        crate::trace_cswitch!(prev as u64, tid as u64);
                        crate::trace_sched_switch!(prev, prev_state, tid, k.state.to_u8());
                        // Safe to reap zombies now that we have switched away from previous stack
                        let new_pid = k.pid;
                        reap_pending_zombies(self, new_pid);
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
                    if k.state == ThreadState::Ready && (!require_ring3 || frame_is_ring3(k)) {
                        let prev = if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
                        } else { self.current_tid };
                        let prev_state = self.find_kthread(prev).map(|t| t.state.to_u8()).unwrap_or(255);
                        self.current_tid = tid;
                        if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::sync_per_cpu_current(ptr, k.pid);
                        }
                        k.state = ThreadState::Running;
                        kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=steal",
                            prev, tid);
                        crate::trace_cswitch!(prev as u64, tid as u64);
                        crate::trace_sched_switch!(prev, prev_state, tid, k.state.to_u8());
                        let new_pid = k.pid;
                        reap_pending_zombies(self, new_pid);
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
        'scan: for priority in 0..PRIORITY_COUNT {
            for offset in 0..self.next_tid {
                let check_tid = (start + offset) % self.next_tid.max(1);
                for k in self.kthreads.iter_mut().flatten() {
                    if k.tid == check_tid && k.state == ThreadState::Ready && k.priority == priority
                        && (!require_ring3 || frame_is_ring3(k))
                    {
                        // P0-3 FIX: Remove from runqueue BEFORE setting state to Running.
                        Scheduler::remove_from_run_queue(&**k);
                        let prev = scan_prev;
                        let prev_state = k.state.to_u8();
                        self.current_tid = check_tid;
                        k.state = ThreadState::Running;
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
            reap_pending_zombies(self, picked_pid);
            return picked_ptr;
        }

        // Fallback to idle thread (TID 1, PRIORITY_IDLE).
        // NOTE: By design, the idle thread is created with state=Ready but is never
        // added to any runqueue. It is a special thread that only runs when no other
        // threads are ready. The remove_from_run_queue() call here is defensive: if
        // the idle thread were ever accidentally enqueued, we remove it to satisfy
        // the invariant (Running => runqueue_count == 0).
        {
            if !self.has_non_idle_threads() {
                kdebug!(LogSubsys::Sched, "[SCHED] idle_fallback: has_non_idle_threads=false (only idle or Suspended threads)");
            }
            let ptr = self.find_kthread_ptr(IDLE_TID);
            if !ptr.is_null() {
                unsafe {
                    let idle = &mut *ptr;
                    if idle.state != ThreadState::Terminated {
                        Scheduler::remove_from_run_queue(idle);
                        let prev = if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
                        } else { self.current_tid };
                        let prev_state = self.find_kthread(prev).map(|t| t.state.to_u8()).unwrap_or(255);
                        self.current_tid = IDLE_TID;
                        if self.kprcb_thread_in_self() {
                            crate::arch::x64::cpu_local::sync_per_cpu_current(ptr, (*ptr).pid);
                        }
                        idle.state = ThreadState::Running;
                        idle.time_slice_remaining = IDLE_TIME_SLICE;
                        kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=idle_fallback",
                            prev, IDLE_TID);
                        crate::trace_cswitch!(prev as u64, IDLE_TID as u64);
                        crate::trace_sched_switch!(prev, prev_state, IDLE_TID, idle.state.to_u8());
                        let new_pid = (*ptr).pid;
                        reap_pending_zombies(self, new_pid);
                        return ptr;
                    }
                }
            }
        }
        panic!("No ready threads and idle is unavailable");
    }

    // ── Timer tick ──


    pub fn on_timer_tick(&mut self, current_rsp: u64) {
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
        if let Some(k) = self.current_kthread_mut() {
            let state_before = k.state.to_u8();
            if k.state == ThreadState::Running {
                k.cpu_ticks += 1;

                if k.time_slice_remaining > 0 {
                    k.time_slice_remaining -= 1;
                }

                if k.time_slice_remaining == 0 {
                    expired_priority = k.priority;
                    k.state = ThreadState::Ready;
                    k.rsp = current_rsp;
                    if k.tid != BOOT_TID && k.tid != IDLE_TID {
                        Self::enqueue_to_cpu_run_queue(k);
                    }
                    needs_resched = true;
                    crate::trace_sched_state!(k.tid, state_before, k.state.to_u8(), 2u8); // TIMESLICE_EXPIRED
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
