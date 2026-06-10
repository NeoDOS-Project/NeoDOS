//! Syscall dispatch — INT 0x80
//!
//! SSDT (Syscall Service Dispatch Table) architecture: indexed dispatch array
//! replacing the monolithic match for better scalability and security.
//!
//! # ABI v0 (STABLE)
//!
//! Calling convention (Ring 3 → kernel):
//!   RAX = syscall number
//!   RBX = arg0, RCX = arg1, RDX = arg2, R8 = arg3, R9 = arg4
//!
//! Return value in RAX:
//!   Non-negative (≥ 0)  → success
//!   Negative (< 0)       → error (`-SyscallError`)

pub mod table;
pub mod permission;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use lazy_static::lazy_static;
use crate::serial_println;
use crate::scheduler::{self, ThreadState};

pub use table::{Registers, SyscallFn, MAX_SYSCALL};
pub use permission::{SyscallPermission, CAP_ADMIN};

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
    LoadLib = 21,
    ThreadCreate = 22,
    ThreadJoin = 23,
}

impl SyscallNum {
    pub const MAX_VALID: u64 = 23;

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
            21 => Some(Self::LoadLib),
            22 => Some(Self::ThreadCreate),
            23 => Some(Self::ThreadJoin),
            _ => None,
        }
    }
}

// ── Standard Error Codes ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum SyscallError {
    Inval = 1,
    NoEnt = 2,
    NoMem = 3,
    Acces = 4,
    BadF = 5,
    Fault = 6,
    NoSys = 7,
    Again = 8,
    Pipe = 9,
    Exist = 10,
    NotDir = 11,
    IsDir = 12,
    Io = 13,
    NoDev = 14,
    Busy = 15,
    Perm = 16, // Permission denied (admin syscall without token)
}

pub fn err_to_u64(e: SyscallError) -> u64 {
    (-(e as i64)) as u64
}

// ── ABI validation ──

pub fn validate_abi() {
    // Assigned syscall numbers that MUST have handlers
    const ASSIGNED: &[u64] = &[
        0, 1, 2, 3, 4, 5, 6,
        9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
        50,
    ];
    // Reserved syscall slots that MUST be None
    const RESERVED: &[u64] = &[7, 8];

    // Verify assigned syscalls have handlers
    for &n in ASSIGNED {
        assert!(
            SYSCALL_TABLE[n as usize].is_some(),
            "SSDT missing handler for assigned syscall {}",
            n
        );
    }

    // Verify reserved slots are empty
    for &n in RESERVED {
        assert!(
            SYSCALL_TABLE[n as usize].is_none(),
            "SSDT reserved slot {} must be None",
            n
        );
    }

    // Verify error encoding
    assert!((err_to_u64(SyscallError::Inval) as i64) < 0);
    assert!((err_to_u64(SyscallError::NoEnt) as i64) < 0);
    assert!((err_to_u64(SyscallError::Perm) as i64) < 0);

    crate::serial_println!("[SYS] SSDT validated ({} assigned, {} reserved)",
        ASSIGNED.len(), RESERVED.len());
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
    // Also set per-CPU flag via GS
    unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
}

#[no_mangle]
pub extern "C" fn clear_need_resched() -> bool {
    crate::globals::flush_cache_if_needed();
    crate::work_queue::WORK_QUEUE.process_high();
    // A2.5: Dispatch pending DPCs at DISPATCH_LEVEL on syscall return
    crate::dpc::dpc_dispatch_pending();
    crate::eventbus::EVENT_BUS.dispatch_pending();
    // Clear both global and per-CPU flags
    let prev_global = NEED_RESCHED.swap(false, Ordering::SeqCst);
    unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(false); }
    prev_global
}

