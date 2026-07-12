#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::syscall;
use libneodos::keyboard;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

#[used]
#[link_section = ".rodata"]
static NEOKEY_HELP: &[u8] = b"::HELP::\
NEOKEY show\r\n\
  Show current keyboard layout, modifiers, and LED state.\r\n\
NEOKEY layout <name>\r\n\
  Change keyboard layout by name (e.g. Spanish, US).\r\n\
NEOKEY layouts\r\n\
  List all available keyboard layouts.\r\n\
NEOKEY repeat <cps>\r\n\
  Set repeat rate (2-60 characters per second).\r\n\
NEOKEY delay <ms>\r\n\
  Set repeat delay (100-2000 milliseconds).\r\n\
NEOKEY leds\r\n\
  Show current LED state.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nNEOKEY commands:\r\n");
    write_str(b"  show              Show current layout, modifiers, LEDs\r\n");
    write_str(b"  layout <name>     Change layout (e.g. Spanish, US)\r\n");
    write_str(b"  layouts           List available layouts\r\n");
    write_str(b"  repeat <cps>      Set repeat rate (2-60 cps)\r\n");
    write_str(b"  delay <ms>        Set repeat delay (100-2000 ms)\r\n");
    write_str(b"  leds              Show LED states\r\n\r\n");
}

fn push_str(buf: &mut [u8], pos: &mut usize, s: &[u8]) {
    if *pos + s.len() <= buf.len() {
        buf[*pos..*pos + s.len()].copy_from_slice(s);
        *pos += s.len();
    }
}

fn format_modifiers_buf(mods: u8, buf: &mut [u8]) -> usize {
    let mut pos = 0;
    if mods & keyboard::KBD_SHIFT != 0 { push_str(buf, &mut pos, b"Shift "); }
    if mods & keyboard::KBD_CTRL != 0 { push_str(buf, &mut pos, b"Ctrl "); }
    if mods & keyboard::KBD_ALT != 0 { push_str(buf, &mut pos, b"Alt "); }
    if mods & keyboard::KBD_ALTGR != 0 { push_str(buf, &mut pos, b"AltGr "); }
    if mods & keyboard::KBD_CAPS != 0 { push_str(buf, &mut pos, b"Caps "); }
    if mods & keyboard::KBD_NUMLOCK != 0 { push_str(buf, &mut pos, b"Num "); }
    if mods & keyboard::KBD_SCROLLLOCK != 0 { push_str(buf, &mut pos, b"Scr "); }
    if pos == 0 { push_str(buf, &mut pos, b"none"); }
    pos
}

fn format_leds_buf(leds: u8, buf: &mut [u8]) -> usize {
    let mut pos = 0;
    if leds & 0x04 != 0 { push_str(buf, &mut pos, b"CapsLock "); }
    if leds & 0x02 != 0 { push_str(buf, &mut pos, b"NumLock "); }
    if leds & 0x01 != 0 { push_str(buf, &mut pos, b"ScrollLock "); }
    if pos == 0 { push_str(buf, &mut pos, b"none"); }
    pos
}

fn cmd_show() {
    match keyboard::kbd_get_state() {
        Ok(state) => {
            let mut mod_buf = [0u8; 64];
            let mod_len = format_modifiers_buf(state.modifiers, &mut mod_buf);
            let mut led_buf = [0u8; 64];
            let led_len = format_leds_buf(state.leds, &mut led_buf);
            write_str(b"\r\nKeyboard state:\r\n");
            write_str(b"  Modifiers: ");
            write_str(&mod_buf[..mod_len]);
            write_str(b"\r\n");
            write_str(b"  LEDs: ");
            write_str(&led_buf[..led_len]);
            write_str(b"\r\n");
        }
        Err(_) => {
            write_err(b"\r\nError reading keyboard state.\r\n");
        }
    }

    match keyboard::kbd_get_layout() {
        Ok(name) => {
            let end = name.iter().position(|&b| b == 0).unwrap_or(32);
            write_str(b"  Active layout: ");
            write_str(&name[..end]);
            write_str(b"\r\n");
        }
        Err(_) => {
            write_err(b"\r\nError reading layout name.\r\n");
        }
    }
}

fn cmd_layout(name: &str) {
    match keyboard::kbd_set_layout(name) {
        Ok(()) => {
            write_str(b"\r\nKeyboard layout changed to ");
            write_str(name.as_bytes());
            write_str(b".\r\n");
        }
        Err(_) => {
            write_err(b"\r\nError: layout '");
            write_err(name.as_bytes());
            write_err(b"' not found.\r\n");
        }
    }
}

