//! Syscall dispatch table — INT 0x80
//!
//! # ABI v0 (STABLE)
//!
//! Calling convention (Ring 3 → kernel):
//!   RAX = syscall number  (see `SyscallNum`)
//!   RBX = arg0
//!   RCX = arg1
//!   RDX = arg2
//!
//! Return value in RAX:
//!   Non-negative (≥ 0)  → success, value is the result
//!   Negative (< 0)       → error, value is `-(SyscallError)`.
//!     User code checks for error with `cmp rax, -1` / `jl error` or
//!     compares against `SYSERR_*` constants.
//!
//! Error codes are returned as `u64` containing the twos-complement
//! representation of the negative error.  Example:
//!   `SYSERR_NOENT = 2`  → RAX = `0xFFFF_FFFF_FFFF_FFFE` (= -2 as i64).
//!
//! Legacy: syscalls that never fail (sys_getpid, sys_yield, sys_exit)
//! return 0 on success.  New code should treat 0 as success.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::serial_println;
use crate::scheduler::{self, ProcessState};

// ── Syscall Number Constants (frozen ABI) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SyscallNum {
    Exit = 0,
    Write = 1,
    Yield = 2,
    GetPid = 3,
    Read = 4,
    Pipe = 5,
    Dup2 = 6,

    WaitPid = 9,
    Open = 10,
    ReadFile = 11,
    WriteFile = 12,
    Close = 13,
    Ioctl = 14,
    RegisterDevice = 15,
    ChDir = 16,
    GetCwd = 17,
    Brk = 18,
    Mmap = 19,
    Munmap = 20,
}

impl SyscallNum {
    pub const MAX_VALID: u64 = 20;

    pub fn from_u64(n: u64) -> Option<Self> {
        match n {
            0 => Some(Self::Exit),
            1 => Some(Self::Write),
            2 => Some(Self::Yield),
            3 => Some(Self::GetPid),
            4 => Some(Self::Read),
            5 => Some(Self::Pipe),
            6 => Some(Self::Dup2),
            9 => Some(Self::WaitPid),
            10 => Some(Self::Open),
            11 => Some(Self::ReadFile),
            12 => Some(Self::WriteFile),
            13 => Some(Self::Close),
            14 => Some(Self::Ioctl),
            15 => Some(Self::RegisterDevice),
            16 => Some(Self::ChDir),
            17 => Some(Self::GetCwd),
            18 => Some(Self::Brk),
            19 => Some(Self::Mmap),
            20 => Some(Self::Munmap),
            _ => None,
        }
    }
}

// ── Standard Error Codes ──
//
// Returned as negative u64: `err_to_u64(SyscallError::NoEnt)` → 0xFFFF_FFFF_FFFF_FFFE.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum SyscallError {
    /// Invalid argument
    Inval = 1,
    /// No such file or directory
    NoEnt = 2,
    /// Out of memory
    NoMem = 3,
    /// Permission denied
    Acces = 4,
    /// Bad file descriptor / handle
    BadF = 5,
    /// Bad address
    Fault = 6,
    /// Function not implemented
    NoSys = 7,
    /// Resource temporarily unavailable
    Again = 8,
    /// Broken pipe
    Pipe = 9,
    /// File exists
    Exist = 10,
    /// Not a directory
    NotDir = 11,
    /// Is a directory
    IsDir = 12,
    /// I/O error
    Io = 13,
    /// No such device
    NoDev = 14,
    /// Device or resource busy
    Busy = 15,
}

/// Convert a `SyscallError` into the `u64` return value convention
/// (negative encoding: `NoEnt=2` → RAX = `0xFFFF_FFFF_FFFF_FFFE` = -2 as i64).
pub fn err_to_u64(e: SyscallError) -> u64 {
    (-(e as i64)) as u64
}



// ── ABI validation ──

