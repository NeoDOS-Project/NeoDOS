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
pub use self::tests::register_syscall_table_tests;

// ── Syscall Number Constants (frozen ABI) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SyscallNum {
    Exit = 0,
    Write = 1,
    Yield = 2,
    // GetPid = 3, — removed; use ob_open + ob_query_info(ProcessId)
    Read = 4,
    Dup2 = 6,
    ReadDir = 8,
    WaitPid = 9,
    WriteFile = 12,
    Close = 13,
    ChDir = 16,
    Brk = 18,
    Mmap = 19,
    Munmap = 20,
    LoadLib = 21,
    ThreadCreate = 22,
    ThreadJoin = 23,
    WaitAlertable = 40,
    SleepEx = 41,
    // Poweroff = 42, — removed; use ob_set_info(PowerShutdown/PowerReboot)
    GetVolumeLabel = 46,
    ChDirParent = 47,
    KObjEnum = 48,
    SetPriority = 51,
    KillProcess = 52,
    SetExceptionHandler = 29,
    CursorBlink = 53,
    SetVolumeLabel = 54,
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
    // B2.1 Z6: Registry hive database (Cm)
    CmOpenKey = 67,
    CmCreateKey = 68,
    CmQueryValue = 69,
    CmSetValue = 70,
    CmEnumKey = 71,
    CmEnumValue = 72,
    CmDeleteKey = 73,
    CmFlushKey = 74,
    CmLoadHive = 75,
    CmUnloadHive = 76,
    ObService = 77,
}

impl SyscallNum {
    pub const MAX_VALID: u64 = 77;
    pub const HIGHEST_ASSIGNED: u64 = 77;

