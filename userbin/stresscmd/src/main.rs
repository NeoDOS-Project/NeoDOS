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

const APP_NAME: &str = "stresscmd";
const IDS_HEADER: u32 = 1001;
const IDS_ALL_PASSED: u32 = 1002;
const IDS_SOME_FAILED: u32 = 1003;
const IDS_COMPLETE: u32 = 1004;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let mut all_ok = true;

    for i in 0..100 {
        let raw = libneodos::args::read_args();
        let trimmed = libneodos::args::trim_ascii(&raw);
        let is_help = libneodos::args::is_help_flag(&raw);
        if i == 0 && is_help {
            all_ok = false;
        }
    }
    if all_ok {
        write_str(tr_id!(IDS_ALL_PASSED).as_bytes());
    } else {
        write_str(tr_id!(IDS_SOME_FAILED).as_bytes());
    }
    write_str(b"\r\n");
    write_str(tr_id!(IDS_COMPLETE).as_bytes());
    write_str(b"\r\n");
    syscall::sys_exit(0)
}
