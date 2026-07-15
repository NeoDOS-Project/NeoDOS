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

const APP_NAME: &str = "corecls";
const IDS_HELP_USAGE: u32 = 1001;
const IDS_HELP_DESC: u32 = 1002;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn print_help() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_HELP_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_HELP_DESC).as_bytes());
    write_str(b"\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }
    write_str(b"\x1b[2J\x1b[H");
    syscall::sys_exit(0)
}
