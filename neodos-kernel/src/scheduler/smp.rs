//! Scheduler SMP / work stealing — extracted from mod.rs
use crate::scheduler::Scheduler;

impl Scheduler {
    /// Try to steal a thread from another CPU's run queue.
    /// Returns the TID if found, or None if all queues are empty.
    /// K20 fix: migration updates Kthread.cpu atomically under scheduler lock
    /// so physical queue owner and logical ownership stay consistent.
    pub(crate) fn try_work_steal(&mut self) -> Option<u32> {
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
    /// Returns number of TIDs successfully migrated.
    /// On destination-full, restores old cpu and pushes TID back to victim.
    pub(crate) unsafe fn steal_and_migrate(&mut self, victim: usize, thief: usize) -> u32 {
        // SAFETY: KPRCB pages are initialized, queues are per-CPU and accessed
        // only while holding scheduler lock in schedule() path.
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
