//! Regional formats: dates, numbers and currencies (`I18N-P10`).
//!
//! A regional-format block in NLTv3 is:
//!
//! ```text
//! u8  version = 1
//! u8  flags   (bit0 = 24h clock, bit1 = currency symbol before value,
//!              bit2 = group thousands with the thousands separator)
//! NUL-terminated UTF-8 strings, in order:
//!   decimal_separator
//!   thousands_separator
//!   currency_symbol
//!   date_short_pattern
//!   date_long_pattern
//!   time_pattern
//! ```
//!
//! Pattern tokens: `yyyy yy y`, `MM M`, `dd d`, `HH H`, `hh h`, `mm`, `ss`,
//! `A`/`a` (AM/PM). Anything else is copied literally.

/// A parsed regional-format block, borrowing from the NLT payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region<'a> {
    pub decimal_separator: &'a str,
    pub thousands_separator: &'a str,
    pub currency_symbol: &'a str,
    pub date_short: &'a str,
    pub date_long: &'a str,
    pub time_pattern: &'a str,
    pub time_24h: bool,
    pub currency_before: bool,
    pub group_thousands: bool,
}

/// Field order used by [`encode`].
const FIELDS: usize = 6;

/// Parse a regional-format block.
pub fn parse(data: &[u8]) -> Option<Region<'_>> {
    if data.len() < 2 {
        return None;
    }
    let version = data[0];
    if version != 1 {
        return None;
    }
    let flags = data[1];
    let mut fields: [&str; FIELDS] = [""; FIELDS];
    let mut i = 2usize;
    for f in fields.iter_mut() {
        let end = data[i..].iter().position(|&b| b == 0)?;
        *f = core::str::from_utf8(&data[i..i + end]).ok()?;
        i += end + 1;
    }
    Some(Region {
        decimal_separator: fields[0],
        thousands_separator: fields[1],
        currency_symbol: fields[2],
        date_short: fields[3],
        date_long: fields[4],
        time_pattern: fields[5],
        time_24h: flags & 0x01 != 0,
        currency_before: flags & 0x02 != 0,
        group_thousands: flags & 0x04 != 0,
    })
}

/// Encode a regional-format block into `out`; returns bytes written.
#[allow(clippy::too_many_arguments)]
pub fn encode(
    decimal_separator: &str,
    thousands_separator: &str,
    currency_symbol: &str,
    date_short: &str,
    date_long: &str,
    time_pattern: &str,
    time_24h: bool,
    currency_before: bool,
    group_thousands: bool,
    out: &mut [u8],
) -> Option<usize> {
    let mut flags = 0u8;
    if time_24h {
        flags |= 0x01;
    }
    if currency_before {
        flags |= 0x02;
    }
    if group_thousands {
        flags |= 0x04;
    }
    let mut pos = 0usize;
    pos = put(out, pos, &[1, flags]);
    for f in [
        decimal_separator,
        thousands_separator,
        currency_symbol,
        date_short,
        date_long,
        time_pattern,
    ] {
        pos = put(out, pos, f.as_bytes());
        pos = put(out, pos, &[0]);
    }
    Some(pos)
}

fn put(out: &mut [u8], pos: usize, bytes: &[u8]) -> usize {
    let mut p = pos;
    for &b in bytes {
        if p < out.len() {
            out[p] = b;
            p += 1;
        }
    }
    p
}

/// Format an integer with thousands grouping.
pub fn format_number(value: i64, region: &Region<'_>, out: &mut [u8]) -> usize {
    let mut digits = [0u8; 24];
    let neg = value < 0;
    let mut mag = if neg { (value as i128).unsigned_abs() as u64 } else { value as u64 };
    let mut n = 0usize;
    if mag == 0 {
        digits[0] = b'0';
        n = 1;
    } else {
        while mag > 0 {
            digits[n] = b'0' + (mag % 10) as u8;
            mag /= 10;
            n += 1;
        }
    }
    // digits are little-endian
    let mut pos = 0usize;
    if neg {
        pos = put(out, pos, b"-");
    }
    for idx in (0..n).rev() {
        pos = put(out, pos, &[digits[idx]]);
        let remaining = idx;
        if region.group_thousands
            && remaining > 0
            && remaining % 3 == 0
            && !region.thousands_separator.is_empty()
        {
            pos = put(out, pos, region.thousands_separator.as_bytes());
        }
    }
    pos
}

