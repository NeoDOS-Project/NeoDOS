use crate::error::ret;
use crate::syscall::syscall_3;

// ============================================================
// Raw syscall wrappers — IO domain
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_sys_write(fd: u8, buf: *const u8, len: usize) -> i64 {
    ret(unsafe { syscall_3(20, fd as u64, buf as u64, len as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_read(fd: u8, buf: *mut u8, len: usize) -> i64 {
    ret(unsafe { syscall_3(21, fd as u64, buf as u64, len as u64) })
}

// ============================================================
// IO: stdout, stdin, stderr, _print, _eprint
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_stdout_write(buf: *const u8, len: usize) -> i64 {
    nxl_sys_write(1, buf, len)
}

#[no_mangle]
pub extern "C" fn nxl_stderr_write(buf: *const u8, len: usize) -> i64 {
    nxl_sys_write(2, buf, len)
}

#[no_mangle]
pub extern "C" fn nxl_stdin_read(buf: *mut u8, len: usize) -> i64 {
    nxl_sys_read(0, buf, len)
}

#[no_mangle]
pub extern "C" fn nxl_print(fmt_ptr: *const u8, fmt_len: usize) {
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(fmt_ptr, fmt_len)) };
    let _ = nxl_sys_write(1, s.as_ptr(), s.len());
}

#[no_mangle]
pub extern "C" fn nxl_eprint(fmt_ptr: *const u8, fmt_len: usize) {
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(fmt_ptr, fmt_len)) };
    let _ = nxl_sys_write(2, s.as_ptr(), s.len());
}
