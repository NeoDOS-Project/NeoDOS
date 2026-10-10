//! Ob set — service control.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError, copy_from_user};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::ServiceStart as u32
        || info_class == ObSetInfoClass::ServiceStop as u32
        || info_class == ObSetInfoClass::ServiceRestart as u32
        || info_class == ObSetInfoClass::ServiceSetConfig as u32
}

/// Dispatch the `service` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObSetInfoClass::ServiceStart as u32 => {
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
            let mut sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            match sm.start_service(idx) {
                Ok(()) => 0,
                Err(_e) => err_to_u64(SyscallError::Busy),
            }
        }
        _ if info_class == ObSetInfoClass::ServiceStop as u32 => {
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
            let timeout_ms = if buf_size >= 4 {
                let mut b = [0u8; 4];
                if copy_from_user(&mut b, buf_ptr).is_err() {
                    return err_to_u64(SyscallError::Fault);
                }
                u32::from_ne_bytes(b)
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
                Err(_e) => err_to_u64(SyscallError::Busy),
            }
        }
        _ if info_class == ObSetInfoClass::ServiceRestart as u32 => {
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
            let timeout_ms = if buf_size >= 4 {
                let mut b = [0u8; 4];
                if copy_from_user(&mut b, buf_ptr).is_err() {
                    return err_to_u64(SyscallError::Fault);
                }
                u32::from_ne_bytes(b)
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
                Err(_e) => err_to_u64(SyscallError::Busy),
            }
        }
        _ if info_class == ObSetInfoClass::ServiceSetConfig as u32 => {
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
            if buf_size < 6 {
                return err_to_u64(SyscallError::Inval);
            }
            let mut cfg = [0u8; 6];
            if copy_from_user(&mut cfg, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let start_type = cfg[0];
            let restart_policy = cfg[1];
            let max_failures = u32::from_ne_bytes([cfg[2], cfg[3], cfg[4], cfg[5]]);

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
        _ => err_to_u64(SyscallError::Inval),
    }
}
