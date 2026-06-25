use core::arch::asm;

use crate::error::ret;
use crate::syscall::{syscall_0, syscall_1, syscall_2, syscall_5};

// ============================================================
// Raw syscall wrappers — Process domain
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_sys_exit(code: u32) -> ! {
    unsafe { syscall_1(0, code as u64); }
    loop { unsafe { asm!("hlt"); } }
}

#[no_mangle]
pub extern "C" fn nxl_sys_getpid() -> u32 {
    unsafe { syscall_0(3) as u32 }
}

#[no_mangle]
pub extern "C" fn nxl_sys_yield() {
    unsafe { syscall_0(2); }
}

#[no_mangle]
pub extern "C" fn nxl_sys_pipe(fds: *mut u64) -> i64 {
    ret(unsafe { syscall_1(5, fds as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_dup2(old_fd: u8, new_fd: u8) -> i64 {
    ret(unsafe { syscall_2(6, old_fd as u64, new_fd as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_waitpid(pid: u32) -> i64 {
    ret(unsafe { syscall_1(9, pid as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_chdir(path: *const u8) -> i64 {
    ret(unsafe { syscall_1(16, path as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_chdir_parent(path: *const u8) -> i64 {
    ret(unsafe { syscall_1(47, path as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_loadlib(path: *const u8) -> i64 {
    ret(unsafe { syscall_1(21, path as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_spawn(path: *const u8, stdin_fd: u8, stdout_fd: u8, stderr_fd: u8) -> i64 {
    ret(unsafe { syscall_5(7, path as u64, stdin_fd as u64, stdout_fd as u64, stderr_fd as u64, 0) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_readdir(fd: u8, buf: *mut u8) -> i64 {
    ret(unsafe { syscall_2(8, fd as u64, buf as u64) })
}
