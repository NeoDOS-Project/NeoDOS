//! Ring 3 bindings: RTC via `ObInfoClass::DateTime` and the `ntpd` status from
//! the Registry.
//!
//! Enabled with the `sys` feature so the pure policy above stays host-testable.

use crate::{evaluate, unix_from_rtc, Clock, ClockError, NtpdStatus, RtcFields};
use libneodos::registry::RegistryKey;
use libneodos::syscall;

/// Registry key `ntpd` publishes `State`/`LastSync` to.
pub const NTPD_STATUS_KEY: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\Ntpd\\Status";

/// Read the RTC through `ObInfoClass::DateTime`.
pub fn rtc() -> Result<RtcFields, ClockError> {
    let dt = syscall::ob_datetime().map_err(|_| ClockError::RtcInvalid)?;
    Ok(RtcFields {
        second: dt.second,
        minute: dt.minute,
        hour: dt.hour,
        day: dt.day,
        month: dt.month,
        year: dt.year,
        valid: dt.valid != 0,
    })
}

/// Unix seconds from the RTC, without the trust signal.
///
/// Use [`trusted_now_unix`] whenever the value decides something security
/// relevant (certificate validity, update signatures).
pub fn unix_seconds() -> Result<i64, ClockError> {
    unix_from_rtc(rtc()?)
}

/// Trusted wall-clock seconds: the RTC must be valid **and** `ntpd` must have
/// reported a successful sync. Missing status reads as unsynchronised.
pub fn trusted_now_unix() -> Result<i64, ClockError> {
    let rtc = rtc()?;
    let mut state_buf = [0u8; 32];
    let mut last_sync = 0u32;
    let mut state_len = 0usize;
    if let Ok(key) = RegistryKey::open(NTPD_STATUS_KEY) {
        state_len = key.query_string("State", &mut state_buf);
        last_sync = key.query_dword("LastSync").unwrap_or(0);
    }
    let state = core::str::from_utf8(&state_buf[..state_len]).unwrap_or("");
    evaluate(
        rtc,
        NtpdStatus {
            state,
            last_sync_unix: last_sync,
        },
    )
}

/// [`Clock`] backed by the system RTC and the `ntpd` trust signal.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix(&self) -> Option<i64> {
        trusted_now_unix().ok()
    }
}