/// Format a currency amount. `value` is in minor units scaled by `10^decimals`.
pub fn format_currency(
    value: i64,
    decimals: u8,
    region: &Region<'_>,
    out: &mut [u8],
) -> usize {
    let mut pos = 0usize;
    if region.currency_before {
        pos = put(out, pos, region.currency_symbol.as_bytes());
        pos = put(out, pos, b" ");
    }
    pos = put_scaled(value, decimals, region, out, pos);
    if !region.currency_before {
        pos = put(out, pos, b" ");
        pos = put(out, pos, region.currency_symbol.as_bytes());
    }
    pos
}

fn put_scaled(value: i64, decimals: u8, region: &Region<'_>, out: &mut [u8], pos: usize) -> usize {
    if decimals == 0 {
        return format_number(value, region, &mut out[pos..]) + pos;
    }
    let neg = value < 0;
    let mut mag = if neg { (value as i128).unsigned_abs() as u64 } else { value as u64 };
    let scale = 10u64.pow(decimals as u32);
    let int_part = mag / scale;
    mag %= scale;
    let mut p = pos;
    if neg {
        p = put(out, p, b"-");
    }
    p = format_number(int_part as i64, region, &mut out[p..]) + p;
    p = put(out, p, region.decimal_separator.as_bytes());
    // fractional part, zero padded to `decimals`
    let mut frac = [b'0'; 6];
    let mut i = decimals as usize;
    while i > 0 {
        frac[i - 1] = b'0' + (mag % 10) as u8;
        mag /= 10;
        i -= 1;
    }
    put(out, p, &frac[..decimals as usize])
}

/// Format a date using `region.date_short` or `region.date_long`.
pub fn format_date(
    year: i32,
    month: u8,
    day: u8,
    long: bool,
    region: &Region<'_>,
    out: &mut [u8],
) -> usize {
    let pattern = if long { region.date_long } else { region.date_short };
    render_pattern(pattern, year, month, day, 0, 0, 0, region, out)
}

