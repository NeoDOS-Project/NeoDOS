#![no_std]
#![no_main]
#![allow(dead_code)]

use core::sync::atomic::{AtomicU8, Ordering};
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

// ── NeoEvent structure (matches kernel's eventbus::Event) ──

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

// ── HAL functions imported from kernel ──

extern "C" {
    fn hst_push_input_byte(byte: u8);
    fn hst_log(level: u32, msg: *const u8, len: usize);
}

// ── Driver state ──

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);
static MODS: AtomicU8 = AtomicU8::new(0);
static EXTENDED: AtomicU8 = AtomicU8::new(0);
static DEAD_KEY: AtomicU8 = AtomicU8::new(0);
static OUTPUT_PENDING0: AtomicU8 = AtomicU8::new(0);
static OUTPUT_PENDING1: AtomicU8 = AtomicU8::new(0);

const MOD_SHIFT: u8 = 0x01;
const MOD_CTRL: u8 = 0x02;
const MOD_ALT: u8 = 0x04;
const MOD_CAPS: u8 = 0x08;
const MOD_NUMLOCK: u8 = 0x10;

// ── US Keyboard layout tables ──

static US_NORMAL: [u8; 128] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, b'`',  0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, b'q', b'1',  0x00,
    0x00, 0x00, b'z', b's', b'a', b'w', b'2',  0x00,
    0x00, b'c', b'x', b'd', b'e', b'4', b'3',  0x00,
    0x00, b' ', b'v', b'f', b't', b'r', b'5',  0x00,
    0x00, b'n', b'b', b'h', b'g', b'y', b'6',  0x00,
    0x00, 0x00, b'm', b'j', b'u', b'7', b'8',  0x00,
    0x00, b',', b'k', b'i', b'o', b'0', b'9',  0x00,
    0x00, b'.', b'/', b'l', b';', b'p', b'-',  0x00,
    0x00, 0x00, b'\'', 0x00, b'[', b'=',  0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

static US_SHIFT: [u8; 128] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, b'~',  0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, b'Q', b'!',  0x00,
    0x00, 0x00, b'Z', b'S', b'A', b'W', b'@',  0x00,
    0x00, b'C', b'X', b'D', b'E', b'$', b'#',  0x00,
    0x00, b' ', b'V', b'F', b'T', b'R', b'%',  0x00,
    0x00, b'N', b'B', b'H', b'G', b'Y', b'^',  0x00,
    0x00, 0x00, b'M', b'J', b'U', b'&', b'*',  0x00,
    0x00, b'<', b'K', b'I', b'O', b')', b'(',  0x00,
    0x00, b'>', b'?', b'L', b':', b'P', b'_',  0x00,
    0x00, 0x00, b'"',  0x00, b'{', b'+',  0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

// ── Entry points ──

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }
    INITIALIZED.store(1, Ordering::Release);

    let msg = b"ps2kbd.nem: init OK\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()); }

    0
}

#[no_mangle]
pub extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    if event.is_null() {
        return -1;
    }

    let ev = unsafe { &*event };

    if ev.event_type != 1 {
        return 1;
    }

    let scancode = ev.data0 as u8;
    process_scancode(scancode);
    0
}

#[no_mangle]
pub extern "C" fn driver_fini() {
    ACTIVE.store(0, Ordering::Release);
    INITIALIZED.store(0, Ordering::Release);
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    ACTIVE.store(1, Ordering::Release);

    let msg = b"ps2kbd.nem: activated\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()); }

    0
}

#[no_mangle]
pub extern "C" fn driver_is_active() -> i32 {
    if ACTIVE.load(Ordering::Relaxed) != 0 { 1 } else { 0 }
}

// ── Scan code processing ──

fn process_scancode(scancode: u8) {
    let b0 = OUTPUT_PENDING0.swap(0, Ordering::Relaxed);
    if b0 != 0 {
        unsafe { hst_push_input_byte(b0); }
        let b1 = OUTPUT_PENDING1.swap(0, Ordering::Relaxed);
        if b1 != 0 {
            unsafe { hst_push_input_byte(b1); }
        }
        return;
    }

    if let Some(ascii) = translate_scancode(scancode) {
        unsafe { hst_push_input_byte(ascii); }
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
        0x3A => { MODS.fetch_xor(MOD_CAPS, Ordering::Relaxed); }
        0x45 => { MODS.fetch_xor(MOD_NUMLOCK, Ordering::Relaxed); }
        _ => {}
    }
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
            _ => None,
        };
    }

    if code >= 128 {
        return None;
    }

    let ch = if (mods & MOD_SHIFT) != 0 {
        US_SHIFT[code as usize]
    } else {
        US_NORMAL[code as usize]
    };

    if ch == 0 {
        return None;
    }

    if (mods & MOD_CAPS) != 0 {
        if ch.is_ascii_lowercase() {
            return Some(ch - 32);
        } else if ch.is_ascii_uppercase() {
            return Some(ch + 32);
        }
    }

    Some(ch)
}
