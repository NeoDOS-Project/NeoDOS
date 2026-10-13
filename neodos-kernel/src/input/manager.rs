use crate::input::vt::{VtInputQueue, VtShadowBuffer, ConsoleState, VT_COUNT};
use crate::input::kbd::ring::{KeyEvent, KeyEventRing};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

pub struct InputManager {
    active_vt: AtomicUsize,
    vt_queues: [VtInputQueue; VT_COUNT],
    vt_states: [ConsoleState; VT_COUNT],
    pub vt_shadow: [VtShadowBuffer; VT_COUNT],
    /// Foreground process per VT, set by `ObWait` on a process object. It is
    /// read from the keyboard IRQ (Ctrl+C) and written from syscall context, so
    /// it must be atomic.
    vt_foreground_pid: [AtomicU32; VT_COUNT],
    /// Raw keyboard event ring per VT (#466), drained by the foreground client.
    key_rings: [KeyEventRing; VT_COUNT],
    /// Per-VT raw mode: when on, scancodes go to `key_rings` instead of cooked
    /// bytes on stdin (#466).
    raw_mode: [AtomicBool; VT_COUNT],
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
            key_rings: [
                KeyEventRing::new(),
                KeyEventRing::new(),
                KeyEventRing::new(),
                KeyEventRing::new(),
            ],
            raw_mode: [const { AtomicBool::new(false) }; VT_COUNT],
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
        for (i, q) in self.vt_foreground_pid.iter().enumerate() {
            if q.load(Ordering::Acquire) == pid {
                q.store(0, Ordering::Release);
                // A crashed/exited foreground client must not leave the VT deaf
                // to cooked input (#466).
                self.raw_mode[i].store(false, Ordering::Release);
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

    /// Push a raw key event to VT `vt` and wake a blocked raw reader (#466).
    pub fn push_key_event_to(&self, vt: usize, ev: KeyEvent) {
        if vt >= VT_COUNT {
            return;
        }
        self.key_rings[vt].push(ev);
        crate::kwait::kwait_wake(&crate::kwait::WaitReason::Keyboard { vt: vt as u32 });
    }

    pub fn key_ring(&self, vt: usize) -> Option<&KeyEventRing> {
        if vt < VT_COUNT { Some(&self.key_rings[vt]) } else { None }
    }

    pub fn set_raw_mode(&self, vt: usize, on: bool) {
        if vt < VT_COUNT {
            if on {
                // Start from a clean ring so cooked-mode keystrokes are not
                // replayed as raw events (#466).
                self.key_rings[vt].clear();
            }
            self.raw_mode[vt].store(on, Ordering::Release);
        }
    }

    pub fn raw_mode(&self, vt: usize) -> bool {
        vt < VT_COUNT && self.raw_mode[vt].load(Ordering::Acquire)
    }
}

pub fn init() {
    unsafe { INPUT_MANAGER.vt_states[0] = crate::console::save_state(); }
}
pub fn active_vt() -> usize { unsafe { INPUT_MANAGER.active_vt() } }
pub fn switch_vt(vt: usize) { unsafe { INPUT_MANAGER.switch_vt(vt); } }
pub fn push_byte(b: u8) -> Result<(), ()> { unsafe { INPUT_MANAGER.push_byte(b) } }
pub fn pop_byte_from_vt(vt: usize) -> Option<u8> { unsafe { INPUT_MANAGER.pop_byte_from_vt(vt) } }

/// Push a raw key event to the active VT (#466). Called from the scancode path.
pub fn push_key_event(ev: KeyEvent) {
    let vt = active_vt();
    unsafe { INPUT_MANAGER.push_key_event_to(vt, ev) };
}

/// Raw event ring for `vt` (None when out of range).
pub fn key_ring_for(vt: usize) -> Option<&'static KeyEventRing> {
    unsafe { INPUT_MANAGER.key_ring(vt) }
}

/// Enable/disable raw mode for `vt` (#466). Idempotent.
pub fn set_raw_mode(vt: usize, on: bool) {
    unsafe { INPUT_MANAGER.set_raw_mode(vt, on); }
}

/// Whether `vt` is in raw mode.
pub fn raw_mode(vt: usize) -> bool {
    unsafe { INPUT_MANAGER.raw_mode(vt) }
}
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
/// Deferred system shutdown, queued from the keyboard IRQ (Ctrl+Alt+Del).
///
/// `power_shutdown()` flushes the registry hives, the page cache and the VFS with
/// blocking `spin::Mutex` acquisitions. Running it in the keyboard IRQ (IF=0)
/// could spin forever on a lock an interrupted thread holds with IF=1 (INV-3,
/// Rule 3.5.1), so it is executed later from the work queue (the same pattern as
/// Ctrl+C / `interrupt_foreground_work`).
fn power_shutdown_work(_: *mut u8) {
    crate::object::power::power_shutdown(); // -> ! (powers off the machine)
}

pub fn request_shutdown() {
    let _ = crate::work_queue::WORK_QUEUE.push_high(power_shutdown_work, core::ptr::null_mut());
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
