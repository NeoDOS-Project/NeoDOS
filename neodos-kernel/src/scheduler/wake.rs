//! Scheduler wake paths — extracted from mod.rs
use crate::scheduler::types::{ThreadState};
use crate::scheduler::Scheduler;

static FOREGROUND_CLEAR_HOOK: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

pub fn set_foreground_clear_hook(hook: fn(u32)) {
    FOREGROUND_CLEAR_HOOK.store(hook as usize, core::sync::atomic::Ordering::Release);
}

fn clear_foreground_hook(pid: u32) {
    let raw = FOREGROUND_CLEAR_HOOK.load(core::sync::atomic::Ordering::Acquire);
    if raw != 0 {
        let f: fn(u32) = unsafe { core::mem::transmute(raw) };
        f(pid);
    }
}

impl Scheduler {
    pub fn wake_waiters(&mut self, pid: u32) {
        // The foreground process is gone: Ctrl+C must no longer target it.
        // Called on both normal exit and termination. Input is wired via a
        // boot-registered hook (scheduler must not depend on input).
        clear_foreground_hook(pid);
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