/// Validate syscall ABI assumptions at boot time.
/// Called once during kernel init.
pub fn validate_abi() {
    // SyscallError values must fit in a negative i64 when cast to u64
    assert!((err_to_u64(SyscallError::Inval) as i64) < 0);
    assert!((err_to_u64(SyscallError::NoEnt) as i64) < 0);
    assert!((err_to_u64(SyscallError::NoMem) as i64) < 0);

    // Syscall numbers must not overlap or exceed max
    assert_eq!(SyscallNum::MAX_VALID, 20);
    for n in 0..=SyscallNum::MAX_VALID {
        if n == 7 || n == 8 {
            assert!(SyscallNum::from_u64(n).is_none(), "reserved hole {} must stay free", n);
        } else {
            assert!(SyscallNum::from_u64(n).is_some(), "syscall {} must be assigned", n);
        }
    }

    crate::serial_println!("[SYS] ABI v0 validated ({} syscalls, {} error codes)",
        SyscallNum::MAX_VALID + 1, 16);
}

pub static NEED_RESCHED: AtomicBool = AtomicBool::new(false);

// Device handler registry - max 8 devices
const MAX_DEVICES: usize = 8;

#[derive(Clone, Copy)]
pub struct DeviceHandler {
    pub device_id: u32,
    pub owner_pid: u32,
}

static mut DEVICE_HANDLERS: [Option<DeviceHandler>; MAX_DEVICES] = [None; MAX_DEVICES];

pub fn register_device(device_id: u32, owner_pid: u32) -> bool {
    if device_id as usize >= MAX_DEVICES {
        return false;
    }
    unsafe {
        DEVICE_HANDLERS[device_id as usize] = Some(DeviceHandler { device_id, owner_pid });
    }
    true
}

pub fn get_device_handler(device_id: u32) -> Option<DeviceHandler> {
    if device_id as usize >= MAX_DEVICES {
        return None;
    }
    unsafe { DEVICE_HANDLERS[device_id as usize] }
}

