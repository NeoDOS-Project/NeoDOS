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
mod handlers;
mod ob;
mod cm;
mod tests;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use lazy_static::lazy_static;
use crate::serial_println;
use crate::scheduler::{self, ThreadState};

pub use table::{Registers, SyscallFn, MAX_SYSCALL};
pub use permission::{SyscallPermission, CAP_ADMIN};

// Import handler functions from sub-modules for SSDT registration
use self::handlers::*;
use self::ob::*;
use self::cm::*;
pub use self::tests::{register_syscall_table_tests, register_sync_tests};

// ── Syscall Number Constants (frozen ABI) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SyscallNum {
    // Process (0-9)
    Exit = 0,
    Yield = 1,
    WaitAlertable = 2,
    SleepEx = 3,
    SetExceptionHandler = 4,
    // Memory (10-19)
    Brk = 10,
    Mmap = 11,
    Munmap = 12,
    // I/O (20-29)
    Write = 20,
    Read = 21,
    Dup2 = 22,
    Close = 23,
    Poll = 24,
    LoadLib = 25,
    // Console (30-39)
    CursorBlink = 30,
    // Driver (35-39)
    DriverUnload = 35,
    // Object Manager (40-49)
    ObOpen = 40,
    ObCreate = 41,
    ObQueryInfo = 42,
    ObSetInfo = 43,
    ObEnum = 44,
    ObWait = 45,
    ObDestroy = 46,
    ObService = 47,
    // Object Manager (48) — snapshot
    ObSnapshot = 48,
    // Registry Cm (50-59)
    CmOpenKey = 50,
    CmCreateKey = 51,
    CmQueryValue = 52,
    CmSetValue = 53,
    CmEnumKey = 54,
    CmEnumValue = 55,
    CmDeleteKey = 56,
    CmFlushKey = 57,
    CmLoadHive = 58,
    CmUnloadHive = 59,
}

impl SyscallNum {
    pub const MAX_VALID: u64 = 59;
    pub const HIGHEST_ASSIGNED: u64 = 59;

