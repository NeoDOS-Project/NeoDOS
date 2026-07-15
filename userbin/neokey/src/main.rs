#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::i18n;
use libneodos::keyboard;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "neokey";
const IDS_KBD_STATE: u32 = 1005;
const IDS_MODIFIERS: u32 = 1006;
const IDS_LEDS: u32 = 1007;
const IDS_ACTIVE_LAYOUT: u32 = 1008;
const IDS_AVAIL_LAYOUTS: u32 = 1009;
const IDS_UNKNOWN_CMD: u32 = 1010;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

fn print_help() {
    write_str(b"\r\nNEOKEY [option]\r\n  Display keyboard state and available layouts.\r\n  NEOKEY              shows keyboard state\r\n  NEOKEY --list       lists available layouts\r\n\r\n");
}

fn format_bits(val: u8, names: &[(&[u8], u8)]) {
    let mut first = true;
    for (name, bit) in names {
        if val & bit != 0 {
            if !first { write_str(b" "); }
            write_str(name);
            first = false;
        }
    }
    if first { write_str(b"(none)"); }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }

    let args = libneodos::args::trim_ascii(&raw);

    if !args.is_empty() {
        let arg_str = core::str::from_utf8(args).unwrap_or("");
        if arg_str.eq_ignore_ascii_case("--list") || arg_str.eq_ignore_ascii_case("-l") {
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
                }
                Err(_) => {
                    write_err(b"\r\nError reading layout list\r\n");
                }
            }
            syscall::sys_exit(0);
        }

        write_err(b"\r\n");
        write_err(tr_id!(IDS_UNKNOWN_CMD).as_bytes());
        write_err(b"\r\n");
        syscall::sys_exit(1);
    }

    match keyboard::kbd_get_state() {
        Ok(state) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_KBD_STATE).as_bytes());
            write_str(b"\r\n");
            write_str(b"  ");
            write_str(tr_id!(IDS_MODIFIERS).as_bytes());
            write_str(b" ");
            format_bits(state.modifiers, &[
                (b"LCTRL", 1), (b"LSHIFT", 2), (b"LALT", 4), (b"LGUI", 8),
                (b"RCTRL", 16), (b"RSHIFT", 32), (b"RALT", 64), (b"RGUI", 128),
            ]);
            write_str(b"\r\n");

            write_str(b"  ");
            write_str(tr_id!(IDS_LEDS).as_bytes());
            write_str(b" ");
            format_bits(state.leds, &[
                (b"NumLock", 1), (b"CapsLock", 2), (b"ScrollLock", 4),
            ]);
            write_str(b"\r\n");
        }
        Err(_) => {
            write_err(b"\r\nCannot read keyboard state\r\n");
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
        Err(_) => {}
    }

    write_str(b"\r\n");
    syscall::sys_exit(0)
}