#[no_mangle]
pub extern "C" fn syscall_try_resched(current_rsp: u64) -> u64 {
    if cfg!(feature = "validation") && crate::invariants::is_in_timer_irq() {
        crate::serial_println!("[SYS] resched called from timer IRQ context!");
    }

    let has_non_idle = crate::hal::without_interrupts(|| {
        let scheduler = scheduler::current_scheduler().lock();
        scheduler.has_non_idle_threads()
    });

    if !has_non_idle {
        return current_rsp;
    }

    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut scheduler = s.lock();

        let tid = scheduler.current_tid;
        if tid > 0 {
            if let Some(k) = scheduler.current_kthread_mut() {
                k.rsp = current_rsp;
                if k.state == ThreadState::Running {
                    k.state = ThreadState::Ready;
                } else if cfg!(feature = "validation") {
                    crate::serial_println!("[SYS] Context switch from non-Running state: {:?}", k.state);
                }
            }
        }

        let next = scheduler.schedule();

        let next_ks_top = unsafe { (*next).kernel_stack_top };
        crate::arch::x64::gdt::set_kernel_stack(next_ks_top);

        let next_rsp = unsafe { (*next).rsp };
        crate::trace_cswitch!(tid as u64, unsafe { (*next).tid } as u64);
        next_rsp
    })
}

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

pub(crate) fn is_user_ptr_valid(ptr: u64, len: u64) -> bool {
    if ptr >= 0x400000 && ptr.saturating_add(len) <= 0x800000 {
        return true;
    }
    let (heap_base, heap_break) = crate::scheduler::current_process_heap_range();
    if heap_base != 0 && ptr >= heap_base && ptr.saturating_add(len) <= heap_break {
        return true;
    }
    let regions = crate::scheduler::current_process_mmap_regions();
    for r in &regions {
        if ptr >= r.base && ptr.saturating_add(len) <= r.base + r.len {
            return true;
        }
    }
    false
}

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

// ═══════════════════════════════════════════════════════════════════════
// SSDT Handlers — each wraps the original match-arm logic
// ═══════════════════════════════════════════════════════════════════════

