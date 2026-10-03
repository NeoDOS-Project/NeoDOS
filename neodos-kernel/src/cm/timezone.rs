//! Timezone / DST support for the system clock (#357).
//!
//! The RTC is authoritative **UTC**. Local time is derived by applying a
//! configurable standard offset and an optional DST window. Configuration is
//! read from the Registry under
//! `CurrentControlSet\Control\TimeZoneInformation`:
//!
//! | Value | Type | Meaning |
//! | ----- | ---- | ------- |
//! | `UtcOffsetMinutes` | DWORD | Standard offset, local = UTC + offset (signed) |
//! | `DaylightOffsetMinutes` | DWORD | Additional offset applied during DST |
//! | `DaylightEnabled` | DWORD | 0/1 |
//! | `DaylightStartMonth` / `DaylightStartDay` | DWORD | DST start (inclusive) |
//! | `DaylightEndMonth` / `DaylightEndDay` | DWORD | DST end (exclusive) |
//!
//! DST boundaries are evaluated on the UTC date for simplicity (the smallest
//! unit is a day). This is intentionally not a full IANA tzdata engine.

use crate::drivers::rtc_bridge::DateTime;

pub const TZ_KEY: &str = "CurrentControlSet\\Control\\TimeZoneInformation";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeZone {
    /// Standard offset in minutes: `local = UTC + utc_offset_minutes`.
    pub utc_offset_minutes: i32,
    /// Extra minutes added while DST is active.
    pub dst_offset_minutes: i32,
    pub dst_enabled: bool,
    pub dst_start_month: u8,
    pub dst_start_day: u8,
    pub dst_end_month: u8,
    pub dst_end_day: u8,
}

impl TimeZone {
    /// UTC, no DST.
    pub const UTC: TimeZone = TimeZone {
        utc_offset_minutes: 0,
        dst_offset_minutes: 0,
        dst_enabled: false,
        dst_start_month: 3,
        dst_start_day: 1,
        dst_end_month: 10,
        dst_end_day: 1,
    };

    /// Whether DST is active for the given date. The window is
    /// `[start, end)`; if `start > end` it wraps the year boundary
    /// (southern hemisphere).
    pub fn in_dst(&self, month: u8, day: u8) -> bool {
        if !self.dst_enabled {
            return false;
        }
        let cur = month as u16 * 100 + day as u16;
        let start = self.dst_start_month as u16 * 100 + self.dst_start_day as u16;
        let end = self.dst_end_month as u16 * 100 + self.dst_end_day as u16;
        if start <= end {
            cur >= start && cur < end
        } else {
            cur >= start || cur < end
        }
    }

    /// Active offset in minutes for the given date.
    pub fn offset_for(&self, month: u8, day: u8) -> i32 {
        if self.in_dst(month, day) {
            self.utc_offset_minutes + self.dst_offset_minutes
        } else {
            self.utc_offset_minutes
        }
    }

    /// Derive local time from authoritative UTC.
    pub fn to_local(&self, utc: &DateTime) -> DateTime {
        add_minutes(utc, self.offset_for(utc.month, utc.day))
    }
}

pub fn days_in_month(year: u8, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            // 2000..=2099: every year divisible by 4 is a leap year.
            if year % 4 == 0 { 29 } else { 28 }
        }
        _ => 30,
    }
}

/// Add `delta` minutes to a UTC timestamp, normalizing day/month/year.
fn add_minutes(dt: &DateTime, delta: i32) -> DateTime {
    let mut total = dt.hour as i32 * 60 + dt.minute as i32 + delta;
    let mut day = dt.day as i32;
    let mut month = dt.month as i32;
    let mut year = dt.year as i32;

    while total < 0 {
        total += 1440;
        day -= 1;
    }
    while total >= 1440 {
        total -= 1440;
        day += 1;
    }

    loop {
        let dim = days_in_month(year as u8, month as u8) as i32;
        if day < 1 {
            month -= 1;
            if month < 1 {
                month = 12;
                year = if year == 0 { 99 } else { year - 1 };
            }
            day += days_in_month(year as u8, month as u8) as i32;
        } else if day > dim {
            day -= dim;
            month += 1;
            if month > 12 {
                month = 1;
                year = if year >= 99 { 0 } else { year + 1 };
            }
        } else {
            break;
        }
    }

    DateTime {
        second: dt.second,
        minute: (total % 60) as u8,
        hour: (total / 60) as u8,
        day: day as u8,
        month: month as u8,
        year: year as u8,
    }
}

fn dword(key: u64, name: &str) -> Option<i32> {
    let v = crate::cm::cm_query_value(key, name).ok()?;
    if v.data.len() < 4 {
        return None;
    }
    Some(i32::from_le_bytes([v.data[0], v.data[1], v.data[2], v.data[3]]))
}