pub fn set_need_resched() {
    NEED_RESCHED.store(true, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn clear_need_resched() -> bool {
    crate::globals::flush_cache_if_needed();
    NEED_RESCHED.swap(false, Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn syscall_try_resched(current_rsp: u64) -> u64 {
    // Invariant: must not be called from inside timer IRQ handler
    if cfg!(feature = "validation") && crate::invariants::is_in_timer_irq() {
        crate::serial_println!("[SYS] resched called from timer IRQ context!");
    }

    let has_non_idle = crate::hal::without_interrupts(|| {
        let scheduler = scheduler::current_scheduler().lock();
        scheduler.has_non_idle_processes()
    });

    if !has_non_idle {
        return current_rsp;
    }

    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut scheduler = s.lock();

        let pid = scheduler.current_pid;
        if pid > 0 {
            if let Some(current) = scheduler.current_process_mut() {
                current.rsp = current_rsp;
                // Only transition from Running → Ready.
                // Blocked processes (pipe reads, etc.) stay Blocked
                // so the scheduler skips them.
                if current.state == ProcessState::Running {
                    current.state = ProcessState::Ready;
                } else if cfg!(feature = "validation") {
                    crate::serial_println!("[SYS] Context switch from non-Running state: {:?}", current.state);
                }
            }
        }

        let next = scheduler.schedule();

        // Update TSS.RSP0 to the next process's private kernel stack
        let next_ks_top = unsafe { (*next).kernel_stack_top };
        crate::arch::x64::gdt::set_kernel_stack(next_ks_top);

        let next_rsp = unsafe { (*next).rsp };
        crate::trace_cswitch!(pid, unsafe { (*next).pid } as u64);
        next_rsp
    })
}

/// Normalize a DOS path: resolve `.`/`..`, collapse separators.
fn normalize_dos_path(path: &str) -> String {
    let mut drive_prefix = [0u8; 2];
    let rest = if path.len() >= 2 && path.as_bytes()[1] == b':' {
        drive_prefix[0] = path.as_bytes()[0].to_ascii_uppercase();
        drive_prefix[1] = b':';
        &path[2..]
    } else {
        path
    };

    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split(|c| c == '\\' || c == '/') {
        match part {
            "" | "." => {}
            ".." => { parts.pop(); }
            _ => parts.push(part),
        }
    }

    let mut result = String::new();
    if drive_prefix[0] != 0 {
        result.push(drive_prefix[0] as char);
        result.push(':');
    }
    result.push('\\');
    for (i, part) in parts.iter().enumerate() {
        if i > 0 { result.push('\\'); }
        result.push_str(part);
    }
    result
}

/// Check if `[ptr, ptr+len)` is a valid user-accessible address range.
/// Valid ranges: the standard user slot window (4–8 MB), the current
/// process's heap region, or any active mmap region.
pub(crate) fn is_user_ptr_valid(ptr: u64, len: u64) -> bool {
    if ptr >= 0x400000 && ptr.saturating_add(len) <= 0x800000 {
        return true;
    }
    let (heap_base, heap_break) = crate::scheduler::current_process_heap_range();
    if heap_base != 0 && ptr >= heap_base && ptr.saturating_add(len) <= heap_break {
        return true;
    }
    // Check mmap regions
    let regions = crate::scheduler::current_process_mmap_regions();
    for r in &regions {
        if ptr >= r.base && ptr.saturating_add(len) <= r.base + r.len {
            return true;
        }
    }
    false
}

/// Copy a null-terminated string from user space (up to 255 bytes).
fn copy_user_string(ptr: u64) -> Result<String, ()> {
    if !is_user_ptr_valid(ptr, 1) {
        return Err(());
    }
    let mut buf = [0u8; 256];
    let mut len = 0usize;
    unsafe {
        while len < 255 {
            let byte = (ptr as *const u8).add(len).read();
            if byte == 0 { break; }
            buf[len] = byte;
            len += 1;
        }
    }
    core::str::from_utf8(&buf[..len]).map(|s| s.to_string()).map_err(|_| ())
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(rax: u64, rbx: u64, rcx: u64, rdx: u64, r8: u64, r9: u64) -> u64 {
    crate::trace_syscall!(rax, rbx, rcx, rdx);

    // ABI validation: reject unknown syscall numbers
    let num = match SyscallNum::from_u64(rax) {
        Some(n) => n,
        None => {
            serial_println!("[SYS] INVALID syscall number: {}", rax);
            return err_to_u64(SyscallError::NoSys);
        }
    };

    match num {
        SyscallNum::Exit => {
            let code = rbx;

            let pid = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut scheduler = s.lock();
                let pid = scheduler.current_pid;

                if pid > 0 {
                    if let Some(proc) = scheduler.current_process_mut() {
                        proc.state = ProcessState::Terminated;

                        if let Some(slot) = proc.user_slot.take() {
                            crate::arch::x64::paging::free_user_slot(slot);
                        }
                        if proc.heap_base != 0 {
                            crate::arch::x64::paging::heap_free_range(
                                proc.heap_base,
                                proc.heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE,
                            );
                            let idx = ((proc.heap_base
                                - crate::arch::x64::paging::PROCESS_HEAP_BASE)
                                / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as u8;
                            crate::arch::x64::paging::free_heap_slot(idx);
                            proc.heap_base = 0;
                            proc.heap_break = 0;
                        }
                        // Free all mmap regions
                        for r in proc.mmap_regions.iter() {
                            crate::arch::x64::paging::mmap_free_range(r.base, r.base + r.len);
                        }
                        proc.mmap_regions.clear();
                        proc.mmap_next = crate::arch::x64::paging::MMAP_BASE;

                        // Close all pipe fds
                        for fd_entry in proc.fd_table.iter_mut() {
                            match fd_entry.kind {
                                crate::pipe::FD_PIPE_READ => {
                                    crate::pipe::PIPE_MANAGER.dec_read_ref(fd_entry.pipe_id);
                                }
                                crate::pipe::FD_PIPE_WRITE => {
                                    crate::pipe::PIPE_MANAGER.dec_write_ref(fd_entry.pipe_id);
                                }
                                _ => {}
                            }
                            *fd_entry = crate::pipe::FdEntry::closed();
                        }
                    }
                }

                scheduler.wake_waiters(pid);
                pid
            });

            if pid > 0 && pid == crate::usermode::current_wait_pid() {
                crate::usermode::request_exit_to_kernel();
            }

            code
        }

        SyscallNum::Write => {
            let fd = rbx as u8;
            let ptr = rcx as *const u8;
            let len = rdx as usize;

            // fd 1 = stdout (console), fd 2 = stderr (console)
            let entry = current_fd_entry(fd);

            match entry.kind {
                crate::pipe::FD_STDOUT => {
                    if !is_user_ptr_valid(rcx, len as u64) || len > 4096 {
                        return err_to_u64(SyscallError::Fault);
                    }
                    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
                    if let Ok(s) = core::str::from_utf8(slice) {
                        crate::console::print_str(s);
                    }
                    len as u64
                }
                crate::pipe::FD_PIPE_WRITE => {
                    if !is_user_ptr_valid(rcx, len as u64) || len > 4096 {
                        return err_to_u64(SyscallError::Fault);
                    }
                    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
                    match crate::pipe::PIPE_MANAGER.write(entry.pipe_id, slice) {
                        Ok(n) => n as u64,
                        Err(_) => err_to_u64(SyscallError::Pipe), // Broken pipe
                    }
                }
                _ => {
                    err_to_u64(SyscallError::BadF)
                }
            }
        }

        SyscallNum::Yield => {
            0
        }

        SyscallNum::GetPid => {
            let pid = crate::hal::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid
            });
            pid as u64
        }

        SyscallNum::Read => {
            let fd = rbx as u8;
            let buf_ptr = rcx as *mut u8;
            let count = rdx as usize;

            if !is_user_ptr_valid(rcx, count as u64) || count > 4096 {
                return err_to_u64(SyscallError::Fault);
            }

            let entry = current_fd_entry(fd);

            match entry.kind {
                crate::pipe::FD_STDIN => {
                    let mut bytes_read = 0usize;
                    while bytes_read < count {
                        match crate::input::pop_byte() {
                            Some(byte) => {
                                unsafe { buf_ptr.add(bytes_read).write(byte); }
                                bytes_read += 1;
                                if byte == b'\r' || byte == b'\n' {
                                    break;
                                }
                            }
                            None => {
                                if bytes_read > 0 {
                                    break;
                                }
                                loop {
                                    if let Some(b) = crate::input::pop_byte() {
                                        unsafe { buf_ptr.add(bytes_read).write(b); }
                                        bytes_read += 1;
                                        break;
                                    }
                                    crate::hal::hlt_once();
                                }
                            }
                        }
                    }
                    bytes_read as u64
                }
                crate::pipe::FD_PIPE_READ => {
                    let pipe_id = entry.pipe_id;
                    // Read loop: try to read, block if empty
                    let mut temp_buf = alloc::vec::Vec::with_capacity(count);
                    temp_buf.resize(count, 0u8);
                    loop {
                        match crate::pipe::PIPE_MANAGER.read(pipe_id, &mut temp_buf) {
                            Ok(0) => {
                                // EOF — write end closed
                                return 0;
                            }
                            Ok(n) => {
                                unsafe {
                                    core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr, n);
                                }
                                return n as u64;
                            }
                            Err(()) => {
                                // No data yet — block and resched
                                crate::pipe::block_current_for_pipe(pipe_id);
                                // After wake-up: continue loop to retry read
                                // The return from block_current_for_pipe means
                                // NEED_RESCHED is set; the assembly will handle
                                // the context switch. When we resume and return
                                // to user space, the process can call read again.
                                return err_to_u64(SyscallError::Again);
                            }
                        }
                    }
                }
                _ => {
                    err_to_u64(SyscallError::BadF)
                }
            }
        }

        SyscallNum::Pipe => {
            let fds_ptr = rbx as *mut u64;
            // Validate user output buffer (2 u64s = read_fd, write_fd)
            if !is_user_ptr_valid(rbx, 16) {
                return err_to_u64(SyscallError::Fault);
            }

            // Allocate a pipe
            let pipe_id = match crate::pipe::PIPE_MANAGER.alloc() {
                Some(pid) => pid,
                None => return err_to_u64(SyscallError::NoMem),
            };

            // Find two free fds (lowest available)
            let mut read_fd: Option<u8> = None;
            let mut write_fd: Option<u8> = None;

            crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(proc) = lock.current_process_mut() {
                    for i in 3..(crate::pipe::MAX_FDS as u8) {
                        if proc.fd_table[i as usize].kind == crate::pipe::FD_CLOSED {
                            if read_fd.is_none() {
                                read_fd = Some(i);
                            } else if write_fd.is_none() {
                                write_fd = Some(i);
                                break;
                            }
                        }
                    }
                }
            });

            let (rfd, wfd) = match (read_fd, write_fd) {
                (Some(r), Some(w)) => (r, w),
                _ => {
                    crate::pipe::PIPE_MANAGER.dec_read_ref(pipe_id);
                    crate::pipe::PIPE_MANAGER.dec_write_ref(pipe_id);
                    return err_to_u64(SyscallError::NoMem);
                }
            };

            // Assign fds
            crate::pipe::PIPE_MANAGER.inc_read_ref(pipe_id);
            crate::pipe::PIPE_MANAGER.inc_write_ref(pipe_id);
            set_current_fd(rfd, crate::pipe::FdEntry::pipe_read(pipe_id));
            set_current_fd(wfd, crate::pipe::FdEntry::pipe_write(pipe_id));

            // Write [read_fd, write_fd] to user buffer
            unsafe {
                fds_ptr.write(rfd as u64);
                fds_ptr.add(1).write(wfd as u64);
            }
            0
        }

        SyscallNum::Dup2 => {
            let old_fd = rbx as u8;
            let new_fd = rcx as u8;

            if new_fd as usize >= crate::pipe::MAX_FDS {
                return err_to_u64(SyscallError::BadF);
            }
            if old_fd as usize >= crate::pipe::MAX_FDS {
                return err_to_u64(SyscallError::BadF);
            }

            // Get the source entry
            let src_entry = current_fd_entry(old_fd);
            if src_entry.kind == crate::pipe::FD_CLOSED {
                return err_to_u64(SyscallError::BadF);
            }

            // If new_fd is already open, close it first
            let dst_entry = current_fd_entry(new_fd);
            match dst_entry.kind {
                crate::pipe::FD_PIPE_READ => {
                    crate::pipe::PIPE_MANAGER.dec_read_ref(dst_entry.pipe_id);
                }
                crate::pipe::FD_PIPE_WRITE => {
                    crate::pipe::PIPE_MANAGER.dec_write_ref(dst_entry.pipe_id);
                }
                _ => {}
            }

            // Increment ref for the duplicated fd
            match src_entry.kind {
                crate::pipe::FD_PIPE_READ => {
                    crate::pipe::PIPE_MANAGER.inc_read_ref(src_entry.pipe_id);
                }
                crate::pipe::FD_PIPE_WRITE => {
                    crate::pipe::PIPE_MANAGER.inc_write_ref(src_entry.pipe_id);
                }
                _ => {}
            }

            set_current_fd(new_fd, src_entry);
            new_fd as u64
        }

        SyscallNum::WaitPid => {
            let wait_pid = rbx as u32;

            loop {
                let is_terminated = crate::hal::without_interrupts(|| {
                    let s = crate::scheduler::current_scheduler();
                    let scheduler = s.lock();
                    scheduler.processes.iter().any(|p| {
                        p.as_ref().is_some_and(|proc| proc.pid == wait_pid && proc.state == ProcessState::Terminated)
                    })
                });

                if is_terminated { break; }
                crate::hal::hlt_once();
            }

            // Recycle the slot and free kernel stack of the waited-for process
            crate::scheduler::cleanup_terminated_process(wait_pid);

            0
        }

        SyscallNum::Open => {
            let path_ptr = rbx as *const u8;
            let _flags = rcx;

            if !is_user_ptr_valid(rbx, 1) {
                return err_to_u64(SyscallError::Fault);
            }

            let mut path_bytes = [0u8; 256];
            let mut path_len = 0usize;

            unsafe {
                while path_len < 255 {
                    let byte = path_ptr.add(path_len).read();
                    if byte == 0 { break; }
                    path_bytes[path_len] = byte;
                    path_len += 1;
                }
            }

            if path_len == 0 {
                return err_to_u64(SyscallError::NoEnt);
            }

            let path = match core::str::from_utf8(&path_bytes[..path_len]) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Inval),
            };

            let result = crate::globals::with_vfs(|vfs| {
                let has_drive = path.contains(':');
                let starts_with_sep = path.starts_with('\\') || path.starts_with('/');
                if has_drive || starts_with_sep {
                    vfs.resolve_path(path)
                } else {
                    let (drive, cwd_path) = crate::scheduler::get_current_cwd();
                    let drive_char = (b'A' + drive) as char;
                    let abs = alloc::format!("{}:{}\\{}", drive_char, cwd_path, path);
                    vfs.resolve_path(&abs)
                }
            });

            match result {
                Ok((drive_idx, node)) => {
                    if (node.mode & crate::fs::vfs::MODE_FILE) == 0 {
                        return err_to_u64(SyscallError::IsDir);
                    }
                    let handle = ((drive_idx as u64) << 32) | (node.inode as u64);
                    handle
                }
                Err(_) => err_to_u64(SyscallError::NoEnt),
            }
        }

        SyscallNum::ReadFile => {
            let handle = rbx;
            let drive_idx = (handle >> 32) as usize;
            let inode_num = (handle & 0xFFFFFFFF) as u32;
            let buf_ptr = rcx as *mut u8;
            let count = rdx as usize;

            if drive_idx >= 26 || handle == u64::MAX {
                return err_to_u64(SyscallError::BadF);
            }

            if !is_user_ptr_valid(rcx, count as u64) || count > 4096 {
                return err_to_u64(SyscallError::Fault);
            }

            let mut temp_buf = Vec::with_capacity(count);
            temp_buf.resize(count, 0u8);

            let result = crate::globals::with_vfs(|vfs| {
                vfs.read(drive_idx, inode_num, 0, &mut temp_buf)
            });

            match result {
                Ok(bytes_read) => {
                    unsafe {
                        core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr, bytes_read);
                    }
                    bytes_read as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }

        SyscallNum::WriteFile => {
            let handle = rbx;
            let drive_idx = (handle >> 32) as usize;
            let inode_num = (handle & 0xFFFFFFFF) as u32;
            let buf_ptr = rcx as *const u8;
            let count = rdx as usize;

            if drive_idx >= 26 || handle == u64::MAX {
                return err_to_u64(SyscallError::BadF);
            }

            if !is_user_ptr_valid(rcx, count as u64) || count > 4096 {
                return err_to_u64(SyscallError::Fault);
            }

            let mut temp_buf = Vec::with_capacity(count);
            temp_buf.resize(count, 0u8);
            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr, temp_buf.as_mut_ptr(), count);
            }

            let result = crate::globals::with_vfs(|vfs| {
                vfs.write(drive_idx, inode_num, 0, &temp_buf)
            });

            match result {
                Ok(bytes_written) => bytes_written as u64,
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }

        SyscallNum::Close => {
            let fd = rbx as u8;
            let entry = current_fd_entry(fd);
            match entry.kind {
                crate::pipe::FD_PIPE_READ => {
                    crate::pipe::PIPE_MANAGER.dec_read_ref(entry.pipe_id);
                }
                crate::pipe::FD_PIPE_WRITE => {
                    crate::pipe::PIPE_MANAGER.dec_write_ref(entry.pipe_id);
                }
                _ => {
                    // stdin/stdout/stderr: just ignore close
                }
            }
            set_current_fd(fd, crate::pipe::FdEntry::closed());
            0
        }

        SyscallNum::Ioctl => {
            let device_id = rbx as u32;
            let cmd = rcx as u32;
            let buf_ptr = rdx as *mut u8;
            let count = 4;

            let handler = get_device_handler(device_id);
            match handler {
                Some(_h) => {
                    let addr = buf_ptr as u64;

                    if addr == 0 {
                        let pending = unsafe {
                            crate::drivers::DEVICE_EVENTS[device_id as usize]
                                .pending
                                .load(core::sync::atomic::Ordering::Relaxed)
                        };
                        if pending {
                            unsafe {
                                crate::drivers::DEVICE_EVENTS[device_id as usize]
                                    .pending
                                    .store(false, core::sync::atomic::Ordering::Relaxed)
                            };
                            return 1;
                        }
                        return 0;
                    }

                    if !is_user_ptr_valid(addr, count as u64) || count > 4096 {
                        return err_to_u64(SyscallError::Fault);
                    }

                    let data = [cmd as u8, (cmd >> 8) as u8,
                                (cmd >> 16) as u8, (cmd >> 24) as u8];
                    unsafe {
                        core::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr, count);
                    }
                    count as u64
                }
                None => err_to_u64(SyscallError::NoDev),
            }
        }

        SyscallNum::RegisterDevice => {
            let device_id = rbx as u32;
            let current_pid = crate::hal::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid
            });

            if register_device(device_id, current_pid) {
                0
            } else {
                err_to_u64(SyscallError::Busy)
            }
        }

        SyscallNum::ChDir => {
            let path_str = match copy_user_string(rbx) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };

            let (cwd_drive, cwd_path) = crate::scheduler::get_current_cwd();

            let is_absolute = path_str.contains(':')
                || path_str.starts_with('\\')
                || path_str.starts_with('/');

            let raw = if is_absolute {
                path_str
            } else {
                alloc::format!("{}\\{}", cwd_path, path_str)
            };

            let normalized = normalize_dos_path(&raw);

            let (new_drive, new_cwd_path) = if normalized.contains(':') {
                let colon = match normalized.find(':') {
                    Some(c) => c,
                    None => return err_to_u64(SyscallError::Inval),
                };
                let dl = match normalized[..colon].chars().next() {
                    Some(c) => c.to_ascii_uppercase(),
                    None => return err_to_u64(SyscallError::Inval),
                };
                let idx = match crate::fs::vfs::Vfs::drive_index(dl) {
                    Some(i) => i as u8,
                    None => return err_to_u64(SyscallError::NoEnt),
                };
                (idx, normalized[colon + 1..].to_string())
            } else {
                (cwd_drive, normalized)
            };

            let vfs_path = alloc::format!("{}:{}", (b'A' + new_drive) as char, &new_cwd_path);
            let result = crate::globals::with_vfs(|vfs| {
                let (_, node) = vfs.resolve_path(&vfs_path)?;
                if node.mode != crate::fs::vfs::MODE_DIR {
                    return Err(crate::fs::vfs::VfsError::NotADirectory);
                }
                Ok(())
            });

            match result {
                Ok(()) => {
                    crate::scheduler::set_current_cwd(new_drive, &new_cwd_path);
                    0
                }
                Err(_) => err_to_u64(SyscallError::NoEnt),
            }
        }

        SyscallNum::GetCwd => {
            let buf_ptr = rbx as *mut u8;
            let buf_len = rcx as usize;

            if !is_user_ptr_valid(rbx, buf_len as u64) || buf_len > 4096 {
                return err_to_u64(SyscallError::Fault);
            }

            let (drive, path) = crate::scheduler::get_current_cwd();
            let full = alloc::format!("{}:{}", (b'A' + drive) as char, path);

            let bytes = full.as_bytes();
            let to_copy = core::cmp::min(bytes.len(), buf_len.saturating_sub(1));

            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr, to_copy);
                buf_ptr.add(to_copy).write(0);
            }

            to_copy as u64
        }

        SyscallNum::Brk => {
            let new_break = rbx;
            let (heap_base, current_break) = crate::scheduler::current_process_heap_range();

            if heap_base == 0 {
                return err_to_u64(SyscallError::NoMem);
            }

            if new_break == 0 {
                return current_break;
            }

            let heap_limit = heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE;

            if new_break < heap_base || new_break > heap_limit {
                return err_to_u64(SyscallError::Inval);
            }

            if new_break > current_break {
                let start_page = (current_break + 0xFFF) & !0xFFF;
                let end_page = new_break & !0xFFF;
                if end_page > start_page {
                    let mut page = start_page;
                    while page < end_page {
                        match crate::arch::x64::paging::heap_alloc_page(page) {
                            Some(_) => {
                                unsafe { core::ptr::write_volatile(page as *mut u8, 0); }
                            }
                            None => {
                                crate::arch::x64::paging::heap_free_range(start_page, page);
                                crate::scheduler::set_current_heap_break(current_break);
                                return err_to_u64(SyscallError::NoMem);
                            }
                        }
                        page += crate::arch::x64::paging::PAGE_4K;
                    }
                }
                unsafe {
                    core::ptr::write_bytes(current_break as *mut u8, 0,
                        (new_break - current_break) as usize);
                }
            } else if new_break < current_break {
                let shrink_start = new_break;
                let shrink_end = current_break;
                let start_page = (shrink_start + crate::arch::x64::paging::PAGE_4K - 1)
                    & !(crate::arch::x64::paging::PAGE_4K - 1);
                let end_page = shrink_end & !(crate::arch::x64::paging::PAGE_4K - 1);
                let mut page = start_page;
                while page < end_page {
                    crate::arch::x64::paging::heap_free_page(page);
                    page += crate::arch::x64::paging::PAGE_4K;
                }
            }

            crate::scheduler::set_current_heap_break(new_break);
            new_break
        }

        SyscallNum::Mmap => {
            // RBX = addr_hint (0 = auto), RCX = length, RDX = prot
            // R8 = flags (bit0=1 anonymous, bit1=1 shared), R9 = file_handle
            let _addr_hint = rbx;
            let length = rcx;
            let prot = rdx as u16;
            let flags = r8 as u16;
            let file_handle = r9;

            if length == 0 || length > 0x100000 {
                return err_to_u64(SyscallError::Inval);
            }
            if prot & !3 != 0 {
                return err_to_u64(SyscallError::Inval);
            }

            let is_anon = (flags & 1) != 0;

            if is_anon {
                // Anonymous mmap — lazy zero-filled pages
                let alloc_size = (length + 0xFFF) & !0xFFF;
                let region = crate::scheduler::MmapRegion {
                    base: 0,
                    len: alloc_size,
                    prot,
                    flags: 1, // anonymous
                    drive: 0,
                    inode: 0,
                    file_size: 0,
                };
                match crate::scheduler::add_current_mmap_region(region) {
                    Some(base) => base,
                    None => err_to_u64(SyscallError::NoMem),
                }
            } else {
                // File-backed mmap — lazy loading from file
                let drive_idx = (file_handle >> 32) as usize;
                let inode_num = (file_handle & 0xFFFFFFFF) as u32;
                if drive_idx >= 26 || file_handle == u64::MAX {
                    return err_to_u64(SyscallError::BadF);
                }

                // Stat the file to get its size
                let file_info = crate::globals::with_vfs(|vfs| {
                    vfs.stat(drive_idx, inode_num)
                });
                let (file_size, file_mode) = match file_info {
                    Ok(node) => (node.size, node.mode),
                    Err(_) => return err_to_u64(SyscallError::NoEnt),
                };
                if (file_mode & crate::fs::vfs::MODE_FILE) == 0 {
                    return err_to_u64(SyscallError::IsDir);
                }

                let alloc_size = (length + 0xFFF) & !0xFFF;
                let region = crate::scheduler::MmapRegion {
                    base: 0,
                    len: alloc_size,
                    prot,
                    flags: 0, // file-backed
                    drive: drive_idx as u8,
                    inode: inode_num,
                    file_size,
                };
                match crate::scheduler::add_current_mmap_region(region) {
                    Some(base) => base,
                    None => err_to_u64(SyscallError::NoMem),
                }
            }
        }

        SyscallNum::Munmap => {
            // RBX = addr, RCX = length
            let addr = rbx;
            let length = rcx;

            if length == 0 || addr & 0xFFF != 0 {
                return err_to_u64(SyscallError::Inval);
            }

            // Find and remove the VMA
            let region = crate::scheduler::remove_current_mmap_region(addr);
            match region {
                Some(r) => {
                    // Free all physical pages in the range
                    crate::scheduler::free_current_mmap_pages(r.base, r.len);
                    0
                }
                None => err_to_u64(SyscallError::Inval),
            }
        }
    }
}

// ── FD table helpers ──

fn current_fd_entry(fd: u8) -> crate::pipe::FdEntry {
    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(proc) = lock.current_process_mut() {
            if (fd as usize) < crate::pipe::MAX_FDS {
                return proc.fd_table[fd as usize];
            }
        }
        crate::pipe::FdEntry::closed()
    })
}

fn set_current_fd(fd: u8, entry: crate::pipe::FdEntry) {
    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(proc) = lock.current_process_mut() {
            if (fd as usize) < crate::pipe::MAX_FDS {
                proc.fd_table[fd as usize] = entry;
            }
        }
    });
}

pub fn wake_blocked_readers() {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut scheduler = s.lock();
        
        for proc in scheduler.processes.iter_mut() {
            if let Some(p) = proc {
                if matches!(p.state, ProcessState::Blocked { waiting_for: 0xFFFFFFFF }) {
                    p.state = ProcessState::Ready;
                    p.waiting_for = None;
                    set_need_resched();
                }
            }
        }
    });
}
