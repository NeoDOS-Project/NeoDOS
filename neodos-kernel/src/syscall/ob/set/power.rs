//! Ob set — system shutdown/reboot.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::PowerShutdown as u32
        || info_class == ObSetInfoClass::PowerReboot as u32
}

/// Dispatch the `power` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    _buf_ptr: u64,
    _buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObSetInfoClass::PowerShutdown as u32 => {
            ktrace!(crate::log::LogSubsys::Power, "dispatch PowerShutdown: entering");
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            ktrace!(crate::log::LogSubsys::Power, "dispatch PowerShutdown: ob_lookup ok type={:?}", obj.obj_type);
            if obj.obj_type != crate::object::ObType::PowerManager {
                return err_to_u64(SyscallError::Inval);
            }
            ktrace!(crate::log::LogSubsys::Power, "dispatch PowerShutdown: calling power_shutdown");
            crate::object::power::power_shutdown();
        }
        _ if info_class == ObSetInfoClass::PowerReboot as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::PowerManager {
                return err_to_u64(SyscallError::Inval);
            }
            crate::object::power::power_reboot();
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