fn handler_exit(regs: Registers) -> u64 {
    let code = regs.rbx;
    crate::hal::without_interrupts(|| {
        crate::serial_println!("[EXIT] enter");
        let s = crate::scheduler::current_scheduler();
        let mut scheduler = s.lock();
        let tid = scheduler.current_tid;
        if tid > 0 {
            crate::serial_println!("[EXIT] tid={} start", tid);
            if let Some(k) = scheduler.current_kthread_mut() {
                k.state = ThreadState::Terminated;
            }
            crate::serial_println!("[EXIT] marked Terminated");
            let pid = scheduler.current_pid();
            crate::serial_println!("[EXIT] pid={}", pid);
            if pid > 0 {
                crate::serial_println!("[EXIT] getting eproc");
                let eproc = scheduler.current_eprocess_mut();
                crate::serial_println!("[EXIT] got eproc: {:?}", eproc.is_some());
                if let Some(ep) = eproc {
                    ep.thread_count = ep.thread_count.saturating_sub(1);
                    ep.exit_code = code as i64;
                    crate::serial_println!("[EXIT] thread_count={}", ep.thread_count);
                    if ep.thread_count == 0 {
                        crate::serial_println!("[EXIT] freeing resources");
                        if let Some(slot) = ep.user_slot.take() {
                            crate::serial_println!("[EXIT] free_user_slot");
                            crate::arch::x64::paging::free_user_slot(slot);
                        }
                        if ep.heap_base != 0 {
                            crate::serial_println!("[EXIT] heap_free_range");
                            crate::arch::x64::paging::heap_free_range(
                                ep.heap_base,
                                ep.heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE,
                            );
                            crate::serial_println!("[EXIT] free_heap_slot");
                            let heap_idx = ((ep.heap_base
                                - crate::arch::x64::paging::PROCESS_HEAP_BASE)
                                / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as u8;
                            crate::arch::x64::paging::free_heap_slot(heap_idx);
                            ep.heap_base = 0;
                            ep.heap_break = 0;
                        }
                        crate::serial_println!("[EXIT] mmap regions count={}", ep.mmap_regions.len());
                        for r in ep.mmap_regions.iter() {
                            crate::serial_println!("[EXIT] mmap_free_range base=0x{:x}", r.base);
                            crate::arch::x64::paging::mmap_free_range(r.base, r.base + r.len);
                        }
                        ep.mmap_regions.clear();
                        ep.mmap_next = crate::arch::x64::paging::MMAP_BASE;
                        crate::serial_println!("[EXIT] handle_table len={}", ep.handle_table.len());
                        for i in 0..ep.handle_table.len() {
                            let h = ep.handle_table[i];
                            match h.kind {
                                crate::handle::HANDLE_PIPE_READ => {
                                    crate::pipe::PIPE_MANAGER.dec_read_ref(h.id as u8);
                                }
                                crate::handle::HANDLE_PIPE_WRITE => {
                                    crate::pipe::PIPE_MANAGER.dec_write_ref(h.id as u8);
                                }
                                _ => {}
                            }
                            ep.handle_table.set(i as u8, crate::handle::HandleEntry::closed());
                        }
                        scheduler.wake_waiters(pid);
                    }
                    crate::serial_println!("[EXIT] after resource freeing");
                }
            }
            crate::serial_println!("[EXIT] wake_thread_joiner");
            scheduler.wake_blocked_on_magic(tid | 0x8000_0000);
            crate::serial_println!("[EXIT] checking: pid={} wait_pid={}",
                pid, crate::usermode::current_wait_pid());
            if pid > 0 && pid == crate::usermode::current_wait_pid() {
                let eproc = scheduler.current_eprocess();
                if eproc.map_or(true, |ep| ep.thread_count == 0) {
                    crate::serial_println!("[EXIT] calling request_exit_to_kernel()");
                    crate::usermode::request_exit_to_kernel();
                    crate::serial_println!("[EXIT] after request_exit_to_kernel");
                } else {
                    crate::serial_println!("[EXIT] NOT calling: thread_count != 0");
                }
            } else {
                crate::serial_println!("[EXIT] NOT calling: pid={} wait_pid={}",
                    pid, crate::usermode::current_wait_pid());
            }
        }
        crate::serial_println!("[EXIT] done (after if tid > 0 block)");
    });
    crate::serial_println!("[EXIT] returned from without_interrupts");
    code
}

fn handler_write(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let ptr = regs.rcx as *const u8;
    let len = regs.rdx as usize;

    let entry = current_handle_entry(fd);

    match entry.kind {
        crate::handle::HANDLE_STDOUT | crate::handle::HANDLE_STDERR => {
            if !is_user_ptr_valid(regs.rcx, len as u64) || len > 4096 {
                return err_to_u64(SyscallError::Fault);
            }
            let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
            if let Ok(s) = core::str::from_utf8(slice) {
                crate::console::print_str(s);
            }
            len as u64
        }
        crate::handle::HANDLE_PIPE_WRITE => {
            if !is_user_ptr_valid(regs.rcx, len as u64) || len > 4096 {
                return err_to_u64(SyscallError::Fault);
            }
            let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
            match crate::pipe::PIPE_MANAGER.write(entry.id as u8, slice) {
                Ok(n) => n as u64,
                Err(_) => err_to_u64(SyscallError::Pipe),
            }
        }
        _ => {
            err_to_u64(SyscallError::BadF)
        }
    }
}

fn handler_yield(_regs: Registers) -> u64 {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        let tid = lock.current_tid;
        if tid > 0 {
            if let Some(k) = lock.current_kthread_mut() {
                if k.state == ThreadState::Running {
                    k.state = ThreadState::Ready;
                }
                let idx = (k.priority as usize).min(
                    crate::scheduler::PRIORITY_COUNT as usize - 1);
                k.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
            }
        }
    });
    NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
    0
}

fn handler_getpid(_regs: Registers) -> u64 {
    let pid = crate::hal::without_interrupts(|| {
        crate::scheduler::current_scheduler().lock().current_pid()
    });
    pid as u64
}