/// Load the timezone configuration from the Registry. Falls back to UTC when
/// the key or a value is absent, so an empty hive is safe.
pub fn load() -> TimeZone {
    let mut tz = TimeZone::UTC;
    let key = match crate::cm::cm_open_key(0, TZ_KEY) {
        Ok(k) => k,
        Err(_) => return tz,
    };
    if let Some(v) = dword(key, "UtcOffsetMinutes") {
        tz.utc_offset_minutes = v;
    }
    if let Some(v) = dword(key, "DaylightOffsetMinutes") {
        tz.dst_offset_minutes = v;
    }
    if let Some(v) = dword(key, "DaylightEnabled") {
        tz.dst_enabled = v != 0;
    }
    if let Some(v) = dword(key, "DaylightStartMonth") {
        tz.dst_start_month = v.clamp(1, 12) as u8;
    }
    if let Some(v) = dword(key, "DaylightStartDay") {
        tz.dst_start_day = v.clamp(1, 31) as u8;
    }
    if let Some(v) = dword(key, "DaylightEndMonth") {
        tz.dst_end_month = v.clamp(1, 12) as u8;
    }
    if let Some(v) = dword(key, "DaylightEndDay") {
        tz.dst_end_day = v.clamp(1, 31) as u8;
    }
    tz
}

// ── Tests ──────────────────────────────────────────────────────────────

pub fn register_timezone_tests() {
    use crate::{test_case, test_eq, test_true};

    fn dt(hour: u8, minute: u8, day: u8, month: u8, year: u8) -> DateTime {
        DateTime { second: 0, minute, hour, day, month, year }
    }

    test_case!("tz_offset_positive", {
        let tz = TimeZone { utc_offset_minutes: 60, ..TimeZone::UTC };
        let l = tz.to_local(&dt(12, 0, 15, 6, 26));
        test_eq!(l.hour, 13);
        test_eq!(l.minute, 0);
        test_eq!(l.day, 15);
        test_eq!(l.month, 6);
    });

    test_case!("tz_offset_negative", {
        let tz = TimeZone { utc_offset_minutes: -300, ..TimeZone::UTC };
        let l = tz.to_local(&dt(12, 0, 15, 6, 26));
        test_eq!(l.hour, 7);
        test_eq!(l.day, 15);
    });

    test_case!("tz_offset_wraps_next_day", {
        let tz = TimeZone { utc_offset_minutes: 60, ..TimeZone::UTC };
        let l = tz.to_local(&dt(23, 30, 31, 12, 26));
        test_eq!(l.hour, 0);
        test_eq!(l.minute, 30);
        test_eq!(l.day, 1);
        test_eq!(l.month, 1);
        test_eq!(l.year, 27);
    });

    test_case!("tz_offset_wraps_prev_day", {
        let tz = TimeZone { utc_offset_minutes: -60, ..TimeZone::UTC };
        let l = tz.to_local(&dt(0, 30, 1, 3, 26));
        test_eq!(l.hour, 23);
        test_eq!(l.day, 28);
        test_eq!(l.month, 2);
        test_eq!(l.year, 26);
    });

    test_case!("tz_dst_boundary", {
        // Standard +60, DST +60 (so +120 in summer): Mar 30 .. Oct 26.
        let tz = TimeZone {
            utc_offset_minutes: 60,
            dst_offset_minutes: 60,
            dst_enabled: true,
            dst_start_month: 3,
            dst_start_day: 30,
            dst_end_month: 10,
            dst_end_day: 26,
        };
        test_eq!(tz.offset_for(3, 29), 60);
        test_eq!(tz.offset_for(3, 30), 120);
        test_eq!(tz.offset_for(10, 25), 120);
        test_eq!(tz.offset_for(10, 26), 60);
        test_true!(tz.in_dst(6, 1));
        test_true!(!tz.in_dst(12, 1));
    });

    test_case!("tz_dst_southern_wrap", {
        // DST from Oct 1 to Mar 1 (wraps the year end).
        let tz = TimeZone {
            utc_offset_minutes: 600,
            dst_offset_minutes: 60,
            dst_enabled: true,
            dst_start_month: 10,
            dst_start_day: 1,
            dst_end_month: 3,
            dst_end_day: 1,
        };
        test_eq!(tz.offset_for(11, 15), 660);
        test_eq!(tz.offset_for(1, 15), 660);
        test_eq!(tz.offset_for(3, 1), 600);
        test_eq!(tz.offset_for(6, 1), 600);
    });

    test_case!("tz_disabled_is_standard", {
        let tz = TimeZone { utc_offset_minutes: -480, dst_enabled: false, ..TimeZone::UTC };
        test_eq!(tz.offset_for(7, 1), -480);
        test_true!(!tz.in_dst(7, 1));
    });

    test_case!("tz_days_in_month", {
        test_eq!(days_in_month(26, 2), 28);
        test_eq!(days_in_month(24, 2), 29);
        test_eq!(days_in_month(26, 4), 30);
        test_eq!(days_in_month(26, 1), 31);
    });
}
