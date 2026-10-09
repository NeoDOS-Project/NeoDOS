//! Shared NTP/SNTP wire format and clock math for NeoDOS.
//!
//! This crate contains the protocol logic (SNTP client request/response,
//! RFC 5905 offset/delay computation and civil-time conversion) with no kernel
//! or syscall dependency, so it can be unit tested on the host. The `ntpd`
//! userland daemon provides the transport (UDP sockets via `net.nxl`), the
//! Registry-backed configuration and the clock-setting call on top of it.
//!
//! Only SNTP unicast client mode (NTPv4, mode 3/4) is implemented. There is no
//! authentication (NTS/MAC), no broadcast/multicast mode and no clock
//! discipline (slew/drift) — the daemon applies a direct step.
//!
//! When built for NeoDOS it is `no_std`; under `cargo test` it uses the host
//! `std` test harness.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ── Protocol constants ──

/// UDP port for NTP/SNTP.
pub const NTP_PORT: u16 = 123;
/// NTP packet size in bytes (SNTP unicast).
pub const NTP_PACKET_SIZE: usize = 48;
/// Seconds between the NTP epoch (1900-01-01) and the Unix epoch (1970-01-01).
pub const NTP_EPOCH_OFFSET: u32 = 2_208_988_800;

pub const NTP_MODE_CLIENT: u8 = 3;
pub const NTP_MODE_SERVER: u8 = 4;
pub const NTP_VERSION: u8 = 4;

pub const NTP_LEAP_NO_WARNING: u8 = 0;
pub const NTP_LEAP_ALARM: u8 = 3;

/// `stratum` 0 in a reply is a Kiss-o'-Death packet, not a time source.
pub const NTP_STRATUM_KOD: u8 = 0;
/// Valid server strata are 1..=15.
pub const NTP_STRATUM_MAX: u8 = 15;

// ── Errors ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NtpError {
    /// Packet shorter than [`NTP_PACKET_SIZE`].
    ShortPacket,
    /// Reply mode is not server (4).
    NotServer,
    /// Leap indicator is 3 (clock not synchronized / alarm).
    LeapAlarm,
    /// Stratum is 0 (Kiss-o'-Death) or out of range.
    InvalidStratum,
    /// Transmit timestamp is zero (server did not answer yet).
    ZeroTransmit,
    /// Originate timestamp does not match the request we sent.
    OriginateMismatch,
}

// ── Packet ──

/// A decoded SNTP reply, in NTP timestamp units (seconds since 1900 + fraction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtpReply {
    pub leap: u8,
    pub version: u8,
    pub mode: u8,
    pub stratum: u8,
    pub poll: i8,
    pub precision: i8,
    pub root_delay: u32,
    pub root_dispersion: u32,
    pub reference_id: u32,
    /// 64-bit NTP timestamp (reference clock update time).
    pub reference_ts: u64,
    /// 64-bit NTP timestamp (copy of the client's transmit timestamp).
    pub originate_ts: u64,
    /// 64-bit NTP timestamp (server receive time).
    pub receive_ts: u64,
    /// 64-bit NTP timestamp (server transmit time).
    pub transmit_ts: u64,
}

/// Build a 48-byte SNTP client request.
///
/// `transmit_ts` is the client's transmit timestamp (64-bit NTP units); pass
/// `0` if unknown. The server echoes it back in the reply's `originate_ts`,
/// which lets the client reject stale/mismatched datagrams.
pub fn build_request(transmit_ts: u64) -> [u8; NTP_PACKET_SIZE] {
    let mut pkt = [0u8; NTP_PACKET_SIZE];
    // LI = 0, VN = 4, Mode = 3 (client).
    pkt[0] = (NTP_VERSION << 3) | NTP_MODE_CLIENT;
    // Stratum/poll/precision left zero: ignored by the server in a request.
    pkt[40..48].copy_from_slice(&transmit_ts.to_be_bytes());
    pkt
}

fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn read_u64(buf: &[u8], off: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[off..off + 8]);
    u64::from_be_bytes(b)
}

