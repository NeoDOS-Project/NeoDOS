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

const APP_NAME: &str = "coremd";
const IDS_USAGE: u32 = 1004;
const IDS_USAGE_LINE2: u32 = 1005;
const IDS_USAGE_LINE3: u32 = 1006;
const IDS_ERR_CANNOT_CREATE: u32 = 1007;
const IDS_ERR_EINVAL: u32 = 1008;
const IDS_ERR_ENOENT: u32 = 1009;
const IDS_ERR_EACCES: u32 = 1010;
const IDS_ERR_EEXIST: u32 = 1011;
const IDS_ERR_EIO: u32 = 1012;
const IDS_ERR_UNKNOWN: u32 = 1013;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

use libneoutil::to_ob_path;

use libneodos::path::normalize_path_required as normalize_path;

fn print_usage() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE3).as_bytes());
    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw_args = libneodos::args::read_args();
    let args = libneodos::args::trim_ascii(&raw_args);

    if args.is_empty() {
        print_usage();
        syscall::sys_exit(0);
    }

    if libneodos::args::is_help_flag(args) {
        print_usage();
        syscall::sys_exit(0);
    }

    let normalized = normalize_path(args);
    let end = normalized.iter().position(|&b| b == 0).unwrap_or(normalized.len());
    let path = core::str::from_utf8(&normalized[..end]).unwrap_or("C:\\");

    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(path, &mut ob_buf);
    match syscall::sys_ob_create(ob_path, 11, None, 0) {
        Ok(_) => {
            write_str(b"\r\n");
            syscall::sys_exit(0);
        }
        Err(e) => {
            write_err(b"\r\n");
            write_err(tr_id!(IDS_ERR_CANNOT_CREATE).as_bytes());
            let err_str: &[u8] = match e {
                -1 => tr_id!(IDS_ERR_EINVAL).as_bytes(),
                -2 => tr_id!(IDS_ERR_ENOENT).as_bytes(),
                -4 => tr_id!(IDS_ERR_EACCES).as_bytes(),
                -10 => tr_id!(IDS_ERR_EEXIST).as_bytes(),
                -13 => tr_id!(IDS_ERR_EIO).as_bytes(),
                _ => tr_id!(IDS_ERR_UNKNOWN).as_bytes(),
            };
            write_err(err_str);
            write_err(b"\r\n");
            syscall::sys_exit(1);
        }
    }
}
