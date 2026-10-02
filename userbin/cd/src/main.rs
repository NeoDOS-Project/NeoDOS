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

const APP_NAME: &str = "cd";
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_USAGE_LINE3: u32 = 1003;
const IDS_ERR_NOT_FOUND: u32 = 1004;

/// Shared result buffer read by NeoShell after the process exits.
const ARGS_ADDR: u64 = 0x41F000;

/// Ob path of the per-process current-directory object.
const CWD_OBJ: &str = "\\Global\\Info\\Cwd";

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_err(s: &[u8]) {
    let _ = syscall::sys_write(2, s);
}

/// Publish the resolved directory back to the shell through the shared buffer.
fn write_result(path: &[u8]) {
    unsafe {
        let dst = ARGS_ADDR as *mut u8;
        core::ptr::write_bytes(dst, 0, 256);
        let len = path.len().min(255);
        core::ptr::copy_nonoverlapping(path.as_ptr(), dst, len);
        dst.add(len).write(0);
    }
}

/// Ask the kernel to make `path` this process's working directory.
///
/// This deliberately reuses the single canonicalization/validation layer that
/// lives in the VFS path resolver behind `SET_CWD`: the kernel resolves the
/// path relative to the current directory, canonicalizes `.` / `..`, checks
/// that the target exists and is a directory, and only then commits the state.
/// Doing it this way keeps exactly one canonicalizer in the system instead of
/// a second (and subtly divergent) copy in userland.
fn set_cwd(path: &[u8]) -> Result<(), ()> {
    let mut buf = [0u8; 256];
    let n = path.len().min(255);
    buf[..n].copy_from_slice(&path[..n]);
    // buf[n] stays 0, so the kernel's bounded string copy terminates here.

    let fd = syscall::sys_ob_open(CWD_OBJ, syscall::ob_access::WRITE).map_err(|_| ())?;
    let r = syscall::sys_ob_set_info(fd, syscall::ob_set_info_class::SET_CWD, &buf[..n]);
    let _ = syscall::sys_close(fd);
    r.map_err(|_| ())
}

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
        // No argument: report the current directory through the result buffer.
        let mut cwd_buf = [0u8; 256];
        if let Ok(n) = syscall::sys_getcwd(&mut cwd_buf) {
            write_result(libneodos::args::trim_ascii(&cwd_buf[..n]));
        } else {
            write_result(b"C:\\");
        }
        syscall::sys_exit(0);
    }

    if libneodos::args::is_help_flag(args) {
        print_usage();
        syscall::sys_exit(0);
    }

    let path_args = if args.len() >= 2
        && ((args[0] == b'"' && args[args.len() - 1] == b'"')
            || (args[0] == b'\'' && args[args.len() - 1] == b'\''))
    {
        &args[1..args.len() - 1]
    } else {
        args
    };

    // The kernel canonicalizes + validates + commits atomically on this child.
    if set_cwd(path_args).is_err() {
        write_err(b"\r\n");
        write_err(tr_id!(IDS_ERR_NOT_FOUND).as_bytes());
        write_err(b"\r\n");
        // Empty result tells the shell not to commit anything (cwd unchanged).
        write_result(b"");
        syscall::sys_exit(1);
    }

    // Return the canonical path so the shell can mirror it in its own process.
    let mut cwd_buf = [0u8; 256];
    match syscall::sys_getcwd(&mut cwd_buf) {
        Ok(n) => write_result(libneodos::args::trim_ascii(&cwd_buf[..n])),
        Err(_) => write_result(path_args),
    }
    syscall::sys_exit(0)
}
