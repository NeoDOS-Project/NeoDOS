//! Export table for libneodos NXL at 0x1e000000.
//! Thin-client library calls into the NXL via this ABI table.

pub const NXL_BASE: u64 = 0x1e00_0000;
pub const EXPORT_TABLE_OFFSET: u64 = 0x00;
pub const ABI_VERSION: u32 = 4;

/// Mirrors `AbiTable` from libneodos-nxl
#[repr(C)]
pub struct AbiTable {
    pub sys_exit: extern "C" fn(u32) -> !,
    pub sys_write: extern "C" fn(u8, *const u8, usize) -> i64,
    pub sys_read: extern "C" fn(u8, *mut u8, usize) -> i64,
    pub sys_getpid: extern "C" fn() -> u32,
    pub sys_yield: extern "C" fn(),
    pub sys_open: extern "C" fn(*const u8) -> i64,
    pub sys_readfile: extern "C" fn(u8, *mut u8, usize) -> i64,
    pub sys_writefile: extern "C" fn(u8, *const u8, usize) -> i64,
    pub sys_close: extern "C" fn(u8) -> i64,
    pub sys_brk: extern "C" fn(u64) -> i64,
    pub sys_mmap: extern "C" fn(u64, u64, u16, u16, u64) -> i64,
    pub sys_munmap: extern "C" fn(u64, u64) -> i64,
    pub sys_pipe: extern "C" fn(*mut u64) -> i64,
    pub sys_dup2: extern "C" fn(u8, u8) -> i64,
    pub sys_waitpid: extern "C" fn(u32) -> i64,
    pub stdout_write: extern "C" fn(*const u8, usize) -> i64,
    pub stderr_write: extern "C" fn(*const u8, usize) -> i64,
    pub stdin_read: extern "C" fn(*mut u8, usize) -> i64,
    pub dll_print: extern "C" fn(*const u8, usize),
    pub dll_eprint: extern "C" fn(*const u8, usize),
    pub file_open: extern "C" fn(*const u8) -> i64,
    pub file_read: extern "C" fn(u8, *mut u8, usize) -> i64,
    pub file_write: extern "C" fn(u8, *const u8, usize) -> i64,
    pub brk: extern "C" fn(u64) -> i64,
    pub sbrk: extern "C" fn(i64) -> i64,
    pub mmap: extern "C" fn(u64, u16, u16) -> i64,
    pub munmap: extern "C" fn(u64, u64) -> i64,
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
    pub sys_chdir: extern "C" fn(*const u8) -> i64,
    pub sys_chdir_parent: extern "C" fn(*const u8) -> i64,
    pub sys_getcwd: extern "C" fn(*mut u8, usize) -> i64,
    pub sys_loadlib: extern "C" fn(*const u8) -> i64,
    pub sys_getcpuinfo: extern "C" fn(*mut u8, usize) -> i64,
    pub sys_spawn: extern "C" fn(*const u8, u8, u8, u8) -> i64,
    pub sys_readdir: extern "C" fn(u8, *mut u8) -> i64,
    pub sys_mkdir: extern "C" fn(*const u8) -> i64,
    pub sys_unlink: extern "C" fn(*const u8) -> i64,
    pub sys_rmdir: extern "C" fn(*const u8) -> i64,
    pub sys_rename: extern "C" fn(*const u8, *const u8) -> i64,
    pub sys_get_version: extern "C" fn(*mut u8, usize) -> i64,
    pub sys_get_datetime: extern "C" fn(*mut u8) -> i64,
    pub sys_get_meminfo: extern "C" fn(*mut u8) -> i64,
    pub version: u32,
}

/// Get a reference to the DLL export table
pub fn get_table() -> &'static AbiTable {
    unsafe { &*((NXL_BASE + EXPORT_TABLE_OFFSET) as *const AbiTable) }
}
