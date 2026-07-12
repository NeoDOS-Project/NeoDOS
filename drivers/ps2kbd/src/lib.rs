#![no_std]
#![no_main]
#![allow(dead_code)]
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
static MODS: AtomicU8 = AtomicU8::new(0);
static EXTENDED: AtomicU8 = AtomicU8::new(0);

const EVENT_KEYB_LAYOUT: u32 = 9;

const MOD_SHIFT: u8 = 0x01;
const MOD_CTRL: u8 = 0x02;
const MOD_ALT: u8 = 0x04;
const MOD_CAPS: u8 = 0x08;
const MOD_NUMLOCK: u8 = 0x10;

fn update_mod(scancode: u8, set: bool) {
    let bit = match scancode {
        0x2A | 0x36 => MOD_SHIFT,
        0x1D => MOD_CTRL,
        0x38 => MOD_ALT,
        _ => return,
    };
    if set {
        MODS.fetch_or(bit, Ordering::Relaxed);
    } else {
        MODS.fetch_and(!bit, Ordering::Relaxed);
    }
}

fn toggle_mod(scancode: u8) {
    match scancode {
        0x3A => MODS.fetch_xor(MOD_CAPS, Ordering::Relaxed),
        0x45 => MODS.fetch_xor(MOD_NUMLOCK, Ordering::Relaxed),
        _ => 0,
    };
}

fn process_scancode(scancode: u8) {
    let _extended = EXTENDED.swap(0, Ordering::Relaxed);

    if scancode == 0xE0 {
        EXTENDED.store(1, Ordering::Relaxed);
        return;
    }

    let code = scancode & 0x7F;
    let is_make = (scancode & 0x80) == 0;

    update_mod(code, is_make);

    if is_make && (code == 0x3A || code == 0x45) {
        toggle_mod(code);
    }
}

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }
    INITIALIZED.store(1, Ordering::Release);
    let msg = b"ps2kbd.nem: init OK (NeoKBD mode)\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    ACTIVE.store(1, Ordering::Release);
    let msg = b"ps2kbd.nem: activated\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 || event.is_null() {
        return -1;
    }
    let ev = unsafe { &*event };
    match ev.event_type {
        1 => {
            process_scancode(ev.data0 as u8);
            0
        }
        _ => 1,
    }
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
