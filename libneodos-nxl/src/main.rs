#![no_std]
#![no_main]

use core::arch::asm;

// ============================================================
// NXL entry point — never actually executed (passive library)
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_entry() -> ! {
    loop { unsafe { asm!("hlt"); } }
}

// ============================================================
// Error constants
// ============================================================
pub const EINVAL: i64 = -1;
pub const ENOENT: i64 = -2;
pub const ENOMEM: i64 = -3;
pub const EACCES: i64 = -4;
pub const EBADF: i64 = -5;
pub const EFAULT: i64 = -6;
pub const ENOSYS: i64 = -7;
pub const EAGAIN: i64 = -8;
pub const EPIPE: i64 = -9;
pub const EEXIST: i64 = -10;
pub const ENOTDIR: i64 = -11;
pub const EISDIR: i64 = -12;
pub const EIO: i64 = -13;
pub const ENODEV: i64 = -14;
pub const EBUSY: i64 = -15;

// ============================================================
// Syscall wrappers (raw inline asm)
// ============================================================
unsafe fn syscall_0(n: u64) -> u64 {
    let r: u64;
    asm!("mov rax, {}", "int 0x80", in(reg) n, out("rax") r);
    r
}

unsafe fn syscall_1(n: u64, a0: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx",
        "mov rax, {n}", "mov rbx, {a0}", "int 0x80",
        "pop rbx",
        n = in(reg) n, a0 = in(reg) a0,
        out("rax") r,
    );
    r
}

unsafe fn syscall_2(n: u64, a0: u64, a1: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "int 0x80",
        "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1,
        out("rax") r,
    );
    r
}

unsafe fn syscall_3(n: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx", "push rdx",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "mov rdx, {a2}", "int 0x80",
        "pop rdx", "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1, a2 = in(reg) a2,
        out("rax") r,
    );
    r
}

unsafe fn syscall_5(n: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx", "push rdx", "push r8", "push r9",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "mov rdx, {a2}",
        "mov r8, {a3}", "mov r9, {a4}", "int 0x80",
        "pop r9", "pop r8", "pop rdx", "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1,
        a2 = in(reg) a2, a3 = in(reg) a3, a4 = in(reg) a4,
        out("rax") r,
    );
    r
}

fn ret(val: u64) -> i64 {
    let signed = val as i64;
    if signed < 0 { signed } else { val as i64 }
}

// ============================================================
// Extern "C" Syscall wrappers
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_sys_exit(code: u32) -> ! {
    unsafe { syscall_1(0, code as u64); }
    loop { unsafe { asm!("hlt"); } }
}

