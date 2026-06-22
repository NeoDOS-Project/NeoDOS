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
    Spawn = 7,
    ReadDir = 8,
    WaitPid = 9,
    Open = 10,
    MkDir = 25,
    Unlink = 26,
    RmDir = 27,
    Rename = 28,
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
    GetCpuInfo = 24,
    WaitAlertable = 40,
    SleepEx = 41,
    Poweroff = 42,
    GetVersion = 43,
    GetDateTime = 44,
    GetMemInfo = 45,
    GetVolumeLabel = 46,
    ChDirParent = 47,
    KObjEnum = 48,
    SetKeyboardLayout = 49,
    SetPriority = 51,
    KillProcess = 52,
    SetExceptionHandler = 29,
    CursorBlink = 53,
    SetVolumeLabel = 54,
    GetDrives = 33,
}

impl SyscallNum {
    pub const MAX_VALID: u64 = 54;

    pub fn from_u64(n: u64) -> Option<Self> {
        match n {
            0 => Some(Self::Exit),
            1 => Some(Self::Write),
            2 => Some(Self::Yield),
            3 => Some(Self::GetPid),
            4 => Some(Self::Read),
            5 => Some(Self::Pipe),
            6 => Some(Self::Dup2),
            7 => Some(Self::Spawn),
            8 => Some(Self::ReadDir),
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
            24 => Some(Self::GetCpuInfo),
            25 => Some(Self::MkDir),
            26 => Some(Self::Unlink),
            27 => Some(Self::RmDir),
             28 => Some(Self::Rename),
             29 => Some(Self::SetExceptionHandler),
             40 => Some(Self::WaitAlertable),
            41 => Some(Self::SleepEx),
            42 => Some(Self::Poweroff),
            43 => Some(Self::GetVersion),
            44 => Some(Self::GetDateTime),
            45 => Some(Self::GetMemInfo),
             33 => Some(Self::GetDrives),
            46 => Some(Self::GetVolumeLabel),
            47 => Some(Self::ChDirParent),
             48 => Some(Self::KObjEnum),
             49 => Some(Self::SetKeyboardLayout),
             51 => Some(Self::SetPriority),
             52 => Some(Self::KillProcess),
              53 => Some(Self::CursorBlink),
              54 => Some(Self::SetVolumeLabel),
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
        0, 1, 2, 3, 4, 5, 6, 7, 8,
        9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29,
        33,
         40, 41, 42, 43, 44, 45, 46, 47, 48, 49,
         50, 51, 52, 53, 54,
    ];
    // Reserved syscall slots that MUST be None
    const RESERVED: &[u64] = &[];

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

/// Called from asm syscall handler. Returns 1 if the current thread was
/// terminated by the exit syscall but exit_to_kernel was not requested
/// (non-last thread in a multi-threaded process). Returns 0 otherwise.
/// Used to determine whether to reschedule instead of returning to user mode.
#[no_mangle]
pub extern "C" fn is_thread_terminated() -> u64 {
    use crate::scheduler::ThreadState;
    let s = scheduler::current_scheduler();
    let mut scheduler = s.lock();
    if scheduler.current_tid > 0 {
        if let Some(k) = scheduler.current_kthread_mut() {
            if k.state == ThreadState::Terminated {
                return 1;
            }
        }
    }
    0
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
    if ptr >= crate::arch::x64::paging::USER_BASE && ptr.saturating_add(len) <= crate::arch::x64::paging::USER_LIMIT {
        return true;
    }
    // NXL region (shared libraries): 0x1E000000..0x1E200000
    if ptr >= 0x1E000000 && ptr.saturating_add(len) <= 0x1E200000 {
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

fn copy_handle_entry_for_child(entry: &crate::handle::HandleEntry) -> crate::handle::HandleEntry {
    match entry.kind {
        crate::handle::HANDLE_PIPE_READ => {
            crate::pipe::PIPE_MANAGER.inc_read_ref(entry.id as u8);
        }
        crate::handle::HANDLE_PIPE_WRITE => {
            crate::pipe::PIPE_MANAGER.inc_write_ref(entry.id as u8);
        }
        _ => {}
    }
    *entry
}

fn handler_spawn(regs: Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }
    let stdin_fd = regs.rcx as u8;
    let stdout_fd = regs.rdx as u8;
    let stderr_fd = regs.r8 as u8;
    crate::serial_println!("[SPAWN] path='{}' stdin_fd={} stdout_fd={} stderr_fd={}",
        path_str, stdin_fd, stdout_fd, stderr_fd);

    // Read binary from VFS
    const MAX_BIN: usize = 65536;
    static mut BIN_BUF: [u8; MAX_BIN] = [0u8; MAX_BIN];
    let bin_size = crate::globals::with_vfs(|vfs| {
        match vfs.resolve_path(&path_str) {
            Ok((drive_idx, node)) => {
                if (node.mode & crate::fs::vfs::MODE_FILE) == 0 { return 0; }
                unsafe {
                    match vfs.read(drive_idx, node.inode, 0, &mut BIN_BUF) {
                        Ok(n) => { if n > MAX_BIN { 0 } else { n } }
                        Err(_) => 0,
                    }
                }
            }
            Err(_) => 0,
        }
    });
    if bin_size < 4 {
        return err_to_u64(SyscallError::NoEnt);
    }

    // Save NeoInit's code+stack (slot 0: 0x400000..0x420000, 128 KB)
    const SAVE_SIZE: usize = 0x20000;
    let mut save_buf = alloc::vec![0u8; SAVE_SIZE];
    unsafe {
        core::ptr::copy_nonoverlapping(0x400000 as *const u8, save_buf.as_mut_ptr(), SAVE_SIZE);
    }

    // Load ELF at 0x400000 (overwrites NeoInit)
    let data = unsafe { &BIN_BUF[..bin_size] };
    let result = match crate::elf::load_elf(data, None) {
        Ok(r) => r,
        Err(_) => {
            unsafe {
                core::ptr::copy_nonoverlapping(save_buf.as_ptr(), 0x400000 as *mut u8, SAVE_SIZE);
            }
            return err_to_u64(SyscallError::Inval);
        }
    };
    let entry = result.entry;

    // Collect parent handle entries for fd redirection before creating child
    let (parent_stdin_entry, parent_stdout_entry, parent_stderr_entry) = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        let get_parent_entry = |fd: u8| -> Option<crate::handle::HandleEntry> {
            if let Some(ep) = lock.current_eprocess() {
                Some(ep.handle_table.get(fd))
            } else { None }
        };
        let sin = if stdin_fd != 0xFF { get_parent_entry(stdin_fd) } else { None };
        let sout = if stdout_fd != 0xFF { get_parent_entry(stdout_fd) } else { None };
        let serr = if stderr_fd != 0xFF { get_parent_entry(stderr_fd) } else { None };
        (sin, sout, serr)
    });

    // Validate redirected fds
    if let Some(ref e) = parent_stdin_entry {
        if e.kind == crate::handle::HANDLE_CLOSED {
            unsafe { core::ptr::copy_nonoverlapping(save_buf.as_ptr(), 0x400000 as *mut u8, SAVE_SIZE); }
            return err_to_u64(SyscallError::BadF);
        }
    }
    if let Some(ref e) = parent_stdout_entry {
        if e.kind == crate::handle::HANDLE_CLOSED {
            unsafe { core::ptr::copy_nonoverlapping(save_buf.as_ptr(), 0x400000 as *mut u8, SAVE_SIZE); }
            return err_to_u64(SyscallError::BadF);
        }
    }
    if let Some(ref e) = parent_stderr_entry {
        if e.kind == crate::handle::HANDLE_CLOSED {
            unsafe { core::ptr::copy_nonoverlapping(save_buf.as_ptr(), 0x400000 as *mut u8, SAVE_SIZE); }
            return err_to_u64(SyscallError::BadF);
        }
    }

    // Allocate user slot for child
    let slot = match crate::arch::x64::paging::alloc_user_slot() {
        Some(s) => s,
        None => {
            unsafe {
                core::ptr::copy_nonoverlapping(save_buf.as_ptr(), 0x400000 as *mut u8, SAVE_SIZE);
            }
            return err_to_u64(SyscallError::NoMem);
        }
    };

    // Get current process info for child's cwd
    let (cwd_drive, cwd_path, parent_pid) = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler().lock();
        let pid = s.current_pid();
        let cwd = if let Some(ep) = s.find_eprocess(pid) {
            (ep.cwd_drive, ep.cwd_path.clone())
        } else {
            (2u8, String::from("\\"))
        };
        (cwd.0, cwd.1, pid)
    });

    // Spawn child process
    let child_pid = crate::usermode::spawn_usermode(
        entry, slot.stack_top, slot.slot_idx,
        cwd_drive, &cwd_path, parent_pid,
    );
    crate::serial_println!("[SPAWN] child PID={}", child_pid);

    // Apply fd redirection: customize child's handle table
    if stdin_fd != 0xFF || stdout_fd != 0xFF || stderr_fd != 0xFF {
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut lock = s.lock();
            if let Some(ep) = lock.find_eprocess_mut(child_pid) {
                if let Some(ref entry) = parent_stdin_entry {
                    let child_entry = copy_handle_entry_for_child(entry);
                    ep.handle_table.set(0, child_entry);
                }
                if let Some(ref entry) = parent_stdout_entry {
                    let child_entry = copy_handle_entry_for_child(entry);
                    ep.handle_table.set(1, child_entry);
                }
                if let Some(ref entry) = parent_stderr_entry {
                    let child_entry = copy_handle_entry_for_child(entry);
                    ep.handle_table.set(2, child_entry);
                }
            }
        });
    }

    // Get child's kernel stack top, TID, and NeoInit's kernel stack top
    let (child_kernel_top, child_tid, neoinit_kernel_top) = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler().lock();
        let mut ct = 0u64;
        let mut c_tid = 0u32;
        let mut neo_top = 0u64;
        for th in s.kthreads.iter() {
            if let Some(k) = th {
                if k.pid == child_pid && k.tid > 0 {
                    ct = k.kernel_stack_top;
                    c_tid = k.tid;
                }
                if k.pid == parent_pid && k.tid > 0 {
                    neo_top = k.kernel_stack_top;
                }
            }
        }
        (ct, c_tid, neo_top)
    });

    // Set scheduler current_tid to child and make Running
    crate::hal::without_interrupts(|| {
        let mut s = crate::scheduler::current_scheduler().lock();
        for th in s.kthreads.iter() {
            if let Some(k) = th {
                if k.tid == child_tid {
                    s.current_tid = k.tid;
                    break;
                }
            }
        }
        if let Some(k) = s.current_kthread_mut() {
            k.state = ThreadState::Running;
        }
    });

    // Set TSS.RSP0 to child's kernel stack for Ring 3→0 transitions
    crate::arch::x64::gdt::set_kernel_stack(child_kernel_top);

    // Set WAIT_PID so child's sys_exit triggers request_exit_to_kernel()
    crate::usermode::set_wait_pid(child_pid);

    // ── Enter child (blocks until exit) ──
    crate::serial_println!("[SPAWN] entering child at entry=0x{:x}, stack=0x{:x}", entry, slot.stack_top);
    crate::usermode::execute_usermode(entry, slot.stack_top);

    // ── Child exited ──
    crate::serial_println!("[SPAWN] child exited, restoring NeoInit");

    // Restore TSS.RSP0 to NeoInit's kernel stack
    crate::arch::x64::gdt::set_kernel_stack(neoinit_kernel_top);
    // Restore scheduler current_tid to parent (NeoInit's TID)
    crate::hal::without_interrupts(|| {
        let mut s = crate::scheduler::current_scheduler().lock();
        for th in s.kthreads.iter() {
            if let Some(k) = th {
                if k.pid == parent_pid && k.tid > 0 {
                    s.current_tid = k.tid;
                    break;
                }
            }
        }
    });

    // Restore NeoInit code+stack
    unsafe {
        core::ptr::copy_nonoverlapping(save_buf.as_ptr(), 0x400000 as *mut u8, SAVE_SIZE);
    }

    // Cleanup child (kernel-side)
    crate::scheduler::cleanup_terminated_process(child_pid);
    crate::serial_println!("[SPAWN] done, returning child PID {}", child_pid);

    child_pid as u64
}

