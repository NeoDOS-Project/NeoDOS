//! Syscall dispatch table — INT 0x80
//!
//! Calling convention (Ring 3 → kernel):
//!   RAX = syscall number
//!   RBX = arg0
//!   RCX = arg1
//!   RDX = arg2
//!
//! Return value in RAX (passed back from this function to the asm trampolín).
//!
//! Syscall numbers:
//!   0  sys_exit      — terminate current process
//!   1  sys_write     — write bytes to the console
//!   2  sys_yield     — voluntarily give up the CPU
//!   3  sys_getpid    — return current PID
//!   4  sys_read      — read from stdin (keyboard)
//!   9  sys_waitpid   — wait for process to terminate
//!  10  sys_open      — open file
//!  11  sys_readfile  — read from file
//!  12  sys_writefile — write to file
//!  13  sys_close     — close file
//!  14  sys_ioctl     — device I/O control
//!  15  sys_register_device — register device handler
//!  16  sys_chdir     — change current working directory
//!  17  sys_getcwd    — get current working directory
//!  18  sys_brk       — adjust program break (demand-paged)
//!  19  sys_mmap      — allocate zero-filled memory

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::serial_println;
use crate::scheduler::{self, ProcessState};

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
    let has_non_idle = x86_64::instructions::interrupts::without_interrupts(|| {
        let scheduler = scheduler::current_scheduler().lock();
        scheduler.has_non_idle_processes()
    });

    if !has_non_idle {
        return current_rsp;
    }

    x86_64::instructions::interrupts::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut scheduler = s.lock();

        let pid = scheduler.current_pid;
        if pid > 0 {
            if let Some(current) = scheduler.current_process_mut() {
                if current.state == ProcessState::Running {
                    current.rsp = current_rsp;
                    current.state = ProcessState::Ready;
                }
            }
        }

        let next = scheduler.schedule();
        unsafe { (*next).rsp }
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
/// Valid ranges: the standard user slot window (4–8 MB) or the current
/// process's heap region.
fn is_user_ptr_valid(ptr: u64, len: u64) -> bool {
    if ptr >= 0x400000 && ptr.saturating_add(len) <= 0x800000 {
        return true;
    }
    let (heap_base, heap_break) = crate::scheduler::current_process_heap_range();
    heap_base != 0 && ptr >= heap_base && ptr.saturating_add(len) <= heap_break
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
pub extern "C" fn syscall_dispatch(rax: u64, rbx: u64, rcx: u64, _rdx: u64) -> u64 {
    match rax {
        // ---- sys_exit(code: u64) ----
        0 => {
            let code = rbx;
            serial_println!("[syscall] sys_exit({})", code);

            let pid = x86_64::instructions::interrupts::without_interrupts(|| {
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
                            // Free all physically allocated heap pages
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
                    }
                }

                scheduler.wake_waiters(pid);
                pid
            });

            if pid > 0 && pid == crate::usermode::current_wait_pid() {
                crate::usermode::request_exit_to_kernel();
            }

            0
        }

        // ---- sys_write(ptr: *const u8, len: usize) ----
        1 => {
            let ptr = rbx as *const u8;
            let len = rcx as usize;

            if !is_user_ptr_valid(rbx, len as u64) || len > 4096 {
                serial_println!("[syscall] sys_write: bad address 0x{:x} len {}", rbx, len);
                return u64::MAX;
            }

            let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
            if let Ok(s) = core::str::from_utf8(slice) {
                crate::console::print_str(s);
                serial_println!("[user] {}", s);
            }
            len as u64
        }

        // ---- sys_yield() ----
        2 => {
            serial_println!("[syscall] sys_yield");
            0
        }

        // ---- sys_getpid() ----
        3 => {
            let pid = x86_64::instructions::interrupts::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid
            });
            serial_println!("[syscall] sys_getpid -> {}", pid);
            pid as u64
        }

        // ---- sys_read(fd: u64, buf: *mut u8, count: usize) ----
        4 => {
            let fd = rbx;
            let buf_ptr = rcx as *mut u8;
            let count = _rdx as usize;

            if fd != 0 {
                serial_println!("[syscall] sys_read: unsupported fd {}", fd);
                return u64::MAX;
            }

            if !is_user_ptr_valid(rcx, count as u64) || count > 4096 {
                serial_println!("[syscall] sys_read: bad address 0x{:x} len {}", rcx, count);
                return u64::MAX;
            }

            let mut bytes_read = 0usize;
            
            while bytes_read < count {
                match crate::input::pop_byte() {
                    Some(byte) => {
                        unsafe {
                            buf_ptr.add(bytes_read).write(byte);
                        }
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
                                unsafe {
                                    buf_ptr.add(bytes_read).write(b);
                                }
                                bytes_read += 1;
                                break;
                            }
                            unsafe { core::arch::asm!("hlt") };
                        }
                    }
                }
            }

            serial_println!("[syscall] sys_read -> {} bytes", bytes_read);
            bytes_read as u64
        }

        // ---- sys_waitpid(pid: u32) -> exit_code ----
        9 => {
            let wait_pid = rbx as u32;
            serial_println!("[syscall] sys_waitpid({})", wait_pid);

            {
                let already_terminated = x86_64::instructions::interrupts::without_interrupts(|| {
                    let s = crate::scheduler::current_scheduler();
                    let scheduler = s.lock();
                    
                    let mut terminated = false;
                    for proc in scheduler.processes.iter() {
                        if let Some(p) = proc {
                            if p.pid == wait_pid && p.state == ProcessState::Terminated {
                                terminated = true;
                                break;
                            }
                        }
                    }
                    terminated
                });
                
                if !already_terminated {
                    loop {
                        let is_terminated = x86_64::instructions::interrupts::without_interrupts(|| {
                            let s2 = crate::scheduler::current_scheduler();
                            let scheduler2 = s2.lock();
                            
                            let mut terminated = false;
                            for proc in scheduler2.processes.iter() {
                                if let Some(p) = proc {
                                    if p.pid == wait_pid && p.state == ProcessState::Terminated {
                                        terminated = true;
                                        break;
                                    }
                                }
                            }
                            terminated
                        });
                        
                        if is_terminated {
                            break;
                        }
                        
                        unsafe { core::arch::asm!("hlt") };
                    }
                }
            }

            0
        }

        // ---- sys_open(path: *const u8, flags: u64) -> inode_num / u64::MAX ----
        10 => {
            let path_ptr = rbx as *const u8;
            let _flags = rcx;

            if !is_user_ptr_valid(rbx, 1) {
                serial_println!("[syscall] sys_open: bad path address 0x{:x}", rbx);
                return u64::MAX;
            }

            let mut path_bytes = [0u8; 256];
            let mut path_len = 0usize;

            unsafe {
                while path_len < 255 {
                    let byte = path_ptr.add(path_len).read();
                    if byte == 0 {
                        break;
                    }
                    path_bytes[path_len] = byte;
                    path_len += 1;
                }
            }

            if path_len == 0 {
                serial_println!("[syscall] sys_open: empty path");
                return u64::MAX;
            }

            let path = match core::str::from_utf8(&path_bytes[..path_len]) {
                Ok(s) => s,
                Err(_) => {
                    serial_println!("[syscall] sys_open: invalid UTF-8");
                    return u64::MAX;
                }
            };

            serial_println!("[syscall] sys_open('{}')", path);

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
                        serial_println!("[syscall] sys_open -> Not a file");
                        return u64::MAX;
                    }
                    let handle = ((drive_idx as u64) << 32) | (node.inode as u64);
                    serial_println!("[syscall] sys_open -> handle 0x{:x}", handle);
                    handle
                }
                Err(_) => {
                    serial_println!("[syscall] sys_open -> File not found");
                    u64::MAX
                }
            }
        }

        // ---- sys_readfile(handle: u64, buf: *mut u8, count: usize) -> bytes_read ----
        11 => {
            let handle = rbx;
            let drive_idx = (handle >> 32) as usize;
            let inode_num = (handle & 0xFFFFFFFF) as u32;
            let buf_ptr = rcx as *mut u8;
            let count = _rdx as usize;

            if drive_idx >= 26 || handle == u64::MAX {
                serial_println!("[syscall] sys_readfile: invalid handle 0x{:x}", handle);
                return u64::MAX;
            }

            if !is_user_ptr_valid(rcx, count as u64) || count > 4096 {
                serial_println!("[syscall] sys_readfile: bad buffer 0x{:x} len {}", rcx, count);
                return u64::MAX;
            }

            serial_println!("[syscall] sys_readfile(handle=0x{:x}, count={})", handle, count);

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
                    serial_println!("[syscall] sys_readfile -> {} bytes", bytes_read);
                    bytes_read as u64
                }
                Err(_) => {
                    serial_println!("[syscall] sys_readfile -> error");
                    u64::MAX
                }
            }
        }

        // ---- sys_writefile(handle: u64, buf: *const u8, count: usize) -> bytes_written ----
        12 => {
            let handle = rbx;
            let drive_idx = (handle >> 32) as usize;
            let inode_num = (handle & 0xFFFFFFFF) as u32;
            let buf_ptr = rcx as *const u8;
            let count = _rdx as usize;

            if drive_idx >= 26 || handle == u64::MAX {
                serial_println!("[syscall] sys_writefile: invalid handle 0x{:x}", handle);
                return u64::MAX;
            }

            if !is_user_ptr_valid(rcx, count as u64) || count > 4096 {
                serial_println!("[syscall] sys_writefile: bad buffer 0x{:x} len {}", rcx, count);
                return u64::MAX;
            }

            serial_println!("[syscall] sys_writefile(handle=0x{:x}, count={})", handle, count);

            let mut temp_buf = Vec::with_capacity(count);
            temp_buf.resize(count, 0u8);

            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr, temp_buf.as_mut_ptr(), count);
            }

            let result = crate::globals::with_vfs(|vfs| {
                vfs.write(drive_idx, inode_num, 0, &temp_buf)
            });

            match result {
                Ok(bytes_written) => {
                    serial_println!("[syscall] sys_writefile -> {} bytes", bytes_written);
                    bytes_written as u64
                }
                Err(_) => {
                    serial_println!("[syscall] sys_writefile -> error");
                    u64::MAX
                }
            }
        }

        // ---- sys_close(fd: u64) ----
        13 => {
            serial_println!("[syscall] sys_close({})", rbx);
            0
        }

        // ---- sys_ioctl(device_id, cmd, buf, count) ----
        // Convention: rbx=device_id, rcx=cmd, rdx=buf, (no 4th arg - count is derived from buf validation)
        14 => {
            let device_id = rbx as u32;
            let cmd = rcx as u32;
            let buf_ptr = _rdx as *mut u8;
            let count = 4; // default count for now

            serial_println!("[syscall] sys_ioctl(dev={}, cmd={}, buf=0x{:x}", device_id, cmd, buf_ptr as u64);

            // Check if device has a registered handler
            let handler = get_device_handler(device_id);
            match handler {
                Some(h) => {
                    let addr = buf_ptr as u64;

                    // Allow buf=0 for polling (check if there are pending events)
                    if addr == 0 {
                        // Poll mode - check if driver has pending commands
                        let pending = unsafe { crate::drivers::DEVICE_EVENTS[device_id as usize].pending.load(core::sync::atomic::Ordering::Relaxed) };
                        if pending {
                            // Clear pending flag and return success
                            unsafe { crate::drivers::DEVICE_EVENTS[device_id as usize].pending.store(false, core::sync::atomic::Ordering::Relaxed) };
                            serial_println!("[syscall] sys_ioctl: poll -> pending event!");
                            return 1; // 1 means there was a pending event
                        }
                        serial_println!("[syscall] sys_ioctl: poll -> no events");
                        return 0; // No pending events
                    }

                    // Validate buffer address
                    if !is_user_ptr_valid(addr, count as u64) || count > 4096 {
                        serial_println!("[syscall] sys_ioctl: bad buffer 0x{:x} len {}", addr, count);
                        return u64::MAX;
                    }

                    // Copy data to user buffer
                    let data = [cmd as u8, (cmd >> 8) as u8, (cmd >> 16) as u8, (cmd >> 24) as u8];
                    unsafe {
                        core::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr, count);
                    }

                    serial_println!("[syscall] sys_ioctl: forwarded to PID {}", h.owner_pid);
                    count as u64
                }
                None => {
                    serial_println!("[syscall] sys_ioctl: no handler for device {}", device_id);
                    u64::MAX
                }
            }
        }

        // ---- sys_register_device(device_id) ----
        15 => {
            let device_id = rbx as u32;
            let current_pid = x86_64::instructions::interrupts::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid
            });

            serial_println!("[syscall] sys_register_device(dev={}) for PID {}", device_id, current_pid);

            if register_device(device_id, current_pid) {
                serial_println!("[syscall] sys_register_device: OK");
                0
            } else {
                serial_println!("[syscall] sys_register_device: failed");
                u64::MAX
            }
        }

        // ---- sys_chdir(path: *const u8) ----
        16 => {
            let path_str = match copy_user_string(rbx) {
                Ok(s) => s,
                Err(_) => {
                    serial_println!("[syscall] sys_chdir: bad path");
                    return u64::MAX;
                }
            };

            serial_println!("[syscall] sys_chdir('{}')", path_str);

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
                let colon = normalized.find(':').unwrap();
                let dl = normalized[..colon].chars().next().unwrap().to_ascii_uppercase();
                let idx = crate::fs::vfs::Vfs::drive_index(dl).unwrap() as u8;
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
                    serial_println!("[syscall] sys_chdir -> OK");
                    0
                }
                Err(e) => {
                    serial_println!("[syscall] sys_chdir: {:?}", e);
                    u64::MAX
                }
            }
        }

        // ---- sys_getcwd(buf: *mut u8, len: usize) ----
        17 => {
            let buf_ptr = rbx as *mut u8;
            let buf_len = rcx as usize;

            if !is_user_ptr_valid(rbx, buf_len as u64) || buf_len > 4096 {
                serial_println!("[syscall] sys_getcwd: bad buffer 0x{:x} len {}", rbx, buf_len);
                return u64::MAX;
            }

            let (drive, path) = crate::scheduler::get_current_cwd();
            let full = alloc::format!("{}:{}", (b'A' + drive) as char, path);

            let bytes = full.as_bytes();
            let to_copy = core::cmp::min(bytes.len(), buf_len.saturating_sub(1));

            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr, to_copy);
                buf_ptr.add(to_copy).write(0);
            }

            serial_println!("[syscall] sys_getcwd -> '{}'", full);
            to_copy as u64
        }

        // ---- sys_brk(new_break: u64) -> u64 ----
        // Adjust program break. Returns current/updated break, or u64::MAX on error.
        // RBX = 0 → query only (return current break)
        // RBX < heap_base or > heap_limit → error
        //
        // Physical pages are allocated on demand by the page fault handler.
        // This syscall only adjusts the break pointer and zeroes new pages.
        18 => {
            let new_break = rbx;
            let (heap_base, current_break) = crate::scheduler::current_process_heap_range();

            if heap_base == 0 {
                serial_println!("[syscall] sys_brk: no heap allocated");
                return u64::MAX;
            }

            if new_break == 0 {
                return current_break;
            }

            let heap_limit = heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE;

            if new_break < heap_base || new_break > heap_limit {
                serial_println!("[syscall] sys_brk: 0x{:x} out of range [0x{:x}..0x{:x})",
                    new_break, heap_base, heap_limit);
                return u64::MAX;
            }

            // When growing, zero-fill new pages (page faults will handle allocation).
            if new_break > current_break {
                let start_page = (current_break + 0xFFF) & !0xFFF;
                let end_page = new_break & !0xFFF;
                if end_page > start_page {
                    // Touch each new page to trigger demand allocation
                    let mut page = start_page;
                    while page < end_page {
                        unsafe {
                            core::ptr::write_volatile(page as *mut u8, 0);
                        }
                        page += crate::arch::x64::paging::PAGE_4K;
                    }
                }
                // Zero-fill the remainder
                unsafe {
                    core::ptr::write_bytes(current_break as *mut u8, 0, (new_break - current_break) as usize);
                }
            } else if new_break < current_break {
                // When shrinking, free pages that are no longer in range
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

            serial_println!("[syscall] sys_brk(0x{:x}) -> 0x{:x}", new_break, new_break);
            new_break
        }

        // ---- sys_mmap(size: u64) -> u64 ----
        // Allocate 'size' bytes of zero-filled memory, return virtual address.
        // Uses the frame allocator directly. Only for user-mode processes.
        // RBX = size (will be rounded up to page boundary).
        // Returns virtual address, or u64::MAX on error.
        19 => {
            let size = rbx;
            if size == 0 || size > 0x100000 { // max 1 MB per call
                serial_println!("[syscall] sys_mmap: invalid size {}", size);
                return u64::MAX;
            }

            let (heap_base, current_break) = crate::scheduler::current_process_heap_range();
            if heap_base == 0 {
                serial_println!("[syscall] sys_mmap: no heap allocated");
                return u64::MAX;
            }

            let heap_limit = heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE;
            let alloc_size = (size + 0xFFF) & !0xFFF;
            let new_break = current_break + alloc_size;

            if new_break > heap_limit {
                serial_println!("[syscall] sys_mmap: out of heap space (break 0x{:x} > limit 0x{:x})",
                    new_break, heap_limit);
                return u64::MAX;
            }

            // Allocate physical pages and map them
            let mut page = current_break;
            while page < new_break {
                if let Some(_phys) = crate::arch::x64::paging::heap_alloc_page(page) {
                    unsafe {
                        core::ptr::write_volatile(page as *mut u8, 0);
                    }
                } else {
                    // Free partial allocation
                    crate::arch::x64::paging::heap_free_range(current_break, page);
                    crate::scheduler::set_current_heap_break(current_break);
                    serial_println!("[syscall] sys_mmap: OOM at 0x{:x}", page);
                    return u64::MAX;
                }
                page += crate::arch::x64::paging::PAGE_4K;
            }

            crate::scheduler::set_current_heap_break(new_break);

            serial_println!("[syscall] sys_mmap({}) -> 0x{:x}", size, current_break);
            current_break
        }

        _ => {
            serial_println!("[syscall] unknown syscall RAX={}", rax);
            u64::MAX
        }
    }
}

pub fn wake_blocked_readers() {
    x86_64::instructions::interrupts::without_interrupts(|| {
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