/// Format a time using `region.time_pattern`.
pub fn format_time(
    hour: u8,
    minute: u8,
    second: u8,
    region: &Region<'_>,
    out: &mut [u8],
) -> usize {
    render_pattern(
        region.time_pattern,
        0,
        0,
        0,
        hour,
        minute,
        second,
        region,
        out,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_pattern(
    pattern: &str,
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    region: &Region<'_>,
    out: &mut [u8],
) -> usize {
    let p = pattern.as_bytes();
    let mut i = 0usize;
    let mut pos = 0usize;
    while i < p.len() {
        let c = p[i];
        let run = p[i..].iter().take_while(|&&b| b == c).count();
        match c {
            b'y' => {
                if run >= 4 {
                    pos = put_num(out, pos, year as i64, 4);
                } else if run == 2 {
                    pos = put_num(out, pos, (year.rem_euclid(100)) as i64, 2);
                } else {
                    pos = put_num(out, pos, year as i64, 0);
                }
            }
            b'M' => {
                if run >= 2 {
                    pos = put_num(out, pos, month as i64, 2);
                } else {
                    pos = put_num(out, pos, month as i64, 0);
                }
            }
            b'd' => {
                if run >= 2 {
                    pos = put_num(out, pos, day as i64, 2);
                } else {
                    pos = put_num(out, pos, day as i64, 0);
                }
            }
            b'H' => {
                let h = if region.time_24h { hour } else { hour % 12 };
                pos = if run >= 2 {
                    put_num(out, pos, h as i64, 2)
                } else {
                    put_num(out, pos, h as i64, 0)
                };
            }
            b'h' => {
                let h = {
                    let h12 = hour % 12;
                    if h12 == 0 {
                        12
                    } else {
                        h12
                    }
                };
                pos = if run >= 2 {
                    put_num(out, pos, h as i64, 2)
                } else {
                    put_num(out, pos, h as i64, 0)
                };
            }
            b'm' => {
                pos = if run >= 2 {
                    put_num(out, pos, minute as i64, 2)
                } else {
                    put_num(out, pos, minute as i64, 0)
                };
            }
            b's' => {
                pos = if run >= 2 {
                    put_num(out, pos, second as i64, 2)
                } else {
                    put_num(out, pos, second as i64, 0)
                };
            }
            b'A' | b'a' => {
                let ampm: &[u8] = if hour < 12 { b"AM" } else { b"PM" };
                if c == b'a' {
                    let mut buf = [ampm[0], ampm[1]];
                    buf[0] = buf[0].to_ascii_lowercase();
                    buf[1] = buf[1].to_ascii_lowercase();
                    pos = put(out, pos, &buf);
                } else {
                    pos = put(out, pos, ampm);
                }
            }
            _ => {
                for _ in 0..run {
                    pos = put(out, pos, &[c]);
                }
            }
        }
        i += run;
    }
    pos
}

fn put_num(out: &mut [u8], pos: usize, v: i64, width: usize) -> usize {
    let mut buf = [0u8; 24];
    let neg = v < 0;
    let mut mag = if neg { (-(v as i128)) as u64 } else { v as u64 };
    let mut n = 0usize;
    if mag == 0 {
        buf[0] = b'0';
        n = 1;
    } else {
        while mag > 0 {
            buf[n] = b'0' + (mag % 10) as u8;
            mag /= 10;
            n += 1;
        }
    }
    let mut pos = pos;
    if neg {
        pos = put(out, pos, b"-");
    }
    let mut digits = 0usize;
    while digits < width.saturating_sub(n) {
        pos = put(out, pos, b"0");
        digits += 1;
    }
    for i in (0..n).rev() {
        pos = put(out, pos, &[buf[i]]);
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn es_region() -> [u8; 64] {
        let mut buf = [0u8; 64];
        let n = encode(
            ",", ".", "€", "dd/MM/yyyy", "d 'de' MMMM 'de' yyyy", "HH:mm:ss", true, false, true,
            &mut buf,
        )
        .unwrap();
        let mut out = [0u8; 64];
        out[..n].copy_from_slice(&buf[..n]);
        out
    }

    #[test]
    fn parse_roundtrip() {
        let raw = es_region();
        let r = parse(&raw).unwrap();
        assert_eq!(r.decimal_separator, ",");
        assert_eq!(r.thousands_separator, ".");
        assert_eq!(r.currency_symbol, "€");
        assert!(r.time_24h && r.group_thousands && !r.currency_before);
    }

    #[test]
    fn number_grouping() {
        let raw = es_region();
        let r = parse(&raw).unwrap();
        let mut out = [0u8; 32];
        let n = format_number(1234567, &r, &mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "1.234.567");
    }

    #[test]
    fn currency_scaled() {
        let raw = es_region();
        let r = parse(&raw).unwrap();
        let mut out = [0u8; 32];
        let n = format_currency(123456, 2, &r, &mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "1.234,56 €");
    }

    #[test]
    fn date_short() {
        let raw = es_region();
        let r = parse(&raw).unwrap();
        let mut out = [0u8; 32];
        let n = format_date(2026, 3, 7, false, &r, &mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "07/03/2026");
    }

    #[test]
    fn time_24h() {
        let raw = es_region();
        let r = parse(&raw).unwrap();
        let mut out = [0u8; 32];
        let n = format_time(9, 5, 3, &r, &mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "09:05:03");
    }
}