fn handler_read(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let buf_ptr = regs.rcx as *mut u8;
    let count = regs.rdx as usize;

    if !is_user_ptr_valid(regs.rcx, count as u64) || count > 4096 {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(fd);

    match entry.kind {
        crate::handle::HANDLE_STDIN => {
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
        crate::handle::HANDLE_PIPE_READ => {
            let pipe_id = entry.id as u8;
            let mut temp_buf = alloc::vec::Vec::with_capacity(count);
            temp_buf.resize(count, 0u8);
            loop {
                match crate::pipe::PIPE_MANAGER.read(pipe_id, &mut temp_buf) {
                    Ok(0) => {
                        return 0;
                    }
                    Ok(n) => {
                        unsafe {
                            core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr, n);
                        }
                        return n as u64;
                    }
                    Err(()) => {
                        crate::pipe::block_current_for_pipe(pipe_id);
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

fn handler_pipe(regs: Registers) -> u64 {
    let fds_ptr = regs.rbx as *mut u64;
    if !is_user_ptr_valid(regs.rbx, 16) {
        return err_to_u64(SyscallError::Fault);
    }

    let pipe_id = match crate::pipe::PIPE_MANAGER.alloc() {
        Some(pid) => pid,
        None => return err_to_u64(SyscallError::NoMem),
    };

    let handle_result = crate::hal::without_interrupts(|| -> Result<(u8, u8), ()> {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let read_entry = crate::handle::HandleEntry::pipe_read(pipe_id);
            let write_entry = crate::handle::HandleEntry::pipe_write(pipe_id);
            match crate::handle::alloc_two_handles(&mut ep.handle_table, read_entry, write_entry) {
                Some((r, w)) => Ok((r, w)),
                None => Err(()),
            }
        } else {
            Err(())
        }
    });

    let (rfd, wfd) = match handle_result {
        Ok(pair) => pair,
        Err(_) => {
            crate::pipe::PIPE_MANAGER.dec_read_ref(pipe_id);
            crate::pipe::PIPE_MANAGER.dec_write_ref(pipe_id);
            return err_to_u64(SyscallError::NoMem);
        }
    };

    crate::pipe::PIPE_MANAGER.inc_read_ref(pipe_id);
    crate::pipe::PIPE_MANAGER.inc_write_ref(pipe_id);

    unsafe {
        fds_ptr.write(rfd as u64);
        fds_ptr.add(1).write(wfd as u64);
    }
    0
}

fn handler_dup2(regs: Registers) -> u64 {
    let old_fd = regs.rbx as u8;
    let new_fd = regs.rcx as u8;

    let src_entry = current_handle_entry(old_fd);
    if src_entry.kind == crate::handle::HANDLE_CLOSED {
        return err_to_u64(SyscallError::BadF);
    }

    let dst_entry = current_handle_entry(new_fd);
    match dst_entry.kind {
        crate::handle::HANDLE_PIPE_READ => {
            crate::pipe::PIPE_MANAGER.dec_read_ref(dst_entry.id as u8);
        }
        crate::handle::HANDLE_PIPE_WRITE => {
            crate::pipe::PIPE_MANAGER.dec_write_ref(dst_entry.id as u8);
        }
        _ => {}
    }

    match src_entry.kind {
        crate::handle::HANDLE_PIPE_READ => {
            crate::pipe::PIPE_MANAGER.inc_read_ref(src_entry.id as u8);
        }
        crate::handle::HANDLE_PIPE_WRITE => {
            crate::pipe::PIPE_MANAGER.inc_write_ref(src_entry.id as u8);
        }
        _ => {}
    }

    set_current_handle(new_fd, src_entry);
    new_fd as u64
}

fn handler_waitpid(regs: Registers) -> u64 {
    let wait_pid = regs.rbx as u32;

    loop {
        let is_terminated = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let scheduler = s.lock();
            if let Some(ep) = scheduler.find_eprocess(wait_pid) {
                ep.thread_count == 0
            } else {
                true
            }
        });

        if is_terminated { break; }
        crate::hal::hlt_once();
    }

    crate::scheduler::cleanup_terminated_process(wait_pid);
    0
}

fn handler_open(regs: Registers) -> u64 {
    let path_ptr = regs.rbx as *const u8;
    let _flags = regs.rcx;

    if !is_user_ptr_valid(regs.rbx, 1) {
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

    let (drive_idx, node) = match crate::globals::with_vfs(|vfs| {
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
    }) {
        Ok(result) => result,
        Err(_) => return err_to_u64(SyscallError::NoEnt),
    };

    if (node.mode & crate::fs::vfs::MODE_FILE) == 0 {
        return err_to_u64(SyscallError::IsDir);
    }

    let entry = crate::handle::HandleEntry::file(drive_idx as u8, node.inode);
    let fd = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            crate::handle::alloc_handle(&mut ep.handle_table, entry)
        } else {
            None
        }
    });

    match fd {
        Some(fd) => fd as u64,
        None => err_to_u64(SyscallError::NoMem),
    }
}

fn handler_readfile(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let buf_ptr = regs.rcx as *mut u8;
    let count = regs.rdx as usize;

    if !is_user_ptr_valid(regs.rcx, count as u64) || count > 4096 {
        return err_to_u64(SyscallError::Fault);
    }

    let (drive_idx, inode_num, offset) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if entry.kind == crate::handle::HANDLE_FILE {
                (entry.extra as usize, entry.id, entry.offset)
            } else {
                return (usize::MAX, 0, 0);
            }
        } else {
            (usize::MAX, 0, 0)
        }
    });

    if drive_idx == usize::MAX {
        return err_to_u64(SyscallError::BadF);
    }

    let mut temp_buf = Vec::with_capacity(count);
    temp_buf.resize(count, 0u8);

    let result = crate::globals::with_vfs(|vfs| {
        vfs.read(drive_idx, inode_num, offset, &mut temp_buf)
    });

    match result {
        Ok(bytes_read) => {
            unsafe {
                core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr, bytes_read);
            }
            crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    ep.handle_table[fd as usize].offset += bytes_read as u64;
                }
            });
            bytes_read as u64
        }
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

