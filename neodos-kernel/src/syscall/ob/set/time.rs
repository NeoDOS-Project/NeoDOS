//! Ob set — system date/time.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError};
use super::validate_datetime;

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::DateTime as u32
}

/// Dispatch the `time` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObSetInfoClass::DateTime as u32 => {
            if !crate::syscall::is_current_admin() {
                return err_to_u64(SyscallError::Perm);
            }
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            // Only the \Global\Info\DateTime object (Key native_id 5) accepts it.
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 5 {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<crate::syscall::ob::types::SysDateTime>();
            if buf_size < sz {
                return err_to_u64(SyscallError::Inval);
            }
            let mut raw = [0u8; 7];
            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr as *const u8, raw.as_mut_ptr(), sz);
            }
            let (second, minute, hour, day, month, year) =
                (raw[0], raw[1], raw[2], raw[3], raw[4], raw[5]);
            if !validate_datetime(second, minute, hour, day, month, year) {
                return err_to_u64(SyscallError::Inval);
            }
            let dt = crate::drivers::rtc_bridge::DateTime {
                second, minute, hour, day, month, year,
            };
            if crate::drivers::rtc_bridge::set_datetime(&dt) {
                0
            } else {
                err_to_u64(SyscallError::Io)
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}

/// Register the `DateTime` clock-set tests.
///
/// Kept in this module (not `mod.rs`) because `rtc_bridge` is a dependency of
/// `time` already; this avoids adding a new `syscall -> drivers` dependency
/// edge from the dispatcher facade.
pub(super) fn register_tests() {
    use crate::{test_case, test_true};

    // #491: `ob_set_datetime` only succeeds if the bound RTC driver
    // acknowledges `EVENT_RTC_WRITE` with a fresh `EVENT_RTC_DATA` read-back.
    // A stale `rtc.nem` that predates `EVENT_RTC_WRITE` support binds the same
    // way but never ACKs, so this guards the image against packaging an
    // obsolete RTC driver. Skipped when no RTC driver is bound (e.g. a
    // kernel-only image), where the syscall path cannot be exercised.
    test_case!("ob_set_datetime_rtc_write_acks", {
        let rtc_bound = crate::eventbus::EVENT_BUS
            .count_handlers(crate::eventbus::EVENT_RTC_WRITE) > 0;
        if rtc_bound {
            match crate::drivers::rtc_bridge::request_datetime() {
                // Write the current time back and require the driver ACK.
                Some(now) => test_true!(crate::drivers::rtc_bridge::set_datetime(&now)),
                // Driver is bound but the read-back path is broken.
                None => test_true!(false),
            }
        }
    });
}
