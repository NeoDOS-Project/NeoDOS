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
use libneodos::syscall::ObInfoClass;
use libneodos::tr_id;

const APP_NAME: &str = "ver";
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_FALLBACK: u32 = 1003;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn print_help() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
    write_str(b"\r\n\r\n");
}

fn get_version_via_ob(buf: &mut [u8]) -> Result<usize, i64> {
    let fd = syscall::sys_ob_open("\\Global\\Info\\Version", libneodos::syscall::ob_access::READ)?;
    let n = syscall::sys_ob_query_info(fd, ObInfoClass::Version, buf)?;
    let _ = syscall::sys_close(fd);
    let end = buf.iter().position(|&b| b == 0).unwrap_or(n);
    Ok(end)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }
    let mut buf = [0u8; 256];
    match get_version_via_ob(&mut buf) {
        Ok(n) if n > 0 => {
            write_str(b"\r\n");
            write_str(&buf[..n]);
            write_str(b"\r\n\r\n");
        }
        _ => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_FALLBACK).as_bytes());
            write_str(b"\r\n\r\n");
        }
    }
    syscall::sys_exit(0)
}
