use crate::input::vt::{VtInputQueue, VtShadowBuffer, ConsoleState, VT_COUNT};
use core::sync::atomic::{AtomicUsize, Ordering};

pub struct InputManager {
    active_vt: AtomicUsize,
    vt_queues: [VtInputQueue; VT_COUNT],
    vt_states: [ConsoleState; VT_COUNT],
    pub vt_shadow: [VtShadowBuffer; VT_COUNT],
    vt_foreground_pid: [u32; VT_COUNT],
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
            vt_foreground_pid: [0; VT_COUNT],
        }
    }

    pub fn active_vt(&self) -> usize {
        self.active_vt.load(Ordering::Relaxed)
    }

    pub fn foreground_pid(&self) -> u32 {
        let vt = self.active_vt();
        self.vt_foreground_pid[vt]
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