fn handler_writefile(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let buf_ptr = regs.rcx as *const u8;
    let count = regs.rdx as usize;

    if !is_user_ptr_valid(regs.rcx, count as u64) || count > 4096 {
        return err_to_u64(SyscallError::Fault);
    }

    let (drive_idx, inode_num, offset) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if entry.kind == crate::handle::HANDLE_FILE {
                (entry.extra as usize, entry.id, entry.offset)
            } else {
                return (usize::MAX, 0, 0);
            }
        } else {
            (usize::MAX, 0, 0)
        }
    });

    if drive_idx == usize::MAX {
        return err_to_u64(SyscallError::BadF);
    }

    let mut temp_buf = Vec::with_capacity(count);
    temp_buf.resize(count, 0u8);
    unsafe {
        core::ptr::copy_nonoverlapping(buf_ptr, temp_buf.as_mut_ptr(), count);
    }

    let result = crate::globals::with_vfs(|vfs| {
        vfs.write(drive_idx, inode_num, offset, &temp_buf)
    });

    match result {
        Ok(bytes_written) => {
            crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    ep.handle_table[fd as usize].offset += bytes_written as u64;
                }
            });
            bytes_written as u64
        }
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

fn handler_close(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let entry = current_handle_entry(fd);
    match entry.kind {
        crate::handle::HANDLE_PIPE_READ => {
            crate::pipe::PIPE_MANAGER.dec_read_ref(entry.id as u8);
        }
        crate::handle::HANDLE_PIPE_WRITE => {
            crate::pipe::PIPE_MANAGER.dec_write_ref(entry.id as u8);
        }
        crate::handle::HANDLE_FILE | crate::handle::HANDLE_DEVICE | crate::handle::HANDLE_EVENT => {}
        _ => {}
    }
    set_current_handle(fd, crate::handle::HandleEntry::closed());
    0
}

