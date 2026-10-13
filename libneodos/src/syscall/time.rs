//! Time-related Ob wrappers.

use super::{sys_ob_open, sys_ob_query_info, sys_ob_set_info, sys_close, ObInfoClass, ObSetInfoClass, ob_access, DateTime};

/// Monotonic system uptime in milliseconds and the tick rate in Hz (#467).
///
/// Never decreases. Use it for profiling, frame timing, timeouts or animation;
/// it is unrelated to the wall clock (RTC/`ob_set_datetime`). The design-doc
/// name is `ob_uptime_ms`; this returns both the ms value and the tick rate.
pub fn ob_uptime() -> Result<(u64, u32), i64> {
    let fd = sys_ob_open("\\Global\\Info\\DateTime", ob_access::READ)?;
    let mut buf = [0u8; 16];
    let r = sys_ob_query_info(fd, ObInfoClass::Uptime, &mut buf);
    let _ = sys_close(fd);
    let n = r?;
    if n < 16 {
        return Err(super::EINVAL);
    }
    let ms = u64::from_le_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ]);
    let hz = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    Ok((ms, hz))
}

/// Monotonic uptime in milliseconds (never decreases) (#467).
pub fn ob_uptime_ms() -> Result<u64, i64> {
    Ok(ob_uptime()?.0)
}

/// High-resolution monotonic time `(tsc, tsc_khz)` from the calibrated TSC
/// (#467). Derive sub-millisecond deltas as `(tsc2-tsc1) / tsc_khz` ms.
pub fn ob_high_res_time() -> Result<(u64, u32), i64> {
    let fd = sys_ob_open("\\Global\\Info\\DateTime", ob_access::READ)?;
    let mut buf = [0u8; 16];
    let r = sys_ob_query_info(fd, ObInfoClass::HighResTime, &mut buf);
    let _ = sys_close(fd);
    let n = r?;
    if n < 16 {
        return Err(super::EINVAL);
    }
    let tsc = u64::from_le_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ]);
    let khz = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    Ok((tsc, khz))
}

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