fn handler_poweroff(_regs: Registers) -> u64 {
    crate::serial_println!("[POWEROFF] sys_poweroff called — shutting down");
    crate::globals::flush_cache_if_needed();
    let _ = crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_SHUTDOWN,
        crate::eventbus::SOURCE_KERNEL,
        0, 0, 0, 0,
    );
    crate::eventbus::EVENT_BUS.dispatch_pending();
    crate::hal::poweroff();
}

fn handler_exit(regs: Registers) -> u64 {
    let code = regs.rbx;
    crate::hal::without_interrupts(|| {
        //crate::serial_println!("[EXIT] enter");
        let s = crate::scheduler::current_scheduler();
        let mut scheduler = s.lock();
        let tid = scheduler.current_tid;
        if tid > 0 {
            //crate::serial_println!("[EXIT] tid={} start", tid);
            if let Some(k) = scheduler.current_kthread_mut() {
                k.state = ThreadState::Terminated;
            }
            //crate::serial_println!("[EXIT] marked Terminated");
            let pid = scheduler.current_pid();
            //crate::serial_println!("[EXIT] pid={}", pid);
            if pid > 0 {
                //crate::serial_println!("[EXIT] getting eproc");
                let eproc = scheduler.current_eprocess_mut();
                //crate::serial_println!("[EXIT] got eproc: {:?}", eproc.is_some());
                if let Some(ep) = eproc {
                    ep.thread_count = ep.thread_count.saturating_sub(1);
                    ep.exit_code = code as i64;
                    //crate::serial_println!("[EXIT] thread_count={}", ep.thread_count);
                    if ep.thread_count == 0 {
                        //crate::serial_println!("[EXIT] freeing resources");
                        if let Some(slot) = ep.user_slot.take() {
                            //crate::serial_println!("[EXIT] free_user_slot");
                            crate::arch::x64::paging::free_user_slot(slot);
                        }
                        if ep.heap_base != 0 {
                            //crate::serial_println!("[EXIT] heap_free_range");
                            crate::arch::x64::paging::heap_free_range(
                                ep.heap_base,
                                ep.heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE,
                            );
                            //crate::serial_println!("[EXIT] free_heap_slot");
                            let heap_idx = ((ep.heap_base
                                - crate::arch::x64::paging::PROCESS_HEAP_BASE)
                                / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as u8;
                            crate::arch::x64::paging::free_heap_slot(heap_idx);
                            ep.heap_base = 0;
                            ep.heap_break = 0;
                        }
                        //crate::serial_println!("[EXIT] mmap regions count={}", ep.mmap_regions.len());
                        for r in ep.mmap_regions.iter() {
                            //crate::serial_println!("[EXIT] mmap_free_range base=0x{:x}", r.base);
                            crate::arch::x64::paging::mmap_free_range(r.base, r.base + r.len);
                        }
                        ep.mmap_regions.clear();
                        ep.mmap_next = crate::arch::x64::paging::MMAP_BASE;
                        //crate::serial_println!("[EXIT] handle_table len={}", ep.handle_table.len());
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
                    //crate::serial_println!("[EXIT] after resource freeing");
                }
            }
            //crate::serial_println!("[EXIT] wake_thread_joiner");
            scheduler.wake_blocked_on_magic(tid | 0x8000_0000);
            //crate::serial_println!("[EXIT] checking: pid={} thread_count", pid);
            // Always request exit_to_kernel when the last thread exits,
            // regardless of whether someone is waiting via sys_waitpid.
            // Without this, the asm handler returns to user mode and the
            // NXL's nxl_sys_exit hits the privileged HLT instruction → GPF.
            if pid > 0 {
                let eproc = scheduler.current_eprocess();
                if eproc.map_or(true, |ep| ep.thread_count == 0) {
                    //crate::serial_println!("[EXIT] calling request_exit_to_kernel()");
                    crate::usermode::request_exit_to_kernel();
                    //crate::serial_println!("[EXIT] after request_exit_to_kernel");
                }
            }
        }
        //crate::serial_println!("[EXIT] done (after if tid > 0 block)");
    });
    //crate::serial_println!("[EXIT] returned from without_interrupts");
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
    set_need_resched();
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
                            crate::eventbus::EVENT_BUS.dispatch_pending();
                            if let Some(b) = crate::input::pop_byte() {
                                unsafe { buf_ptr.add(bytes_read).write(b); }
                                bytes_read += 1;
                                break;
                            }
                            unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
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

    if wait_pid == 0xFFFFFFFF {
        // Check for any terminated child without blocking
        let child_pid = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let scheduler = s.lock();
            let my_pid = scheduler.current_pid();
            for ep in scheduler.eprocesses.iter() {
                if let Some(ep) = ep {
                    if ep.parent_pid == my_pid && ep.thread_count == 0 {
                        return Some(ep.pid);
                    }
                }
            }
            None
        });

        if let Some(pid) = child_pid {
            crate::scheduler::cleanup_terminated_process(pid);
            return pid as u64;
        }
        // No terminated child — yield to let other threads run
        crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let mut lock = s.lock();
            let tid = lock.current_tid;
            if tid > 0 {
                if let Some(k) = lock.current_kthread_mut() {
                    if k.state == ThreadState::Running {
                        k.state = ThreadState::Ready;
                    }
                }
            }
        });
        set_need_resched();
        return 0;
    } else {
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
            unsafe { core::arch::asm!("sti; hlt; cli", options(nomem, nostack)); }
        }

        crate::scheduler::cleanup_terminated_process(wait_pid);
        0
    }
}

