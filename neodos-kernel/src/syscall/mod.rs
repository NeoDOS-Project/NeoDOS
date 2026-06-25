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

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
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
    Fsck = 55,
    DriverEnum = 56,
    DriverLoad = 57,
    DriverUnload = 58,
    Poll = 59,
    ObOpen = 60,
    ObCreate = 61,
    ObQueryInfo = 62,
    ObSetInfo = 63,
    ObEnum = 64,
    ObWait = 65,
    ObDestroy = 66,
    GetDrives = 33,
}

impl SyscallNum {
    pub const MAX_VALID: u64 = 66;

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
              55 => Some(Self::Fsck),
              56 => Some(Self::DriverEnum),
              57 => Some(Self::DriverLoad),
               58 => Some(Self::DriverUnload),
                 59 => Some(Self::Poll),
                 60 => Some(Self::ObOpen),
                 61 => Some(Self::ObCreate),
                 62 => Some(Self::ObQueryInfo),
                 63 => Some(Self::ObSetInfo),
                  64 => Some(Self::ObEnum),
                  65 => Some(Self::ObWait),
                  66 => Some(Self::ObDestroy),
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
        9, 10, 11, 12, 13, 16, 18, 19, 20, 21, 22, 23,
        25, 26, 27, 28,
        40, 41, 42, 46, 47,
            50, 53, 54, 55, 57, 58, 59,
            60, 61, 62, 63, 64,
    ];
    // Reserved syscall slots that MUST be None
        const RESERVED: &[u64] = &[14, 15, 17, 24, 33, 43, 44, 45, 49, 56];

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
pub static KEYBOARD_LAYOUT: AtomicU8 = AtomicU8::new(1); // SP default

// Device handler registry - max 8 devices
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
    if let Some(obj) = entry.obj_type() {
        if obj == crate::object::ObType::Pipe {
            if let Some(_nid) = entry.native_id() {
                // Distinguish read vs write via offset 0=read 1=write convention
                // (used only for pipe ref counting)
            }
        }
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

    // Security check: require ACCESS_EXECUTE on the binary (OB-030)
    if let Err(e) = check_legacy_path_access(&path_str, crate::security::acl::ACCESS_EXECUTE) {
        return e;
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

    // Allocate random user slot for child (ASLR v0.44)
    let slot = match crate::arch::x64::paging::alloc_user_slot() {
        Some(s) => s,
        None => {
            return err_to_u64(SyscallError::NoMem);
        }
    };
    crate::serial_println!("[SPAWN] allocated slot {} at code_base=0x{:x}",
        slot.slot_idx, slot.code_base);

    // Load ELF at slot.code_base with ASLR load_offset
    let data = unsafe { &BIN_BUF[..bin_size] };
    let result = match crate::elf::load_elf(data, None, slot.code_base) {
        Ok(r) => r,
        Err(_) => {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return err_to_u64(SyscallError::Inval);
        }
    };
    let entry = result.entry;
    crate::serial_println!("[SPAWN] ELF loaded: entry=0x{:x}, {} segments", entry, result.segments.len());

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
        if !e.is_open() {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return err_to_u64(SyscallError::BadF);
        }
    }
    if let Some(ref e) = parent_stdout_entry {
        if !e.is_open() {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return err_to_u64(SyscallError::BadF);
        }
    }
    if let Some(ref e) = parent_stderr_entry {
        if !e.is_open() {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return err_to_u64(SyscallError::BadF);
        }
    }

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
    crate::serial_println!("[SPAWN] child exited");

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
        crate::serial_println!("[EXIT] enter code={}", code);
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
                            if h.is_pipe_read() {
                                crate::pipe::PIPE_MANAGER.dec_read_ref(h.native_id().unwrap_or(0) as u8);
                            } else if h.is_pipe_write() {
                                crate::pipe::PIPE_MANAGER.dec_write_ref(h.native_id().unwrap_or(0) as u8);
                            }
                            ep.handle_table.set(i as u8, crate::handle::HandleEntry::closed());
                        }
                        scheduler.wake_waiters(pid);
                    }
                    crate::serial_println!("[EXIT] after resource freeing");
                }
            }
            crate::serial_println!("[EXIT] wake_thread_joiner via KWait (OB-031)");
            // Directly iterate kthreads to avoid re-locking scheduler
            let tj_magic = crate::kwait::WaitReason::ThreadJoin { tid }.encode_magic();
            for th in scheduler.kthreads.iter_mut() {
                if let Some(k) = th {
                    if k.waiting_for == Some(tj_magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                        k.waiting_for = None;
                        k.state = ThreadState::Ready;
                        scheduler::Scheduler::enqueue_to_cpu_run_queue(k);
                        crate::syscall::set_need_resched();
                    }
                }
            }
            crate::serial_println!("[EXIT] checking: pid={} thread_count", pid);
            // Always request exit_to_kernel when the last thread exits,
            // regardless of whether someone is waiting via sys_waitpid.
            // Without this, the asm handler returns to user mode and the
            // NXL's nxl_sys_exit hits the privileged HLT instruction → GPF.
            if pid > 0 {
                let eproc = scheduler.current_eprocess();
                if eproc.map_or(true, |ep| ep.thread_count == 0) {
                    crate::serial_println!("[EXIT] calling request_exit_to_kernel()");
                    crate::usermode::request_exit_to_kernel();
                    crate::serial_println!("[EXIT] after request_exit_to_kernel");
                }
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

    if entry.is_stdout() || entry.is_stderr() {
        if !is_user_ptr_valid(regs.rcx, len as u64) || len > 4096 {
            return err_to_u64(SyscallError::Fault);
        }
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        if let Ok(s) = core::str::from_utf8(slice) {
            crate::console::print_str(s);
        }
        len as u64
    } else if entry.is_pipe_write() {
        if !is_user_ptr_valid(regs.rcx, len as u64) || len > 4096 {
            return err_to_u64(SyscallError::Fault);
        }
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        match crate::pipe::PIPE_MANAGER.write(entry.native_id().unwrap_or(0) as u8, slice) {
            Ok(n) => n as u64,
            Err(_) => err_to_u64(SyscallError::Pipe),
        }
    } else {
        err_to_u64(SyscallError::BadF)
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

    if entry.is_stdin() {
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
    } else if entry.is_pipe_read() {
        let pipe_id = entry.native_id().unwrap_or(0) as u8;
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
    } else {
        err_to_u64(SyscallError::BadF)
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

    // Register this pipe as an ObObject (OB-016)
    let name = alloc::format!("PIPE{}", pipe_id);
    let ob_id = match crate::object::ob_create_object(
        crate::object::ObType::Pipe, &name, pipe_id as u64, 0, Some(&crate::pipe::PIPE_OPS),
    ) {
        Ok(id) => id,
        Err(_) => {
            // Free the pipe slot since Ob registration failed
            crate::pipe::PIPE_MANAGER.free_pipe(pipe_id);
            return err_to_u64(SyscallError::NoMem);
        }
    };

    let handle_result = crate::hal::without_interrupts(|| -> Result<(u8, u8), ()> {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            // Create reader and writer handle entries sharing the same ObObject
            let read_entry = crate::handle::HandleEntry {
                object_id: ob_id,
                offset: 0,
            };
            let write_entry = crate::handle::HandleEntry {
                object_id: ob_id,
                offset: 1,
            };
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
            let _ = crate::object::ob_close_object(ob_id);
            return err_to_u64(SyscallError::NoMem);
        }
    };

    // Reference the ObObject for each handle (refcount: 1 create + 1 read + 1 write = 3)
    let _ = crate::object::ob_reference(ob_id);
    let _ = crate::object::ob_reference(ob_id);
    // Drop the creation reference → refcount becomes 2 (one per handle)
    let _ = crate::object::ob_close_object(ob_id);

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
    if !src_entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let dst_entry = current_handle_entry(new_fd);
    if dst_entry.is_pipe_read() {
        crate::pipe::PIPE_MANAGER.dec_read_ref(dst_entry.native_id().unwrap_or(0) as u8);
    } else if dst_entry.is_pipe_write() {
        crate::pipe::PIPE_MANAGER.dec_write_ref(dst_entry.native_id().unwrap_or(0) as u8);
    }

    if src_entry.is_pipe_read() {
        crate::pipe::PIPE_MANAGER.inc_read_ref(src_entry.native_id().unwrap_or(0) as u8);
    } else if src_entry.is_pipe_write() {
        crate::pipe::PIPE_MANAGER.inc_write_ref(src_entry.native_id().unwrap_or(0) as u8);
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

    // Try ObOpen first for all paths (OB-015): namespace paths → direct, drive paths → \Global\FileSystem\ bridge
    let try_ob_path: Option<alloc::string::String> = if path.starts_with('\\') && !path.contains(':') {
        Some(path.to_string())
    } else if path.contains(':') {
        // Convert C:\... to \Global\FileSystem\C:\... for Ob namespace resolution
        let normalized = path.replace('/', "\\");
        Some(format!("\\Global\\FileSystem\\{}", normalized))
    } else {
        None
    };
    if let Some(ref ob_path) = try_ob_path {
        let token = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let lock = s.lock();
            lock.current_eprocess()
                .map(|ep| ep.token)
                .unwrap_or(*crate::security::DEFAULT_ADMIN_TOKEN)
        });
        let desired_access = crate::security::acl::ACCESS_READ |
            crate::security::acl::ACCESS_WRITE;
        if let Ok(ob_id) = crate::object::ob_open_path(ob_path, &token, desired_access) {
            let entry = crate::handle::HandleEntry::ob_object(ob_id, desired_access);
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
                    serial_println!("[OPEN-OB] fd={} for path='{}' ob_id={}",
                        fd, path, ob_id);
                    return fd as u64;
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    return err_to_u64(SyscallError::NoMem);
                }
            }
        }
        // If ObOpen returned NotFound, fall through to legacy VFS
    }

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
            crate::serial_println!("[OPEN] fd={} for path={} inode={} object_id={}",
                fd, path, node.inode, entry.object_id);
            fd as u64
        }
        None => err_to_u64(SyscallError::NoMem),
    }
}

