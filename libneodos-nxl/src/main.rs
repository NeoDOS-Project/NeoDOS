#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

mod error;
mod syscall;
mod io;
mod fs;
mod process;
mod mem;
mod info;


// ============================================================
// NXL entry point — never actually executed (passive library)
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_entry() -> ! {
    loop { unsafe { core::arch::asm!("hlt"); } }
}

// ============================================================
// Panic handler (DLL version — prints and exits)
// ============================================================
#[panic_handler]
fn nxl_panic(info: &core::panic::PanicInfo) -> ! {
    let msg = core::format_args!("DLL PANIC: {}\r\n", info.message());
    if let Ok(s) = core::str::from_utf8(msg.as_str().unwrap_or("").as_bytes()) {
        let _ = crate::io::nxl_sys_write(2, s.as_ptr(), s.len());
    }
    crate::process::nxl_sys_exit(1)
}

// ============================================================
// Export Table v7 — Ob-based ABI, removed legacy dead entries
// ============================================================
#[repr(C)]
pub struct AbiTable {
    // Core syscall wrappers
    pub sys_exit: extern "C" fn(u32) -> !,
    pub sys_write: extern "C" fn(u8, *const u8, usize) -> i64,
    pub sys_read: extern "C" fn(u8, *mut u8, usize) -> i64,
    pub sys_getpid: extern "C" fn() -> u32,
    pub sys_yield: extern "C" fn(),
    pub sys_close: extern "C" fn(u8) -> i64,
    pub sys_brk: extern "C" fn(u64) -> i64,
    pub sys_mmap: extern "C" fn(u64, u64, u16, u16, u64) -> i64,
    pub sys_munmap: extern "C" fn(u64, u64) -> i64,
    // I/O helpers
    pub stdout_write: extern "C" fn(*const u8, usize) -> i64,
    pub stderr_write: extern "C" fn(*const u8, usize) -> i64,
    pub stdin_read: extern "C" fn(*mut u8, usize) -> i64,
    pub nxl_print: extern "C" fn(*const u8, usize),
    pub nxl_eprint: extern "C" fn(*const u8, usize),
    // Ob-based file I/O (replaces legacy RAX 12/25-28)
    pub file_open: extern "C" fn(*const u8) -> i64,
    pub file_read: extern "C" fn(u8, *mut u8, usize) -> i64,
    pub file_write: extern "C" fn(u8, *const u8, usize) -> i64,
    // Memory helpers
    pub brk: extern "C" fn(u64) -> i64,
    pub sbrk: extern "C" fn(i64) -> i64,
    pub mmap: extern "C" fn(u64, u16, u16) -> i64,
    pub munmap: extern "C" fn(u64, u64) -> i64,
    // Error constants
    pub err_einval: i64,
    pub err_enonet: i64,
    pub err_enomem: i64,
    pub err_eacces: i64,
    pub err_ebadf: i64,
    pub err_efault: i64,
    pub err_enosys: i64,
    pub err_eagain: i64,
    pub err_epipe: i64,
    pub err_eenoent: i64,
    pub err_enotdir: i64,
    pub err_eisdir: i64,
    pub err_eio: i64,
    pub err_enodev: i64,
    pub err_ebusy: i64,
    // Process / library
    pub sys_loadlib: extern "C" fn(*const u8) -> i64,
    // Object Manager (Ob) API — RAX 60–66
    pub sys_ob_open: extern "C" fn(*const u8, u32) -> i64,
    pub sys_ob_create: extern "C" fn(*const u8, u32, *mut u64, u64) -> i64,
    pub sys_ob_query_info: extern "C" fn(u8, u32, *mut u8, usize) -> i64,
    pub sys_ob_set_info: extern "C" fn(u8, u32, *const u8, usize) -> i64,
    pub sys_ob_enum: extern "C" fn(u8, *mut u8, usize) -> i64,
    pub sys_ob_wait: extern "C" fn(u8) -> i64,
    pub version: u32,
}

#[no_mangle]
#[link_section = ".export_table"]
pub static EXPORT_TABLE: AbiTable = AbiTable {
    sys_exit: crate::process::nxl_sys_exit,
    sys_write: crate::io::nxl_sys_write,
    sys_read: crate::io::nxl_sys_read,
    sys_getpid: crate::process::nxl_sys_getpid,
    sys_yield: crate::process::nxl_sys_yield,
    sys_close: crate::fs::nxl_sys_close,
    sys_brk: crate::mem::nxl_sys_brk,
    sys_mmap: crate::mem::nxl_sys_mmap,
    sys_munmap: crate::mem::nxl_sys_munmap,
    stdout_write: crate::io::nxl_stdout_write,
    stderr_write: crate::io::nxl_stderr_write,
    stdin_read: crate::io::nxl_stdin_read,
    nxl_print: crate::io::nxl_print,
    nxl_eprint: crate::io::nxl_eprint,
    file_open: crate::fs::nxl_file_open,
    file_read: crate::fs::nxl_file_read,
    file_write: crate::fs::nxl_file_write,
    brk: crate::mem::nxl_brk,
    sbrk: crate::mem::nxl_sbrk,
    mmap: crate::mem::nxl_mmap,
    munmap: crate::mem::nxl_munmap,
    err_einval: crate::error::EINVAL,
    err_enonet: crate::error::ENOENT,
    err_enomem: crate::error::ENOMEM,
    err_eacces: crate::error::EACCES,
    err_ebadf: crate::error::EBADF,
    err_efault: crate::error::EFAULT,
    err_enosys: crate::error::ENOSYS,
    err_eagain: crate::error::EAGAIN,
    err_epipe: crate::error::EPIPE,
    err_eenoent: crate::error::EEXIST,
    err_enotdir: crate::error::ENOTDIR,
    err_eisdir: crate::error::EISDIR,
    err_eio: crate::error::EIO,
    err_enodev: crate::error::ENODEV,
    err_ebusy: crate::error::EBUSY,
    sys_loadlib: crate::process::nxl_sys_loadlib,
    sys_ob_open: crate::fs::nxl_sys_ob_open,
    sys_ob_create: crate::fs::nxl_sys_ob_create,
    sys_ob_query_info: crate::fs::nxl_sys_ob_query_info,
    sys_ob_set_info: crate::fs::nxl_sys_ob_set_info,
    sys_ob_enum: crate::fs::nxl_sys_ob_enum,
    sys_ob_wait: crate::fs::nxl_sys_ob_wait,
    version: 7,
};