fn handler_ioctl(regs: Registers) -> u64 {
    let device_id = regs.rbx as u32;
    let cmd = regs.rcx as u32;
    let buf_ptr = regs.rdx as *mut u8;
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

fn handler_register_device(regs: Registers) -> u64 {
    let device_id = regs.rbx as u32;
    let current_pid = crate::hal::without_interrupts(|| {
        crate::scheduler::current_scheduler().lock().current_pid()
    });

    if register_device(device_id, current_pid) {
        0
    } else {
        err_to_u64(SyscallError::Busy)
    }
}

fn handler_chdir(regs: Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
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

fn handler_getcwd(regs: Registers) -> u64 {
    let buf_ptr = regs.rbx as *mut u8;
    let buf_len = regs.rcx as usize;

    if !is_user_ptr_valid(regs.rbx, buf_len as u64) || buf_len > 4096 {
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

fn handler_brk(regs: Registers) -> u64 {
    let new_break = regs.rbx;
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

fn handler_mmap(regs: Registers) -> u64 {
    let _addr_hint = regs.rbx;
    let length = regs.rcx;
    let prot = regs.rdx as u16;
    let flags = regs.r8 as u16;
    let fd = regs.r9 as u8;

    if length == 0 || length > 0x100000 {
        return err_to_u64(SyscallError::Inval);
    }
    if prot & !3 != 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let is_anon = (flags & 1) != 0;

    if is_anon {
        let alloc_size = (length + 0xFFF) & !0xFFF;
        let region = crate::scheduler::MmapRegion {
            base: 0,
            len: alloc_size,
            prot,
            flags: 1,
            drive: 0,
            inode: 0,
            file_size: 0,
        };
        match crate::scheduler::add_current_mmap_region(region) {
            Some(base) => base,
            None => err_to_u64(SyscallError::NoMem),
        }
    } else {
        let (drive_idx, inode_num) = crate::hal::without_interrupts(|| {
            let s = scheduler::current_scheduler();
            let mut lock = s.lock();
            if let Some(ep) = lock.current_eprocess_mut() {
                let entry = ep.handle_table[fd as usize];
                if entry.kind == crate::handle::HANDLE_FILE {
                    return (entry.extra as usize, entry.id);
                }
            }
            (usize::MAX, 0)
        });

        if drive_idx == usize::MAX {
            return err_to_u64(SyscallError::BadF);
        }

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
            flags: 0,
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

fn handler_munmap(regs: Registers) -> u64 {
    let addr = regs.rbx;
    let length = regs.rcx;

    if length == 0 || addr & 0xFFF != 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let region = crate::scheduler::remove_current_mmap_region(addr);
    match region {
        Some(r) => {
            crate::scheduler::free_current_mmap_pages(r.base, r.len);
            0
        }
        None => err_to_u64(SyscallError::Inval),
    }
}

fn handler_loadlib(regs: Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    match crate::nxl::nxl_load(&path_str) {
        Some(base) => {
            serial_println!("[SYS] sys_loadlib '{}' => 0x{:x}", path_str, base);
            base
        }
        None => {
            serial_println!("[SYS] sys_loadlib FAILED '{}'", path_str);
            err_to_u64(SyscallError::NoEnt)
        }
    }
}

fn handler_thread_create(regs: Registers) -> u64 {
    let entry = regs.rbx;
    let user_stack = regs.rcx;

    if entry == 0 || entry >= 0x800000 {
        return err_to_u64(SyscallError::Inval);
    }

    let result = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        let pid = lock.current_pid();
        if pid == 0 {
            return Err(SyscallError::Inval);
        }

        let stack = if user_stack != 0 {
            user_stack
        } else {
            if let Some(ep) = lock.find_eprocess(pid) {
                if let Some(slot_idx) = ep.user_slot {
                    let slot_size = 0x20000u64;
                    let max_bin = 0x10000u64;
                    let user_stack_size = 0x10000u64;
                    let stack_top = crate::arch::x64::paging::USER_BASE
                        + slot_idx as u64 * slot_size
                        + max_bin + user_stack_size;
                    stack_top - 0x1000
                } else {
                    0
                }
            } else {
                0
            }
        };

        if stack == 0 {
            return Err(SyscallError::NoMem);
        }

        match lock.add_thread_to_process(pid, entry, stack) {
            Some(tid) => {
                serial_println!("[SYS] thread_create: PID {} TID {} entry=0x{:x} stack=0x{:x}",
                    pid, tid, entry, stack);
                Ok(tid)
            }
            None => Err(SyscallError::NoMem),
        }
    });

    match result {
        Ok(tid) => tid as u64,
        Err(e) => err_to_u64(e),
    }
}

fn handler_thread_join(regs: Registers) -> u64 {
    let target_tid = regs.rbx as u32;

    loop {
        let is_done = crate::hal::without_interrupts(|| {
            let s = scheduler::current_scheduler();
            let lock = s.lock();
            if let Some(k) = lock.find_kthread(target_tid) {
                k.state == ThreadState::Terminated
            } else {
                true
            }
        });

        if is_done { break; }

        crate::scheduler::block_current_for_thread(target_tid);
        return err_to_u64(SyscallError::Again);
    }

    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        lock.recycle_thread(target_tid);
    });

    0
}

/// Admin syscall stub (RAX=50). Placeholder for NDREG operations from user-space.
fn handler_ndreg(_regs: Registers) -> u64 {
    serial_println!("[SYS] sys_ndreg (RAX=50) called - admin stub");
    0
}

// ═══════════════════════════════════════════════════════════════════════
// SSDT + Permission Tables
// ═══════════════════════════════════════════════════════════════════════

lazy_static! {
    pub static ref SYSCALL_TABLE: [Option<SyscallFn>; 256] = {
        let mut t: [Option<SyscallFn>; 256] = [None; 256];
        t[0] = Some(handler_exit as SyscallFn);
        t[1] = Some(handler_write as SyscallFn);
        t[2] = Some(handler_yield as SyscallFn);
        t[3] = Some(handler_getpid as SyscallFn);
        t[4] = Some(handler_read as SyscallFn);
        t[5] = Some(handler_pipe as SyscallFn);
        t[6] = Some(handler_dup2 as SyscallFn);
        t[9] = Some(handler_waitpid as SyscallFn);
        t[10] = Some(handler_open as SyscallFn);
        t[11] = Some(handler_readfile as SyscallFn);
        t[12] = Some(handler_writefile as SyscallFn);
        t[13] = Some(handler_close as SyscallFn);
        t[14] = Some(handler_ioctl as SyscallFn);
        t[15] = Some(handler_register_device as SyscallFn);
        t[16] = Some(handler_chdir as SyscallFn);
        t[17] = Some(handler_getcwd as SyscallFn);
        t[18] = Some(handler_brk as SyscallFn);
        t[19] = Some(handler_mmap as SyscallFn);
        t[20] = Some(handler_munmap as SyscallFn);
        t[21] = Some(handler_loadlib as SyscallFn);
        t[22] = Some(handler_thread_create as SyscallFn);
        t[23] = Some(handler_thread_join as SyscallFn);
        t[50] = Some(handler_ndreg as SyscallFn);
        t
    };

    pub static ref SYSCALL_PERMISSIONS: [SyscallPermission; 256] = {
        let mut t: [SyscallPermission; 256] = [SyscallPermission::free(); 256];
        t[0] = SyscallPermission::user();
        t[1] = SyscallPermission::user();
        t[2] = SyscallPermission::user();
        t[3] = SyscallPermission::user();
        t[4] = SyscallPermission::user();
        t[5] = SyscallPermission::user();
        t[6] = SyscallPermission::user();
        t[9] = SyscallPermission::user();
        t[10] = SyscallPermission::user();
        t[11] = SyscallPermission::user();
        t[12] = SyscallPermission::user();
        t[13] = SyscallPermission::user();
        t[14] = SyscallPermission::user();
        t[15] = SyscallPermission::user();
        t[16] = SyscallPermission::user();
        t[17] = SyscallPermission::user();
        t[18] = SyscallPermission::user();
        t[19] = SyscallPermission::user();
        t[20] = SyscallPermission::user();
        t[21] = SyscallPermission::user();
        t[22] = SyscallPermission::user();
        t[23] = SyscallPermission::user();
        t[50] = SyscallPermission::admin();
        t
    };
}

/// Check whether the caller is allowed to invoke syscall `num`.
pub fn check_syscall_permission(num: u64, is_admin: bool) -> Result<(), u64> {
    if num >= 256 {
        return Err(err_to_u64(SyscallError::NoSys));
    }
    let perm = SYSCALL_PERMISSIONS[num as usize];
    if perm.admin && !is_admin {
        return Err(err_to_u64(SyscallError::Perm));
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Dispatch
// ═══════════════════════════════════════════════════════════════════════

fn is_current_admin() -> bool {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        let pid = lock.current_pid();
        pid == 0 || pid == 1
    })
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(rax: u64, rbx: u64, rcx: u64, rdx: u64, r8: u64, r9: u64) -> u64 {
    crate::trace_syscall!(rax, rbx, rcx, rdx);

    if rax >= 256 {
        serial_println!("[SYS] INVALID syscall number: {}", rax);
        return err_to_u64(SyscallError::NoSys);
    }

    // Permission check
    let is_admin = is_current_admin();
    if let Err(e) = check_syscall_permission(rax, is_admin) {
        serial_println!("[SYS] syscall {} denied (admin={})", rax, is_admin);
        return e;
    }

    // Look up handler in SSDT
    match SYSCALL_TABLE[rax as usize] {
        Some(handler) => {
            let regs = Registers::new(rax, rbx, rcx, rdx, r8, r9);
            handler(regs)
        }
        None => {
            serial_println!("[SYS] No handler for syscall {}", rax);
            err_to_u64(SyscallError::NoSys)
        }
    }
}

// ── Handle table helpers ──

fn current_handle_entry(fd: u8) -> crate::handle::HandleEntry {
    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let lock = s.lock();
        if let Some(ep) = lock.current_eprocess() {
            return ep.handle_table.get(fd);
        }
        crate::handle::HandleEntry::closed()
    })
}

fn set_current_handle(fd: u8, entry: crate::handle::HandleEntry) {
    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            ep.handle_table.set(fd, entry);
        }
    });
}