fn handler_open(regs: Registers) -> u64 {
    let path_ptr = regs.rbx as *const u8;
    let flags = regs.rcx;

    const O_CREAT: u64 = 1;

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

    fn resolve_path_inner(vfs: &mut crate::fs::vfs::Vfs, path: &str) -> Result<(usize, crate::fs::vfs::VfsNode), crate::fs::vfs::VfsError> {
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
    }

    let (drive_idx, node) = match crate::globals::with_vfs(|vfs| resolve_path_inner(vfs, path)) {
        Ok(result) => result,
        Err(_) => {
            if (flags & O_CREAT) != 0 {
                match crate::globals::with_vfs(|vfs| vfs.create(path)) {
                    Ok(_) => {}
                    Err(_) => return err_to_u64(SyscallError::Io),
                }
                match crate::globals::with_vfs(|vfs| resolve_path_inner(vfs, path)) {
                    Ok((drv, created_node)) => {
                        let entry = if (created_node.mode & crate::fs::vfs::MODE_FILE) != 0 {
                            crate::handle::HandleEntry::file(drv as u8, created_node.inode)
                        } else {
                            return err_to_u64(SyscallError::Inval);
                        };
                        let fd = crate::hal::without_interrupts(|| {
                            let s = scheduler::current_scheduler();
                            let mut lock = s.lock();
                            if let Some(ep) = lock.current_eprocess_mut() {
                                crate::handle::alloc_handle(&mut ep.handle_table, entry)
                            } else {
                                None
                            }
                        });
                        return match fd {
                            Some(fd) => {
                                crate::serial_println!("[OPEN-O_CREAT] fd={} for path={} inode={}",
                                    fd, path, created_node.inode);
                                fd as u64
                            }
                            None => err_to_u64(SyscallError::NoMem),
                        };
                    }
                    Err(_) => return err_to_u64(SyscallError::Io),
                }
            }
            return err_to_u64(SyscallError::NoEnt);
        }
    };

    let entry = if (node.mode & crate::fs::vfs::MODE_FILE) != 0 {
        crate::handle::HandleEntry::file(drive_idx as u8, node.inode)
    } else if (node.mode & crate::fs::vfs::MODE_DIR) != 0 {
        crate::handle::HandleEntry::dir(drive_idx as u8, node.inode)
    } else {
        return err_to_u64(SyscallError::Inval);
    };
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
        Some(fd) => {
            crate::serial_println!("[OPEN] fd={} for path={} inode={} kind={}",
                fd, path, node.inode, entry.kind);
            fd as u64
        }
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
    if entry.object_id != 0 {
        let _ = crate::object::ob_close_object(entry.object_id);
    }
    set_current_handle(fd, crate::handle::HandleEntry::closed());
    0
}

