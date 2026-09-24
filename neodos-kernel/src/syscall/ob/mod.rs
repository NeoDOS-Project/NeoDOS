//! Ob syscall handlers — modular split (re-exports)
pub mod types;
pub mod open;
pub mod create;
pub mod create_process;
pub mod query;
pub mod set;
pub mod r#enum;
pub mod wait;
pub mod destroy;

pub use types::*;
// Re-export handlers for SSDT registration (pub(super) in submodules)
pub use open::handler_ob_open;
pub use create::handler_ob_create;
pub use query::handler_ob_query_info;
pub use set::handler_ob_set_info;
pub use r#enum::handler_ob_enum;
pub use wait::handler_ob_wait;
pub use destroy::handler_ob_destroy;

// Snapshot and service remain in mod.rs (not split per spec, kept for minimal change)
use crate::syscall::{err_to_u64, SyscallError};
use crate::syscall::util::is_user_ptr_valid;

const SNAPSHOT_OP_CREATE: u32 = 0;
const SNAPSHOT_OP_RESTORE: u32 = 1;
const SNAPSHOT_OP_LIST: u32 = 2;
const SNAPSHOT_OP_PURGE: u32 = 3;

const SERVICE_CONTROL_START: u32 = 0;
const SERVICE_CONTROL_STOP: u32 = 1;
const SERVICE_CONTROL_RESTART: u32 = 2;
const SERVICE_CONTROL_QUERY_STATUS: u32 = 3;
const SERVICE_CONTROL_SET_CONFIG: u32 = 4;