/// Parse and validate an SNTP server reply.
///
/// `expected_originate` is the transmit timestamp sent in the request; when
/// non-zero the reply's originate timestamp must match it, otherwise the
/// datagram is rejected as stale.
pub fn parse_reply(buf: &[u8], expected_originate: u64) -> Result<NtpReply, NtpError> {
    if buf.len() < NTP_PACKET_SIZE {
        return Err(NtpError::ShortPacket);
    }

    let leap = buf[0] >> 6;
    let version = (buf[0] >> 3) & 0x7;
    let mode = buf[0] & 0x7;

    if mode != NTP_MODE_SERVER {
        return Err(NtpError::NotServer);
    }
    if leap == NTP_LEAP_ALARM {
        return Err(NtpError::LeapAlarm);
    }

    let stratum = buf[1];
    if stratum == NTP_STRATUM_KOD || stratum > NTP_STRATUM_MAX {
        return Err(NtpError::InvalidStratum);
    }

    let transmit_ts = read_u64(buf, 40);
    if transmit_ts == 0 {
        return Err(NtpError::ZeroTransmit);
    }

    let originate_ts = read_u64(buf, 24);
    if expected_originate != 0 && originate_ts != expected_originate {
        return Err(NtpError::OriginateMismatch);
    }

    Ok(NtpReply {
        leap,
        version,
        mode,
        stratum,
        poll: buf[2] as i8,
        precision: buf[3] as i8,
        root_delay: read_u32(buf, 4),
        root_dispersion: read_u32(buf, 8),
        reference_id: read_u32(buf, 12),
        reference_ts: read_u64(buf, 16),
        originate_ts,
        receive_ts: read_u64(buf, 32),
        transmit_ts,
    })
}

// ── Timestamp conversion ──

/// Convert a 64-bit NTP timestamp to nanoseconds since the Unix epoch.
pub fn ntp_to_unix_ns(ts: u64) -> i128 {
    let secs = (ts >> 32) as u32;
    let frac = ts as u32;
    let unix_secs = secs.wrapping_sub(NTP_EPOCH_OFFSET) as i128;
    let frac_ns = ((frac as u128) * 1_000_000_000u128) >> 32;
    unix_secs * 1_000_000_000 + frac_ns as i128
}

/// Convert Unix seconds to a 64-bit NTP timestamp.
pub fn unix_secs_to_ntp(secs: i64) -> u64 {
    let ntp_secs = secs.wrapping_add(NTP_EPOCH_OFFSET as i64) as u32;
    (ntp_secs as u64) << 32
}

/// Round-trip offset and network delay, in nanoseconds (RFC 5905 §8).
///
/// * `t1` — client transmit time (Unix ns)
/// * `t2` — server receive time (Unix ns, from the reply)
/// * `t3` — server transmit time (Unix ns, from the reply)
/// * `t4` — client receive time (Unix ns)
///
/// `offset` is how much the client clock must be advanced (positive) to match
/// the server. `delay` is the round-trip network delay.
pub fn offset_and_delay(t1: i128, t2: i128, t3: i128, t4: i128) -> (i128, i128) {
    let offset = ((t2 - t1) + (t3 - t4)) / 2;
    let delay = (t4 - t1) - (t3 - t2);
    (offset, delay)
}

// ── Civil time ──

/// A UTC calendar date/time with two-digit years, matching NeoDOS `SysDateTime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtcDateTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
}

/// Days from civil date to days since 1970-01-01 (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as i64; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`]: civil date from days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Convert Unix seconds to a UTC calendar date/time.
///
/// The `year` field is the last two digits of the Gregorian year (0–99), the
/// format NeoDOS `SysDateTime` uses. Returns `None` for dates before 1970 or
/// after 2069, which cannot be represented in two digits.
pub fn unix_secs_to_utc(secs: i64) -> Option<UtcDateTime> {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    if !(1970..=2069).contains(&year) {
        return None;
    }
    Some(UtcDateTime {
        second: (rem % 60) as u8,
        minute: ((rem / 60) % 60) as u8,
        hour: (rem / 3600) as u8,
        day: day as u8,
        month: month as u8,
        year: (year % 100) as u8,
    })
}

/// Convert a UTC calendar date/time (two-digit year) to Unix seconds.
pub fn utc_to_unix_secs(dt: &UtcDateTime) -> i64 {
    let year = 2000 + dt.year as i64;
    let days = days_from_civil(year, dt.month as u32, dt.day as u32);
    days * 86_400
        + dt.hour as i64 * 3600
        + dt.minute as i64 * 60
        + dt.second as i64
}

/// Validate a UTC date/time field range. Two-digit years are accepted as 0–99.
pub fn is_valid_datetime(dt: &UtcDateTime) -> bool {
    if dt.month < 1 || dt.month > 12 {
        return false;
    }
    if dt.day < 1 || dt.day > 31 {
        return false;
    }
    if dt.hour > 23 || dt.minute > 59 || dt.second > 60 {
        return false;
    }
    // Reject impossible day-of-month values (e.g. Feb 30) via round-trip.
    let year = 2000 + dt.year as i64;
    let days = days_from_civil(year, dt.month as u32, dt.day as u32);
    let (y, m, d) = civil_from_days(days);
    y == year && m == dt.month as u32 && d == dt.day as u32
}

// ── Configuration helpers ──

