//! Scheduler SMP / work stealing — extracted from mod.rs
use crate::scheduler::Scheduler;

pub(crate) static STEAL_ATTEMPTS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub(crate) static STEAL_SUCCESS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

impl Scheduler {
    /// Try to steal a thread from another CPU's run queue.
    /// Returns the TID if found, or None if all queues are empty.
    /// K20 fix: migration updates Kthread.cpu atomically under scheduler lock
    /// so physical queue owner and logical ownership stay consistent.
    pub(crate) fn try_work_steal(&mut self) -> Option<u32> {
        STEAL_ATTEMPTS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // Phase 4: during k18/k19 tests, AP stealing via the *global*
        // scheduler is paused to avoid racing with the test's manipulation
        // of global runqueues. The test's *local* Scheduler (kprcb_thread_in_self()==false)
        // must still be able to steal, so we only block the global path.
        if crate::scheduler::SCHED_TEST_MODE.load(core::sync::atomic::Ordering::Relaxed)
            && self.kprcb_thread_in_self()
        {
            return None;
        }
        let my_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
        for victim in 0..crate::arch::x64::cpu_local::MAX_CPUS {
            if victim == my_cpu { continue; }
            let stolen = unsafe { self.steal_and_migrate(victim, my_cpu) };
            if stolen > 0 {
                // We stole at least one thread, pop from our queue
                return Self::try_dequeue_local();
            }
        }
        None
    }

    /// Steal TIDs from victim CPU's runqueue to thief CPU's runqueue,
    /// updating Kthread.cpu for each migrated thread.
    /// Must be called with scheduler lock held and interrupts disabled.
    /// SMP-safe: locks both queues in consistent order.
    /// Returns number of TIDs successfully migrated.
    /// On destination-full, restores old cpu and pushes TID back to victim.
    pub(crate) unsafe fn steal_and_migrate(&mut self, victim: usize, thief: usize) -> u32 {
        use crate::arch::x64::cpu_local::{KPRCB_PAGES, MAX_CPUS};
        let need_skip = unsafe { victim >= MAX_CPUS || thief >= MAX_CPUS || KPRCB_PAGES[victim] == 0 || KPRCB_PAGES[thief] == 0 };
        if need_skip {
            return 0;
        }
        // Lock both queues in order to avoid deadlock
        let (first, second) = if victim < thief { (victim, thief) } else { (thief, victim) };
        let _g1 = crate::arch::x64::cpu_local::RUNQUEUE_LOCKS[first].lock();
        let _g2 = if first != second {
            Some(crate::arch::x64::cpu_local::RUNQUEUE_LOCKS[second].lock())
        } else {
            None
        };
        let victim_rq = crate::arch::x64::cpu_local::cpu_run_queue_mut(victim);
        let thief_rq = crate::arch::x64::cpu_local::cpu_run_queue_mut(thief);
        let mut stolen: u32 = 0;
        while victim_rq.count > 0 {
            if (thief_rq.count as usize) >= thief_rq.entries.len() {
                break;
            }
            // Peek tid at victim head
            let tid = victim_rq.entries[(victim_rq.head_idx as usize) % victim_rq.entries.len()];
            // Pop victim
            let tid_popped = victim_rq.pop().unwrap();
            debug_assert_eq!(tid, tid_popped);
            // Update ownership optimistically
            let old_cpu: Option<u32> = if let Some(k) = self.find_kthread_mut(tid_popped) {
                let old = k.cpu;
                k.cpu = thief as u32;
                Some(old)
            } else {
                None
            };
            // Try push to thief
            if thief_rq.push(tid_popped) {
                stolen += 1;
                STEAL_SUCCESS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            } else {
                // Rollback: restore cpu and push back to victim
                if let Some(old) = old_cpu {
                    if let Some(k) = self.find_kthread_mut(tid_popped) {
                        k.cpu = old;
                    }
                }
                let _ = victim_rq.push(tid_popped);
                break;
            }
        }
        stolen
    }

}
