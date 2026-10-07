//! Scheduler run queue — extracted from mod.rs
use crate::scheduler::types::{Kthread, BOOT_TID, PRIORITY_COUNT, TIME_SLICES, ThreadState};
use crate::scheduler::Scheduler;

/// Counts how many times `enqueue_to_cpu_run_queue` failed because the target
/// sub-queue was full. Surfaced in the boot `[STEAL] ... rq_overflow=N` line
/// (intended for a future `ObInfoClass` stats query) so operators can detect
/// queue saturation in production.
///
/// A non-zero value means at least one thread had to fall back to the global
/// priority scan (`schedule_with_handoff` step 3) to be dispatched. That path
/// is correct but slower; sustained overflow indicates the queue capacity
/// (`RUNQUEUE_PRIO_CAP`) needs to be increased.
pub static RUNQUEUE_OVERFLOW: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

impl Scheduler {
    /// Enqueue `k` onto its assigned CPU's per-priority run queue.
    ///
    /// # Overflow recovery
    /// If the target priority sub-queue is full, the push is refused and
    /// `RUNQUEUE_OVERFLOW` is incremented. The thread **stays** in state
    /// `Ready` inside `self.kthreads`, so the global priority scan (step 3
    /// of `schedule_with_handoff`) will still find and dispatch it.  This is
    /// the explicit documented recovery path: the fast O(1) queue miss falls
    /// back to the O(N) scan, which is correct but slower.
    ///
    /// Persistent overflow → increase `RUNQUEUE_PRIO_CAP`.
    pub fn enqueue_to_cpu_run_queue(k: &Kthread) {
        if k.tid == BOOT_TID || k.is_idle {
            return;
        }
        let cpu = k.cpu as usize;
        if cpu >= crate::arch::x64::cpu_local::MAX_CPUS { return; }
        let my_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;

        // IRQ-safe lock: `with_runqueue` disables interrupts before acquiring
        // the spinlock, preventing a timer-handler re-entry deadlock.
        let already_queued = crate::arch::x64::cpu_local::with_runqueue(cpu, |rq| {
            if rq.contains(k.tid) {
                true // duplicate – idempotent enqueue
            } else {
                let pushed = rq.push_priority(k.tid, k.priority);
                if !pushed {
                    // Sub-queue full: increment overflow counter and log.
                    // Recovery: the global scan in schedule_with_handoff will
                    // still find this thread via kthreads (state == Ready) and
                    // dispatch it. This is the explicit fallback.
                    RUNQUEUE_OVERFLOW.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    crate::serial_println!(
                        "[SMPSCHED_WARN] CpuRunQueue FULL cpu={} tid={} prio={} overflow_total={}",
                        cpu, k.tid, k.priority,
                        RUNQUEUE_OVERFLOW.load(core::sync::atomic::Ordering::Relaxed),
                    );
                }
                false
            }
        });

        if already_queued {
            return;
        }

        #[cfg(feature = "forensic")]
        if k.pid >= 4 {
            let rq_len = crate::arch::x64::cpu_local::with_runqueue(cpu, |rq| rq.len());
            crate::serial_println!("[SMPSCHED] ENQUEUE_OK pid={} tid={} cpu_target={} rq_len={}", k.pid, k.tid, cpu, rq_len);
        }

        // Send IPI_RESCHEDULE to the target CPU if it's a different CPU.
        #[cfg(feature = "forensic")]
        if cpu != my_cpu && k.pid >= 4 {
            crate::serial_println!("[SMPSCHED] WAKE_NOTIFY from_cpu={} target_cpu={} pid={} tid={} action=IPI_RESCHEDULE", my_cpu, cpu, k.pid, k.tid);
        } else if k.pid >= 4 {
            #[cfg(feature = "forensic")]
            crate::serial_println!("[SMPSCHED] WAKE_NOTIFY from_cpu={} target_cpu={} pid={} tid={} action=same_cpu_no_ipi", my_cpu, cpu, k.pid, k.tid);
        }

        if cpu != my_cpu {
            unsafe {
                let kprcb = crate::arch::x64::cpu_local::kprcb_page(cpu);
                if let Some(kprcb_addr) = kprcb {
                    let apic_id = core::ptr::read_volatile(
                        (kprcb_addr + 4) as *const u32 // apic_id at offset 0x004
                    );
                    crate::arch::x64::ipi::send_ipi(
                        apic_id,
                        crate::arch::x64::ipi::IPI_RESCHEDULE,
                    );
                }
            }
        }
    }

    /// Transition a thread to Ready state and enqueue it exactly once.
    /// Safe to call if thread is already Ready (no-op, avoids duplicate enqueue).
    /// Must be called under scheduler lock + interrupts disabled.
    pub fn make_thread_ready(k: &mut Kthread) {
        if k.state == ThreadState::Ready {
            return;
        }
        crate::scheduler::diag::ev(
            crate::scheduler::diag::EV_WAKE_READY, k.cpu, k.tid, k.rsp, k.state.to_u8() as u64);
        k.state = ThreadState::Ready;
        // Phase 13-A: a thread published as Ready has had any pending yield
        // intent consumed (by the switch-out that saved its `rsp`, or by this
        // wake from Blocked). Leaving it set would trigger a spurious
        // preemption on its next timeslice.
        k.yield_requested = false;
        let idx = (k.priority as usize).min(PRIORITY_COUNT as usize - 1);
        k.time_slice_remaining = TIME_SLICES[idx];
        k.ticks_since_scheduled = 0;
        Self::enqueue_to_cpu_run_queue(k);
    }

    /// Remove a thread from its assigned CPU's per-CPU run queue.
    /// Called when a thread transitions away from Ready state.
    /// Safe to call even if the thread is not currently in the run queue.
    /// Must be called under scheduler lock + interrupts disabled.
    pub fn remove_from_run_queue(k: &Kthread) {
        let cpu = k.cpu as usize;
        if cpu >= crate::arch::x64::cpu_local::MAX_CPUS {
            return;
        }
        unsafe {
            crate::arch::x64::cpu_local::remove_from_cpu_run_queue(cpu, k.tid);
        }
    }

    /// Try to dequeue the next thread from the current CPU's local run queue.
    /// Returns the TID if found, or None if the queue is empty.
    /// IRQ-safe: `with_this_runqueue` disables interrupts before locking.
    pub(crate) fn try_dequeue_local() -> Option<u32> {
        crate::arch::x64::cpu_local::with_this_runqueue(|rq| rq.pop())
    }

}