#[no_mangle]
pub extern "C" fn nxl_sys_write(fd: u8, buf: *const u8, len: usize) -> i64 {
    ret(unsafe { syscall_3(1, fd as u64, buf as u64, len as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_read(fd: u8, buf: *mut u8, len: usize) -> i64 {
    ret(unsafe { syscall_3(4, fd as u64, buf as u64, len as u64) })
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
pub extern "C" fn nxl_sys_brk(new_break: u64) -> i64 {
    ret(unsafe { syscall_1(18, new_break) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_mmap(hint: u64, len: u64, prot: u16, flags: u16, file_handle: u64) -> i64 {
    ret(unsafe { syscall_5(19, hint, len, prot as u64, flags as u64, file_handle) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_munmap(addr: u64, len: u64) -> i64 {
    ret(unsafe { syscall_2(20, addr, len) })
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
pub extern "C" fn nxl_sys_getcwd(buf: *mut u8, len: usize) -> i64 {
    ret(unsafe { syscall_2(17, buf as u64, len as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_loadlib(path: *const u8) -> i64 {
    ret(unsafe { syscall_1(21, path as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_getcpuinfo(buf: *mut u8, len: usize) -> i64 {
    ret(unsafe { syscall_2(24, buf as u64, len as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_spawn(path: *const u8, stdin_fd: u8, stdout_fd: u8, stderr_fd: u8) -> i64 {
    ret(unsafe { syscall_5(7, path as u64, stdin_fd as u64, stdout_fd as u64, stderr_fd as u64, 0) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_readdir(fd: u8, buf: *mut u8) -> i64 {
    ret(unsafe { syscall_2(8, fd as u64, buf as u64) })
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

#[no_mangle]
pub extern "C" fn nxl_sys_get_version(buf: *mut u8, len: usize) -> i64 {
    ret(unsafe { syscall_2(43, buf as u64, len as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_get_datetime(buf: *mut u8) -> i64 {
    ret(unsafe { syscall_1(44, buf as u64) })
}

#[no_mangle]
pub extern "C" fn nxl_sys_get_meminfo(buf: *mut u8) -> i64 {
    ret(unsafe { syscall_1(45, buf as u64) })
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
    let new = (current as i64).checked_add(increment).unwrap_or(-1);
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

// ============================================================
// Panic handler (DLL version — prints and exits)
// ============================================================
#[panic_handler]
fn nxl_panic(info: &core::panic::PanicInfo) -> ! {
    let msg = core::format_args!("DLL PANIC: {}\r\n", info.message());
    if let Ok(s) = core::str::from_utf8(msg.as_str().unwrap_or("").as_bytes()) {
        let _ = nxl_sys_write(2, s.as_ptr(), s.len());
    }
    nxl_sys_exit(1)
}

// ============================================================
// Export Table — placed in .export_table section at known offset
// ============================================================
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
    pub nxl_print: extern "C" fn(*const u8, usize),
    pub nxl_eprint: extern "C" fn(*const u8, usize),
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

#[no_mangle]
#[link_section = ".export_table"]
pub static EXPORT_TABLE: AbiTable = AbiTable {
    sys_exit: nxl_sys_exit,
    sys_write: nxl_sys_write,
    sys_read: nxl_sys_read,
    sys_getpid: nxl_sys_getpid,
    sys_yield: nxl_sys_yield,
    sys_open: nxl_sys_open,
    sys_readfile: nxl_sys_readfile,
    sys_writefile: nxl_sys_writefile,
    sys_close: nxl_sys_close,
    sys_brk: nxl_sys_brk,
    sys_mmap: nxl_sys_mmap,
    sys_munmap: nxl_sys_munmap,
    sys_pipe: nxl_sys_pipe,
    sys_dup2: nxl_sys_dup2,
    sys_waitpid: nxl_sys_waitpid,
    stdout_write: nxl_stdout_write,
    stderr_write: nxl_stderr_write,
    stdin_read: nxl_stdin_read,
    nxl_print: nxl_print,
    nxl_eprint: nxl_eprint,
    file_open: nxl_file_open,
    file_read: nxl_file_read,
    file_write: nxl_file_write,
    brk: nxl_brk,
    sbrk: nxl_sbrk,
    mmap: nxl_mmap,
    munmap: nxl_munmap,
    err_einval: EINVAL,
    err_enonet: ENOENT,
    err_enomem: ENOMEM,
    err_eacces: EACCES,
    err_ebadf: EBADF,
    err_efault: EFAULT,
    err_enosys: ENOSYS,
    err_eagain: EAGAIN,
    err_epipe: EPIPE,
    err_eenoent: EEXIST,
    err_enotdir: ENOTDIR,
    err_eisdir: EISDIR,
    err_eio: EIO,
    err_enodev: ENODEV,
    err_ebusy: EBUSY,
    sys_chdir: nxl_sys_chdir,
    sys_chdir_parent: nxl_sys_chdir_parent,
    sys_getcwd: nxl_sys_getcwd,
    sys_loadlib: nxl_sys_loadlib,
    sys_getcpuinfo: nxl_sys_getcpuinfo,
    sys_spawn: nxl_sys_spawn,
    sys_readdir: nxl_sys_readdir,
    sys_mkdir: nxl_sys_mkdir,
    sys_unlink: nxl_sys_unlink,
    sys_rmdir: nxl_sys_rmdir,
    sys_rename: nxl_sys_rename,
    sys_get_version: nxl_sys_get_version,
    sys_get_datetime: nxl_sys_get_datetime,
    sys_get_meminfo: nxl_sys_get_meminfo,
    version: 4,
};
