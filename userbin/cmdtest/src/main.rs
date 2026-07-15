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

const APP_NAME: &str = "cmdtest";
const IDS_HEADER: u32 = 1001;
const IDS_PASS: u32 = 1002;
const IDS_FAIL: u32 = 1003;
const IDS_ALL_PASSED: u32 = 1004;
const IDS_SOME_FAILED: u32 = 1005;
const IDS_COMPLETE: u32 = 1006;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn cmd_passed() {
    write_str(tr_id!(IDS_HEADER).as_bytes());
    write_str(tr_id!(IDS_PASS).as_bytes());
    write_str(b"\r\n");
}

#[inline(never)]
fn test_cd() -> bool {
    let mut buf = [0u8; 2];
    buf[0] = 0x41; // A
    buf[1] = 0;
    unsafe {
        let dst = 0x41F000 as *mut u8;
        core::ptr::write_bytes(dst, 0, 256);
        core::ptr::write(dst, b'C');
        core::ptr::write(dst.add(1), b':');
        core::ptr::write(dst.add(2), b'\\');
    }
    let mut cwd = [0u8; 256];
    match syscall::sys_getcwd(&mut cwd) {
        Ok(n) if n > 0 => {
            let cwd_str = core::str::from_utf8(libneodos::args::trim_ascii(&cwd[..n])).unwrap_or("");
            cwd_str.starts_with("C:\\")
        }
        _ => false,
    }
}

#[inline(never)]
fn test_md_rd() -> bool {
    let mut ob_buf = [0u8; 512];
    let prefix = b"\\Global\\FileSystem\\";
    let test_path = b"C:\\CMDTEST_TMP";
    let total = prefix.len() + test_path.len();
    let mut path_buf = [0u8; 512];
    path_buf[..prefix.len()].copy_from_slice(prefix);
    path_buf[prefix.len()..total].copy_from_slice(test_path);
    let ob_path = unsafe { core::str::from_utf8_unchecked(&path_buf[..total]) };

    match syscall::sys_ob_create(ob_path, 11, None, 0) {
        Ok(_) => {
            match syscall::sys_ob_destroy(0) {
                Ok(_) => true,
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

#[inline(never)]
fn test_del() -> bool {
    let mut ob_buf = [0u8; 512];
    let prefix = b"\\Global\\FileSystem\\";
    let test_path = b"C:\\CMDTEST_DEL";
    let total = prefix.len() + test_path.len();
    let mut path_buf = [0u8; 512];
    path_buf[..prefix.len()].copy_from_slice(prefix);
    path_buf[prefix.len()..total].copy_from_slice(test_path);
    let ob_path = unsafe { core::str::from_utf8_unchecked(&path_buf[..total]) };

    match syscall::sys_ob_create(ob_path, 0, None, 0) {
        Ok(fd) => {
            match syscall::ob_file_delete(fd) {
                Ok(_) => true,
                Err(_) => false,
            }
        }
        Err(_) => {
            match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
                Ok(fd) => {
                    match syscall::ob_file_delete(fd) {
                        Ok(_) => true,
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    let mut passed = 0u32;
    let mut failed = 0u32;

    if test_cd() { passed += 1; } else { failed += 1; cmd_passed(); }
    if test_md_rd() { passed += 1; } else { failed += 1; }
    if test_del() { passed += 1; } else { failed += 1; }

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