fn cmd_layouts() {
    match keyboard::kbd_list_layouts() {
        Ok(layouts) => {
            write_str(b"\r\nAvailable keyboard layouts:\r\n");
            for info in &layouts {
                let name_end = info.name.iter().position(|&b| b == 0).unwrap_or(32);
                let tag_end = info.lang_tag.iter().position(|&b| b == 0).unwrap_or(16);
                write_str(b"  ");
                write_str(&info.name[..name_end]);
                write_str(b" (");
                write_str(&info.lang_tag[..tag_end]);
                write_str(b")\r\n");
            }
            write_str(b"\r\n");
        }
        Err(_) => {
            write_err(b"\r\nError listing layouts.\r\n");
        }
    }
}

fn cmd_repeat(cps_str: &[u8]) {
    let cps = parse_u32(cps_str);
    if cps < 2 || cps > 60 {
        write_err(b"\r\nInvalid rate. Use 2-60 cps.\r\n");
        return;
    }
    match keyboard::kbd_get_repeat() {
        Ok((delay, _)) => {
            match keyboard::kbd_set_repeat(delay, cps) {
                Ok(()) => {
                    write_str(b"\r\nRepeat rate set to ");
                    write_str(cps_str);
                    write_str(b" cps.\r\n");
                }
                Err(_) => {
                    write_err(b"\r\nError setting repeat rate.\r\n");
                }
            }
        }
        Err(_) => {
            write_err(b"\r\nError reading current repeat config.\r\n");
        }
    }
}

fn cmd_delay(ms_str: &[u8]) {
    let ms = parse_u32(ms_str);
    if ms < 100 || ms > 2000 {
        write_err(b"\r\nInvalid delay. Use 100-2000 ms.\r\n");
        return;
    }
    match keyboard::kbd_get_repeat() {
        Ok((_, rate)) => {
            match keyboard::kbd_set_repeat(ms, rate) {
                Ok(()) => {
                    write_str(b"\r\nRepeat delay set to ");
                    write_str(ms_str);
                    write_str(b" ms.\r\n");
                }
                Err(_) => {
                    write_err(b"\r\nError setting repeat delay.\r\n");
                }
            }
        }
        Err(_) => {
            write_err(b"\r\nError reading current repeat config.\r\n");
        }
    }
}

fn cmd_leds() {
    match keyboard::kbd_get_state() {
        Ok(state) => {
            let mut led_buf = [0u8; 64];
            let led_len = format_leds_buf(state.leds, &mut led_buf);
            write_str(b"\r\nLED state: ");
            write_str(&led_buf[..led_len]);
            write_str(b"\r\n");
        }
        Err(_) => {
            write_err(b"\r\nError reading LED state.\r\n");
        }
    }
}

fn parse_u32(s: &[u8]) -> u32 {
    let mut n: u32 = 0;
    for &b in s {
        if b < b'0' || b > b'9' { break; }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    n
}

fn args_to_slice(buf: &[u8; 256]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    &buf[..end]
}

fn skip_whitespace(s: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
        i += 1;
    }
    &s[i..]
}

fn split_first_word(s: &[u8]) -> (&[u8], &[u8]) {
    let s = skip_whitespace(s);
    if s.is_empty() { return (s, &[]); }
    let mut i = 0;
    while i < s.len() && s[i] != b' ' && s[i] != b'\t' {
        i += 1;
    }
    (&s[..i], skip_whitespace(&s[i..]))
}

fn str_from_utf8(s: &[u8]) -> &str {
    core::str::from_utf8(s).unwrap_or("")
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }
    let args = args_to_slice(&raw);
    if args.is_empty() {
        print_help();
        syscall::sys_exit(0);
    }

    let (cmd, rest) = split_first_word(args);

    match str_from_utf8(cmd) {
        "show" => cmd_show(),
        "layout" => {
            let (name, _) = split_first_word(rest);
            if name.is_empty() {
                write_err(b"\r\nUsage: NEOKEY layout <name>\r\n");
            } else {
                cmd_layout(str_from_utf8(name));
            }
        }
        "layouts" => cmd_layouts(),
        "repeat" => {
            let (cps_str, _) = split_first_word(rest);
            if cps_str.is_empty() {
                write_err(b"\r\nUsage: NEOKEY repeat <cps>\r\n");
            } else {
                cmd_repeat(cps_str);
            }
        }
        "delay" => {
            let (ms_str, _) = split_first_word(rest);
            if ms_str.is_empty() {
                write_err(b"\r\nUsage: NEOKEY delay <ms>\r\n");
            } else {
                cmd_delay(ms_str);
            }
        }
        "leds" => cmd_leds(),
        _ => {
            write_err(b"\r\nUnknown command.\r\n");
            print_help();
        }
    }

    syscall::sys_exit(0)
}