/// ABI-stable directory entry returned by sys_readdir (RAX=8).
/// Kernel writes exactly this struct into the user buffer.
#[repr(C)]
pub struct DirEntryRaw {
    pub inode: u32,
    pub mode: u16,
    pub size: u32,
    pub name: [u8; 260],
}

fn handler_readdir(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let buf_ptr = regs.rcx as *mut u8;

    if !is_user_ptr_valid(regs.rcx, core::mem::size_of::<DirEntryRaw>() as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let (drive_idx, dir_inode, current_index) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if entry.kind == crate::handle::HANDLE_DIR {
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

    let vfs_entry = crate::globals::with_vfs(|vfs| {
        vfs.readdir(drive_idx, dir_inode, current_index as usize)
    });

    match vfs_entry {
        Ok(Some(entry)) => {
            let raw = DirEntryRaw {
                inode: entry.node.inode,
                mode: entry.node.mode,
                size: entry.node.size,
                name: {
                    let mut n = [0u8; 260];
                    let name_bytes = entry.name.as_bytes();
                    let copy_len = name_bytes.len().min(259);
                    n[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
                    n
                },
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &raw as *const DirEntryRaw as *const u8,
                    buf_ptr,
                    core::mem::size_of::<DirEntryRaw>(),
                );
            }
            // Advance the directory index in the handle
            crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    ep.handle_table[fd as usize].offset = current_index + 1;
                }
            });
            1
        }
        Ok(None) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

fn handler_mkdir(regs: Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    let result = crate::globals::with_vfs(|vfs| {
        vfs.mkdir(&path_str)
    });

    match result {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

fn handler_unlink(regs: Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    let result = crate::globals::with_vfs(|vfs| {
        vfs.remove_file(&path_str)
    });

    match result {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

fn handler_rmdir(regs: Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    let result = crate::globals::with_vfs(|vfs| {
        vfs.remove_dir(&path_str)
    });

    match result {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

fn handler_rename(regs: Registers) -> u64 {
    let old_path = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    let new_path = match copy_user_string(regs.rcx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    if old_path.is_empty() || new_path.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    // Extract leaf name from new_path (strip directory part if present)
    let new_leaf = match new_path.rfind(|c| c == '\\' || c == '/') {
        Some(idx) => &new_path[idx + 1..],
        None => &new_path,
    };
    if new_leaf.is_empty() {
        return err_to_u64(SyscallError::Inval);
    }

    let result = crate::globals::with_vfs(|vfs| {
        vfs.rename(&old_path, new_leaf)
    });

    match result {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
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
    match resolve_chdir_target(path_str) {
        Ok((new_drive, new_cwd_path)) => {
            crate::scheduler::set_current_cwd(new_drive, &new_cwd_path);
            0
        }
        Err(err) => err_to_u64(err),
    }
}

fn handler_chdir_parent(regs: Registers) -> u64 {
    let path_str = match copy_user_string(regs.rbx) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    let (parent_pid, target_drive, target_path) = match resolve_chdir_target(path_str) {
        Ok((drive, path)) => {
            let parent_pid = crate::hal::without_interrupts(|| {
                let scheduler = crate::scheduler::current_scheduler().lock();
                let current_pid = scheduler.current_pid();
                scheduler.find_eprocess(current_pid).map(|ep| ep.parent_pid).unwrap_or(0)
            });
            if parent_pid == 0 {
                return err_to_u64(SyscallError::NoEnt);
            }
            (parent_pid, drive, path)
        }
        Err(err) => return err_to_u64(err),
    };

    if crate::scheduler::set_cwd_for_pid(parent_pid, target_drive, &target_path) {
        0
    } else {
        err_to_u64(SyscallError::NoEnt)
    }
}

/// ABI-stable KOBJ entry for sys_kobj_enum (RAX=48).
#[repr(C)]
struct KObjEntryRaw {
    id: u64,
    obj_type: u32,
    padding: u32,
    name: [u8; 24],
    refcount: u32,
    native_id: u64,
}

/// sys_kobj_enum (RAX=48): enumerate kernel objects.
/// RBX = buffer ptr, RCX = max entries.
/// Returns number of entries written (0 = none).
fn handler_kobj_enum(regs: Registers) -> u64 {
    let buf_ptr = regs.rbx;
    let max_entries = regs.rcx as usize;

    if buf_ptr == 0 || max_entries == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let entry_size = core::mem::size_of::<KObjEntryRaw>() as u64;
    if !is_user_ptr_valid(buf_ptr, entry_size.saturating_mul(max_entries as u64)) {
        return err_to_u64(SyscallError::Fault);
    }

    let snapshot = crate::kobj::kobj_iter_snapshot();
    let count = core::cmp::min(max_entries, snapshot.len());

    for i in 0..count {
        let (id, obj_type, name, refcount, native_id) = &snapshot[i];
        let raw = KObjEntryRaw {
            id: *id,
            obj_type: *obj_type as u32,
            padding: 0,
            name: *name,
            refcount: *refcount,
            native_id: *native_id,
        };
        unsafe {
            core::ptr::copy_nonoverlapping(
                &raw as *const KObjEntryRaw as *const u8,
                (buf_ptr as *mut u8).add(i * core::mem::size_of::<KObjEntryRaw>()),
                core::mem::size_of::<KObjEntryRaw>(),
            );
        }
    }
    count as u64
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

fn resolve_chdir_target(path_str: String) -> Result<(u8, String), SyscallError> {
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
        let colon = normalized.find(':').ok_or(SyscallError::Inval)?;
        let dl = normalized[..colon].chars().next().ok_or(SyscallError::Inval)?.to_ascii_uppercase();
        let idx = crate::fs::vfs::Vfs::drive_index(dl).ok_or(SyscallError::NoEnt)? as u8;
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
        Ok(()) => Ok((new_drive, new_cwd_path)),
        Err(_) => Err(SyscallError::NoEnt),
    }
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

    if entry == 0 || entry >= crate::arch::x64::paging::USER_LIMIT {
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

/// sys_wait_alertable (RAX=40): If user APC pending, dispatch it and return APC_ALERTED.
/// Otherwise, block the thread in an alertable state and return APC_ALERTED when woken.
fn handler_wait_alertable(_regs: Registers) -> u64 {
    // Check if any user APC is pending for this thread
    if crate::apc::has_pending_user_apcs() {
        crate::apc::dispatch_one_user_apc();
        return crate::apc::APC_ALERTED;
    }
    // No APC pending — block in alertable state
    crate::apc::block_current_alertable();
    // When woken, check for APC again
    if crate::apc::has_pending_user_apcs() {
        crate::apc::dispatch_one_user_apc();
        return crate::apc::APC_ALERTED;
    }
    // Woken by other means (timeout, etc.) — return 0
    0
}

/// sys_sleep_ex (RAX=41): Yield the CPU but remain alertable to APCs.
/// If an APC is pending, dispatch it and return APC_ALERTED.
fn handler_sleep_ex(_regs: Registers) -> u64 {
    // Check for pending user APCs before yielding
    if crate::apc::has_pending_user_apcs() {
        crate::apc::dispatch_one_user_apc();
        return crate::apc::APC_ALERTED;
    }
    // Yield — same as sys_yield but alertable
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        let tid = lock.current_tid;
        if tid > 0 {
            if let Some(k) = lock.current_kthread_mut() {
                if k.state == crate::scheduler::ThreadState::Running {
                    k.state = crate::scheduler::ThreadState::Ready;
                }
                let idx = (k.priority as usize).min(
                    crate::scheduler::PRIORITY_COUNT as usize - 1);
                k.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
            }
        }
    });
    crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
    // After reschedule, check for APC again
    if crate::apc::has_pending_user_apcs() {
        crate::apc::dispatch_one_user_apc();
        return crate::apc::APC_ALERTED;
    }
    0
}

/// Admin syscall stub (RAX=50). Placeholder for NDREG operations from user-space.
fn handler_ndreg(_regs: Registers) -> u64 {
    serial_println!("[SYS] sys_ndreg (RAX=50) called - admin stub");
    0
}

/// sys_set_keyboard_layout (RAX=49): change keyboard layout.
/// RBX = layout (0=US, 1=SP).
fn handler_set_keyboard_layout(regs: Registers) -> u64 {
    let layout = regs.rbx;
    if layout > 1 {
        return err_to_u64(SyscallError::Inval);
    }
    match crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_KEYB_LAYOUT,
        crate::eventbus::SOURCE_KERNEL,
        3, layout, 0, 0
    ) {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Again),
    }
}

/// sys_set_priority (RAX=51): set process scheduling priority (admin).
/// RBX = pid, RCX = priority (0-3).
fn handler_set_priority(regs: Registers) -> u64 {
    let pid = regs.rbx as u32;
    let priority = regs.rcx as u8;
    if priority > 3 {
        return err_to_u64(SyscallError::Inval);
    }
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        if lock.set_process_priority(pid, priority) {
            0
        } else {
            err_to_u64(SyscallError::NoEnt)
        }
    })
}

/// sys_kill_process (RAX=52): terminate a process by PID (admin).
/// RBX = pid.
fn handler_kill_process(regs: Registers) -> u64 {
    let pid = regs.rbx as u32;
    if pid == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        if lock.kill_pid(pid) {
            lock.wake_waiters(pid);
            0
        } else {
            err_to_u64(SyscallError::NoEnt)
        }
    })
}

/// sys_set_exception_handler (RAX=29): set the current thread's SEH handler.
/// RBX = handler_fn address (0 to clear), or user-space pointer to callback.
/// The handler receives (exception_type, fault_addr, fault_code) and must return
/// 0 (Continue), 1 (Terminate), or 2 (ReevaluateFilters).
/// Returns 0 on success, -1 if TEB not initialized.
fn handler_set_exception_handler(regs: Registers) -> u64 {
    let handler_fn_addr = regs.rbx;

    if handler_fn_addr == 0 {
        // Clear all handlers for this thread
        let teb_base = crate::scheduler::current_teb_base();
        if teb_base == 0 {
            return (-1i64) as u64;
        }
        let teb = teb_base as *mut crate::exception::Teb;
        unsafe {
            (*teb).exception_list = None;
        }
        return 0;
    }

    // Validate user pointer
    if !is_user_ptr_valid(handler_fn_addr, 1) {
        return err_to_u64(SyscallError::Fault);
    }

    let handler_fn = unsafe {
        core::mem::transmute::<u64, extern "C" fn(u32, u64, u64) -> u32>(handler_fn_addr)
    };

    match crate::exception::set_thread_exception_handler(Some(handler_fn)) {
        0 => 0,
        _ => (-1i64) as u64,
    }
}

/// sys_cursor_blink (RAX=53): enable or disable automatic cursor blinking.
/// RBX = 0 (disable) or 1 (enable).
fn handler_cursor_blink(regs: Registers) -> u64 {
    match regs.rbx {
        0 => { crate::console::set_cursor_blink(false); 0 }
        1 => { crate::console::set_cursor_blink(true); 0 }
        _ => err_to_u64(SyscallError::Inval),
    }
}

/// sys_getcpuinfo (RAX=24): copy CpuInfoFull to user buffer.
/// RBX = pointer to user buffer, RCX = buffer size (for validation).
fn handler_get_cpuinfo(regs: Registers) -> u64 {
    let buf_ptr = regs.rbx;
    let _buf_size = regs.rcx as u64;
    let struct_size = core::mem::size_of::<crate::cpu::CpuInfoFull>() as u64;

    if buf_ptr == 0 || !is_user_ptr_valid(buf_ptr, struct_size) {
        return err_to_u64(SyscallError::Fault);
    }

    let info = crate::cpu::get_cpu_info_full();
    unsafe {
        core::ptr::copy_nonoverlapping(
            &info as *const crate::cpu::CpuInfoFull as *const u8,
            buf_ptr as *mut u8,
            struct_size as usize,
        );
    }
    0
}

/// sys_get_version (RAX=43): copy kernel version string to user buffer.
/// RBX = user buffer ptr, RCX = buffer size.
fn handler_get_version(regs: Registers) -> u64 {
    let buf_ptr = regs.rbx;
    let buf_size = regs.rcx;
    if buf_ptr == 0 || buf_size == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    let ver = crate::KERNEL_VERSION.as_bytes();
    let copy_len = ver.len().min(buf_size as usize);
    if !is_user_ptr_valid(buf_ptr, copy_len as u64) {
        return err_to_u64(SyscallError::Fault);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(ver.as_ptr(), buf_ptr as *mut u8, copy_len);
    }
    ver.len() as u64
}

/// ABI-stable DateTime struct for sys_get_datetime (RAX=44).
#[repr(C)]
pub struct SysDateTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    pub valid: u8,
}

/// sys_get_datetime (RAX=44): copy RTC date/time to user buffer.
/// RBX = user buffer ptr.
fn handler_get_datetime(regs: Registers) -> u64 {
    let buf_ptr = regs.rbx;
    if buf_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    let sz = core::mem::size_of::<SysDateTime>() as u64;
    if !is_user_ptr_valid(buf_ptr, sz) {
        return err_to_u64(SyscallError::Fault);
    }
    let dt = crate::drivers::rtc_bridge::request_datetime();
    let sysdt = match dt {
        Some(d) => SysDateTime {
            second: d.second,
            minute: d.minute,
            hour: d.hour,
            day: d.day,
            month: d.month,
            year: d.year,
            valid: 1,
        },
        None => SysDateTime {
            second: 0, minute: 0, hour: 0,
            day: 0, month: 0, year: 0,
            valid: 0,
        },
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            &sysdt as *const SysDateTime as *const u8,
            buf_ptr as *mut u8,
            sz as usize,
        );
    }
    0
}

/// ABI-stable MemInfo struct for sys_get_meminfo (RAX=45).
#[repr(C)]
pub struct MemInfo {
    pub phys_max: u64,
    pub total_kib: u64,
    pub usable_kib: u64,
    pub free_kib: u64,
    pub used_kib: u64,
    pub reserved_kib: u64,
}

/// sys_get_meminfo (RAX=45): copy memory stats to user buffer.
/// RBX = user buffer ptr.
fn handler_get_meminfo(regs: Registers) -> u64 {
    let buf_ptr = regs.rbx;
    if buf_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    let sz = core::mem::size_of::<MemInfo>() as u64;
    if !is_user_ptr_valid(buf_ptr, sz) {
        return err_to_u64(SyscallError::Fault);
    }
    let stats = crate::memory::stats();
    let info = MemInfo {
        phys_max: stats.phys_max,
        total_kib: stats.total_kib,
        usable_kib: stats.usable_kib,
        free_kib: stats.free_kib,
        used_kib: stats.used_kib,
        reserved_kib: stats.reserved_kib,
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            &info as *const MemInfo as *const u8,
            buf_ptr as *mut u8,
            sz as usize,
        );
    }
    0
}

