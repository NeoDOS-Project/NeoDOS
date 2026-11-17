//! NEOKEY commands: read/print state and apply keyboard configuration.

use libneodos::keyboard;
use libneodos::tr_id;

use crate::output::{format_bits, write_err, write_str, write_u32};
use crate::{
    IDS_ACTIVE_LAYOUT, IDS_AVAIL_LAYOUTS, IDS_CPS, IDS_DELAY, IDS_DELAY_SET, IDS_ERR_SET,
    IDS_ERR_SET_LAYOUT, IDS_INVALID_VALUE, IDS_KBD_STATE, IDS_LAYOUT_SET, IDS_LEDS, IDS_LEDS_SET,
    IDS_MISSING_LAYOUT, IDS_MODIFIERS, IDS_MS, IDS_REPEAT, IDS_REPEAT_SET, IDS_UNKNOWN_CMD,
    IDS_USAGE, IDS_USAGE_LINE2, IDS_USAGE_LINE3, IDS_USAGE_LINE4, IDS_USAGE_LINE5, IDS_USAGE_LINE6,
    IDS_USAGE_LINE7, IDS_USAGE_LINE8,
};

const MODIFIERS: &[(&[u8], u8)] = &[
    (b"LCTRL", 1), (b"LSHIFT", 2), (b"LALT", 4), (b"LGUI", 8),
    (b"RCTRL", 16), (b"RSHIFT", 32), (b"RALT", 64), (b"RGUI", 128),
];

const LEDS: &[(&[u8], u8)] = &[
    (b"NumLock", 1), (b"CapsLock", 2), (b"ScrollLock", 4),
];

pub fn print_help() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE3).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE4).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE5).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE6).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE7).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE8).as_bytes());
    write_str(b"\r\n\r\n");
}

/// Print modifier/LED state, active layout and repeat config. The repeat values
/// come from NeoKBD, which persists them in the Registry
/// (`\Registry\Machine\System\Keyboard`).
pub fn show_state() -> u32 {
    let mut code = 0;

    match keyboard::kbd_get_state() {
        Ok(state) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_KBD_STATE).as_bytes());
            write_str(b"\r\n");
            write_str(b"  ");
            write_str(tr_id!(IDS_MODIFIERS).as_bytes());
            write_str(b" ");
            format_bits(state.modifiers, MODIFIERS);
            write_str(b"\r\n");

            write_str(b"  ");
            write_str(tr_id!(IDS_LEDS).as_bytes());
            write_str(b" ");
            format_bits(state.leds, LEDS);
            write_str(b"\r\n");
        }
        Err(_) => {
            write_err(b"\r\nCannot read keyboard state\r\n");
            code = 1;
        }
    }

    match keyboard::kbd_get_layout() {
        Ok(layout) => {
            let end = layout.iter().position(|&b| b == 0).unwrap_or(32);
            write_str(b"  ");
            write_str(tr_id!(IDS_ACTIVE_LAYOUT).as_bytes());
            write_str(&layout[..end]);
            write_str(b"\r\n");
        }
        Err(_) => {
            write_err(b"\r\nError: cannot retrieve keyboard layout\r\n");
            code = 1;
        }
    }

    if let Ok((delay, rate)) = keyboard::kbd_get_repeat() {
        write_str(b"  ");
        write_str(tr_id!(IDS_REPEAT).as_bytes());
        write_str(b" ");
        write_u32(rate);
        write_str(tr_id!(IDS_CPS).as_bytes());
        write_str(b"\r\n");

        write_str(b"  ");
        write_str(tr_id!(IDS_DELAY).as_bytes());
        write_str(b" ");
        write_u32(delay);
        write_str(tr_id!(IDS_MS).as_bytes());
        write_str(b"\r\n");
    }

    write_str(b"\r\n");
    code
}

/// List the layouts NeoKBD has loaded, with their language tags.
pub fn list_layouts() -> u32 {
    match keyboard::kbd_list_layouts() {
        Ok(list) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_AVAIL_LAYOUTS).as_bytes());
            write_str(b"\r\n");
            for info in &list {
                write_str(b"  ");
                write_str(info.name_str().as_bytes());
                write_str(b" (");
                write_str(info.lang_tag_str().as_bytes());
                write_str(b")\r\n");
            }
            write_str(b"\r\n");
            0
        }
        Err(_) => {
            write_err(b"\r\nError reading layout list\r\n");
            1
        }
    }
}

/// Switch the active layout by name (e.g. `es-ES`, `en-US`).
pub fn set_layout(name: &str) -> u32 {
    match keyboard::kbd_set_layout(name) {
        Ok(()) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_LAYOUT_SET).as_bytes());
            write_str(name.as_bytes());
            write_str(b"\r\n\r\n");
            0
        }
        Err(_) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_SET_LAYOUT).as_bytes());
            write_err(b"\r\n\r\n");
            1
        }
    }
}

/// Set the repeat rate, preserving the stored delay (Registry).
pub fn set_repeat_rate(cps: u32) -> u32 {
    let (delay, _) = match keyboard::kbd_get_repeat() {
        Ok(v) => v,
        Err(_) => return err_set(),
    };
    match keyboard::kbd_set_repeat(delay, cps) {
        Ok(()) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_REPEAT_SET).as_bytes());
            write_u32(cps);
            write_str(tr_id!(IDS_CPS).as_bytes());
            write_str(b"\r\n\r\n");
            0
        }
        Err(_) => err_set(),
    }
}

/// Set the repeat delay, preserving the stored rate (Registry).
pub fn set_repeat_delay(ms: u32) -> u32 {
    let (_, rate) = match keyboard::kbd_get_repeat() {
        Ok(v) => v,
        Err(_) => return err_set(),
    };
    match keyboard::kbd_set_repeat(ms, rate) {
        Ok(()) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_DELAY_SET).as_bytes());
            write_u32(ms);
            write_str(tr_id!(IDS_MS).as_bytes());
            write_str(b"\r\n\r\n");
            0
        }
        Err(_) => err_set(),
    }
}

/// Set the keyboard LEDs to exactly the given mask.
pub fn set_leds(bits: u8) -> u32 {
    match keyboard::kbd_set_leds(bits) {
        Ok(()) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_LEDS_SET).as_bytes());
            format_bits(bits, LEDS);
            write_str(b"\r\n\r\n");
            0
        }
        Err(_) => err_set(),
    }
}

pub fn missing_layout() -> u32 { err_line(IDS_MISSING_LAYOUT) }
pub fn invalid_value() -> u32 { err_line(IDS_INVALID_VALUE) }
pub fn unknown_command() -> u32 { err_line(IDS_UNKNOWN_CMD) }

fn err_set() -> u32 { err_line(IDS_ERR_SET) }

/// Print a translated error line on stderr and return exit code 1.
fn err_line(id: u32) -> u32 {
    write_err(b"\r\n");
    write_err(tr_id!(id).as_bytes());
    write_err(b"\r\n\r\n");
    1
}
