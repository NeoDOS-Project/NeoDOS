#![no_std]
#![no_main]
#![allow(dead_code)]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, Ordering};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

pub mod klc_layout {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/kbd_layout.rs"));
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
    fn hst_push_input_byte(byte: u8);
    fn hst_log(level: u32, msg: *const u8, len: usize);
}

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);
static MODS: AtomicU8 = AtomicU8::new(0);
static LAYOUT: AtomicU8 = AtomicU8::new(1); // SP default
static EXTENDED: AtomicU8 = AtomicU8::new(0);
static DEAD_KEY: AtomicU8 = AtomicU8::new(0);
static OUTPUT_PENDING0: AtomicU8 = AtomicU8::new(0);
static OUTPUT_PENDING1: AtomicU8 = AtomicU8::new(0);

const EVENT_KEYB_LAYOUT: u32 = 9;

const MOD_SHIFT: u8 = 0x01;
const MOD_CTRL: u8 = 0x02;
const MOD_ALT: u8 = 0x04;
const MOD_CAPS: u8 = 0x08;
const MOD_NUMLOCK: u8 = 0x10;

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }
    INITIALIZED.store(1, Ordering::Release);
    let msg = b"ps2kbd.nem: init OK\r\n";
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
pub extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 || event.is_null() {
        return -1;
    }
    let ev = unsafe { &*event };
    match ev.event_type {
        1 => {
            process_scancode(ev.data0 as u8);
            0
        }
        EVENT_KEYB_LAYOUT => {
            LAYOUT.store(if ev.data0 == 0 { 0 } else { 1 }, Ordering::Relaxed);
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

fn process_scancode(scancode: u8) {
    let b0 = OUTPUT_PENDING0.swap(0, Ordering::Relaxed);
    if b0 != 0 {
        unsafe { hst_push_input_byte(b0) };
        let b1 = OUTPUT_PENDING1.swap(0, Ordering::Relaxed);
        if b1 != 0 {
            unsafe { hst_push_input_byte(b1) };
        }
        return;
    }

    if let Some(ascii) = translate_scancode(scancode) {
        unsafe { hst_push_input_byte(ascii) };
    }
}

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

fn translate_scancode(scancode: u8) -> Option<u8> {
    let extended = EXTENDED.swap(0, Ordering::Relaxed);

    if scancode == 0xE0 {
        EXTENDED.store(1, Ordering::Relaxed);
        return None;
    }

    let released = (scancode & 0x80) != 0;
    let code = scancode & 0x7F;

    if released {
        update_mod(code, false);
        return None;
    }

    update_mod(code, true);

    if code == 0x3A || code == 0x45 {
        toggle_mod(code);
    }

    let mods = MODS.load(Ordering::Relaxed);

    if extended != 0 {
        return match code {
            0x35 => Some(b'/'),
            0x1C => Some(b'\n'),
            0x48 => Some(0x01),
            0x50 => Some(0x02),
            0x4B => Some(0x08),
            0x4D => Some(0x09),
            _ => {
                if code == 0x38 {
                    MODS.fetch_or(MOD_ALT | MOD_CTRL, Ordering::Relaxed);
                }
                None
            }
        };
    }

    let layout = LAYOUT.load(Ordering::Relaxed);
    let normal = if layout == 0 { &klc_layout::KBDUS_NORMAL } else { &klc_layout::KBDSP_NORMAL };
    let shift = if layout == 0 { &klc_layout::KBDUS_SHIFT } else { &klc_layout::KBDSP_SHIFT };
    let altgr = if layout == 0 { &klc_layout::KBDUS_ALTGR } else { &klc_layout::KBDSP_ALTGR };
    let normal_dead = if layout == 0 { &klc_layout::KBDUS_NORMAL_DEAD } else { &klc_layout::KBDSP_NORMAL_DEAD };
    let shift_dead = if layout == 0 { &klc_layout::KBDUS_SHIFT_DEAD } else { &klc_layout::KBDSP_SHIFT_DEAD };
    let altgr_dead = if layout == 0 { &klc_layout::KBDUS_ALTGR_DEAD } else { &klc_layout::KBDSP_ALTGR_DEAD };

    let idx = code as usize;
    let (raw, is_dead) = if (mods & MOD_ALT) != 0 && (mods & MOD_CTRL) != 0 {
        (altgr[idx], altgr_dead[idx] != 0)
    } else if (mods & MOD_SHIFT) != 0 {
        (shift[idx], shift_dead[idx] != 0)
    } else {
        (normal[idx], normal_dead[idx] != 0)
    };

    let unicode = raw as u16;
    let dead_key = DEAD_KEY.load(Ordering::Relaxed);

    if dead_key != 0 && is_dead {
        let compose_driver: &[u8] = if layout == 0 { &klc_layout::KBDUS_COMPOSE_DEAD } else { &klc_layout::KBDSP_COMPOSE_DEAD };
        let compose_base: &[u8] = if layout == 0 { &klc_layout::KBDUS_COMPOSE_BASE } else { &klc_layout::KBDSP_COMPOSE_BASE };
        let compose_result: &[u8] = if layout == 0 { &klc_layout::KBDUS_COMPOSE_RESULT } else { &klc_layout::KBDSP_COMPOSE_RESULT };
        for i in 0..compose_driver.len() {
            if compose_driver[i] == dead_key && compose_base[i] as u16 == unicode {
                DEAD_KEY.store(0, Ordering::Relaxed);
                return Some(encode_utf8_first(compose_result[i] as u16));
            }
        }
        DEAD_KEY.store(0, Ordering::Relaxed);
        return Some(b'?');
    }

    if is_dead {
        DEAD_KEY.store(raw as u8, Ordering::Relaxed);
        return None;
    }

    if dead_key != 0 {
        DEAD_KEY.store(0, Ordering::Relaxed);
        return Some(b'?');
    }

    if unicode < 0x80 {
        let ch = unicode as u8;
        if (mods & MOD_CAPS) != 0 {
            return Some(if ch.is_ascii_lowercase() {
                ch - 32
            } else if ch.is_ascii_uppercase() {
                ch + 32
            } else {
                ch
            });
        }
        return Some(ch);
    }

    Some(encode_utf8_first(unicode))
}

fn encode_utf8_first(codepoint: u16) -> u8 {
    if codepoint < 0x80 {
        return codepoint as u8;
    }
    if codepoint < 0x800 {
        let b0 = 0xC0 | ((codepoint >> 6) as u8);
        let b1 = 0x80 | (codepoint as u8 & 0x3F);
        OUTPUT_PENDING0.store(b1, Ordering::Relaxed);
        return b0;
    }
    let b0 = 0xE0 | ((codepoint >> 12) as u8);
    let b1 = 0x80 | ((codepoint >> 6) as u8 & 0x3F);
    let b2 = 0x80 | (codepoint as u8 & 0x3F);
    OUTPUT_PENDING0.store(b1, Ordering::Relaxed);
    OUTPUT_PENDING1.store(b2, Ordering::Relaxed);
    b0
}
