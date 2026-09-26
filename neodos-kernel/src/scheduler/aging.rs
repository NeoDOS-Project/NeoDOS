//! Scheduler aging — extracted from mod.rs
use crate::scheduler::types::{PRIORITY_COUNT, PRIORITY_HIGH, TIME_SLICES, ThreadState, IDLE_TID, AGING_INTERVAL_TICKS, MAX_STARVATION_TICKS};
use crate::scheduler::Scheduler;

impl Scheduler {
    pub fn set_process_priority(&mut self, pid: u32, priority: u8) -> bool {
        if priority >= PRIORITY_COUNT { return false; }
        let mut found = false;
        for k in self.kthreads.iter_mut().flatten() {
            if k.pid == pid {
                k.priority = priority;
                let idx = priority as usize;
                k.time_slice_remaining = TIME_SLICES[idx];
                k.ticks_since_scheduled = 0;
                found = true;
            }
        }
        found
    }

    pub fn reset_time_slice(&mut self) {
        if let Some(k) = self.current_kthread_mut() {
            let idx = (k.priority as usize).min(PRIORITY_COUNT as usize - 1);
            k.time_slice_remaining = TIME_SLICES[idx];
            k.ticks_since_scheduled = 0;
        }
    }

    // ── Aging ──

    pub(crate) fn apply_aging(&mut self) {
        for k in self.kthreads.iter_mut().flatten() {
            if !k.is_idle && k.state == ThreadState::Ready {
                k.ticks_since_scheduled = k.ticks_since_scheduled.saturating_add(AGING_INTERVAL_TICKS);
                if k.ticks_since_scheduled >= MAX_STARVATION_TICKS && k.priority > PRIORITY_HIGH {
                    k.priority -= 1;
                    k.ticks_since_scheduled = 0;
                }
            }
        }
    }
}
