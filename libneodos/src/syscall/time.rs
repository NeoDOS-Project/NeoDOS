//! Time-related Ob wrappers.

use super::{sys_ob_open, sys_ob_set_info, sys_close, ObSetInfoClass, ob_access, DateTime};

/// Set the system (RTC) clock via ob_set_info(DateTime).
///
/// Requires admin privileges. Opens `\Global\Info\DateTime`, writes the
/// `DateTime` payload and closes the handle. This is the API `ntpd` uses to
/// apply an NTP-corrected time (a direct step; there is no slew yet).
pub fn ob_set_datetime(dt: &DateTime) -> Result<(), i64> {
    let fd = sys_ob_open("\\Global\\Info\\DateTime", ob_access::READ | ob_access::WRITE)?;
    let sz = core::mem::size_of::<DateTime>();
    // SAFETY: DateTime is #[repr(C)] and `sz` is its exact size.
    let buf = unsafe { core::slice::from_raw_parts(dt as *const DateTime as *const u8, sz) };
    let r = sys_ob_set_info(fd, ObSetInfoClass::DateTime, buf);
    let _ = sys_close(fd);
    r
}