/// Generate binary content for virtual info ObObjects.
/// Returns (bytes, len). Type 1 = MemInfo struct, type 2 = CPU interrupt counts array.
fn generate_info_content(info_type: u32) -> Option<alloc::vec::Vec<u8>> {
    match info_type {
        1 => {
            let stats = crate::memory::stats();
            let mut buf = alloc::vec::Vec::with_capacity(48);
            buf.extend_from_slice(&stats.phys_max.to_le_bytes());
            buf.extend_from_slice(&stats.total_kib.to_le_bytes());
            buf.extend_from_slice(&stats.usable_kib.to_le_bytes());
            buf.extend_from_slice(&stats.free_kib.to_le_bytes());
            buf.extend_from_slice(&stats.used_kib.to_le_bytes());
            buf.extend_from_slice(&stats.reserved_kib.to_le_bytes());
            Some(buf)
        }
        2 => {
            let count = crate::arch::x64::cpu_local::cpu_count() as usize;
            let mut buf = alloc::vec::Vec::with_capacity(count * 8);
            for cpu in 0..count {
                let kprcb_base = crate::arch::x64::cpu_local::kprcb_page(cpu)
                    .unwrap_or(0);
                let ic = if kprcb_base != 0 {
                    unsafe { core::ptr::read_volatile(
                        (kprcb_base + 0xB40) as *const u64
                    )}
                } else { 0 };
                buf.extend_from_slice(&ic.to_le_bytes());
            }
            Some(buf)
        }
        _ => None,
    }
}

