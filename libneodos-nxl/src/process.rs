use core::arch::asm;

use crate::error::ret;
use crate::syscall::{syscall_0, syscall_1};

// ============================================================
// Raw syscall wrappers — Process domain
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_sys_exit(code: u32) -> ! {
    unsafe { syscall_1(0, code as u64); }
    loop {}
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
pub extern "C" fn nxl_sys_loadlib(path: *const u8) -> i64 {
    ret(unsafe { syscall_1(21, path as u64) })
}


