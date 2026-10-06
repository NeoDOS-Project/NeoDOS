use crate::input::vt::{VtInputQueue, VtShadowBuffer, ConsoleState, VT_COUNT};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub struct InputManager {
    active_vt: AtomicUsize,
    vt_queues: [VtInputQueue; VT_COUNT],
    vt_states: [ConsoleState; VT_COUNT],
    pub vt_shadow: [VtShadowBuffer; VT_COUNT],
    /// Foreground process per VT, set by `ObWait` on a process object. It is
    /// read from the keyboard IRQ (Ctrl+C) and written from syscall context, so
    /// it must be atomic.
    vt_foreground_pid: [AtomicU32; VT_COUNT],
}

static mut INPUT_MANAGER: InputManager = InputManager::new();

impl InputManager {
    pub const fn new() -> Self {
        InputManager {
            active_vt: AtomicUsize::new(0),
            vt_queues: [
                VtInputQueue::new(),
                VtInputQueue::new(),
                VtInputQueue::new(),
                VtInputQueue::new(),
            ],
            vt_states: [
                ConsoleState::new(),
                ConsoleState::new(),
                ConsoleState::new(),
                ConsoleState::new(),
            ],
            vt_shadow: [
                VtShadowBuffer::new(),
                VtShadowBuffer::new(),
                VtShadowBuffer::new(),
                VtShadowBuffer::new(),
            ],
            vt_foreground_pid: [const { AtomicU32::new(0) }; VT_COUNT],
        }
    }

    pub fn active_vt(&self) -> usize {
        self.active_vt.load(Ordering::Relaxed)
    }

    pub fn foreground_pid(&self) -> u32 {
        let vt = self.active_vt();
        self.vt_foreground_pid[vt].load(Ordering::Acquire)
    }

    /// Mark `pid` as the foreground process of VT `vt`. Called from `ObWait`
    /// when a parent blocks on a child, which is exactly "the child is in the
    /// foreground".
    pub fn set_foreground_pid_for(&self, vt: usize, pid: u32) {
        if vt < VT_COUNT {
            self.vt_foreground_pid[vt].store(pid, Ordering::Release);
        }
    }

    /// Clear the foreground marker for any VT currently owned by `pid`.
    /// Called when a process exits or is terminated.
    pub fn clear_foreground_pid(&self, pid: u32) {
        if pid == 0 {
            return;
        }
        for q in self.vt_foreground_pid.iter() {
            if q.load(Ordering::Acquire) == pid {
                q.store(0, Ordering::Release);
            }
        }
    }

    pub fn switch_vt(&mut self, vt_num: usize) {
        if vt_num >= VT_COUNT || vt_num == self.active_vt() {
            return;
        }
        let old_vt = self.active_vt();
        self.vt_states[old_vt] = crate::console::save_state();
        crate::console::restore_state(&self.vt_states[vt_num]);
        self.active_vt.store(vt_num, Ordering::Release);
        crate::console::redraw_from_shadow(&self.vt_shadow[vt_num]);
    }

    pub fn push_byte(&self, byte: u8) -> Result<(), ()> {
        self.vt_queues[self.active_vt()].push(byte)
    }

    pub fn pop_byte_from_vt(&self, vt: usize) -> Option<u8> {
        if vt >= VT_COUNT { None } else { self.vt_queues[vt].pop() }
    }
}

pub fn init() {
    unsafe { INPUT_MANAGER.vt_states[0] = crate::console::save_state(); }
}
pub fn active_vt() -> usize { unsafe { INPUT_MANAGER.active_vt() } }
pub fn switch_vt(vt: usize) { unsafe { INPUT_MANAGER.switch_vt(vt); } }
pub fn push_byte(b: u8) -> Result<(), ()> { unsafe { INPUT_MANAGER.push_byte(b) } }
pub fn pop_byte_from_vt(vt: usize) -> Option<u8> { unsafe { INPUT_MANAGER.pop_byte_from_vt(vt) } }
pub fn input_manager_mut() -> Option<&'static mut InputManager> {
    unsafe { Some(&mut INPUT_MANAGER) }
}
pub fn foreground_pid() -> u32 { unsafe { INPUT_MANAGER.foreground_pid() } }

/// Set the foreground process for the active VT.
pub fn set_foreground_pid(pid: u32) {
    let vt = active_vt();
    unsafe { INPUT_MANAGER.set_foreground_pid_for(vt, pid); }
}

/// Clear the foreground marker for `pid` on all VTs.
pub fn clear_foreground_pid(pid: u32) {
    unsafe { INPUT_MANAGER.clear_foreground_pid(pid); }
}

/// Queue a deferred termination of the active VT's foreground process.
///
/// Called from the keyboard IRQ on Ctrl+C (0x03). Only lock-free atomics and
/// the IRQ-safe work queue are touched here; the actual process termination
/// (`kill_pid` + `wake_waiters`) runs later from syscall context via
/// `crate::syscall::interrupt_foreground_work`. Returns `true` if a foreground
/// process was scheduled for termination (the Ctrl+C byte is then consumed).
pub fn request_foreground_interrupt() -> bool {
    let pid = foreground_pid();
    if pid == 0 {
        return false;
    }
    crate::work_queue::WORK_QUEUE.push_high(
        crate::syscall::interrupt_foreground_work,
        pid as usize as *mut u8,
    )
}
pub fn vt_active_occupancy() -> usize {
    vt_active_occupancy_for(active_vt())
}

/// Number of bytes queued for a specific VT. Used by `sys_poll` to report a
/// readable stdin only when a byte is actually available.
pub fn vt_active_occupancy_for(vt: usize) -> usize {
    if vt >= VT_COUNT {
        return 0;
    }
    unsafe {
        let h = INPUT_MANAGER.vt_queues[vt].head.load(Ordering::Relaxed);
        let t = INPUT_MANAGER.vt_queues[vt].tail.load(Ordering::Relaxed);
        if t >= h { t - h } else { crate::input::vt::VT_QUEUE_SIZE - h + t }
    }
}
pub fn vt_active_head_tail() -> (usize, usize) {
    unsafe {
        let vt = INPUT_MANAGER.active_vt();
        (INPUT_MANAGER.vt_queues[vt].head.load(Ordering::Relaxed),
         INPUT_MANAGER.vt_queues[vt].tail.load(Ordering::Relaxed))
    }
}
