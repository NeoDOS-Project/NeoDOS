#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::console;
use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "progress";
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_USAGE_LINE3: u32 = 1003;
const IDS_TITLE: u32 = 1004;
const IDS_DONE: u32 = 1005;
const IDS_COMPLETED: u32 = 1006;
const IDS_ALL_DONE: u32 = 1007;
const IDS_SPINNER: u32 = 1008;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn spin_delay() {
    for _ in 0..3000000 { core::hint::spin_loop(); }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        write_str(b"\r\n");
        write_str(tr_id!(IDS_USAGE).as_bytes());
        write_str(b"\r\n");
        write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
        write_str(b"\r\n");
        write_str(tr_id!(IDS_USAGE_LINE3).as_bytes());
        write_str(b"\r\n\r\n");
        syscall::sys_exit(0);
    }

    let mut nbars = 3usize;
    let arg_slice = libneodos::args::trim_ascii(&raw);
    if !arg_slice.is_empty() {
        if let Ok(v) = core::str::from_utf8(arg_slice).unwrap_or("").parse::<usize>() {
            if v >= 1 && v <= 8 { nbars = v; }
        }
    }

    write_str(b"\r\n");
    write_str(tr_id!(IDS_TITLE).as_bytes());
    write_str(b"\r\n\r\n");

    for bar in 0..nbars {
        let label = [b"A", b"B", b"C", b"D", b"E", b"F", b"G", b"H"][bar];
        let title_buf = [label[0], 0u8];
        let title = core::str::from_utf8(&title_buf[..1]).unwrap_or("?");
        let id = console::progress_begin(title, 100);
        if id < 0 { continue; }
        for p in 0..=100 {
            console::progress_update(id, p);
            spin_delay();
        }
        console::progress_finish(id);
        write_str(tr_id!(IDS_DONE).as_bytes());
        write_str(b"\r\n");
    }

    write_str(b"\r\n");
    console::spinner_begin(tr_id!(IDS_SPINNER));
    for _ in 0..20 {
        console::spinner_update();
        spin_delay();
    }
    console::spinner_finish();
    write_str(b" [OK]\r\n");

    write_str(b"\r\n");
    write_str(tr_id!(IDS_ALL_DONE).as_bytes());
    write_str(b"\r\n\r\n");
    syscall::sys_exit(0)
}
