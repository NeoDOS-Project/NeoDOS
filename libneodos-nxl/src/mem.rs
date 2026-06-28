use crate::error::{ret, EINVAL};
use crate::syscall::syscall_1;

// ============================================================
// Raw syscall wrappers — Memory domain
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_sys_brk(new_break: u64) -> i64 {
    ret(unsafe { syscall_1(18, new_break) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_mmap(hint: u64, len: u64, prot: u16, flags: u16, file_handle: u64) -> i64 {
    ret(unsafe { crate::syscall::syscall_5(19, hint, len, prot as u64, flags as u64, file_handle) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_munmap(addr: u64, len: u64) -> i64 {
    ret(unsafe { crate::syscall::syscall_2(20, addr, len) })
}

// ============================================================
// Mem: brk, sbrk, mmap, munmap
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_brk(new_break: u64) -> i64 {
    nxl_sys_brk(new_break)
}

#[no_mangle]
pub extern "C" fn nxl_sbrk(increment: i64) -> i64 {
    let current = nxl_sys_brk(0);
    if current < 0 { return current; }
    if increment == 0 { return current; }
    let new = current.checked_add(increment).unwrap_or(-1);
    if new < 0 { return EINVAL; }
    let new = new as u64;
    let result = nxl_sys_brk(new);
    if result < 0 { return result; }
    current
}

#[no_mangle]
pub extern "C" fn nxl_mmap(len: u64, prot: u16, flags: u16) -> i64 {
    nxl_sys_mmap(0, len, prot, flags, 0)
}

#[no_mangle]
pub extern "C" fn nxl_munmap(addr: u64, len: u64) -> i64 {
    nxl_sys_munmap(addr, len)
}
