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
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "colors";
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_USAGE_LINE3: u32 = 1003;
const IDS_UNKNOWN_OPT: u32 = 1004;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u32(mut v: u32) {
    if v == 0 { write_str(b"0"); return; }
    let mut buf = [0u8; 10];
    let mut i = 9;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

const COLORS: &[(&[u8], u8)] = &[
    (b"Black on black", 30), (b"Red on black", 31), (b"Green on black", 32), (b"Yellow on black", 33),
    (b"Blue on black", 34), (b"Magenta on black", 35), (b"Cyan on black", 36), (b"White on black", 37),
];

fn print_help() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE3).as_bytes());
    write_str(b"\r\n\r\n");
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

    write_str(b"\r\n");
    for &(name, code) in COLORS.iter() {
        if code > 0 {
            let mut esc = [0u8; 8];
            esc[0] = 0x1b;
            esc[1] = b'[';
            let mut pos = 2;
            let tens = code / 10;
            let ones = code % 10;
            if tens > 0 { esc[pos] = b'0' + tens; pos += 1; }
            esc[pos] = b'0' + ones; pos += 1;
            esc[pos] = b'm'; pos += 1;
            write_str(&esc[..pos]);
        }
        write_str(name);
        write_str(b"\x1b[0m  ");
        if code > 0 {
            write_u32(code as u32);
        }
        write_str(b"\r\n");
    }
    write_str(b"\r\n");
    syscall::sys_exit(0)
}
