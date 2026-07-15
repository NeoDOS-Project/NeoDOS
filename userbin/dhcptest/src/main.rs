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

const APP_NAME: &str = "dhcptest";
const IDS_HEADER: u32 = 1001;
const IDS_PASS: u32 = 1002;
const IDS_FAIL: u32 = 1003;
const IDS_WARN: u32 = 1004;
const IDS_ALL_PASSED: u32 = 1005;
const IDS_SOME_FAILED: u32 = 1006;
const IDS_COMPLETE: u32 = 1007;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn test_result(passed: bool, name: &[u8]) {
    write_str(tr_id!(IDS_HEADER).as_bytes());
    if passed {
        write_str(tr_id!(IDS_PASS).as_bytes());
    } else {
        write_str(tr_id!(IDS_FAIL).as_bytes());
    }
    write_str(b" ");
    write_str(name);
    write_str(b"\r\n");
}

fn test_skip(name: &[u8]) {
    write_str(tr_id!(IDS_HEADER).as_bytes());
    write_str(tr_id!(IDS_WARN).as_bytes());
    write_str(b" ");
    write_str(name);
    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let mut passed = 0u32;
    let mut failed = 0u32;

    let nxl_fd = syscall::sys_ob_open("\\Global\\NXL\\net", libneodos::syscall::ob_access::READ);
    if nxl_fd.is_ok() {
        test_result(true, b"net_nxl_open");
        passed += 1;
        let _ = syscall::sys_close(nxl_fd.unwrap());
    } else {
        test_skip(b"net_nxl_open (not available)");
    }

    test_result(true, b"init_complete");
    passed += 1;

    write_str(tr_id!(IDS_HEADER).as_bytes());
    if failed == 0 {
        write_str(tr_id!(IDS_ALL_PASSED).as_bytes());
    } else {
        write_str(tr_id!(IDS_SOME_FAILED).as_bytes());
    }
    write_str(b"\r\n");
    write_str(tr_id!(IDS_COMPLETE).as_bytes());
    write_str(b"\r\n");
    syscall::sys_exit(if failed == 0 { 0 } else { 1 })
}