pub fn wake_blocked_readers() {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut scheduler = s.lock();
        scheduler.wake_blocked_on_magic(0xFFFFFFFF);
    });
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

pub fn register_syscall_table_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("syscall_table_sparse_dispatch", {
        // SSDT has entries for valid syscalls, None for unused slots
        test_true!(SYSCALL_TABLE[0].is_some());   // exit
        test_true!(SYSCALL_TABLE[50].is_some());  // ndreg (admin)
        test_true!(SYSCALL_TABLE[99].is_none());  // sparse: no handler
        test_true!(SYSCALL_TABLE[255].is_none()); // end of table
    });

    test_case!("syscall_permission_admin_check", {
        // Admin syscall without admin token → EPERM (Perm error)
        let result = check_syscall_permission(50, false);
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), err_to_u64(SyscallError::Perm));

        // Admin syscall WITH admin token → OK
        let result = check_syscall_permission(50, true);
        test_true!(result.is_ok());

        // Normal user syscall without admin token → OK (no admin flag)
        let result = check_syscall_permission(1, false);
        test_true!(result.is_ok());
    });

    test_case!("syscall_table_validation_boot", {
        const ASSIGNED: &[u64] = &[
            0, 1, 2, 3, 4, 5, 6,
            9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            50,
        ];
        const RESERVED: &[u64] = &[7, 8];
        for &n in ASSIGNED {
            test_true!(SYSCALL_TABLE[n as usize].is_some());
        }
        for &n in RESERVED {
            test_true!(SYSCALL_TABLE[n as usize].is_none());
        }
    });

    test_case!("syscall_enosys_unknown", {
        // Unknown syscall (99) via dispatch → NoSys error
        let result = syscall_dispatch(99, 0, 0, 0, 0, 0);
        test_eq!(result, err_to_u64(SyscallError::NoSys));

        // Out of range (255) → NoSys error
        let result = syscall_dispatch(255, 0, 0, 0, 0, 0);
        test_eq!(result, err_to_u64(SyscallError::NoSys));
    });

    test_case!("syscall_add_new_easy", {
        // Demonstrate that adding a new syscall is just 2 lines:
        // 1 handler function + 1 table entry + 1 permission entry.
        // Verify that existing syscalls work through SSDT.
        test_true!(SYSCALL_TABLE[0].is_some());   // exit
        test_true!(SYSCALL_TABLE[1].is_some());   // write
        test_true!(SYSCALL_TABLE[22].is_some());  // thread_create
        test_true!(SYSCALL_TABLE[23].is_some());  // thread_join

        // Verify permission entries exist
        test_eq!(SYSCALL_PERMISSIONS[0].ring_min, 3);
        test_eq!(SYSCALL_PERMISSIONS[50].admin, true);
    });
}