/// Split a `;`-separated server list, trimming whitespace and dropping empties.
pub fn parse_servers(raw: &str) -> Vec<&str> {
    raw.split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Split a `;`-separated server list into owned strings (userland convenience).
pub fn parse_servers_owned(raw: &str) -> Vec<String> {
    parse_servers(raw).into_iter().map(String::from).collect()
}

// ── Date/time formatting ──

fn put_02(v: u8, buf: &mut [u8], pos: &mut usize) {
    if *pos < buf.len() { buf[*pos] = b'0' + (v / 10) % 10; *pos += 1; }
    if *pos < buf.len() { buf[*pos] = b'0' + v % 10; *pos += 1; }
}

fn put_sep(b: u8, buf: &mut [u8], pos: &mut usize) {
    if *pos < buf.len() { buf[*pos] = b; *pos += 1; }
}

fn put_year4(year: u8, buf: &mut [u8], pos: &mut usize) {
    let y = 2000u16 + year as u16;
    for shift in [1000u16, 100, 10, 1] {
        if *pos < buf.len() { buf[*pos] = b'0' + ((y / shift) % 10) as u8; *pos += 1; }
    }
}

/// Format `DD/MM/YY` (two-digit year) into `buf`; returns bytes written.
pub fn format_date(dt: &UtcDateTime, buf: &mut [u8]) -> usize {
    let mut pos = 0usize;
    put_02(dt.day, buf, &mut pos);
    put_sep(b'/', buf, &mut pos);
    put_02(dt.month, buf, &mut pos);
    put_sep(b'/', buf, &mut pos);
    put_02(dt.year, buf, &mut pos);
    pos.min(buf.len())
}

/// Format `HH:MM:SS` into `buf`; returns bytes written.
pub fn format_time(dt: &UtcDateTime, buf: &mut [u8]) -> usize {
    let mut pos = 0usize;
    put_02(dt.hour, buf, &mut pos);
    put_sep(b':', buf, &mut pos);
    put_02(dt.minute, buf, &mut pos);
    put_sep(b':', buf, &mut pos);
    put_02(dt.second, buf, &mut pos);
    pos.min(buf.len())
}

/// Format `DD/MM/YYYY HH:MM:SS` into `buf`; returns bytes written.
pub fn format_datetime(dt: &UtcDateTime, buf: &mut [u8]) -> usize {
    let mut pos = 0usize;
    put_02(dt.day, buf, &mut pos);
    put_sep(b'/', buf, &mut pos);
    put_02(dt.month, buf, &mut pos);
    put_sep(b'/', buf, &mut pos);
    put_year4(dt.year, buf, &mut pos);
    put_sep(b' ', buf, &mut pos);
    put_02(dt.hour, buf, &mut pos);
    put_sep(b':', buf, &mut pos);
    put_02(dt.minute, buf, &mut pos);
    put_sep(b':', buf, &mut pos);
    put_02(dt.second, buf, &mut pos);
    pos.min(buf.len())
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn reply_with(transmit: u64, receive: u64, originate: u64) -> [u8; 48] {
        let mut b = [0u8; 48];
        b[0] = (NTP_VERSION << 3) | NTP_MODE_SERVER;
        b[1] = 2; // stratum
        b[24..32].copy_from_slice(&originate.to_be_bytes());
        b[32..40].copy_from_slice(&receive.to_be_bytes());
        b[40..48].copy_from_slice(&transmit.to_be_bytes());
        b
    }

    #[test]
    fn request_has_client_mode_and_version() {
        let pkt = build_request(0);
        assert_eq!(pkt.len(), NTP_PACKET_SIZE);
        assert_eq!(pkt[0] & 0x7, NTP_MODE_CLIENT);
        assert_eq!((pkt[0] >> 3) & 0x7, NTP_VERSION);
        assert_eq!(&pkt[40..48], &[0u8; 8]);
    }

    #[test]
    fn request_carries_transmit_timestamp() {
        let ts = unix_secs_to_ntp(1_700_000_000);
        let pkt = build_request(ts);
        assert_eq!(u64::from_be_bytes(pkt[40..48].try_into().unwrap()), ts);
    }

    #[test]
    fn parse_valid_reply() {
        let orig = unix_secs_to_ntp(1_700_000_000);
        let r = parse_reply(&reply_with(orig, orig, orig), orig).unwrap();
        assert_eq!(r.mode, NTP_MODE_SERVER);
        assert_eq!(r.stratum, 2);
        assert_eq!(r.transmit_ts, orig);
    }

    #[test]
    fn parse_rejects_short_packet() {
        let b = [0u8; 20];
        assert_eq!(parse_reply(&b, 0), Err(NtpError::ShortPacket));
    }

    #[test]
    fn parse_rejects_non_server_mode() {
        let mut b = reply_with(1, 1, 1);
        b[0] = (NTP_VERSION << 3) | NTP_MODE_CLIENT;
        assert_eq!(parse_reply(&b, 0), Err(NtpError::NotServer));
    }

    #[test]
    fn parse_rejects_leap_alarm() {
        let mut b = reply_with(1, 1, 1);
        b[0] = (NTP_LEAP_ALARM << 6) | (NTP_VERSION << 3) | NTP_MODE_SERVER;
        assert_eq!(parse_reply(&b, 0), Err(NtpError::LeapAlarm));
    }

    #[test]
    fn parse_rejects_kiss_o_death() {
        let mut b = reply_with(1, 1, 1);
        b[1] = 0;
        assert_eq!(parse_reply(&b, 0), Err(NtpError::InvalidStratum));
    }

    #[test]
    fn parse_rejects_zero_transmit() {
        let mut b = reply_with(0, 1, 1);
        b[40..48].copy_from_slice(&[0u8; 8]);
        assert_eq!(parse_reply(&b, 0), Err(NtpError::ZeroTransmit));
    }

    #[test]
    fn parse_rejects_originate_mismatch() {
        let b = reply_with(100, 100, 100);
        assert_eq!(parse_reply(&b, 999), Err(NtpError::OriginateMismatch));
    }

    #[test]
    fn offset_and_delay_symmetric() {
        // Symmetric path: server exactly 5s ahead, 100ms each way.
        let t1 = 0i128;
        let t4 = 200_000_000i128; // 200ms RTT
        let t2 = 5_000_000_000i128 + 100_000_000;
        let t3 = 5_000_000_000i128 + 100_000_000;
        let (offset, delay) = offset_and_delay(t1, t2, t3, t4);
        assert_eq!(offset, 5_000_000_000);
        assert_eq!(delay, 200_000_000);
    }

    #[test]
    fn offset_handles_asymmetric_delay() {
        // 10ms outbound, 90ms inbound, server +1s.
        let t1 = 0i128;
        let t2 = 1_000_000_000i128 + 10_000_000;
        let t3 = 1_000_000_000i128 + 10_000_000;
        let t4 = 100_000_000i128;
        let (offset, delay) = offset_and_delay(t1, t2, t3, t4);
        assert_eq!(offset, 1_000_000_000 - 40_000_000);
        assert_eq!(delay, 100_000_000);
    }

    #[test]
    fn ntp_timestamp_round_trip() {
        let unix = 1_700_000_000i64;
        let ntp = unix_secs_to_ntp(unix);
        assert_eq!(ntp_to_unix_ns(ntp), unix as i128 * 1_000_000_000);
    }

    #[test]
    fn civil_round_trip_known_dates() {
        for &(y, m, d) in &[(1970, 1, 1), (2000, 2, 29), (2024, 12, 31), (2069, 12, 31)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d));
        }
    }

    #[test]
    fn unix_to_utc_epoch() {
        let dt = unix_secs_to_utc(0).unwrap();
        assert_eq!(dt, UtcDateTime { second: 0, minute: 0, hour: 0, day: 1, month: 1, year: 70 });
    }

    #[test]
    fn utc_round_trip() {
        let secs = 1_700_000_000i64;
        let dt = unix_secs_to_utc(secs).unwrap();
        assert!(is_valid_datetime(&dt));
        assert_eq!(utc_to_unix_secs(&dt), secs);
    }

    #[test]
    fn utc_rejects_out_of_range() {
        assert!(unix_secs_to_utc(-1).is_none());
        assert!(unix_secs_to_utc(4_000_000_000).is_none());
    }

    #[test]
    fn datetime_validation() {
        let good = UtcDateTime { second: 0, minute: 0, hour: 0, day: 29, month: 2, year: 24 };
        assert!(is_valid_datetime(&good));
        let bad = UtcDateTime { second: 0, minute: 0, hour: 0, day: 30, month: 2, year: 24 };
        assert!(!is_valid_datetime(&bad));
        let bad_month = UtcDateTime { second: 0, minute: 0, hour: 0, day: 1, month: 13, year: 24 };
        assert!(!is_valid_datetime(&bad_month));
    }

    #[test]
    fn parse_servers_trims_and_drops_empty() {
        assert_eq!(parse_servers("a; b ;;c;"), alloc::vec!["a", "b", "c"]);
        assert!(parse_servers("   ").is_empty());
    }

    #[test]
    fn format_helpers_pad_fields() {
        let dt = UtcDateTime { second: 5, minute: 7, hour: 9, day: 3, month: 4, year: 24 };
        let mut b = [0u8; 24];
        let n = format_date(&dt, &mut b);
        assert_eq!(&b[..n], b"03/04/24");
        let n = format_time(&dt, &mut b);
        assert_eq!(&b[..n], b"09:07:05");
        let n = format_datetime(&dt, &mut b);
        assert_eq!(&b[..n], b"03/04/2024 09:07:05");
    }
}
