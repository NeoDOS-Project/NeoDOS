//! Ob query — system date/time and timezone.

use crate::object::types::ObInfoClass;
use crate::syscall::{err_to_u64, SyscallError};
use crate::syscall::ob::types::{SysDateTime, SysTimeZone};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::DateTime as u32
        || info_class == ObInfoClass::TimeZone as u32
        || info_class == ObInfoClass::LocalDateTime as u32
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
        _ if info_class == ObInfoClass::DateTime as u32 => {
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
        _ if info_class == ObInfoClass::TimeZone as u32 => {
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
            let sz = core::mem::size_of::<SysTimeZone>() as usize;
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let tz = crate::cm::timezone::load();
            let sys = SysTimeZone {
                utc_offset_minutes: tz.utc_offset_minutes,
                dst_offset_minutes: tz.dst_offset_minutes,
                dst_enabled: if tz.dst_enabled { 1 } else { 0 },
                dst_start_month: tz.dst_start_month,
                dst_start_day: tz.dst_start_day,
                dst_end_month: tz.dst_end_month,
                dst_end_day: tz.dst_end_day,
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &sys as *const SysTimeZone as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::LocalDateTime as u32 => {
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
            let tz = crate::cm::timezone::load();
            let sysdt = match crate::drivers::rtc_bridge::request_datetime() {
                Some(d) => {
                    let l = tz.to_local(&d);
                    SysDateTime {
                        second: l.second, minute: l.minute, hour: l.hour,
                        day: l.day, month: l.month, year: l.year, valid: 1,
                    }
                }
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
        _ => err_to_u64(SyscallError::Inval),
    }
}