    pub fn from_u64(n: u64) -> Option<Self> {
        match n {
            0 => Some(Self::Exit),
            1 => Some(Self::Yield),
            2 => Some(Self::WaitAlertable),
            3 => Some(Self::SleepEx),
            4 => Some(Self::SetExceptionHandler),
            10 => Some(Self::Brk),
            11 => Some(Self::Mmap),
            12 => Some(Self::Munmap),
            20 => Some(Self::Write),
            21 => Some(Self::Read),
            22 => Some(Self::Dup2),
            23 => Some(Self::Close),
            24 => Some(Self::Poll),
            25 => Some(Self::LoadLib),
            30 => Some(Self::CursorBlink),
            35 => Some(Self::DriverUnload),
            40 => Some(Self::ObOpen),
            41 => Some(Self::ObCreate),
            42 => Some(Self::ObQueryInfo),
            43 => Some(Self::ObSetInfo),
            44 => Some(Self::ObEnum),
            45 => Some(Self::ObWait),
            46 => Some(Self::ObDestroy),
            47 => Some(Self::ObService),
            50 => Some(Self::CmOpenKey),
            51 => Some(Self::CmCreateKey),
            52 => Some(Self::CmQueryValue),
            53 => Some(Self::CmSetValue),
            54 => Some(Self::CmEnumKey),
            55 => Some(Self::CmEnumValue),
            56 => Some(Self::CmDeleteKey),
            57 => Some(Self::CmFlushKey),
            58 => Some(Self::CmLoadHive),
             59 => Some(Self::CmUnloadHive),
             77 => Some(Self::ObSnapshot),
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
    Perm = 16,
}

pub fn err_to_u64(e: SyscallError) -> u64 {
    (-(e as i64)) as u64
}

/// Convert an ObError to a SyscallError for the syscall return boundary.
/// All variants are mapped explicitly — no catch-all.
pub fn ob_err_to_syscall(e: crate::object::ObError) -> SyscallError {
    use crate::object::ObError;
    match e {
        ObError::NotFound => SyscallError::NoEnt,
        ObError::AlreadyExists => SyscallError::Exist,
        ObError::InvalidParam => SyscallError::Inval,
        ObError::RefCountHeld => SyscallError::Busy,
        ObError::OutOfMemory => SyscallError::NoMem,
        ObError::AccessDenied => SyscallError::Acces,
        ObError::NotSupported => SyscallError::NoSys,
        ObError::InvalidType => SyscallError::Inval,
        ObError::TableFull => SyscallError::NoMem,
        ObError::Success => SyscallError::Inval, // should not happen
    }
}

// ── ABI validation ──

pub fn validate_abi() {
    const ASSIGNED: &[u64] = &[
        0, 1, 2, 3, 4,
        10, 11, 12,
        20, 21, 22, 23, 24, 25,
        30, 35,
        40, 41, 42, 43, 44, 45, 46, 47, 48,
        50, 51, 52, 53, 54, 55, 56, 57, 58, 59,
    ];

    for &n in ASSIGNED {
        assert!(
            SYSCALL_TABLE[n as usize].is_some(),
            "SSDT missing handler for assigned syscall {}",
            n
        );
    }

    assert!((err_to_u64(SyscallError::Inval) as i64) < 0);
    assert!((err_to_u64(SyscallError::NoEnt) as i64) < 0);
    assert!((err_to_u64(SyscallError::Perm) as i64) < 0);

    crate::serial_println!("[SYS] SSDT validated ({} assigned syscalls)", ASSIGNED.len());
}

pub static NEED_RESCHED: AtomicBool = AtomicBool::new(false);
pub static KEYBOARD_LAYOUT: AtomicU8 = AtomicU8::new(1);

pub fn set_need_resched() {
    NEED_RESCHED.store(true, Ordering::SeqCst);
    unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
}

#[no_mangle]
pub extern "C" fn clear_need_resched() -> bool {
    crate::globals::flush_cache_if_needed();
    crate::work_queue::WORK_QUEUE.process_high();
    crate::dpc::dpc_dispatch_pending();
    crate::eventbus::EVENT_BUS.dispatch_pending();
    let prev_global = NEED_RESCHED.swap(false, Ordering::SeqCst);
    unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(false); }
    prev_global
}

#[no_mangle]
pub extern "C" fn is_thread_terminated() -> u64 {
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
    for part in rest.split(['\\', '/']) {
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

fn copy_handle_entry_for_child(entry: &crate::handle::HandleEntry) -> crate::handle::HandleEntry {
    if let Some(obj) = entry.obj_type() {
        if obj == crate::object::ObType::Pipe {
            if let Some(_nid) = entry.native_id() {
            }
        }
    }
    *entry
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

// ═══════════════════════════════════════════════════════════════════════
// SSDT + Permission Tables
// ═══════════════════════════════════════════════════════════════════════

lazy_static! {
    pub static ref SYSCALL_TABLE: [Option<SyscallFn>; 256] = {
        let mut t: [Option<SyscallFn>; 256] = [None; 256];
        t[0] = Some(handler_exit as SyscallFn);
        t[1] = Some(handler_yield as SyscallFn);
        t[2] = Some(handler_wait_alertable as SyscallFn);
        t[3] = Some(handler_sleep_ex as SyscallFn);
        t[4] = Some(handler_set_exception_handler as SyscallFn);
        t[10] = Some(handler_brk as SyscallFn);
        t[11] = Some(handler_mmap as SyscallFn);
        t[12] = Some(handler_munmap as SyscallFn);
        t[20] = Some(handler_write as SyscallFn);
        t[21] = Some(handler_read as SyscallFn);
        t[22] = Some(handler_dup2 as SyscallFn);
        t[23] = Some(handler_close as SyscallFn);
        t[24] = Some(handler_poll as SyscallFn);
        t[25] = Some(handler_loadlib as SyscallFn);
        t[30] = Some(handler_cursor_blink as SyscallFn);
        t[35] = Some(handler_driver_unload as SyscallFn);
        t[36] = Some(handler_icmp_ping as SyscallFn);
        t[40] = Some(handler_ob_open as SyscallFn);
        t[41] = Some(handler_ob_create as SyscallFn);
        t[42] = Some(handler_ob_query_info as SyscallFn);
        t[43] = Some(handler_ob_set_info as SyscallFn);
        t[44] = Some(handler_ob_enum as SyscallFn);
        t[45] = Some(handler_ob_wait as SyscallFn);
        t[46] = Some(handler_ob_destroy as SyscallFn);
        t[47] = Some(handler_ob_service as SyscallFn);
        t[48] = Some(handler_ob_snapshot as SyscallFn);
        t[50] = Some(handler_cm_open_key as SyscallFn);
        t[51] = Some(handler_cm_create_key as SyscallFn);
        t[52] = Some(handler_cm_query_value as SyscallFn);
        t[53] = Some(handler_cm_set_value as SyscallFn);
        t[54] = Some(handler_cm_enum_key as SyscallFn);
        t[55] = Some(handler_cm_enum_value as SyscallFn);
        t[56] = Some(handler_cm_delete_key as SyscallFn);
        t[57] = Some(handler_cm_flush_key as SyscallFn);
        t[58] = Some(handler_cm_load_hive as SyscallFn);
        t[59] = Some(handler_cm_unload_hive as SyscallFn);
        t
    };

    pub static ref SYSCALL_PERMISSIONS: [SyscallPermission; 256] = {
        let mut t: [SyscallPermission; 256] = [SyscallPermission::free(); 256];
        t[0] = SyscallPermission::user();
        t[1] = SyscallPermission::user();
        t[2] = SyscallPermission::user();
        t[3] = SyscallPermission::user();
        t[4] = SyscallPermission::user();
        t[10] = SyscallPermission::user();
        t[11] = SyscallPermission::user();
        t[12] = SyscallPermission::user();
        t[20] = SyscallPermission::user();
        t[21] = SyscallPermission::user();
        t[22] = SyscallPermission::user();
        t[23] = SyscallPermission::user();
        t[24] = SyscallPermission::user();
        t[25] = SyscallPermission::user();
        t[30] = SyscallPermission::user();
        t[35] = SyscallPermission::admin();
        t[36] = SyscallPermission::user();
        t[40] = SyscallPermission::user();
        t[41] = SyscallPermission::user();
        t[42] = SyscallPermission::user();
        t[43] = SyscallPermission::user();
        t[44] = SyscallPermission::user();
        t[45] = SyscallPermission::user();
        t[46] = SyscallPermission::user();
        t[47] = SyscallPermission::admin();
        t[48] = SyscallPermission::admin();
        t[50] = SyscallPermission::user();
        t[51] = SyscallPermission::user();
        t[52] = SyscallPermission::user();
        t[53] = SyscallPermission::user();
        t[54] = SyscallPermission::user();
        t[55] = SyscallPermission::user();
        t[56] = SyscallPermission::user();
        t[57] = SyscallPermission::user();
        t[58] = SyscallPermission::admin();
        t[59] = SyscallPermission::admin();
        t
    };
}

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

pub(crate) fn is_current_admin() -> bool {
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
    if rax == 40 || rax == 43 {
        serial_println!("[SYS] syscall rax={} rbx=0x{:x} rcx=0x{:x} rdx=0x{:x}", rax, rbx, rcx, rdx);
    }
    crate::trace_syscall!(rax, rbx, rcx, rdx);

    if rax >= 256 {
        serial_println!("[SYS] INVALID syscall number: {}", rax);
        return err_to_u64(SyscallError::NoSys);
    }

    let is_admin = is_current_admin();
    if let Err(e) = check_syscall_permission(rax, is_admin) {
        serial_println!("[SYS] syscall {} denied (admin={})", rax, is_admin);
        return e;
    }

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
