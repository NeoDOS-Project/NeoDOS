use core::sync::atomic::{AtomicU8, Ordering};
use x86_64::instructions::port::Port;

mod klc_layout {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/kbd_layout.rs"));
}

pub struct KeyboardDriver;

static MODS: AtomicU8 = AtomicU8::new(0);
static LAYOUT: AtomicU8 = AtomicU8::new(1);
static EXTENDED: AtomicU8 = AtomicU8::new(0);
static DEAD_KEY: AtomicU8 = AtomicU8::new(0);
static OUTPUT_PENDING: [AtomicU8; 2] = [AtomicU8::new(0), AtomicU8::new(0)];

const PS2_TIMEOUT: u32 = 100_000;

/// Wait for the PS/2 controller input buffer to be empty (bit 1 of status = 0).
/// This means the controller is ready to accept a command.
/// Returns false if the controller didn't become ready before the timeout.
fn ps2_wait_input() -> bool {
    let mut status = Port::new(0x64u16);
    for _ in 0..PS2_TIMEOUT {
        unsafe {
            let s: u8 = status.read();
            if (s & 0x02) == 0 {
                return true;
            }
        }
    }
    false
}

/// Wait for the PS/2 controller output buffer to be full (bit 0 = 1).
/// Returns false if the controller didn't produce data before the timeout.
fn ps2_wait_output() -> bool {
    let mut status = Port::new(0x64u16);
    for _ in 0..PS2_TIMEOUT {
        unsafe {
            let s: u8 = status.read();
            if (s & 0x01) != 0 {
                return true;
            }
        }
    }
    false
}

/// Flush any stale data from the PS/2 output buffer.
fn ps2_flush_output() {
    let mut status = Port::new(0x64u16);
    let mut data = Port::new(0x60u16);
    for _ in 0..PS2_TIMEOUT {
        unsafe {
            let s: u8 = status.read();
            if (s & 0x01) == 0 {
                break;
            }
            let _: u8 = data.read();
        }
    }
}

