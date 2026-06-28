#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, Ordering};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
pub struct NeoEvent {
    pub event_id: u64,
    pub event_type: u32,
    pub source: u32,
    pub timestamp: u64,
    pub device_id: u32,
    pub driver_target: u32,
    pub data0: u64,
    pub data1: u64,
    pub flags: u32,
}

extern "C" {
    fn hst_log(level: u32, msg: *const u8, len: usize);
}

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);
static PACKET_STATE: AtomicU8 = AtomicU8::new(0);
static B0: AtomicU8 = AtomicU8::new(0);
static B1: AtomicU8 = AtomicU8::new(0);
static B2: AtomicU8 = AtomicU8::new(0);

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 { return -1; }
    INITIALIZED.store(1, Ordering::Release);
    let msg = b"ps2mouse.nem: init OK\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 { return -1; }
    ACTIVE.store(1, Ordering::Release);
    let msg = b"ps2mouse.nem: activated\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 || event.is_null() { return -1; }
    let ev = unsafe { &*event };
    if ev.event_type == 16 {
        process_mouse_byte(ev.data0 as u8);
        0
    } else { 1 }
}

#[no_mangle]
pub extern "C" fn driver_fini() {
    ACTIVE.store(0, Ordering::Release);
    INITIALIZED.store(0, Ordering::Release);
}

#[no_mangle]
pub extern "C" fn driver_is_active() -> i32 {
    if ACTIVE.load(Ordering::Relaxed) != 0 { 1 } else { 0 }
}

fn process_mouse_byte(byte: u8) {
    let state = PACKET_STATE.load(Ordering::Relaxed);
    match state {
        0 => B0.store(byte, Ordering::Relaxed),
        1 => B1.store(byte, Ordering::Relaxed),
        _ => {
            B2.store(byte, Ordering::Relaxed);
            let _ = B0.load(Ordering::Relaxed);
            let _ = B1.load(Ordering::Relaxed) as i8;
            let _ = B2.load(Ordering::Relaxed) as i8;
            PACKET_STATE.store(0, Ordering::Relaxed);
            return;
        }
    }
    PACKET_STATE.store(state + 1, Ordering::Relaxed);
}
