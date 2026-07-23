use crate::object::{ObOperations, ObId};
use crate::log::LogSubsys;
use crate::kwait::{self, WaitReason};
use spin::Mutex;

const MAX_TIMERS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerState {
    Idle,
    Running,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerType {
    Oneshot,
    Periodic,
}

#[derive(Debug, Clone, Copy)]
struct TimerEntry {
    used: bool,
    state: TimerState,
    timer_type: TimerType,
    period_ms: u64,
    remaining_ms: u64,
    ob_id: ObId,
}

impl TimerEntry {
    const fn unused() -> Self {
        TimerEntry {
            used: false,
            state: TimerState::Idle,
            timer_type: TimerType::Oneshot,
            period_ms: 0,
            remaining_ms: 0,
            ob_id: 0,
        }
    }
}

pub struct TimerManager {
    timers: [TimerEntry; MAX_TIMERS],
}

impl TimerManager {
    const fn new() -> Self {
        TimerManager {
            timers: [TimerEntry::unused(); MAX_TIMERS],
        }
    }

    pub fn alloc(&mut self, ob_id: ObId, period_ms: u64, periodic: bool) -> Option<u32> {
        for (i, t) in self.timers.iter_mut().enumerate() {
            if !t.used {
                t.used = true;
                t.state = TimerState::Idle;
                t.timer_type = if periodic { TimerType::Periodic } else { TimerType::Oneshot };
                t.period_ms = period_ms;
                t.remaining_ms = 0;
                t.ob_id = ob_id;
                return Some(i as u32);
            }
        }
        None
    }

    pub fn free(&mut self, timer_id: u32) {
        if let Some(t) = self.timers.get_mut(timer_id as usize) {
            *t = TimerEntry::unused();
        }
    }

    pub fn start(&mut self, timer_id: u32) -> bool {
        if let Some(t) = self.timers.get_mut(timer_id as usize) {
            if t.used {
                t.state = TimerState::Running;
                t.remaining_ms = t.period_ms;
                return true;
            }
        }
        false
    }

    pub fn cancel(&mut self, timer_id: u32) -> bool {
        if let Some(t) = self.timers.get_mut(timer_id as usize) {
            if t.used && t.state == TimerState::Running {
                t.state = TimerState::Idle;
                t.remaining_ms = 0;
                return true;
            }
        }
        false
    }

    pub fn tick(&mut self) {
        for i in 0..MAX_TIMERS {
            let expired = {
                let t = &mut self.timers[i];
                if !t.used || t.state != TimerState::Running {
                    continue;
                }
                if t.remaining_ms > 1 {
                    t.remaining_ms -= 1;
                    continue;
                }
                t.remaining_ms = 0;
                t.state = TimerState::Expired;
                t.timer_type == TimerType::Periodic
            };
            if expired {
                if let Some(t) = self.timers.get_mut(i) {
                    t.state = TimerState::Running;
                    t.remaining_ms = t.period_ms;
                }
            }
            kwait::kwait_wake(&WaitReason::Timer { timer_id: i as u32 });
        }
    }
}

static TIMER_MANAGER: Mutex<TimerManager> = Mutex::new(TimerManager::new());

pub struct TimerObOps;

impl ObOperations for TimerObOps {
    fn on_destroy(&self, _id: ObId, native_id: u64) {
        TIMER_MANAGER.lock().free(native_id as u32);
    }
}

pub static TIMER_OPS: TimerObOps = TimerObOps;

pub fn init_timer_manager() {
    kinfo!(LogSubsys::Kernel, "Timer Manager initialized ({} slots)", MAX_TIMERS);
}

pub fn tick() {
    TIMER_MANAGER.lock().tick();
}

pub fn alloc_timer(ob_id: ObId, period_ms: u64, periodic: bool) -> Option<u32> {
    TIMER_MANAGER.lock().alloc(ob_id, period_ms, periodic)
}

pub fn free_timer(timer_id: u32) {
    TIMER_MANAGER.lock().free(timer_id);
}

pub fn start_timer(timer_id: u32) -> bool {
    TIMER_MANAGER.lock().start(timer_id)
}

pub fn cancel_timer(timer_id: u32) -> bool {
    TIMER_MANAGER.lock().cancel(timer_id)
}

pub fn register_timer_tests() {
    use crate::{test_case, test_eq, test_true};

    test_case!("timer_alloc_free", {
        let mut mgr = TimerManager::new();
        let id = mgr.alloc(42, 1000, false).unwrap();
        test_eq!(id, 0);
        let id2 = mgr.alloc(43, 500, true).unwrap();
        test_eq!(id2, 1);
        mgr.free(id);
        let id3 = mgr.alloc(44, 200, false).unwrap();
        test_eq!(id3, 0);
        mgr.free(id2);
        mgr.free(id3);
    });

    test_case!("timer_start_cancel", {
        let mut mgr = TimerManager::new();
        let id = mgr.alloc(42, 100, false).unwrap();
        test_true!(mgr.start(id));
        test_true!(mgr.cancel(id));
        mgr.free(id);
    });

    test_case!("timer_oneshot_expires", {
        let mut mgr = TimerManager::new();
        let id = mgr.alloc(42, 2, false).unwrap();
        mgr.start(id);
        // After 1 tick, still running
        mgr.tick();
        let t = mgr.timers[id as usize];
        test_eq!(t.state, TimerState::Running);
        // After 2nd tick, expired
        mgr.tick();
        let t = mgr.timers[id as usize];
        test_eq!(t.state, TimerState::Expired);
        mgr.free(id);
    });

    test_case!("timer_periodic_restarts", {
        let mut mgr = TimerManager::new();
        let id = mgr.alloc(42, 2, true).unwrap();
        mgr.start(id);
        mgr.tick();
        mgr.tick();
        let t = mgr.timers[id as usize];
        test_eq!(t.state, TimerState::Running);
        test_eq!(t.remaining_ms, t.period_ms);
        mgr.free(id);
    });

    test_case!("timer_cancel_expired_returns_false", {
        let mut mgr = TimerManager::new();
        let id = mgr.alloc(42, 1, false).unwrap();
        mgr.start(id);
        mgr.tick();
        test_true!(!mgr.cancel(id)); // already expired, cancel returns false
        mgr.free(id);
    });

    test_case!("timer_free_unused", {
        let mut mgr = TimerManager::new();
        let id = mgr.alloc(42, 100, false).unwrap();
        mgr.free(id);
        let t = mgr.timers[id as usize];
        test_true!(!t.used);
    });
}
