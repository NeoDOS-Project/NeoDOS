use crate::error::ret;
use crate::syscall::{syscall_1, syscall_2, syscall_3, syscall_4};

// ============================================================
// Raw syscall wrappers — Filesystem domain
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_sys_open(path: *const u8) -> i64 {
    ret(unsafe { syscall_2(10, path as u64, 0) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_readfile(fd: u8, buf: *mut u8, len: usize) -> i64 {
    ret(unsafe { syscall_3(11, fd as u64, buf as u64, len as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_writefile(fd: u8, buf: *const u8, len: usize) -> i64 {
    ret(unsafe { syscall_3(12, fd as u64, buf as u64, len as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_close(fd: u8) -> i64 {
    ret(unsafe { syscall_1(13, fd as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_mkdir(path: *const u8) -> i64 {
    ret(unsafe { syscall_1(25, path as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_unlink(path: *const u8) -> i64 {
    ret(unsafe { syscall_1(26, path as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_rmdir(path: *const u8) -> i64 {
    ret(unsafe { syscall_1(27, path as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_rename(old_path: *const u8, new_path: *const u8) -> i64 {
    ret(unsafe { syscall_2(28, old_path as u64, new_path as u64) })
}

// ============================================================
// Object Manager (Ob) wrappers — RAX 60–65
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_sys_ob_open(path: *const u8, access_mask: u32) -> i64 {
    ret(unsafe { syscall_2(60, path as u64, access_mask as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_ob_create(path: *const u8, obj_type: u32, fds_out: *mut u64, attrs: u64) -> i64 {
    ret(unsafe { syscall_4(61, path as u64, obj_type as u64, fds_out as u64, attrs) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_ob_query_info(fd: u8, info_class: u32, buf: *mut u8, buf_size: usize) -> i64 {
    ret(unsafe { syscall_4(62, fd as u64, info_class as u64, buf as u64, buf_size as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_ob_set_info(fd: u8, info_class: u32, buf: *const u8, buf_size: usize) -> i64 {
    ret(unsafe { syscall_4(63, fd as u64, info_class as u64, buf as u64, buf_size as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_ob_enum(dir_fd: u8, buf: *mut u8, max_entries: usize) -> i64 {
    ret(unsafe { syscall_3(64, dir_fd as u64, buf as u64, max_entries as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_ob_wait(fd: u8) -> i64 {
    let handles: [u64; 1] = [fd as u64];
    ret(unsafe { syscall_3(65, 1, &handles as *const u64 as u64, 0) })
}

// ============================================================
// FS: File open/read/write
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_file_open(path: *const u8) -> i64 {
    nxl_sys_open(path)
}

#[no_mangle]
pub extern "C" fn nxl_file_read(fd: u8, buf: *mut u8, len: usize) -> i64 {
    nxl_sys_readfile(fd, buf, len)
}

#[no_mangle]
pub extern "C" fn nxl_file_write(fd: u8, buf: *const u8, len: usize) -> i64 {
    nxl_sys_writefile(fd, buf, len)
}