pub(super) fn handler_ob_snapshot(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let op = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    let drive_idx = match crate::syscall::ob::destroy::resolve_handle_drive(fd) {
        Ok(d) => d,
        Err(e) => return e,
    };

    match op {
        SNAPSHOT_OP_CREATE => {
            let id = crate::globals::with_vfs(|vfs| {
                vfs.snapshot_create(drive_idx)
            });
            match id {
                Ok(id) => id,
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        SNAPSHOT_OP_RESTORE => {
            if buf_ptr == 0 || buf_size < 8 {
                return err_to_u64(SyscallError::Inval);
            }
            if !is_user_ptr_valid(buf_ptr, 8) {
                return err_to_u64(SyscallError::Fault);
            }
            let snapshot_id = unsafe { core::ptr::read_volatile(buf_ptr as *const u64) };
            let result = crate::globals::with_vfs(|vfs| {
                vfs.snapshot_restore(drive_idx, snapshot_id)
            });
            match result {
                Ok(()) => 0,
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        SNAPSHOT_OP_LIST => {
            if buf_ptr == 0 || buf_size < 24 {
                return err_to_u64(SyscallError::Inval);
            }
            if !is_user_ptr_valid(buf_ptr, buf_size as u64) {
                return err_to_u64(SyscallError::Fault);
            }
            let mut kernel_buf = alloc::vec::Vec::with_capacity(buf_size);
            kernel_buf.resize(buf_size, 0u8);
            let count = crate::globals::with_vfs(|vfs| {
                vfs.snapshot_list(drive_idx, &mut kernel_buf)
            });
            match count {
                Ok(n) => {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            kernel_buf.as_ptr(),
                            buf_ptr as *mut u8,
                            buf_size.min(n * core::mem::size_of::<crate::fs::snapshot::SnapshotEntryRaw>()),
                        );
                    }
                    n as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        SNAPSHOT_OP_PURGE => {
            let result = crate::globals::with_vfs(|vfs| {
                vfs.snapshot_purge(drive_idx)
            });
            match result {
                Ok(()) => 0,
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ => err_to_u64(SyscallError::NoSys),
    }
}


pub(super) fn handler_ob_service(regs: super::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let control = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_len = regs.r8 as usize;

    let entry = crate::syscall::current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }
    if entry.object_id == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let obj = match crate::object::ob_lookup(entry.object_id) {
        Some(o) => o,
        None => return err_to_u64(SyscallError::BadF),
    };
    if obj.obj_type != crate::object::ObType::Service {
        return err_to_u64(SyscallError::Inval);
    }

    match control {
        SERVICE_CONTROL_START => {
            let mut sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            match sm.start_service(idx) {
                Ok(()) => 0,
                Err(_) => err_to_u64(SyscallError::Busy),
            }
        }
        SERVICE_CONTROL_STOP => {
            let timeout_ms = if buf_len >= 4 && buf_ptr != 0 {
                unsafe { core::ptr::read_volatile(buf_ptr as *const u32) }
            } else {
                0
            };
            let mut sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            match sm.stop_service(idx, timeout_ms) {
                Ok(()) => 0,
                Err(_) => err_to_u64(SyscallError::Busy),
            }
        }
        SERVICE_CONTROL_RESTART => {
            let timeout_ms = if buf_len >= 4 && buf_ptr != 0 {
                unsafe { core::ptr::read_volatile(buf_ptr as *const u32) }
            } else {
                0
            };
            let mut sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            match sm.restart_service(idx, timeout_ms) {
                Ok(()) => 0,
                Err(_) => err_to_u64(SyscallError::Busy),
            }
        }
        SERVICE_CONTROL_QUERY_STATUS => {
            if buf_ptr == 0 || buf_len < 29 {
                return err_to_u64(SyscallError::Inval);
            }
            if !is_user_ptr_valid(buf_ptr, 29) {
                return err_to_u64(SyscallError::Fault);
            }
            let sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            let svc = &sm.services[idx];
            let status: [u8; 29] = [
                svc.state as u8,
                svc.pid as u8, (svc.pid >> 8) as u8,
                (svc.pid >> 16) as u8, (svc.pid >> 24) as u8,
                svc.exit_count as u8, (svc.exit_count >> 8) as u8,
                (svc.exit_count >> 16) as u8, (svc.exit_count >> 24) as u8,
                svc.last_exit_code as u8, (svc.last_exit_code >> 8) as u8,
                (svc.last_exit_code >> 16) as u8, (svc.last_exit_code >> 24) as u8,
                (svc.last_exit_code >> 32) as u8, (svc.last_exit_code >> 40) as u8,
                (svc.last_exit_code >> 48) as u8, (svc.last_exit_code >> 56) as u8,
                svc.failure_count as u8, (svc.failure_count >> 8) as u8,
                (svc.failure_count >> 16) as u8, (svc.failure_count >> 24) as u8,
                svc.start_tick as u8, (svc.start_tick >> 8) as u8,
                (svc.start_tick >> 16) as u8, (svc.start_tick >> 24) as u8,
                (svc.start_tick >> 32) as u8, (svc.start_tick >> 40) as u8,
                (svc.start_tick >> 48) as u8, (svc.start_tick >> 56) as u8,
            ];
            unsafe {
                core::ptr::copy_nonoverlapping(status.as_ptr(), buf_ptr as *mut u8, 29);
            }
            29
        }
        SERVICE_CONTROL_SET_CONFIG => {
            if buf_ptr == 0 || buf_len < 6 {
                return err_to_u64(SyscallError::Inval);
            }
            if !is_user_ptr_valid(buf_ptr, 6) {
                return err_to_u64(SyscallError::Fault);
            }
            let start_type = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
            let restart_policy = unsafe { core::ptr::read_volatile((buf_ptr + 1) as *const u8) };
            let max_failures = unsafe { core::ptr::read_volatile((buf_ptr + 2) as *const u32) };

            use crate::services::{ServiceStartType, ServiceRestartPolicy};
            let st = match start_type {
                0 => ServiceStartType::Boot,
                1 => ServiceStartType::System,
                2 => ServiceStartType::Auto,
                3 => ServiceStartType::Demand,
                4 => ServiceStartType::Disabled,
                _ => return err_to_u64(SyscallError::Inval),
            };
            let rp = match restart_policy {
                0 => ServiceRestartPolicy::Never,
                1 => ServiceRestartPolicy::OnCrash,
                2 => ServiceRestartPolicy::Always,
                _ => return err_to_u64(SyscallError::Inval),
            };
            let mut sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            match sm.set_config(idx, st, rp, max_failures) {
                Ok(()) => 0,
                Err(_) => err_to_u64(SyscallError::Inval),
            }
        }
        _ => err_to_u64(SyscallError::NoSys),
    }
}
