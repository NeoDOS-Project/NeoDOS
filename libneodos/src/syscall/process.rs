//! Process, memory, descriptor and scheduling syscall wrappers.

use crate::export;
use super::{ret, ret_unit, EINVAL};
use super::{sys_ob_open, sys_ob_query_info, ObInfoClass, ob_access};

pub fn sys_exit(code: u32) -> ! {
    (export::get_table().sys_exit)(code)
}

pub fn sys_write(fd: u8, buf: &[u8]) -> Result<usize, i64> {
    let ptr = buf.as_ptr();
    let len = buf.len();
    ret((export::get_table().sys_write)(fd, ptr, len)).map(|v| v as usize)
}

pub fn sys_yield() {
    (export::get_table().sys_yield)()
}

pub fn sys_getpid() -> u32 {
    // Use Ob API: open \Global\Info\Process, query ProcessId, close
    let fd = match sys_ob_open("\\Global\\Info\\Process", ob_access::READ) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut pid = [0u8; 4];
    let r = sys_ob_query_info(fd, ObInfoClass::ProcessId, &mut pid);
    let _ = sys_close(fd);
    match r {
        Ok(_) => u32::from_le_bytes(pid),
        Err(_) => 0,
    }
}

pub fn sys_read(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr();
    let len = buf.len();
    ret((export::get_table().sys_read)(fd, ptr, len)).map(|v| v as usize)
}

fn path_to_null_terminated(path: &str) -> Result<[u8; 256], i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 {
        return Err(EINVAL);
    }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    Ok(buf)
}

// NOTE: sys_open and sys_readfile removed — use ob_open/ob_query_info(ReadContent) instead.

pub fn sys_close(fd: u8) -> Result<(), i64> {
    ret_unit((export::get_table().sys_close)(fd))
}

pub fn sys_getcwd(buf: &mut [u8]) -> Result<usize, i64> {
    let fd = sys_ob_open("\\Global\\Info\\Cwd", ob_access::READ)?;
    let n = sys_ob_query_info(fd, ObInfoClass::Cwd, buf)?;
    let _ = sys_close(fd);
    Ok(n)
}

pub fn sys_brk(new_break: u64) -> Result<u64, i64> {
    ret((export::get_table().sys_brk)(new_break))
}

pub fn sys_mmap(hint: u64, len: u64, prot: u16, flags: u16, file_handle: u64) -> Result<u64, i64> {
    ret((export::get_table().sys_mmap)(hint, len, prot, flags, file_handle))
}

pub fn sys_munmap(addr: u64, len: u64) -> Result<(), i64> {
    ret_unit((export::get_table().sys_munmap)(addr, len))
}

pub fn sys_loadlib(path: &str) -> Result<u64, i64> {
    let buf = path_to_null_terminated(path)?;
    let ptr = buf.as_ptr();
    ret((export::get_table().sys_loadlib)(ptr))
}

/// sys_dup2 (RAX=22): duplicate a file descriptor.
pub fn sys_dup2(old_fd: u8, new_fd: u8) -> Result<u8, i64> {
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 22",
            "mov rbx, {old}",
            "mov rcx, {new}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            old = in(reg) old_fd as u64,
            new = in(reg) new_fd as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(r as u8) }
}

/// sys_cursor_blink (RAX=30): enable/disable automatic cursor blinking.
pub fn sys_cursor_blink(enabled: bool) -> Result<(), i64> {
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov rax, 30",
            "mov rbx, {enable}",
            "int 0x80",
            "pop rbx",
            enable = in(reg) enabled as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_poll: poll file descriptors for readiness (RAX=59).
/// `fds` — array of PollFd entries.
/// `timeout_ms` — 0 = non-blocking, u64::MAX = infinite.
/// Returns number of ready fds.
#[repr(C)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

pub const POLLIN: i16 = 1;
pub const POLLOUT: i16 = 2;
pub const POLLERR: i16 = 4;
pub const POLLHUP: i16 = 8;

pub fn sys_poll(fds: &mut [PollFd], timeout_ms: u64) -> Result<usize, i64> {
    let pfds_ptr = fds.as_ptr() as u64;
    let nfds = fds.len() as u64;
    let r = unsafe { ob_syscall_3!(24, pfds_ptr, nfds, timeout_ms) };
    if r < 0 { Err(r) } else { Ok(r as usize) }
}

/// sys_sleep_ex: yield alertable — cede CPU, chequea APCs pendientes (RAX=41).
pub fn sys_sleep_ex() -> Result<(), i64> {
    let r = unsafe { ob_syscall_2!(3, 0u64, 0u64) };
    if r < 0 { Err(r) } else { Ok(()) }
}

