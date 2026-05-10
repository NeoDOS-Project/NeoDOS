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

use core::sync::atomic::{AtomicBool, Ordering};
use crate::serial_println;
use crate::scheduler::{self, ProcessState};

pub static NEED_RESCHED: AtomicBool = AtomicBool::new(false);

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
    let s = scheduler::current_scheduler();
    let mut scheduler = s.lock();

    if !scheduler.has_non_idle_processes() {
        return current_rsp;
    }

    let pid = scheduler.current_pid;
    if pid > 0 {
        if let Some(current) = scheduler.current_process_mut() {
            current.rsp = current_rsp;
            if current.state == ProcessState::Running {
                current.state = ProcessState::Ready;
            }
        }
    }

    let next = scheduler.schedule();

    unsafe { (*next).rsp }
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(rax: u64, rbx: u64, rcx: u64, _rdx: u64) -> u64 {
    match rax {
        // ---- sys_exit(code: u64) ----
        0 => {
            let code = rbx;
            serial_println!("[syscall] sys_exit({})", code);

            let s = crate::scheduler::current_scheduler();
            let mut scheduler = s.lock();
            let pid = scheduler.current_pid;

            if pid > 0 {
                if let Some(proc) = scheduler.current_process_mut() {
                    proc.state = ProcessState::Terminated;

                    if let Some(slot) = proc.user_slot.take() {
                        crate::arch::x64::paging::free_user_slot(slot);
                    }
                }
            }

            scheduler.wake_waiters(pid);

            if pid > 0 && pid == crate::usermode::current_wait_pid() {
                crate::usermode::request_exit_to_kernel();
            }

            0
        }

        // ---- sys_write(ptr: *const u8, len: usize) ----
        1 => {
            let ptr = rbx as *const u8;
            let len = rcx as usize;

            let addr = rbx;
            if addr < 0x400000 || addr.saturating_add(len as u64) > 0x800000 || len > 4096 {
                serial_println!("[syscall] sys_write: bad address 0x{:x} len {}", addr, len);
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
            let pid = crate::scheduler::current_scheduler().lock().current_pid;
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

            let addr = rcx;
            if addr < 0x400000 || addr.saturating_add(count as u64) > 0x800000 || count > 4096 {
                serial_println!("[syscall] sys_read: bad address 0x{:x} len {}", addr, count);
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
                let s = crate::scheduler::current_scheduler();
                let mut scheduler = s.lock();
                
                let mut already_terminated = false;
                for proc in scheduler.processes.iter() {
                    if let Some(p) = proc {
                        if p.pid == wait_pid && p.state == ProcessState::Terminated {
                            already_terminated = true;
                            break;
                        }
                    }
                }
                
                if !already_terminated {
                    loop {
                        let s2 = crate::scheduler::current_scheduler();
                        let mut scheduler2 = s2.lock();
                        
                        let mut is_terminated = false;
                        for proc in scheduler2.processes.iter() {
                            if let Some(p) = proc {
                                if p.pid == wait_pid && p.state == ProcessState::Terminated {
                                    is_terminated = true;
                                    break;
                                }
                            }
                        }
                        
                        if is_terminated {
                            break;
                        }
                        
                        drop(scheduler2);
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

            let addr = rbx;
            if addr < 0x400000 || addr > 0x800000 {
                serial_println!("[syscall] sys_open: bad path address 0x{:x}", addr);
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

            let result = unsafe {
                let fs = match crate::globals::NEODOS_FS.as_mut() {
                    Some(fs) => fs,
                    None => return u64::MAX,
                };
                let cache = match crate::globals::BLOCK_CACHE.as_mut() {
                    Some(c) => c,
                    None => return u64::MAX,
                };
                let ata = match crate::globals::ATA_DRIVER.as_mut() {
                    Some(a) => a,
                    None => return u64::MAX,
                };

                fs.find_file_in_directory(0, path, cache, ata)
            };

            match result {
                Ok(inode_num) => {
                    serial_println!("[syscall] sys_open -> inode {}", inode_num);
                    inode_num as u64
                }
                Err(_) => {
                    serial_println!("[syscall] sys_open -> File not found");
                    u64::MAX
                }
            }
        }

        // ---- sys_readfile(inode: u64, buf: *mut u8, count: usize) -> bytes_read ----
        11 => {
            let inode_num = rbx as u32;
            let buf_ptr = rcx as *mut u8;
            let count = _rdx as usize;

            let buf_addr = rcx;
            if buf_addr < 0x400000 || buf_addr.saturating_add(count as u64) > 0x800000 || count > 4096 {
                serial_println!("[syscall] sys_readfile: bad buffer 0x{:x} len {}", buf_addr, count);
                return u64::MAX;
            }

            serial_println!("[syscall] sys_readfile(inode={}, count={})", inode_num, count);

            use alloc::vec::Vec;
            let mut temp_buf = Vec::with_capacity(count);
            temp_buf.resize(count, 0u8);

            let result = unsafe {
                let fs = match crate::globals::NEODOS_FS.as_mut() {
                    Some(fs) => fs,
                    None => return u64::MAX,
                };
                let cache = match crate::globals::BLOCK_CACHE.as_mut() {
                    Some(c) => c,
                    None => return u64::MAX,
                };
                let ata = match crate::globals::ATA_DRIVER.as_mut() {
                    Some(a) => a,
                    None => return u64::MAX,
                };

                fs.read_file_to_buf(inode_num, &mut temp_buf, cache, ata)
            };

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

        // ---- sys_writefile(inode: u64, buf: *const u8, count: usize) -> bytes_written ----
        12 => {
            let inode_num = rbx as u32;
            let buf_ptr = rcx as *const u8;
            let count = _rdx as usize;

            let buf_addr = rcx;
            if buf_addr < 0x400000 || buf_addr.saturating_add(count as u64) > 0x800000 || count > 4096 {
                serial_println!("[syscall] sys_writefile: bad buffer 0x{:x} len {}", buf_addr, count);
                return u64::MAX;
            }

            serial_println!("[syscall] sys_writefile(inode={}, count={})", inode_num, count);

            use alloc::vec::Vec;
            let mut temp_buf = Vec::with_capacity(count);
            temp_buf.resize(count, 0u8);

            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr, temp_buf.as_mut_ptr(), count);
            }

            let result = unsafe {
                let fs = match crate::globals::NEODOS_FS.as_mut() {
                    Some(fs) => fs,
                    None => return u64::MAX,
                };
                let cache = match crate::globals::BLOCK_CACHE.as_mut() {
                    Some(c) => c,
                    None => return u64::MAX,
                };
                let ata = match crate::globals::ATA_DRIVER.as_mut() {
                    Some(a) => a,
                    None => return u64::MAX,
                };

                fs.write_file(inode_num, &temp_buf, cache, ata)
            };

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

        _ => {
            serial_println!("[syscall] unknown syscall RAX={}", rax);
            u64::MAX
        }
    }
}

pub fn wake_blocked_readers() {
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
}