/// sys_get_volume_label (RAX=46): get the volume label for a drive.
/// RBX = drive_char (ASCII, e.g. 'C'), RCX = user buffer ptr, RDX = buffer size.
/// Returns number of bytes written (excluding null terminator).
fn handler_get_volume_label(regs: Registers) -> u64 {
    let drive_char = (regs.rbx & 0xFF) as u8 as char;
    let buf_ptr = regs.rcx as *mut u8;
    let buf_size = regs.rdx as usize;

    if buf_ptr.is_null() || buf_size == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(regs.rcx, buf_size as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let result = crate::globals::with_vfs(|vfs| {
        vfs.volume_label(drive_char.to_ascii_uppercase())
    });

    match result {
        Ok(label) => {
            let bytes = label.as_bytes();
            let copy_len = core::cmp::min(bytes.len(), buf_size.saturating_sub(1));
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr, copy_len);
                buf_ptr.add(copy_len).write(0);
            }
            copy_len as u64
        }
        Err(_) => {
            unsafe { buf_ptr.write(0); }
            0
        }
    }
}

/// sys_set_volume_label (RAX=54): set the volume label for a drive.
/// RBX = drive char, RCX = label string pointer.
fn handler_set_volume_label(regs: Registers) -> u64 {
    let drive_char = (regs.rbx & 0xFF) as u8 as char;
    let label_ptr = regs.rcx;

    if label_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let label = match copy_user_string(label_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if label.len() > 11 {
        return err_to_u64(SyscallError::Inval);
    }

    match crate::globals::with_vfs(|vfs| {
        vfs.set_volume_label(drive_char.to_ascii_uppercase(), &label)
    }) {
        Ok(()) => {
            crate::globals::NEED_CACHE_FLUSH.store(true, core::sync::atomic::Ordering::Relaxed);
            0
        }
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

/// ABI-stable drive info for sys_get_drives (RAX=33).
#[repr(C)]
struct DriveInfoRaw {
    letter: u8,
    present: u8,
    fs_type: [u8; 16],
    label: [u8; 32],
    total_sectors: u64,
}

/// sys_get_drives (RAX=33): enumerate mounted drives.
/// RBX = buffer ptr, RCX = max entries.
/// Returns number of entries written.
fn handler_get_drives(regs: Registers) -> u64 {
    let buf_ptr = regs.rbx;
    let max_entries = regs.rcx as usize;

    if buf_ptr == 0 || max_entries == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let entry_size = core::mem::size_of::<DriveInfoRaw>() as u64;
    if !is_user_ptr_valid(buf_ptr, entry_size.saturating_mul(max_entries as u64)) {
        return err_to_u64(SyscallError::Fault);
    }

    crate::globals::with_vfs(|vfs| {
        let mut count = 0usize;
        for i in 0..26 {
            if count >= max_entries {
                break;
            }
            if vfs.drives[i].is_some() {
                let letter = (b'A' + i as u8) as char;
                let label = vfs.volume_label(letter).unwrap_or_default();

                // Get fs_type and total_sectors from the filesystem trait
                let (fs_type_str, total_sectors) = {
                    let fs = vfs.drives[i].as_ref().unwrap();
                    let ft = fs.fs_type();
                    let ts = fs.total_sectors();
                    (ft, ts)
                };

                let mut fs_type_bytes = [0u8; 16];
                let fst = fs_type_str.as_bytes();
                let copy_len = fst.len().min(15);
                fs_type_bytes[..copy_len].copy_from_slice(&fst[..copy_len]);

                let mut label_bytes = [0u8; 32];
                let lbl = label.as_bytes();
                let lbl_len = lbl.len().min(31);
                label_bytes[..lbl_len].copy_from_slice(&lbl[..lbl_len]);

                let raw = DriveInfoRaw {
                    letter: i as u8 + b'A',
                    present: 1,
                    fs_type: fs_type_bytes,
                    label: label_bytes,
                    total_sectors,
                };

                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &raw as *const DriveInfoRaw as *const u8,
                        (buf_ptr as *mut u8).add(count * core::mem::size_of::<DriveInfoRaw>()),
                        core::mem::size_of::<DriveInfoRaw>(),
                    );
                }
                count += 1;
            }
        }
        count as u64
    })
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
        t[7] = Some(handler_spawn as SyscallFn);
        t[8] = Some(handler_readdir as SyscallFn);
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
        t[24] = Some(handler_get_cpuinfo as SyscallFn);
        t[25] = Some(handler_mkdir as SyscallFn);
        t[26] = Some(handler_unlink as SyscallFn);
        t[27] = Some(handler_rmdir as SyscallFn);
        t[28] = Some(handler_rename as SyscallFn);
        t[29] = Some(handler_set_exception_handler as SyscallFn);
        t[33] = Some(handler_get_drives as SyscallFn);
        t[40] = Some(handler_wait_alertable as SyscallFn);
        t[41] = Some(handler_sleep_ex as SyscallFn);
        t[42] = Some(handler_poweroff as SyscallFn);
        t[43] = Some(handler_get_version as SyscallFn);
        t[44] = Some(handler_get_datetime as SyscallFn);
        t[45] = Some(handler_get_meminfo as SyscallFn);
        t[46] = Some(handler_get_volume_label as SyscallFn);
        t[47] = Some(handler_chdir_parent as SyscallFn);
        t[48] = Some(handler_kobj_enum as SyscallFn);
        t[49] = Some(handler_set_keyboard_layout as SyscallFn);
        t[50] = Some(handler_ndreg as SyscallFn);
        t[51] = Some(handler_set_priority as SyscallFn);
        t[52] = Some(handler_kill_process as SyscallFn);
        t[53] = Some(handler_cursor_blink as SyscallFn);
        t[54] = Some(handler_set_volume_label as SyscallFn);
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
        t[7] = SyscallPermission::user();
        t[8] = SyscallPermission::user();
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
        t[24] = SyscallPermission::user();
        t[25] = SyscallPermission::user();
        t[26] = SyscallPermission::user();
        t[27] = SyscallPermission::user();
        t[28] = SyscallPermission::user();
        t[29] = SyscallPermission::user();
        t[33] = SyscallPermission::user();
        t[40] = SyscallPermission::user();
        t[41] = SyscallPermission::user();
        t[42] = SyscallPermission::user();
        t[43] = SyscallPermission::user();
        t[44] = SyscallPermission::user();
        t[45] = SyscallPermission::user();
        t[46] = SyscallPermission::user();
        t[47] = SyscallPermission::user();
        t[48] = SyscallPermission::user();
        t[49] = SyscallPermission::user();
        t[50] = SyscallPermission::admin();
        t[51] = SyscallPermission::admin();
        t[52] = SyscallPermission::admin();
        t[53] = SyscallPermission::user();
        t[54] = SyscallPermission::user();
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
        if let Some(ep) = lock.current_eprocess() {
            ep.token.is_admin_token()
        } else {
            false
        }
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
            0, 1, 2, 3, 4, 5, 6, 7, 8,
            9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28,
            33,
            40, 41, 42, 43, 44, 45, 46, 47, 48, 49,
        50, 51, 52, 53,
        ];
        const RESERVED: &[u64] = &[];
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

        // Verify new syscall entries exist
        test_true!(SYSCALL_TABLE[8].is_some());   // readdir
        test_true!(SYSCALL_TABLE[25].is_some());  // mkdir
        test_true!(SYSCALL_TABLE[26].is_some());  // unlink
        test_true!(SYSCALL_TABLE[27].is_some());  // rmdir
        test_true!(SYSCALL_TABLE[28].is_some());  // rename
    });

    // ── A4.6 Integration tests ──

    test_case!("spawn_hello_binary_path_resolve", {
        // Test that handler_spawn's VFS path resolution works for an existing .nxe
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let result = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path("C:\\Programs\\dir.nxe")
        });
        test_true!(result.is_ok());
        if let Ok((_, node)) = result {
            test_true!(node.mode & crate::fs::vfs::MODE_FILE != 0);
            test_true!(node.size >= 4);
        }
    });

    test_case!("spawn_with_fd_redirection_helpers", {
        // Test that the handle entry copy logic works for different handle types
        let read_entry = crate::handle::HandleEntry::pipe_read(1);
        let write_entry = crate::handle::HandleEntry::pipe_write(1);
        let file_entry = crate::handle::HandleEntry::file(2, 42);
        let dir_entry = crate::handle::HandleEntry::dir(2, 0);

        test_eq!(read_entry.kind, crate::handle::HANDLE_PIPE_READ);
        test_eq!(write_entry.kind, crate::handle::HANDLE_PIPE_WRITE);
        test_eq!(file_entry.kind, crate::handle::HANDLE_FILE);
        test_eq!(dir_entry.kind, crate::handle::HANDLE_DIR);

        // Test valid fd range: 0xFF means "no redirection"
        let no_redir: u8 = 0xFF;
        test_eq!(no_redir, 255);
        test_true!(no_redir != 0);

        // Test closed entry looked up → should return Check
        let closed = crate::handle::HandleEntry::closed();
        test_eq!(closed.kind, crate::handle::HANDLE_CLOSED);
    });

    test_case!("readdir_list_root", {
        // Test that opening root directory and reading entries works
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let entries = crate::globals::with_vfs(|vfs| {
            let (drive_idx, node) = vfs.resolve_path("C:\\")?;
            if node.mode & crate::fs::vfs::MODE_DIR == 0 {
                return Err(crate::fs::vfs::VfsError::NotADirectory);
            }
            let mut count = 0u32;
            for i in 0..100 {
                match vfs.readdir(drive_idx, node.inode, i) {
                    Ok(Some(entry)) => {
                        count += 1;
                        // Each entry should have a name and a valid inode
                        if entry.name.is_empty() || entry.node.inode == 0 {
                            return Err(crate::fs::vfs::VfsError::IOError);
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            Ok(count)
        });
        test_true!(entries.is_ok());
        if let Ok(count) = entries {
            test_true!(count > 0);
        }
    });

    test_case!("mkdir_rmdir_roundtrip", {
        // Test creating and removing a directory via VFS
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let test_dir = "C:\\Temp\\_A46TESTDIR";

        let mkdir_result = crate::globals::with_vfs(|vfs| {
            vfs.mkdir(test_dir)
        });
        test_true!(mkdir_result.is_ok());

        // Verify it exists
        let stat_result = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(test_dir)
        });
        test_true!(stat_result.is_ok());

        // Remove it
        let rmdir_result = crate::globals::with_vfs(|vfs| {
            vfs.remove_dir(test_dir)
        });
        test_true!(rmdir_result.is_ok());

        // Verify it's gone
        let stat_again = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(test_dir)
        });
        test_true!(stat_again.is_err());
    });

    test_case!("unlink_file", {
        // Test creating and deleting a file via VFS
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let test_file = "C:\\Temp\\_A46TESTFILE.TXT";

        // Create test file
        let create_result = crate::globals::with_vfs(|vfs| {
            vfs.create(test_file)
        });
        test_true!(create_result.is_ok());

        // Remove it
        let unlink_result = crate::globals::with_vfs(|vfs| {
            vfs.remove_file(test_file)
        });
        test_true!(unlink_result.is_ok());

        // Verify it's gone
        let stat_again = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(test_file)
        });
        test_true!(stat_again.is_err());
    });

    test_case!("rename_file", {
        // Test renaming a file via VFS
        if crate::globals::VFS.try_lock().is_none() { return Ok(()); }
        let old_name = "C:\\Temp\\_A46RENOLD.TXT";
        let new_name = "RENEWED.TXT";

        // Create test file
        let create_result = crate::globals::with_vfs(|vfs| {
            vfs.create(old_name)
        });
        test_true!(create_result.is_ok());

        // Rename it
        let rename_result = crate::globals::with_vfs(|vfs| {
            vfs.rename(old_name, new_name)
        });
        test_true!(rename_result.is_ok());

        // Old name should be gone
        let old_stat = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(old_name)
        });
        test_true!(old_stat.is_err());

        // New name should exist (in root directory C:\)
        let new_full = "C:\\Temp\\RENEWED.TXT";
        let new_stat = crate::globals::with_vfs(|vfs| {
            vfs.resolve_path(new_full)
        });
        test_true!(new_stat.is_ok());

        // Cleanup
        let _ = crate::globals::with_vfs(|vfs| {
            vfs.remove_file(new_full)
        });
    });

    // ── OB-004: handler_close via ObObject ──

    test_case!("handler_close_file", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Filesystem, "hclose_file", 0, 0, None
        ).unwrap();
        let result = crate::object::ob_close_object(id);
        test_true!(result.is_ok());
        test_true!(crate::object::ob_lookup(id).is_none());
    });

    test_case!("handler_close_pipe", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Pipe, "hclose_pipe", 0, 0, None
        ).unwrap();
        crate::object::ob_open_object(id, 0).unwrap();  // refcount 2
        crate::object::ob_close_object(id).unwrap();     // refcount 1 (kept alive)
        test_true!(crate::object::ob_lookup(id).is_some());
        crate::object::ob_close_object(id).unwrap();     // refcount 0 → destroyed
        test_true!(crate::object::ob_lookup(id).is_none());
    });
}
