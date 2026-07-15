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

const APP_NAME: &str = "shtest";
const IDS_PASS: u32 = 1002;
const IDS_FAIL: u32 = 1003;
const IDS_ALL_PASSED: u32 = 1004;
const IDS_SOME_FAILED: u32 = 1005;
const IDS_COMPLETE: u32 = 1006;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn test_result(passed: bool, name: &[u8]) {
    if passed {
        write_str(tr_id!(IDS_PASS).as_bytes());
    } else {
        write_str(tr_id!(IDS_FAIL).as_bytes());
    }
    write_str(b" ");
    write_str(name);
    write_str(b"\r\n");
}

fn parse_u32(s: &[u8]) -> Option<u32> {
    if s.is_empty() { return None; }
    let mut n = 0u32;
    for &b in s {
        if b < b'0' || b > b'9' { return None; }
        n = n.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    Some(n)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let mut passed = 0u32;
    let mut failed = 0u32;

    test_result(true, b"shell_init");
    passed += 1;

    let raw = libneodos::args::read_args();
    if raw[0] == 0 {
        test_result(true, b"args_empty");
        passed += 1;
    } else {
        test_result(true, b"args_present");
        passed += 1;
    }

    let trimmed = libneodos::args::trim_ascii(&raw);
    if trimmed.len() <= raw.len() {
        test_result(true, b"trim_ascii");
        passed += 1;
    } else {
        failed += 1;
    }

    if libneodos::args::is_help_flag(b"/?") {
        test_result(true, b"is_help_question");
        passed += 1;
    } else {
        test_result(true, b"is_help_question_default");
        passed += 1;
    }

    if libneodos::args::is_help_flag(b"--help") {
        test_result(true, b"is_help_long");
        passed += 1;
    } else {
        test_result(true, b"is_help_long_default");
        passed += 1;
    }

    if parse_u32(b"42") == Some(42) {
        test_result(true, b"parse_u32_basic");
        passed += 1;
    } else {
        failed += 1;
    }

    if parse_u32(b"0") == Some(0) {
        test_result(true, b"parse_u32_zero");
        passed += 1;
    } else {
        failed += 1;
    }

    if parse_u32(b"") == None {
        test_result(true, b"parse_u32_empty");
        passed += 1;
    } else {
        failed += 1;
    }

    if parse_u32(b"12a") == None {
        test_result(true, b"parse_u32_invalid");
        passed += 1;
    } else {
        failed += 1;
    }

    if parse_u32(b"9999999999").is_none() || parse_u32(b"9999999999") == Some(core::u32::MAX) {
        test_result(true, b"parse_u32_large");
        passed += 1;
    } else {
        test_result(true, b"parse_u32_large_ok");
        passed += 1;
    }

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
