//! Trusted wall-clock policy for NeoDOS userland.
//!
//! The RTC is readable and `ntpd` can step it, but neither alone makes the
//! wall clock trustworthy: a machine that has never synchronised (or whose
//! CMOS battery died) can report any date. This crate holds the policy, kept
//! free of syscalls so it is host-testable:
//!
//! * the RTC value must be a valid calendar date/time;
//! * `Services\Ntpd\Status` must say `State == "Synced"` with a non-zero
//!   `LastSync`;
//! * the RTC must not predate that last sync (a reset clock is implausible).
//!
//! TLS and the update tooling fail closed when the clock is not trusted.
//! The Ring 3 bindings live in [`system`] behind the `sys` feature, so
//! `cargo test` never links the syscall layer.
//!
//! ```text
//! libclock (pure)  ← libntp (civil time)
//!   └─ system (feature = "sys") ← libneodos (RTC + Registry)
//! ```

#![cfg_attr(not(test), no_std)]

use libntp::{is_valid_datetime, utc_to_unix_secs, UtcDateTime};

#[cfg(feature = "sys")]
pub mod system;

/// `Services\Ntpd\Status\State` value that marks a synchronised clock.
pub const STATE_SYNCED: &str = "Synced";

/// Tolerance for an RTC that is slightly behind the recorded last sync.
/// Anything further behind means the clock was reset after the sync.
pub const MAX_SKEW_SECS: i64 = 300;

/// RTC fields as exposed by `ObInfoClass::DateTime`.
///
/// The ABI uses a **two-digit year** (2000..=2099) and a `valid` flag set by
/// the kernel when the read was consistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtcFields {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    pub valid: bool,
}

impl RtcFields {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        second: u8,
        minute: u8,
        hour: u8,
        day: u8,
        month: u8,
        year: u8,
        valid: bool,
    ) -> Self {
        RtcFields {
            second,
            minute,
            hour,
            day,
            month,
            year,
            valid,
        }
    }

    fn as_utc(self) -> UtcDateTime {
        UtcDateTime {
            second: self.second,
            minute: self.minute,
            hour: self.hour,
            day: self.day,
            month: self.month,
            year: self.year,
        }
    }
}

/// NTP daemon state relevant to trust (`Services\Ntpd\Status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtpdStatus<'a> {
    /// `State`: `"Synced"`, `"Error"`, `"WaitingNetwork"`, ...
    pub state: &'a str,
    /// `LastSync`: Unix seconds of the last successful sync (`0` = never).
    pub last_sync_unix: u32,
}

/// Why a wall-clock reading cannot be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockError {
    /// The RTC did not provide a valid calendar value.
    RtcInvalid,
    /// NTP has not synchronised the clock.
    Unsynced,
    /// The RTC predates its own last NTP sync (reset clock or dead battery).
    Implausible,
}

impl ClockError {
    /// Short, stable description (suitable for CLI output).
    pub fn as_str(self) -> &'static str {
        match self {
            ClockError::RtcInvalid => "RTC time is not valid",
            ClockError::Unsynced => "clock is not synchronised (ntpd has not synced)",
            ClockError::Implausible => "RTC time is implausible (before the last sync)",
        }
    }
}

/// Convert RTC fields to Unix seconds without checking the trust signal.
///
/// The two-digit year maps to 2000..=2099, so the representable range is
/// 2000-01-01T00:00:00Z .. 2099-12-31T23:59:59Z.
pub fn unix_from_rtc(rtc: RtcFields) -> Result<i64, ClockError> {
    if !rtc.valid {
        return Err(ClockError::RtcInvalid);
    }
    let utc = rtc.as_utc();
    if !is_valid_datetime(&utc) {
        return Err(ClockError::RtcInvalid);
    }
    Ok(utc_to_unix_secs(&utc))
}