/// Initialize the PS/2 controller (8042) for keyboard operation.
///
/// Must be called **before** enabling interrupts so the keyboard
/// is ready to assert IRQ1.
pub fn init_ps2() {
    let mut cmd = Port::new(0x64u16);
    let mut data = Port::new(0x60u16);

    unsafe {
        // 1. Disable keyboard port
        if !ps2_wait_input() { return; }
        cmd.write(0xADu8);

        // 2. Flush any stale data
        ps2_flush_output();

        // 3. Read configuration byte
        if !ps2_wait_input() { return; }
        cmd.write(0x20u8);
        if !ps2_wait_output() { return; }
        let config: u8 = data.read();

        // 4. Set bit 0  = enable keyboard interrupt
        //    Clear bit 4 = enable keyboard clock (don't inhibit)
        let new_config = (config | 0x01) & !0x10;

        // 5. Write configuration byte
        if !ps2_wait_input() { return; }
        cmd.write(0x60u8);
        if !ps2_wait_input() { return; }
        data.write(new_config);

        // 6. Enable keyboard port
        if !ps2_wait_input() { return; }
        cmd.write(0xAEu8);

        // 7. Send "Enable Scanning" command (0xF4) to the keyboard
        if !ps2_wait_input() { return; }
        data.write(0xF4u8);

        // 8. Read ACK (0xFA) — optional; keyboard will send it on success.
        //    Timeout prevents hang if no PS/2 keyboard is connected.
        if ps2_wait_output() {
            let _ack: u8 = data.read();
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KeyboardLayout {
    Us = 0,
    Sp = 1,
}

impl KeyboardDriver {
    const MOD_SHIFT: u8 = 1 << 0;
    const MOD_CTRL: u8 = 1 << 1;
    const MOD_ALT: u8 = 1 << 2;
    const MOD_CAPS: u8 = 1 << 3;
    const MOD_NUMLOCK: u8 = 1 << 4;

    pub fn read_scancode() -> Option<u8> {
        let mut status_port = Port::new(0x64u16);
        let mut data_port = Port::new(0x60u16);
        unsafe {
            let status: u8 = status_port.read();
            if (status & 0x01) != 0 {
                let scancode: u8 = data_port.read();
                return Some(scancode);
            }
        }
        None
    }

    fn drain_output() -> Option<u8> {
        let b0 = OUTPUT_PENDING[0].swap(0, Ordering::Relaxed);
        if b0 == 0 {
            return None;
        }
        OUTPUT_PENDING[0].store(OUTPUT_PENDING[1].swap(0, Ordering::Relaxed), Ordering::Relaxed);
        Some(b0)
    }

    fn queue_output(bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        OUTPUT_PENDING[0].store(bytes[0], Ordering::Relaxed);
        if bytes.len() > 1 {
            OUTPUT_PENDING[1].store(bytes[1], Ordering::Relaxed);
        }
    }

    pub fn codepoint_to_utf8(cp: u32) -> [u8; 3] {
        if cp < 0x80 {
            [cp as u8, 0, 0]
        } else if cp < 0x800 {
            [0xC0 | (cp >> 6) as u8, 0x80 | (cp & 0x3F) as u8, 0]
        } else {
            [0xE0 | (cp >> 12) as u8, 0x80 | ((cp >> 6) & 0x3F) as u8, 0x80 | (cp & 0x3F) as u8]
        }
    }

    pub fn scancode_to_ascii(scancode: u8) -> Option<u8> {
        if scancode == 0xE0 {
            EXTENDED.store(1, Ordering::Relaxed);
            return None;
        }

        if let Some(b) = Self::drain_output() {
            return Some(b);
        }

        let extended = EXTENDED.swap(0, Ordering::Relaxed) != 0;
        let released = (scancode & 0x80) != 0;
        let code = (scancode & 0x7F) as usize;

        match code as u8 {
            0x2A | 0x36 => {
                Self::update_mod(Self::MOD_SHIFT, !released);
                return None;
            }
            0x1D => {
                Self::update_mod(Self::MOD_CTRL, !released);
                return None;
            }
            0x38 => {
                if extended {
                    Self::update_mod(Self::MOD_CTRL, !released);
                    Self::update_mod(Self::MOD_ALT, !released);
                } else {
                    Self::update_mod(Self::MOD_ALT, !released);
                }
                return None;
            }
            0x3A => {
                if !released {
                    Self::toggle_mod(Self::MOD_CAPS);
                }
                return None;
            }
            0x45 if extended => {
                if !released {
                    Self::toggle_mod(Self::MOD_NUMLOCK);
                }
                return None;
            }
            _ => {}
        }

        if released {
            return None;
        }

        // Handle extended (E0-prefixed) keys
        if extended {
            match code as u8 {
                0x35 => return Some(b'/'),
                0x1C => return Some(b'\n'),
                0x37 | 0x47..=0x53 => return None,
                _ => {}
            }
        }

        let mods = MODS.load(Ordering::Relaxed);
        let shift_down = (mods & Self::MOD_SHIFT) != 0;
        let caps_on = (mods & Self::MOD_CAPS) != 0;
        let ctrl_down = (mods & Self::MOD_CTRL) != 0;
        let alt_down = (mods & Self::MOD_ALT) != 0;
        let altgr_down = ctrl_down && alt_down;
        let numlock_on = (mods & Self::MOD_NUMLOCK) != 0;

        // Non-extended numpad keys only produce output when NumLock is ON
        match code as u8 {
            0x37 | 0x47..=0x53 if !numlock_on => return None,
            _ => {}
        }

        let layout = LAYOUT.load(Ordering::Relaxed);

        let (normal, shift, altgr, normal_dead, shift_dead, altgr_dead) = match layout {
            x if x == KeyboardLayout::Us as u8 => (
                &klc_layout::KBDUS_NORMAL,
                &klc_layout::KBDUS_SHIFT,
                &klc_layout::KBDUS_ALTGR,
                &klc_layout::KBDUS_NORMAL_DEAD,
                &klc_layout::KBDUS_SHIFT_DEAD,
                &klc_layout::KBDUS_ALTGR_DEAD,
            ),
            _ => (
                &klc_layout::KBDSP_NORMAL,
                &klc_layout::KBDSP_SHIFT,
                &klc_layout::KBDSP_ALTGR,
                &klc_layout::KBDSP_NORMAL_DEAD,
                &klc_layout::KBDSP_SHIFT_DEAD,
                &klc_layout::KBDSP_ALTGR_DEAD,
            ),
        };

        let is_dead = if altgr_down {
            altgr_dead[code] != 0
        } else if shift_down {
            shift_dead[code] != 0
        } else {
            normal_dead[code] != 0
        };

        let mut cp = if altgr_down {
            altgr[code]
        } else if shift_down {
            shift[code]
        } else {
            normal[code]
        };

        if cp == 0 {
            return None;
        }

        if caps_on && cp <= 0x7F {
            let b = cp as u8;
            if b.is_ascii_alphabetic() {
                cp = if b.is_ascii_lowercase() {
                    b.to_ascii_uppercase() as u16
                } else {
                    b.to_ascii_lowercase() as u16
                };
            }
        }

        if is_dead {
            DEAD_KEY.store(cp as u8, Ordering::Relaxed);
            return None;
        }

        let dead = DEAD_KEY.swap(0, Ordering::Relaxed);
        if dead != 0 {
            if let Some(composed) = Self::lookup_compose(layout, dead, cp as u8) {
                cp = composed as u16;
            } else {
                Self::queue_output(&Self::codepoint_to_utf8(cp as u32));
                return Self::codepoint_to_utf8(dead as u32).first().copied();
            }
        }

        let utf8 = Self::codepoint_to_utf8(cp as u32);
        Self::queue_output(&utf8[1..]);
        Some(utf8[0])
    }

    pub fn lookup_compose(layout: u8, dead: u8, base: u8) -> Option<u8> {
        let (dead_arr, base_arr, result_arr) = match layout {
            x if x == KeyboardLayout::Us as u8 => (
                &klc_layout::KBDUS_COMPOSE_DEAD[..],
                &klc_layout::KBDUS_COMPOSE_BASE[..],
                &klc_layout::KBDUS_COMPOSE_RESULT[..],
            ),
            _ => (
                &klc_layout::KBDSP_COMPOSE_DEAD[..],
                &klc_layout::KBDSP_COMPOSE_BASE[..],
                &klc_layout::KBDSP_COMPOSE_RESULT[..],
            ),
        };
        for i in 0..dead_arr.len() {
            if dead_arr[i] == dead && base_arr[i] == base {
                return Some(result_arr[i]);
            }
        }
        None
    }

    pub fn set_layout(layout: KeyboardLayout) {
        LAYOUT.store(layout as u8, Ordering::Relaxed);
        DEAD_KEY.store(0, Ordering::Relaxed);
        OUTPUT_PENDING[0].store(0, Ordering::Relaxed);
        OUTPUT_PENDING[1].store(0, Ordering::Relaxed);
    }

    pub fn layout() -> KeyboardLayout {
        match LAYOUT.load(Ordering::Relaxed) {
            x if x == KeyboardLayout::Us as u8 => KeyboardLayout::Us,
            _ => KeyboardLayout::Sp,
        }
    }

    #[inline(always)]
    fn update_mod(mask: u8, pressed: bool) {
        if pressed {
            MODS.fetch_or(mask, Ordering::Relaxed);
        } else {
            MODS.fetch_and(!mask, Ordering::Relaxed);
        }
    }

    #[inline(always)]
    fn toggle_mod(mask: u8) {
        let mut old = MODS.load(Ordering::Relaxed);
        loop {
            let new = old ^ mask;
            match MODS.compare_exchange_weak(old, new, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(v) => old = v,
            }
        }
    }
}
