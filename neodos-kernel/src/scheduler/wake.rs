//! Scheduler wake paths — extracted from mod.rs
use crate::scheduler::types::{ThreadState};
use crate::scheduler::Scheduler;

impl Scheduler {
    pub fn wake_waiters(&mut self, pid: u32) {
        // Legacy magic waitpid (0x8000_0000 | pid) — kept for compat, now u64
        let legacy_magic: u64 = (pid as u64) | 0x8000_0000;
        // KWait ChildExit magic — F-03 full-width
        let kwait_magic = crate::kwait::WaitReason::ChildExit { pid }.encode_magic();
        for k in self.kthreads.iter_mut().flatten() {
            if k.waiting_for == Some(legacy_magic) || k.waiting_for == Some(kwait_magic) {
                if matches!(k.state, ThreadState::Blocked { .. }) {
                    k.waiting_for = None;
                    Self::make_thread_ready(k);
                    #[cfg(feature = "forensic")]
                    {
                        let cpu_target = k.cpu;
                        let cpu_current = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
                        let rq_target_len = unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(cpu_target as usize).len() };
                        let rq_current_len = unsafe { crate::arch::x64::cpu_local::this_cpu_run_queue_mut().len() };
                        crate::serial_println!("[SMPSCHED] ENQUEUE pid={} tid={} cpu_target={} cpu_current={} rq_target_len={} rq_current_len={} equal={}", k.pid, k.tid, cpu_target, cpu_current, rq_target_len, rq_current_len, rq_target_len==rq_current_len);
                    }
                }
            }
        }
    }

    pub fn wake_blocked_on_magic(&mut self, magic: u64) {
        for k in self.kthreads.iter_mut().flatten() {
            if k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                k.waiting_for = None;
                Self::make_thread_ready(k);
            }
        }
    }

    // ── Priority ──

}