    pub fn from_u64(n: u64) -> Option<Self> {
        match n {
            0 => Some(Self::Exit),
            1 => Some(Self::Write),
            2 => Some(Self::Yield),
             // 3 → getpid removed; use ob_open + ob_query_info(ProcessId)
            4 => Some(Self::Read),
            6 => Some(Self::Dup2),
            8 => Some(Self::ReadDir),
            9 => Some(Self::WaitPid),
            12 => Some(Self::WriteFile),
            13 => Some(Self::Close),
            16 => Some(Self::ChDir),
            18 => Some(Self::Brk),
            19 => Some(Self::Mmap),
            20 => Some(Self::Munmap),
            21 => Some(Self::LoadLib),
            22 => Some(Self::ThreadCreate),
            23 => Some(Self::ThreadJoin),
            29 => Some(Self::SetExceptionHandler),
            40 => Some(Self::WaitAlertable),
            41 => Some(Self::SleepEx),
            // 42 → Poweroff removed; use Ob API (ob_set_info with PowerShutdown/PowerReboot)
            46 => Some(Self::GetVolumeLabel),
            47 => Some(Self::ChDirParent),
            48 => Some(Self::KObjEnum),
            51 => Some(Self::SetPriority),
            52 => Some(Self::KillProcess),
            53 => Some(Self::CursorBlink),
            54 => Some(Self::SetVolumeLabel),
            // 55 → fsck removed; use ob_query_info/ob_set_info
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
            67 => Some(Self::CmOpenKey),
            68 => Some(Self::CmCreateKey),
            69 => Some(Self::CmQueryValue),
            70 => Some(Self::CmSetValue),
            71 => Some(Self::CmEnumKey),
            72 => Some(Self::CmEnumValue),
            73 => Some(Self::CmDeleteKey),
            74 => Some(Self::CmFlushKey),
            75 => Some(Self::CmLoadHive),
            76 => Some(Self::CmUnloadHive),
            77 => Some(Self::ObService),
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
        0, 1, 2, 4, 6,
        13, 16, 18, 19, 20, 21,
        29,
        40, 41, 47, 53, 58, 59,
        60, 61, 62, 63, 64, 65, 66,
        67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77,
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
            .map(|ep| ep.token.clone())
            .unwrap_or(crate::security::DEFAULT_ADMIN_TOKEN.clone())
    });
    match crate::object::ob_open_path(&ob_path, &token, access) {
        Ok(ob_id) => {
            let _ = crate::object::ob_close_object(ob_id);
            Ok(())
        }
        Err(crate::object::ObError::AccessDenied) => {
            Err(err_to_u64(SyscallError::Acces))
        }
        Err(_) => {
            Ok(())
        }
    }
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
        11 => {
            let vt_num = crate::scheduler::current_vt_num();
            let active_vt = crate::input::active_vt();
            let mut buf = alloc::vec::Vec::with_capacity(4);
            buf.push(vt_num);
            buf.push(active_vt as u8);
            Some(buf)
        }
        _ => None,
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
        // t[3] was handler_getpid — removed; use ob_open + ob_query_info(ProcessId)
        t[4] = Some(handler_read as SyscallFn);
        t[6] = Some(handler_dup2 as SyscallFn);
        t[13] = Some(handler_close as SyscallFn);
        t[16] = Some(handler_chdir as SyscallFn);
        t[18] = Some(handler_brk as SyscallFn);
        t[19] = Some(handler_mmap as SyscallFn);
        t[20] = Some(handler_munmap as SyscallFn);
        t[21] = Some(handler_loadlib as SyscallFn);
        t[29] = Some(handler_set_exception_handler as SyscallFn);
        t[40] = Some(handler_wait_alertable as SyscallFn);
        t[41] = Some(handler_sleep_ex as SyscallFn);
        t[47] = Some(handler_chdir_parent as SyscallFn);
        t[53] = Some(handler_cursor_blink as SyscallFn);
        t[58] = Some(handler_driver_unload as SyscallFn);
        t[59] = Some(handler_poll as SyscallFn);
        t[60] = Some(handler_ob_open as SyscallFn);
        t[61] = Some(handler_ob_create as SyscallFn);
        t[62] = Some(handler_ob_query_info as SyscallFn);
        t[63] = Some(handler_ob_set_info as SyscallFn);
        t[64] = Some(handler_ob_enum as SyscallFn);
        t[65] = Some(handler_ob_wait as SyscallFn);
        t[66] = Some(handler_ob_destroy as SyscallFn);
        t[67] = Some(handler_cm_open_key as SyscallFn);
        t[68] = Some(handler_cm_create_key as SyscallFn);
        t[69] = Some(handler_cm_query_value as SyscallFn);
        t[70] = Some(handler_cm_set_value as SyscallFn);
        t[71] = Some(handler_cm_enum_key as SyscallFn);
        t[72] = Some(handler_cm_enum_value as SyscallFn);
        t[73] = Some(handler_cm_delete_key as SyscallFn);
        t[74] = Some(handler_cm_flush_key as SyscallFn);
        t[75] = Some(handler_cm_load_hive as SyscallFn);
        t[76] = Some(handler_cm_unload_hive as SyscallFn);
        t[77] = Some(handler_ob_service as SyscallFn);
        t
    };

    pub static ref SYSCALL_PERMISSIONS: [SyscallPermission; 256] = {
        let mut t: [SyscallPermission; 256] = [SyscallPermission::free(); 256];
        t[0] = SyscallPermission::user();
        t[1] = SyscallPermission::user();
        t[2] = SyscallPermission::user();
        t[4] = SyscallPermission::user();
        t[5] = SyscallPermission::user();
        t[6] = SyscallPermission::user();
        t[7] = SyscallPermission::user();
        t[8] = SyscallPermission::user();
        t[9] = SyscallPermission::user();
        t[10] = SyscallPermission::user();
        t[11] = SyscallPermission::user();
        t[13] = SyscallPermission::user();
        t[16] = SyscallPermission::user();
        t[18] = SyscallPermission::user();
        t[19] = SyscallPermission::user();
        t[20] = SyscallPermission::user();
        t[21] = SyscallPermission::user();
        t[29] = SyscallPermission::user();
        t[40] = SyscallPermission::user();
        t[41] = SyscallPermission::user();
        t[47] = SyscallPermission::user();
        t[53] = SyscallPermission::user();
        t[58] = SyscallPermission::admin();
        t[59] = SyscallPermission::user();
        t[60] = SyscallPermission::user();
        t[61] = SyscallPermission::user();
        t[62] = SyscallPermission::user();
        t[63] = SyscallPermission::user();
        t[64] = SyscallPermission::user();
        t[65] = SyscallPermission::user();
        t[66] = SyscallPermission::user();
        t[67] = SyscallPermission::user();
        t[68] = SyscallPermission::user();
        t[69] = SyscallPermission::user();
        t[70] = SyscallPermission::user();
        t[71] = SyscallPermission::user();
        t[72] = SyscallPermission::user();
        t[73] = SyscallPermission::user();
        t[74] = SyscallPermission::user();
        t[75] = SyscallPermission::admin();
        t[76] = SyscallPermission::admin();
        t[77] = SyscallPermission::admin();
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