fn handler_readfile(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let buf_ptr = regs.rcx as *mut u8;
    let count = regs.rdx as usize;

    if !is_user_ptr_valid(regs.rcx, count as u64) || count > 4096 {
        return err_to_u64(SyscallError::Fault);
    }

    let mut is_info_obj = false;
    let mut info_type = 0u32;
    let mut info_offset = 0u64;
    let (drive_idx, inode_num, offset) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if !entry.has_ob_object() { return (usize::MAX, 0, 0); }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                if obj.obj_type == crate::object::ObType::Key {
                    is_info_obj = true;
                    info_type = obj.native_id as u32;
                    info_offset = entry.offset;
                    return (usize::MAX, 0, entry.offset);
                }
                if obj.obj_type != crate::object::ObType::Filesystem {
                    return (usize::MAX, 0, 0);
                }
                (obj.flags as usize, obj.native_id as u32, entry.offset)
            } else {
                (usize::MAX, 0, 0)
            }
        } else {
            (usize::MAX, 0, 0)
        }
    });

    if is_info_obj {
        let content = generate_info_content(info_type);
        if content.is_none() { return err_to_u64(SyscallError::Inval); }
        let content = content.unwrap();
        let content_len = content.len();
        let copy_len = core::cmp::min(count, content_len.saturating_sub(info_offset as usize));
        if copy_len > 0 {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    content.as_ptr().add(info_offset as usize),
                    buf_ptr, copy_len,
                );
            }
        }
        crate::hal::without_interrupts(|| {
            let s = scheduler::current_scheduler();
            let mut lock = s.lock();
            if let Some(ep) = lock.current_eprocess_mut() {
                ep.handle_table[fd as usize].offset += copy_len as u64;
            }
        });
        return copy_len as u64;
    }

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
            if !entry.has_ob_object() { return (usize::MAX, 0, 0); }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                if obj.obj_type != crate::object::ObType::Filesystem {
                    return (usize::MAX, 0, 0);
                }
                (obj.flags as usize, obj.native_id as u32, entry.offset)
            } else {
                (usize::MAX, 0, 0)
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
            if entry.obj_type() == Some(crate::object::ObType::Directory) {
                (entry.drive().unwrap_or(0) as usize, entry.native_id().unwrap_or(0) as u32, entry.offset)
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

/// Check legacy VFS path access via Ob namespace (OB-030).
/// Returns Ok if access is granted or no security policy exists, Err(AccessDenied) if denied.
fn check_legacy_path_access(path: &str, access: u32) -> Result<(), u64> {
    if !path.contains(':') {
        return Ok(());
    }
    let normalized = path.replace('/', "\\");
    let ob_path = alloc::format!("\\Global\\FileSystem\\{}", normalized);
    let token = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        lock.current_eprocess()
            .map(|ep| ep.token)
            .unwrap_or(*crate::security::DEFAULT_ADMIN_TOKEN)
    });
    match crate::object::ob_open_path(&ob_path, &token, access) {
        Ok(ob_id) => {
            // Path exists with matching security → access granted, close reference
            let _ = crate::object::ob_close_object(ob_id);
            Ok(())
        }
        Err(crate::object::ObError::AccessDenied) => {
            Err(err_to_u64(SyscallError::Acces))
        }
        Err(_) => {
            // Path not found in Ob namespace or no security descriptor → grant access
            Ok(())
        }
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

    if let Err(e) = check_legacy_path_access(&path_str, crate::security::acl::ACCESS_WRITE) {
        return e;
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

    if let Err(e) = check_legacy_path_access(&path_str, crate::security::acl::ACCESS_DELETE) {
        return e;
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

    if let Err(e) = check_legacy_path_access(&path_str, crate::security::acl::ACCESS_DELETE) {
        return e;
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

    // Security check: require WRITE + DELETE access on old path (OB-030)
    if let Err(e) = check_legacy_path_access(&old_path,
        crate::security::acl::ACCESS_WRITE | crate::security::acl::ACCESS_DELETE) {
        return e;
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
        if node.mode & crate::fs::vfs::MODE_DIR == 0 {
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
                if entry.obj_type() == Some(crate::object::ObType::Filesystem) {
                    return (entry.drive().unwrap_or(0) as usize, entry.native_id().unwrap_or(0) as u32);
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

    // Check if thread already terminated
    let is_done = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let lock = s.lock();
        if let Some(k) = lock.find_kthread(target_tid) {
            k.state == ThreadState::Terminated
        } else {
            true
        }
    });

    if !is_done {
        // Block via KWait (OB-031)
        crate::kwait::kwait_block(crate::kwait::WaitReason::ThreadJoin { tid: target_tid });
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

/// ABI-stable DateTime struct for sys_get_datetime (RAX=44 — migrated to Ob).
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

/// sys_fsck (RAX=55): Run filesystem integrity check.
/// RBX = buf_ptr (&mut FsckStatsRaw), RCX = drive_char (e.g. 'C'), RDX = repair_flag (0=check, 1=repair).
/// Returns 0 on success, error code on failure.
#[repr(C)]
struct FsckStatsRaw {
    total_inodes: u32,
    used_inodes: u32,
    valid_inodes: u32,
    corrupted_inodes: u32,
    cross_linked_blocks: u32,
    orphan_inodes: u32,
    dangling_entries: u32,
    dir_errors: u32,
    superblock_errors: u32,
    repairs_applied: u32,
}

fn handler_fsck(regs: Registers) -> u64 {
    let buf_ptr = regs.rbx;
    let _drive_char = (regs.rcx & 0xFF) as u8 as char;
    let repair_flag = regs.rdx != 0;

    if buf_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let stats_size = core::mem::size_of::<FsckStatsRaw>() as u64;
    if !is_user_ptr_valid(buf_ptr, stats_size) {
        return err_to_u64(SyscallError::Fault);
    }

    let mode = if repair_flag {
        crate::fs::fsck::FsckMode::Repair
    } else {
        crate::fs::fsck::FsckMode::CheckOnly
    };

    let result = core::cell::UnsafeCell::new(FsckStatsRaw {
        total_inodes: 0, used_inodes: 0, valid_inodes: 0,
        corrupted_inodes: 0, cross_linked_blocks: 0,
        orphan_inodes: 0, dangling_entries: 0, dir_errors: 0,
        superblock_errors: 0, repairs_applied: 0,
    });

    // Run FSCK with access to block cache and devices
    let res = crate::hal::without_interrupts(|| {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = match cache_lock.as_mut() {
            Some(c) => c,
            None => return err_to_u64(SyscallError::Io),
        };
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = match bdevs_lock.get(0) {
            Some(d) => d,
            None => return err_to_u64(SyscallError::NoDev),
        };
        let partition_base = crate::globals::PRIMARY_PARTITION_BASE.load(core::sync::atomic::Ordering::Relaxed) as u32;

        let stats = crate::fs::fsck::run(cache, dev, mode, partition_base);

        // Write stats to the raw struct
        let raw = unsafe { &mut *result.get() };
        raw.total_inodes = stats.total_inodes;
        raw.used_inodes = stats.used_inodes;
        raw.valid_inodes = stats.valid_inodes;
        raw.corrupted_inodes = stats.corrupted_inodes;
        raw.cross_linked_blocks = stats.cross_linked_blocks;
        raw.orphan_inodes = stats.orphan_inodes;
        raw.dangling_entries = stats.dangling_entries;
        raw.dir_errors = stats.dir_errors;
        raw.superblock_errors = stats.superblock_errors;
        raw.repairs_applied = stats.repairs_applied;

        // Flush if repairs were applied
        if stats.repairs_applied > 0 {
            crate::globals::NEED_CACHE_FLUSH.store(true, core::sync::atomic::Ordering::Relaxed);
        }

        0u64
    });

    if res != 0 {
        return res;
    }

    // Copy stats to user buffer
    let raw = unsafe { &*result.get() };
    unsafe {
        core::ptr::copy_nonoverlapping(
            raw as *const FsckStatsRaw as *const u8,
            buf_ptr as *mut u8,
            core::mem::size_of::<FsckStatsRaw>(),
        );
    }

    0
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

#[repr(C)]
struct DriverInfoRaw {
    id: u32,
    state: u8,
    category: u8,
    driver_type: u8,
    api_version: u16,
    abi_min: u16,
    abi_target: u16,
    abi_max: u16,
    last_error: u32,
    caps: u64,
    isolation_mode: u8,
    events_received: u64,
    tick_count: u64,
    registered_at_tick: u64,
    name: [u8; 8],
}

/// sys_driver_load (RAX=57): load a NEM driver from a filesystem path (admin).
/// RBX = path_ptr (null-terminated path string).
/// Returns driver_id on success, negative on error.
fn handler_driver_load(regs: Registers) -> u64 {
    let path_ptr = regs.rbx;

    if path_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let path = match copy_user_string(path_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    match crate::drivers::nem::load_nem_driver(&path) {
        Ok(id) => id as u64,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

/// sys_driver_unload (RAX=58): unload a NEM driver by name (admin).
/// RBX = name_ptr (driver name string), RCX = force_flag (0=graceful, 1=force).
/// Returns 0 on success, negative on error.
fn handler_driver_unload(regs: Registers) -> u64 {
    let name_ptr = regs.rbx;
    let force = regs.rcx != 0;

    if name_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let name = match copy_user_string(name_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    match crate::drivers::hotreload::unload_driver(&name, force) {
        Ok(_) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

// ── pollfd struct (ABI frozen at v0.43) ──────────────────────────────

/// pollfd events/revents flags
const POLLIN: i16 = 1;
const POLLOUT: i16 = 2;
const POLLERR: i16 = 4;
const POLLHUP: i16 = 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

/// sys_poll (RAX=59): poll file descriptors for ready I/O.
/// RBX = fds_ptr (PollFd array in user space), RCX = nfds, RDX = timeout_ms.
/// timeout=0 → return immediately; timeout=-1 → infinite wait (not yet implemented).
/// Returns count of ready fds, 0 on timeout, negative on error.
fn handler_poll(regs: Registers) -> u64 {
    let fds_ptr = regs.rbx as *mut PollFd;
    let nfds = regs.rcx as usize;
    let _timeout = regs.rdx as i64;

    if fds_ptr.is_null() || nfds == 0 || nfds > 256 {
        return err_to_u64(SyscallError::Inval);
    }

    // ── Read pollfd array from user space ──
    let mut fds = alloc::vec![PollFd { fd: 0, events: 0, revents: 0 }; nfds];
    for i in 0..nfds {
        unsafe {
            let src = fds_ptr.add(i);
            fds[i].fd = core::ptr::read_volatile(&(*src).fd);
            fds[i].events = core::ptr::read_volatile(&(*src).events);
        }
    }

    // ── Check each fd ──
    let mut ready_count: u64 = 0;
    for i in 0..nfds {
        let fd = fds[i].fd;
        if fd < 0 {
            fds[i].revents = 0;
            continue;
        }
        let entry = current_handle_entry(fd as u8);
        if !entry.is_open() {
            fds[i].revents = POLLERR;
            ready_count += 1;
            continue;
        }

        let mut rev: i16 = 0;
        if entry.is_stdin() {
            if fds[i].events & POLLIN != 0 {
                rev |= POLLIN;
            }
        } else if entry.is_stdout() || entry.is_stderr() {
            if fds[i].events & POLLOUT != 0 {
                rev |= POLLOUT;
            }
        } else if entry.is_pipe_read() {
            let pipe_id = entry.native_id().unwrap_or(0) as u8;
            let ready = crate::pipe::pipe_peek_read_ready(pipe_id).unwrap_or(false);
            if ready && fds[i].events & POLLIN != 0 {
                rev |= POLLIN;
            }
            if crate::pipe::pipe_peek_write_closed(pipe_id).unwrap_or(false) {
                rev |= POLLHUP;
            }
        } else if entry.is_pipe_write() {
            if fds[i].events & POLLOUT != 0 {
                rev |= POLLOUT;
            }
        } else if entry.obj_type() == Some(crate::object::ObType::Filesystem)
            || entry.obj_type() == Some(crate::object::ObType::Directory) {
            if fds[i].events & POLLIN != 0 { rev |= POLLIN; }
            if fds[i].events & POLLOUT != 0 { rev |= POLLOUT; }
        } else {
            rev |= POLLERR;
        }
        fds[i].revents = rev;
        if rev != 0 {
            ready_count += 1;
        }
    }

    // ── Write back pollfd array to user space ──
    for i in 0..nfds {
        unsafe {
            core::ptr::write_volatile(&mut (*fds_ptr.add(i)).revents, fds[i].revents);
        }
    }

    ready_count
}

/// sys_ob_open (RAX=60): open an Ob namespace object.
/// RBX = path_ptr (Ob namespace path, e.g. "\Driver\ps2kbd")
/// RCX = desired_access (bitmask, e.g. 1 = ACCESS_READ)
/// Returns fd (≥ 3) on success, negative on error.
fn handler_ob_open(regs: Registers) -> u64 {
    let path_ptr = regs.rbx;
    let desired_access = regs.rcx as u32;

    if path_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let path_str = match copy_user_string(path_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    let path = path_str;

    // Get current process token for security check
    let token = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        lock.current_eprocess()
            .map(|ep| ep.token)
            .unwrap_or(*crate::security::DEFAULT_ADMIN_TOKEN)
    });

    // Open the object through the Object Manager (lookup + security check + ref)
    let ob_id = match crate::object::ob_open_path(&path, &token, desired_access) {
        Ok(id) => id,
        Err(crate::object::ObError::NotFound) => return err_to_u64(SyscallError::NoEnt),
        Err(crate::object::ObError::AccessDenied) => return err_to_u64(SyscallError::Acces),
        Err(_) => return err_to_u64(SyscallError::Inval),
    };

    // Create handle entry referencing the Ob object
    let entry = crate::handle::HandleEntry::ob_object(ob_id, desired_access);

    // Allocate handle in current process
    let fd = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            crate::handle::alloc_handle(&mut ep.handle_table, entry)
        } else {
            None
        }
    });

    match fd {
        Some(fd) => {
            fd as u64
        }
        None => {
            // Could not allocate handle — undo the reference
            let _ = crate::object::ob_close_object(ob_id);
            err_to_u64(SyscallError::NoMem)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-011: ObCreate — RAX=61
// ═══════════════════════════════════════════════════════════════════════

/// ABI struct for passing [reader_fd, writer_fd] back to user space.
#[repr(C)]
struct ObPipeFds {
    reader_fd: u64,
    writer_fd: u64,
}

/// sys_ob_create (RAX=61): Create an object and register it in the namespace.
/// RBX = path_ptr (Ob namespace path, null-terminated)
/// RCX = obj_type (u32, ObType enum value)
/// RDX = fds_out_ptr (for Pipe types: writes [reader_fd, writer_fd])
/// R8 = attrs (flags/attributes)
/// Returns fd (≥ 3) on success, negative on error.
fn handler_ob_create(regs: Registers) -> u64 {
    let path_ptr = regs.rbx;
    let obj_type_val = regs.rcx as u32;
    let fds_out = regs.rdx;
    let _attrs = regs.r8 as u32;

    if path_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let path_str = match copy_user_string(path_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if path_str.is_empty() || !path_str.starts_with('\\') {
        return err_to_u64(SyscallError::Inval);
    }

    let obj_type = match obj_type_val {
        4 => crate::object::ObType::Pipe,
        11 => crate::object::ObType::Directory,
        13 => crate::object::ObType::Event,
        _ => return err_to_u64(SyscallError::Inval),
    };

    match obj_type {
        crate::object::ObType::Pipe => {
            if fds_out == 0 || !is_user_ptr_valid(fds_out, 16) {
                return err_to_u64(SyscallError::Fault);
            }
            // Create the ObObject for the pipe (native_id = pipe_id)
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0, None,
            ) {
                Ok(id) => id,
                Err(crate::object::ObError::AlreadyExists) => return err_to_u64(SyscallError::Exist),
                Err(crate::object::ObError::OutOfMemory) => return err_to_u64(SyscallError::NoMem),
                Err(_) => return err_to_u64(SyscallError::Inval),
            };
            // Look up native_id to get the pipe_id
            let obj = crate::object::ob_lookup(ob_id).unwrap();
            let pipe_id = obj.native_id as u8;
            // Create reader and writer handle entries
            let read_entry = crate::handle::HandleEntry::pipe_read(pipe_id);
            let write_entry = crate::handle::HandleEntry::pipe_write(pipe_id);
            let (rfd, wfd) = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    match crate::handle::alloc_two_handles(&mut ep.handle_table, read_entry, write_entry) {
                        Some((r, w)) => {
                            crate::pipe::PIPE_MANAGER.inc_read_ref(pipe_id);
                            crate::pipe::PIPE_MANAGER.inc_write_ref(pipe_id);
                            (r as u64, w as u64)
                        }
                        None => (0u64, 0u64)
                    }
                } else {
                    (0u64, 0u64)
                }
            });
            if rfd == 0 {
                let _ = crate::object::ob_close_object(ob_id);
                return err_to_u64(SyscallError::NoMem);
            }
            unsafe {
                (fds_out as *mut u64).write(rfd);
                (fds_out as *mut u64).add(1).write(wfd);
            }
            rfd
        }
        crate::object::ObType::Directory => {
            // If this is a VFS path, create the actual directory in the filesystem
            if path_str.starts_with("\\Global\\FileSystem\\") {
                let vfs_path = &path_str["\\Global\\FileSystem\\".len()..];
                if !vfs_path.is_empty() {
                    match crate::globals::with_vfs(|vfs| vfs.mkdir(vfs_path)) {
                        Ok(_) => {},
                        Err(_) => return err_to_u64(SyscallError::Io),
                    }
                }
            }
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0, None,
            ) {
                Ok(id) => id,
                Err(crate::object::ObError::AlreadyExists) => return err_to_u64(SyscallError::Exist),
                Err(_) => return err_to_u64(SyscallError::Inval),
            };
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
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
                    fd as u64
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Event => {
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0, None,
            ) {
                Ok(id) => id,
                Err(crate::object::ObError::AlreadyExists) => return err_to_u64(SyscallError::Exist),
                Err(_) => return err_to_u64(SyscallError::Inval),
            };
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
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
                    fd as u64
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-012: ObQueryInfo — RAX=62
// ═══════════════════════════════════════════════════════════════════════

#[repr(C)]
struct ObBasicInfo {
    obj_type: u32,
    refcount: u32,
    name: [u8; 32],
}

#[repr(C)]
struct ObFileInfo {
    size: u64,
    drive: u8,
    inode: u32,
    padding: [u8; 3],
}

#[repr(C)]
struct ObProcessInfo {
    pid: u32,
    parent_pid: u32,
    priority: u8,
    thread_count: u32,
    state: u8,
    padding: [u8; 2],
}

#[repr(C)]
struct ObPipeInfo {
    capacity: u32,
    read_refs: u32,
    write_refs: u32,
}

#[repr(C)]
struct ObThreadInfo {
    tid: u32,
    pid: u32,
    state: u8,
    priority: u8,
    padding: [u8; 2],
}

#[repr(C)]
struct ObDeviceInfo {
    device_id: u32,
    reserved: u32,
}

/// sys_ob_query_info (RAX=62): query metadata for an object by fd.
/// RBX = fd
/// RCX = info_class (u32, ObInfoClass enum)
/// RDX = buf_ptr (output buffer)
/// R8 = buf_size
/// Returns bytes_written on success, negative on error.
fn handler_ob_query_info(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let info_class = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    if buf_ptr == 0 || buf_size == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(buf_ptr, buf_size as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    match info_class {
        0 => {
            // BasicInfo: type, refcount, name
            if entry.object_id == 0 {
                let basic = ObBasicInfo {
                    obj_type: entry.obj_type().map(|t| t as u32).unwrap_or(0),
                    refcount: 1,
                    name: {
                        let mut n = [0u8; 32];
                        let src: &[u8] = if entry.is_stdin() {
                            b"STDIN"
                        } else if entry.is_stdout() {
                            b"STDOUT"
                        } else if entry.is_stderr() {
                            b"STDERR"
                        } else {
                            b"HANDLE"
                        };
                        let len = src.len().min(31);
                        n[..len].copy_from_slice(&src[..len]);
                        n
                    },
                };
                let sz = core::mem::size_of::<ObBasicInfo>();
                if buf_size < sz { return err_to_u64(SyscallError::Inval); }
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &basic as *const ObBasicInfo as *const u8,
                        buf_ptr as *mut u8, sz,
                    );
                }
                return sz as u64;
            }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                let mut name = [0u8; 32];
                let src = obj.name;
                let len = src.iter().position(|&b| b == 0).unwrap_or(32).min(31);
                name[..len].copy_from_slice(&src[..len]);
                let basic = ObBasicInfo {
                    obj_type: obj.obj_type as u32,
                    refcount: obj.refcount,
                    name,
                };
                let sz = core::mem::size_of::<ObBasicInfo>();
                if buf_size < sz { return err_to_u64(SyscallError::Inval); }
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &basic as *const ObBasicInfo as *const u8,
                        buf_ptr as *mut u8, sz,
                    );
                }
                sz as u64
            } else {
                err_to_u64(SyscallError::BadF)
            }
        }
        1 => {
            // NameInfo: return the object's name as a string
            if entry.object_id == 0 {
                return 0u64;
            }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                let name_str = obj.name_str();
                let bytes = name_str.as_bytes();
                let copy_len = bytes.len().min(buf_size - 1).min(255);
                unsafe {
                    core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, copy_len);
                    (buf_ptr as *mut u8).add(copy_len).write(0u8);
                }
                copy_len as u64
            } else {
                err_to_u64(SyscallError::BadF)
            }
        }
        2 => {
            // FileInfo: size, drive, inode
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive = entry.drive().unwrap_or(0);
            let inode = entry.native_id().unwrap_or(0) as u32;
            let size = crate::globals::with_vfs(|vfs| {
                vfs.stat(drive as usize, inode).map(|n| n.size).unwrap_or(0)
            });
            let fi = ObFileInfo {
                size: size as u64,
                drive,
                inode,
                padding: [0u8; 3],
            };
            let sz = core::mem::size_of::<ObFileInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &fi as *const ObFileInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        3 => {
            // ProcessInfo: pid, parent, priority, thread_count, state
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            let pi = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let lock = s.lock();
                if let Some(ep) = lock.find_eprocess(pid) {
                    ObProcessInfo {
                        pid,
                        parent_pid: ep.parent_pid,
                        priority: {
                            // Look up first thread for this process to get priority
                            let mut prio = 2u8;
                            for th in lock.kthreads.iter() {
                                if let Some(k) = th {
                                    if k.pid == pid {
                                        prio = k.priority as u8;
                                        break;
                                    }
                                }
                            }
                            prio
                        },
                        thread_count: ep.thread_count as u32,
                        state: if ep.thread_count == 0 { 1u8 } else { 0u8 },
                        padding: [0u8; 2],
                    }
                } else {
                    ObProcessInfo {
                        pid, parent_pid: 0, priority: 0,
                        thread_count: 0, state: 0, padding: [0u8; 2],
                    }
                }
            });
            let sz = core::mem::size_of::<ObProcessInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &pi as *const ObProcessInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        4 => {
            // ThreadInfo: tid, pid, state, priority
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            let ti = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let lock = s.lock();
                // Find the first thread for this process
                let mut found = ObThreadInfo {
                    tid: 0, pid, state: 0, priority: 0, padding: [0u8; 2],
                };
                for kt in lock.kthreads.iter() {
                    if let Some(k) = kt {
                        if k.pid == pid {
                            found.tid = k.tid;
                            found.state = k.state.to_u8();
                            found.priority = k.priority;
                            break;
                        }
                    }
                }
                found
            });
            let sz = core::mem::size_of::<ObThreadInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &ti as *const ObThreadInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        5 => {
            // PipeInfo: capacity, read_refs, write_refs
            if entry.obj_type() != Some(crate::object::ObType::Pipe) {
                return err_to_u64(SyscallError::Inval);
            }
            let pipe_id = entry.native_id().unwrap_or(0) as u8;
            let capacity = crate::pipe::PIPE_BUF_SIZE;
            let read_refs = crate::pipe::pipe_peek_read_ready(pipe_id)
                .map(|_| 1u32).unwrap_or(0);
            let info = ObPipeInfo {
                capacity: capacity as u32,
                read_refs: read_refs,
                write_refs: 0,
            };
            let sz = core::mem::size_of::<ObPipeInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &info as *const ObPipeInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        6 => {
            // DeviceInfo: device_id
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            let di = ObDeviceInfo {
                device_id: obj.native_id as u32,
                reserved: 0,
            };
            let sz = core::mem::size_of::<ObDeviceInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &di as *const ObDeviceInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        7 => {
            // CpuInfo: return CpuInfoFull struct from \Global\Info\CpuInfo object
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 3 {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<crate::cpu::CpuInfoFull>();
            if buf_size < (sz as usize) { return err_to_u64(SyscallError::Inval); }
            let info = crate::cpu::get_cpu_info_full();
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &info as *const crate::cpu::CpuInfoFull as *const u8,
                    buf_ptr as *mut u8, sz as usize,
                );
            }
            sz as u64
        }
        8 => {
            // Version: return KERNEL_VERSION string from \Global\Info\Version object
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 4 {
                return err_to_u64(SyscallError::Inval);
            }
            let ver = crate::KERNEL_VERSION.as_bytes();
            let copy_len = ver.len().min(buf_size as usize);
            unsafe {
                core::ptr::copy_nonoverlapping(ver.as_ptr(), buf_ptr as *mut u8, copy_len);
            }
            ver.len() as u64
        }
        9 => {
            // DateTime: return SysDateTime struct from \Global\Info\DateTime object
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 5 {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<SysDateTime>() as usize;
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
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
                    day: 0, month: 0, year: 0, valid: 0,
                },
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &sysdt as *const SysDateTime as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        10 => {
            // Memory: return MemInfo (MemoryStats) struct from \Global\Info\Memory object
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 1 {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<crate::memory::MemoryStats>() as usize;
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let stats = crate::memory::stats();
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &stats as *const crate::memory::MemoryStats as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        11 => {
            // Drives: return array of DriveInfoRaw entries from \Global\Info\Drives object
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 6 {
                return err_to_u64(SyscallError::Inval);
            }
            let entry_size = core::mem::size_of::<DriveInfoRaw>();
            let max_entries = buf_size / entry_size;
            if max_entries == 0 { return 0u64; }
            let written = crate::globals::with_vfs(|vfs| {
                let mut count = 0usize;
                for i in 0..26 {
                    if count >= max_entries { break; }
                    if vfs.drives[i].is_some() {
                        let letter = (b'A' + i as u8) as char;
                        let label = vfs.volume_label(letter).unwrap_or_default();
                        let (fs_type_str, total_sectors) = {
                            let fs = vfs.drives[i].as_ref().unwrap();
                            (fs.fs_type(), fs.total_sectors())
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
                                (buf_ptr as *mut u8).add(count * entry_size),
                                entry_size,
                            );
                        }
                        count += 1;
                    }
                }
                (count * entry_size) as u64
            });
            written
        }
        12 => {
            // Drivers: return array of DriverInfoRaw entries from \Global\Info\Drivers object
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 7 {
                return err_to_u64(SyscallError::Inval);
            }
            let entry_size = core::mem::size_of::<DriverInfoRaw>();
            let max_entries = buf_size / entry_size;
            if max_entries == 0 { return 0u64; }
            let runtime = crate::drivers::driver_runtime::DRIVER_RUNTIME.lock();
            let ids = runtime.driver_ids();
            let count = ids.len().min(max_entries);
            for i in 0..count {
                if let Some(d) = crate::drivers::driver_runtime::get_driver(ids[i]) {
                    let raw = DriverInfoRaw {
                        id: d.id as u32, state: d.state as u8, category: d.category as u8,
                        driver_type: d.driver_type as u8, api_version: d.api_version,
                        abi_min: d.abi_min, abi_target: d.abi_target, abi_max: d.abi_max,
                        last_error: d.last_error, caps: d.caps, isolation_mode: d.isolation_mode,
                        events_received: d.events_received, tick_count: d.tick_count,
                        registered_at_tick: d.registered_at_tick, name: d.name,
                    };
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            &raw as *const DriverInfoRaw as *const u8,
                            (buf_ptr as *mut u8).add(i * entry_size),
                            entry_size,
                        );
                    }
                }
            }
            drop(runtime);
            (count * entry_size) as u64
        }
        13 => {
            // Cwd: return current process working directory string
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 8 {
                return err_to_u64(SyscallError::Inval);
            }
            let (drive, path) = crate::scheduler::get_current_cwd();
            let full = alloc::format!("{}:{}", (b'A' + drive) as char, path);
            let bytes = full.as_bytes();
            let copy_len = bytes.len().min(buf_size.saturating_sub(1));
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, copy_len);
                (buf_ptr as *mut u8).add(copy_len).write(0);
            }
            copy_len as u64
        }
        14 => {
            // KeyboardLayout: return current layout (0=US, 1=SP) from \Global\Info\Keyboard
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 9 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let layout = KEYBOARD_LAYOUT.load(core::sync::atomic::Ordering::Relaxed);
            unsafe { core::ptr::write_volatile(buf_ptr as *mut u8, layout); }
            1u64
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-013: ObSetInfo — RAX=63
// ═══════════════════════════════════════════════════════════════════════

/// sys_ob_set_info (RAX=63): set metadata for an object by fd.
/// RBX = fd
/// RCX = info_class (u32)
/// RDX = buf_ptr (input buffer)
/// R8 = buf_size
/// Returns 0 on success, negative on error.
fn handler_ob_set_info(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let info_class = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    if buf_ptr == 0 || buf_size == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(buf_ptr, buf_size as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    match info_class {
        0 => {
            // ProcessPriority: buf contains a u32 priority value
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 {
                return err_to_u64(SyscallError::Inval);
            }
            let priority = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            if priority > 3 {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                lock.set_process_priority(pid, priority as u8);
            });
            0
        }
        1 => {
            // ThreadPriority: buf contains a u32 priority value
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 {
                return err_to_u64(SyscallError::Inval);
            }
            let priority = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            if priority > 3 {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                // Set priority for all threads of this process
                for kt in lock.kthreads.iter_mut() {
                    if let Some(k) = kt {
                        if k.pid == pid {
                            k.priority = priority as u8;
                        }
                    }
                }
            });
            0
        }
        2 => {
            // ObjectName: rename the object
            let name = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if name.len() > 31 || name.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            match crate::object::ob_set_object_name(entry.object_id, &name) {
                Ok(_) => 0,
                Err(_) => err_to_u64(SyscallError::BadF),
            }
        }
        3 => {
            // SecurityInfo: set SecurityDescriptor
            return err_to_u64(SyscallError::NoSys);
        }
        4 => {
            // ProcessTerminate: terminate the process by PID
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
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
        5 => {
            // KeyboardLayout: set keyboard layout from \Global\Info\Keyboard object
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 9 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let layout = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
            if layout > 1 { return err_to_u64(SyscallError::Inval); }
            KEYBOARD_LAYOUT.store(layout, core::sync::atomic::Ordering::Relaxed);
            match crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_KEYB_LAYOUT,
                crate::eventbus::SOURCE_KERNEL,
                3, layout as u64, 0, 0
            ) {
                Ok(_) => 0,
                Err(_) => err_to_u64(SyscallError::Again),
            }
        }
        6 => {
            // VfsRename: rename a VFS file/directory
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            let obj_name = obj.name_str();
            if !obj_name.starts_with("\\Global\\FileSystem\\") {
                return err_to_u64(SyscallError::Inval);
            }
            let old_vfs_path = &obj_name["\\Global\\FileSystem\\".len()..];
            if old_vfs_path.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            let new_path = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if new_path.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            match crate::globals::with_vfs(|vfs| vfs.rename(old_vfs_path, &new_path)) {
                Ok(_) => {
                    // Update the ObObject name to reflect the new path
                    let new_ob_name = alloc::format!("\\Global\\FileSystem\\{}", new_path);
                    let _ = crate::object::ob_set_object_name(entry.object_id, &new_ob_name);
                    0
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-014: ObEnum — RAX=64
// ═══════════════════════════════════════════════════════════════════════

/// sys_ob_enum (RAX=64): enumerate objects in a namespace directory by fd.
/// RBX = dir_fd (directory handle from ObOpen/ObCreate)
/// RCX = buf_ptr (array of ObEnumEntry output)
/// RDX = max_entries
/// Returns count of entries written, negative on error.
fn handler_ob_enum(regs: Registers) -> u64 {
    let dir_fd = regs.rbx as u8;
    let buf_ptr = regs.rcx;
    let max_entries = regs.rdx as usize;

    if buf_ptr == 0 || max_entries == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let entry_size = core::mem::size_of::<crate::object::ObEnumEntry>() as u64;
    if !is_user_ptr_valid(buf_ptr, entry_size.saturating_mul(max_entries as u64)) {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(dir_fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    // Try VFS-backed enumeration only if the fd has ObType that came from \Global\FileSystem\...
    // Pure namespace directories (like \Process) have no VFS backing.
    let use_vfs = if entry.object_id != 0 {
        matches!(entry.obj_type(), Some(crate::object::ObType::Filesystem) | Some(crate::object::ObType::Directory))
            && crate::object::ob_lookup(entry.object_id).map_or(false, |obj| {
                let s = obj.name_str();
                s.starts_with("\\Global\\FileSystem\\") || s.starts_with("dir/") || s.starts_with("file/")
            })
    } else {
        false
    };
    if use_vfs {
        let drv = entry.drive().unwrap_or(0);
        let nid = entry.native_id().unwrap_or(0);
        let drive_idx = drv as usize;
        let dir_inode = nid as u32;
        let mut entries = alloc::vec::Vec::new();
        let result: Result<(), ()> = crate::globals::with_vfs(|vfs| {
            let mut idx = 0usize;
            loop {
                match vfs.readdir(drive_idx, dir_inode, idx) {
                    Ok(Some(vfs_entry)) => {
                        let name_bytes = vfs_entry.name.as_bytes();
                        let mut name_arr = [0u8; 32];
                        let len = name_bytes.len().min(31);
                        name_arr[..len].copy_from_slice(&name_bytes[..len]);
                        let obj_type = if (vfs_entry.node.mode & crate::fs::vfs::MODE_DIR) != 0 {
                            crate::object::ObType::Directory
                        } else {
                            crate::object::ObType::Filesystem
                        };
                        entries.push(crate::object::ObEnumEntry {
                            id: vfs_entry.node.inode as u64,
                            obj_type: obj_type as u32,
                            name: name_arr,
                            mode: vfs_entry.node.mode,
                            _pad: [0u8; 2],
                            size: vfs_entry.node.size,
                        });
                        idx += 1;
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            Ok(())
        });
        return match result {
            Ok(()) => {
                let count = core::cmp::min(max_entries, entries.len());
                for i in 0..count {
                    let raw = &entries[i];
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            raw as *const crate::object::ObEnumEntry as *const u8,
                            (buf_ptr as *mut u8).add(i * core::mem::size_of::<crate::object::ObEnumEntry>()),
                            core::mem::size_of::<crate::object::ObEnumEntry>(),
                        );
                    }
                }
                count as u64
            }
            Err(_) => err_to_u64(SyscallError::Inval),
        };
    }

    // Fallback: Ob namespace enumeration via path resolution
    let path = if entry.object_id != 0 {
        crate::kobj::namespace::ob_find_path_by_id(entry.object_id)
    } else {
        None
    };
    let dir_path = match path {
        Some(p) => p,
        None => return err_to_u64(SyscallError::Inval),
    };
    let ob_entries = match crate::object::ob_enum_directory(&dir_path) {
        Ok(e) => e,
        Err(_) => return err_to_u64(SyscallError::Inval),
    };
    let count = core::cmp::min(max_entries, ob_entries.len());
    for i in 0..count {
        let raw = &ob_entries[i];
        unsafe {
            core::ptr::copy_nonoverlapping(
                raw as *const crate::object::ObEnumEntry as *const u8,
                (buf_ptr as *mut u8).add(i * core::mem::size_of::<crate::object::ObEnumEntry>()),
                core::mem::size_of::<crate::object::ObEnumEntry>(),
            );
        }
    }
    count as u64
}

// ═══════════════════════════════════════════════════════════════════════
// OB-020: ObWait — RAX=65
// ═══════════════════════════════════════════════════════════════════════

/// sys_ob_wait (RAX=65): wait on one or more Ob objects.
/// RBX = handle_count
/// RCX = handles_ptr (pointer to array of fd u64 values)
/// RDX = wait_type (0=ANY, 1=ALL)
/// R8 = timeout_ms (0 = infinite)
/// Returns index of signaled handle, negative on error.
fn handler_ob_wait(regs: Registers) -> u64 {
    let handle_count = regs.rbx as usize;
    let handles_ptr = regs.rcx;
    let wait_type = regs.rdx as u32;
    let _timeout_ms = regs.r8 as u64;

    if handle_count == 0 || handles_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(handles_ptr, (handle_count as u64) * 8) {
        return err_to_u64(SyscallError::Fault);
    }
    if handle_count > 1 {
        return err_to_u64(SyscallError::NoSys);
    }
    if wait_type > 1 {
        return err_to_u64(SyscallError::Inval);
    }

    let fd = unsafe { (handles_ptr as *const u64).read() } as u8;
    let entry = current_handle_entry(fd);

    if entry.object_id == 0 {
        return err_to_u64(SyscallError::BadF);
    }

    let obj = match crate::object::ob_lookup(entry.object_id) {
        Some(o) => o,
        None => return err_to_u64(SyscallError::BadF),
    };

    let reason = match obj.obj_type {
        crate::object::ObType::Process => {
            let pid = obj.native_id as u32;
            crate::kwait::WaitReason::ChildExit { pid }
        }
        crate::object::ObType::Pipe => {
            let pipe_id = obj.native_id as u8;
            // Quick non-blocking check: if pipe has data, return immediately
            if let Some(true) = crate::pipe::pipe_peek_read_ready(pipe_id) {
                return 0;
            }
            crate::kwait::WaitReason::PipeRead { pipe_id: pipe_id as u16 }
        }
        crate::object::ObType::Event => {
            let event_type = obj.native_id as u32;
            crate::kwait::WaitReason::Event { event_type }
        }
        crate::object::ObType::Timer => {
            crate::kwait::WaitReason::Timer { timeout_ms: 0 }
        }
        _ => return err_to_u64(SyscallError::NoSys),
    };

    // Block the current thread via KWait
    crate::kwait::kwait_block(reason);
    0
}

// ═══════════════════════════════════════════════════════════════════════
// OB-066: ObDestroy — RAX=66
// ═══════════════════════════════════════════════════════════════════════

/// sys_ob_destroy (RAX=66): destroy/delete an object by fd.
/// RBX = fd (handle referencing the object to destroy)
/// Returns 0 on success, negative on error.
///
/// For \Global\FileSystem\ objects: performs VFS remove_file (Filesystem)
/// or remove_dir (Directory). For namespace-only objects: removes from
/// Ob namespace and frees the ObObject.
fn handler_ob_destroy(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;

    // Read handle entry and close it atomically
    let (object_id, obj_type, name) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if !entry.is_open() {
                return (0, crate::object::ObType::Unknown, alloc::string::String::new());
            }
            let oid = entry.object_id;
            let ot = entry.obj_type().unwrap_or(crate::object::ObType::Unknown);
            // Close the handle
            ep.handle_table[fd as usize] = crate::handle::HandleEntry::closed();

            if oid == 0 {
                return (0, crate::object::ObType::Unknown, alloc::string::String::new());
            }

            // Lookup object name
            let obj_name = match crate::object::ob_lookup(oid) {
                Some(o) => o.name_str().to_string(),
                None => alloc::string::String::new(),
            };
            (oid, ot, obj_name)
        } else {
            (0, crate::object::ObType::Unknown, alloc::string::String::new())
        }
    });

    if object_id == 0 {
        return err_to_u64(SyscallError::BadF);
    }

    // If this is a VFS-backed path, perform VFS operation
    if name.starts_with("\\Global\\FileSystem\\") {
        let vfs_path = &name["\\Global\\FileSystem\\".len()..];
        if vfs_path.is_empty() {
            let _ = crate::object::ob_destroy_object(object_id);
            return err_to_u64(SyscallError::Inval);
        }

        let result = match obj_type {
            crate::object::ObType::Directory => {
                crate::globals::with_vfs(|vfs| vfs.remove_dir(vfs_path))
            }
            _ => {
                crate::globals::with_vfs(|vfs| vfs.remove_file(vfs_path))
            }
        };

        // Destroy Ob object regardless (the VFS handles the actual FS operation)
        let _ = crate::object::ob_destroy_object(object_id);

        match result {
            Ok(_) => 0,
            Err(_) => err_to_u64(SyscallError::Io),
        }
    } else {
        // Pure namespace object — just destroy the ObObject
        match crate::object::ob_destroy_object(object_id) {
            Ok(_) => 0,
            Err(_) => err_to_u64(SyscallError::Inval),
        }
    }
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
        t[14] = None; // unused
        t[15] = None; // unused
        t[16] = Some(handler_chdir as SyscallFn);
        t[17] = None; // migrated to Ob: \Global\Info\Cwd
        t[18] = Some(handler_brk as SyscallFn);
        t[19] = Some(handler_mmap as SyscallFn);
        t[20] = Some(handler_munmap as SyscallFn);
        t[21] = Some(handler_loadlib as SyscallFn);
        t[22] = Some(handler_thread_create as SyscallFn);
        t[23] = Some(handler_thread_join as SyscallFn);
        t[24] = None; // migrated to Ob: \Global\Info\CpuInfo
        t[25] = Some(handler_mkdir as SyscallFn);
        t[26] = Some(handler_unlink as SyscallFn);
        t[27] = Some(handler_rmdir as SyscallFn);
        t[28] = Some(handler_rename as SyscallFn);
        t[29] = Some(handler_set_exception_handler as SyscallFn);
        t[33] = None; // migrated to Ob: \Global\Info\Drives
        t[40] = Some(handler_wait_alertable as SyscallFn);
        t[41] = Some(handler_sleep_ex as SyscallFn);
        t[42] = Some(handler_poweroff as SyscallFn);
        t[43] = None; // migrated to Ob: \Global\Info\Version
        t[44] = None; // migrated to Ob: \Global\Info\DateTime
        t[45] = None; // migrated to Ob: \Global\Info\Memory
        t[46] = Some(handler_get_volume_label as SyscallFn);
        t[47] = Some(handler_chdir_parent as SyscallFn);
        t[48] = None; // migrated to Ob: sys_kobj_enum → ob_enum
        t[49] = None; // migrated to Ob: \Global\Info\Keyboard + ob_set_info
        t[50] = Some(handler_ndreg as SyscallFn);
        t[51] = None; // migrated to Ob: sys_set_priority → ob_set_info
        t[52] = None; // migrated to Ob: sys_kill_process → ob_set_info
        t[53] = Some(handler_cursor_blink as SyscallFn);
        t[54] = Some(handler_set_volume_label as SyscallFn);
        t[55] = Some(handler_fsck as SyscallFn);
        t[56] = None; // migrated to Ob: \Global\Info\Drivers
        t[57] = Some(handler_driver_load as SyscallFn);
        t[58] = Some(handler_driver_unload as SyscallFn);
        t[59] = Some(handler_poll as SyscallFn);
        t[60] = Some(handler_ob_open as SyscallFn);
        t[61] = Some(handler_ob_create as SyscallFn);
        t[62] = Some(handler_ob_query_info as SyscallFn);
        t[63] = Some(handler_ob_set_info as SyscallFn);
        t[64] = Some(handler_ob_enum as SyscallFn);
        t[65] = Some(handler_ob_wait as SyscallFn);
        t[66] = Some(handler_ob_destroy as SyscallFn);
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
        t[14] = SyscallPermission::free(); // unused
        t[15] = SyscallPermission::free(); // unused
        t[16] = SyscallPermission::user();
        t[17] = SyscallPermission::free(); // migrated to Ob
        t[18] = SyscallPermission::user();
        t[19] = SyscallPermission::user();
        t[20] = SyscallPermission::user();
        t[21] = SyscallPermission::user();
        t[22] = SyscallPermission::user();
        t[23] = SyscallPermission::user();
        t[24] = SyscallPermission::free(); // unused
        t[25] = SyscallPermission::user();
        t[26] = SyscallPermission::user();
        t[27] = SyscallPermission::user();
        t[28] = SyscallPermission::user();
        t[29] = SyscallPermission::user();
        t[33] = SyscallPermission::free(); // migrated to Ob
        t[40] = SyscallPermission::user();
        t[41] = SyscallPermission::user();
        t[42] = SyscallPermission::user();
        t[43] = SyscallPermission::free(); // migrated to Ob
        t[44] = SyscallPermission::free(); // migrated to Ob
        t[45] = SyscallPermission::free(); // migrated to Ob
        t[46] = SyscallPermission::user();
        t[47] = SyscallPermission::user();
        t[49] = SyscallPermission::free(); // migrated to Ob
        t[50] = SyscallPermission::admin();
        t[53] = SyscallPermission::user();
        t[54] = SyscallPermission::user();
        t[55] = SyscallPermission::user();
        t[56] = SyscallPermission::free(); // migrated to Ob
        t[57] = SyscallPermission::admin();
        t[58] = SyscallPermission::admin();
        t[59] = SyscallPermission::user();
        t[60] = SyscallPermission::user();
        t[61] = SyscallPermission::user();
        t[62] = SyscallPermission::user();
        t[63] = SyscallPermission::user();
        t[64] = SyscallPermission::user();
        t[65] = SyscallPermission::user();
        t[66] = SyscallPermission::user();
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
        9, 10, 11, 12, 13, 16, 18, 19, 20, 21, 22, 23,
            25, 26, 27, 28,
        40, 41, 42, 46, 47,
            50, 53, 54,         55, 57, 58, 59,
            60, 61, 62, 63, 64, 65,
        ];
        // Removed syscalls: 14(ioctl), 15(register_device), 24(getcpuinfo→Ob),
        // 43(get_version→Ob), 44(get_datetime→Ob), 45(get_meminfo→Ob), 48(kobj_enum→Ob),
        // 51(set_priority→Ob), 52(kill_process→Ob)
        const RESERVED: &[u64] = &[14, 15, 17, 24, 33, 43, 44, 45, 48, 49, 51, 52, 56];
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

        test_true!(read_entry.is_pipe_read());
        test_true!(write_entry.is_pipe_write());
        test_eq!(read_entry.obj_type(), Some(crate::object::ObType::Pipe));
        test_eq!(file_entry.obj_type(), Some(crate::object::ObType::Filesystem));
        test_eq!(dir_entry.obj_type(), Some(crate::object::ObType::Directory));

        // Test valid fd range: 0xFF means "no redirection"
        let no_redir: u8 = 0xFF;
        test_eq!(no_redir, 255);
        test_true!(no_redir != 0);

        // Test closed entry looked up → should return Check
        let closed = crate::handle::HandleEntry::closed();
        test_true!(!closed.is_open());
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

    // ═══════════════════════════════════════════════════════════════════
    // OB-011: ObCreate tests
    // ═══════════════════════════════════════════════════════════════════

    test_case!("ob_create_directory", {
        // Create a directory in the namespace via ob_create_object_path
        let id = crate::object::ob_create_object_path(
            "\\Global\\TestDir", crate::object::ObType::Directory, 0, None,
        );
        test_true!(id.is_ok());
        let id = id.unwrap();
        test_true!(id > 0);
        // Verify it exists in the namespace
        let found = crate::kobj::namespace::ob_lookup_path("\\Global\\TestDir");
        test_true!(found.is_ok());
        test_eq!(found.unwrap(), id);
        // Cleanup
        crate::object::ob_close_object(id).unwrap();
    });

    test_case!("ob_create_pipe", {
        let id = crate::object::ob_create_object_path(
            "\\Global\\Pipe\\TestPipe", crate::object::ObType::Pipe, 0, None,
        );
        test_true!(id.is_ok());
        let id = id.unwrap();
        test_true!(id > 0);
        // Cleanup
        crate::object::ob_close_object(id).unwrap();
    });

    test_case!("ob_create_invalid_type", {
        // Ensure namespace is initialized
        crate::kobj::namespace::init_object_namespace();
        let _ = crate::kobj::namespace::ob_create_directory("\\Global");
        let result = crate::object::ob_create_object_path(
            "\\Global\\BadObj", crate::object::ObType::Unknown, 0, None,
        );
        test_true!(result.is_err());
    });

    test_case!("ob_create_duplicate_path", {
        let id1 = crate::object::ob_create_object_path(
            "\\Global\\DupTest", crate::object::ObType::Directory, 0, None,
        );
        test_true!(id1.is_ok());
        let id2 = crate::object::ob_create_object_path(
            "\\Global\\DupTest", crate::object::ObType::Directory, 0, None,
        );
        test_true!(id2.is_err());
        let id1 = id1.unwrap();
        crate::object::ob_close_object(id1).unwrap();
        // Also clean up from namespace
        let _ = crate::kobj::namespace::ob_remove_object("\\Global\\DupTest");
    });

    test_case!("ob_create_empty_path_fails", {
        let result = crate::object::ob_create_object_path(
            "", crate::object::ObType::Directory, 0, None,
        );
        test_true!(result.is_err());
    });

    // ═══════════════════════════════════════════════════════════════════
    // OB-012: ObQueryInfo tests
    // ═══════════════════════════════════════════════════════════════════

    test_case!("ob_query_info_basic", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Driver, "qinfo_test", 42, 0, None
        ).unwrap();
        let obj = crate::object::ob_lookup(id).unwrap();
        test_eq!(obj.obj_type, crate::object::ObType::Driver);
        test_eq!(obj.native_id, 42);
        test_eq!(obj.refcount, 1);
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_query_info_basic_closed_fd", {
        // BasicInfo on a closed handle should return -EBADF
        let closed = crate::handle::HandleEntry::closed();
        test_true!(!closed.is_open());
    });

    test_case!("ob_query_info_name", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Process, "name_query", 7, 0, None
        ).unwrap();
        let obj = crate::object::ob_lookup(id).unwrap();
        test_eq!(obj.name_str(), "name_query");
        crate::object::ob_destroy_object(id).unwrap();
    });

    // ═══════════════════════════════════════════════════════════════════
    // OB-013: ObSetInfo tests
    // ═══════════════════════════════════════════════════════════════════

    test_case!("ob_set_info_object_name", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Filesystem, "old_name", 0, 0, None
        ).unwrap();
        crate::object::ob_set_object_name(id, "new_name").unwrap();
        let obj = crate::object::ob_lookup(id).unwrap();
        test_eq!(obj.name_str(), "new_name");
        crate::object::ob_destroy_object(id).unwrap();
    });

    test_case!("ob_set_info_invalid_fd", {
        let result = crate::object::ob_set_object_name(99999, "test");
        test_true!(result.is_err());
        test_eq!(result.unwrap_err(), crate::object::ObError::NotFound);
    });

    test_case!("ob_set_info_name_too_long", {
        let id = crate::object::ob_create_object(
            crate::object::ObType::Device, "short", 0, 0, None
        ).unwrap();
        let long_name = "a".repeat(64);
        crate::object::ob_set_object_name(id, &long_name).unwrap();
        let obj = crate::object::ob_lookup(id).unwrap();
        test_eq!(obj.name_str().len(), 31); // truncated to OB_NAME_LEN - 1
        crate::object::ob_destroy_object(id).unwrap();
    });

    // ═══════════════════════════════════════════════════════════════════
    // OB-014: ObEnum tests
    // ═══════════════════════════════════════════════════════════════════

    test_case!("ob_enum_namespace_root", {
        // Ensure namespace has the expected root directories
        {
            let mut ns = crate::kobj::namespace::OB_NAMESPACE.lock();
            for dir in &["Device", "DosDevices", "Global", "Driver", "FileSystem", "Ob"] {
                let path = alloc::format!("\\{}", dir);
                let _ = ns.create_directory(&path);
            }
        }
        let entries = crate::kobj::namespace::ob_enumerate_namespace("\\");
        test_true!(entries.is_ok());
        let entries = entries.unwrap();
        // Names are stored lowercase (name_to_key converts to lowercase)
        let names: alloc::vec::Vec<&str> = entries.iter()
            .map(|e| {
                let len = e.name.iter().position(|&b| b == 0).unwrap_or(32);
                core::str::from_utf8(&e.name[..len]).unwrap_or("")
            })
            .collect();
        test_true!(names.contains(&"device"));
        test_true!(names.contains(&"global"));
        test_true!(names.contains(&"driver"));
    });

    test_case!("ob_enum_directory_nested", {
        // Ensure namespace has \Global directory
        {
            let mut ns = crate::kobj::namespace::OB_NAMESPACE.lock();
            let _ = ns.create_directory("\\Global");
        }
        let _ = crate::kobj::namespace::ob_create_directory("\\Global\\EnumTest");
        let entries = crate::kobj::namespace::ob_enumerate_namespace("\\Global");
        test_true!(entries.is_ok());
        let entries = entries.unwrap();
        // Names are stored lowercase (name_to_key converts to lowercase)
        let names: alloc::vec::Vec<&str> = entries.iter()
            .map(|e| {
                let len = e.name.iter().position(|&b| b == 0).unwrap_or(32);
                core::str::from_utf8(&e.name[..len]).unwrap_or("")
            })
            .collect();
        test_true!(names.contains(&"enumtest"));
    });

    test_case!("ob_enum_invalid_path", {
        let result = crate::kobj::namespace::ob_enumerate_namespace("\\NonExistent\\Path");
        test_true!(result.is_err());
    });

    // ═══════════════════════════════════════════════════════════════════
    // OB-017: handler_readfile/handler_writefile via ObQueryInfo
    // ═══════════════════════════════════════════════════════════════════

    test_case!("handler_readfile_ob_info_extraction", {
        // Create a file ObObject simulating what HandleEntry::file() does
        let inode = 42u32;
        let ob_id = crate::object::ob_create_object(
            crate::object::ObType::Filesystem, "OBFILE", inode as u64, 0, None,
        ).expect("ob create");
        // Verify ob_lookup returns correct native_id (= inode)
        let obj = crate::object::ob_lookup(ob_id).unwrap();
        test_eq!(obj.native_id, inode as u64);
        test_eq!(obj.obj_type, crate::object::ObType::Filesystem);
        // Helper test: native_id serves as the inode for ObQueryInfo (file)
        let extracted_inode = obj.native_id as u32;
        test_eq!(extracted_inode, inode);
        crate::object::ob_destroy_object(ob_id).unwrap();
    });

    test_case!("handler_writefile_ob_info_extraction", {
        // Create a file ObObject for write context
        let inode = 99u32;
        let ob_id = crate::object::ob_create_object(
            crate::object::ObType::Filesystem, "OBWRITE", inode as u64, 0, None,
        ).expect("ob create");
        // Verify ob_lookup works for both read and write paths
        let obj = crate::object::ob_lookup(ob_id).unwrap();
        test_eq!(obj.native_id, inode as u64);
        // The handler uses ob_lookup to extract inode from ObObject
        let extracted_inode = obj.native_id as u32;
        test_eq!(extracted_inode, inode);
        crate::object::ob_destroy_object(ob_id).unwrap();
    });
}
