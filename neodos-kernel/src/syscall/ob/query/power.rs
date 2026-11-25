//! Ob query — power manager state (`ObInfoClass::PowerState`, #510).

use crate::object::types::ObInfoClass;
use crate::syscall::{copy_to_user, err_to_u64, SyscallError};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::PowerState as u32
}

/// Dispatch the `power` info class.
///
/// Writes the current `PowerSystemState` discriminant as a little-endian `u32`
/// (0 = Active, 1 = ShuttingDown, 2 = Rebooting, 3 = Suspending,
/// 4 = Hibernating, 5 = Off) and returns the number of bytes written.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObInfoClass::PowerState as u32 => {
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
            let sz = core::mem::size_of::<u32>();
            if buf_size < sz {
                return err_to_u64(SyscallError::Inval);
            }
            let state = crate::power::POWER_MANAGER.lock().state() as u32;
            if copy_to_user(buf_ptr, &state.to_le_bytes()).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            sz as u64
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