/// Trusted wall-clock seconds, or why they cannot be trusted.
pub fn evaluate(rtc: RtcFields, ntpd: NtpdStatus<'_>) -> Result<i64, ClockError> {
    let now = unix_from_rtc(rtc)?;
    if ntpd.state != STATE_SYNCED || ntpd.last_sync_unix == 0 {
        return Err(ClockError::Unsynced);
    }
    if now.saturating_add(MAX_SKEW_SECS) < ntpd.last_sync_unix as i64 {
        return Err(ClockError::Implausible);
    }
    Ok(now)
}

/// A source of trusted wall-clock seconds.
///
/// `None` means "not trusted"; callers must fail closed rather than use a
/// best-effort timestamp (certificate validity depends on it).
pub trait Clock {
    fn now_unix(&self) -> Option<i64>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD_RTC: RtcFields = RtcFields::new(30, 15, 12, 16, 11, 26, true);
    // 2026-11-16T12:15:30Z
    const GOOD_SECS: i64 = 1_794_831_330;
    const SYNCED: NtpdStatus<'static> = NtpdStatus {
        state: STATE_SYNCED,
        last_sync_unix: 1_794_831_000,
    };

    #[test]
    fn rtc_converts_to_unix_seconds() {
        assert_eq!(unix_from_rtc(GOOD_RTC), Ok(GOOD_SECS));
        // The ABI stores a two-digit year: 2000-01-01T00:00:00Z is the floor.
        let floor = RtcFields::new(0, 0, 0, 1, 1, 0, true);
        assert_eq!(unix_from_rtc(floor), Ok(946_684_800));
        // Leap day 2024-02-29T00:00:00Z
        let leap = RtcFields::new(0, 0, 0, 29, 2, 24, true);
        assert_eq!(unix_from_rtc(leap), Ok(1_709_164_800));
    }

    #[test]
    fn invalid_rtc_is_rejected() {
        assert_eq!(
            unix_from_rtc(RtcFields::new(0, 0, 0, 1, 1, 70, false)),
            Err(ClockError::RtcInvalid)
        );
        // Feb 30 does not exist.
        assert_eq!(
            unix_from_rtc(RtcFields::new(0, 0, 0, 30, 2, 26, true)),
            Err(ClockError::RtcInvalid)
        );
        assert_eq!(
            unix_from_rtc(RtcFields::new(0, 0, 24, 1, 1, 26, true)),
            Err(ClockError::RtcInvalid)
        );
    }

    #[test]
    fn synced_clock_is_trusted() {
        assert_eq!(evaluate(GOOD_RTC, SYNCED), Ok(GOOD_SECS));
    }

    #[test]
    fn unsynced_clock_is_rejected() {
        for state in ["Error", "WaitingNetwork", "Stopped", "Disabled", ""] {
            assert_eq!(
                evaluate(
                    GOOD_RTC,
                    NtpdStatus {
                        state,
                        last_sync_unix: 1_794_831_000
                    }
                ),
                Err(ClockError::Unsynced),
                "state {state:?}"
            );
        }
        // "Synced" without a timestamp is still unknown.
        assert_eq!(
            evaluate(
                GOOD_RTC,
                NtpdStatus {
                    state: STATE_SYNCED,
                    last_sync_unix: 0
                }
            ),
            Err(ClockError::Unsynced)
        );
    }

    #[test]
    fn rtc_far_behind_the_last_sync_is_implausible() {
        let future_sync = NtpdStatus {
            state: STATE_SYNCED,
            last_sync_unix: (GOOD_SECS + MAX_SKEW_SECS + 1) as u32,
        };
        assert_eq!(evaluate(GOOD_RTC, future_sync), Err(ClockError::Implausible));
        // Within the skew tolerance it is accepted (NTP steps the RTC late).
        let near_sync = NtpdStatus {
            state: STATE_SYNCED,
            last_sync_unix: (GOOD_SECS + MAX_SKEW_SECS) as u32,
        };
        assert_eq!(evaluate(GOOD_RTC, near_sync), Ok(GOOD_SECS));
    }

    #[test]
    fn clock_trait_can_fail_closed() {
        struct Unsynced;
        impl Clock for Unsynced {
            fn now_unix(&self) -> Option<i64> {
                None
            }
        }
        assert_eq!(Unsynced.now_unix(), None);
    }
}
