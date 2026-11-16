//! Minimal DER reader (ITU-T X.690) for X.509 and TLS structures.

/// DER parsing failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerError {
    Truncated,
    UnexpectedTag { expected: u8, found: u8 },
    Malformed,
    UnsupportedTime,
}

pub type Result<T> = core::result::Result<T, DerError>;

pub const TAG_INTEGER: u8 = 0x02;
pub const TAG_BIT_STRING: u8 = 0x03;
pub const TAG_OCTET_STRING: u8 = 0x04;
pub const TAG_NULL: u8 = 0x05;
pub const TAG_OID: u8 = 0x06;
pub const TAG_UTF8_STRING: u8 = 0x0c;
pub const TAG_SEQUENCE: u8 = 0x30;
pub const TAG_SET: u8 = 0x31;
pub const TAG_PRINTABLE_STRING: u8 = 0x13;
pub const TAG_UTC_TIME: u8 = 0x17;
pub const TAG_GENERALIZED_TIME: u8 = 0x18;
pub const TAG_BOOLEAN: u8 = 0x01;

/// One decoded TLV.
#[derive(Debug, Clone, Copy)]
pub struct Tlv<'a> {
    pub tag: u8,
    pub value: &'a [u8],
    /// The complete encoding (tag + length + value), needed for signatures.
    pub full: &'a [u8],
}

/// Cursor over a DER buffer.
pub struct Der<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Der<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Der { data, pos: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn peek_tag(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    /// Read the next TLV.
    pub fn next_tlv(&mut self) -> Result<Tlv<'a>> {
        let start = self.pos;
        let tag = *self.data.get(self.pos).ok_or(DerError::Truncated)?;
        self.pos += 1;
        let first_len = *self.data.get(self.pos).ok_or(DerError::Truncated)?;
        self.pos += 1;
        let len = if first_len & 0x80 == 0 {
            first_len as usize
        } else {
            let n = (first_len & 0x7f) as usize;
            if n == 0 || n > 4 {
                return Err(DerError::Malformed);
            }
            let mut v = 0usize;
            for _ in 0..n {
                let b = *self.data.get(self.pos).ok_or(DerError::Truncated)?;
                self.pos += 1;
                v = (v << 8) | b as usize;
            }
            v
        };
        let end = self.pos.checked_add(len).ok_or(DerError::Malformed)?;
        if end > self.data.len() {
            return Err(DerError::Truncated);
        }
        let value = &self.data[self.pos..end];
        self.pos = end;
        Ok(Tlv {
            tag,
            value,
            full: &self.data[start..end],
        })
    }

    /// Read the next TLV and require `tag`.
    pub fn expect(&mut self, tag: u8) -> Result<Tlv<'a>> {
        let tlv = self.next_tlv()?;
        if tlv.tag != tag {
            return Err(DerError::UnexpectedTag {
                expected: tag,
                found: tlv.tag,
            });
        }
        Ok(tlv)
    }

    /// Read a SEQUENCE and return a cursor over its contents.
    pub fn expect_sequence(&mut self) -> Result<Der<'a>> {
        Ok(Der::new(self.expect(TAG_SEQUENCE)?.value))
    }
}

/// Read an OBJECT IDENTIFIER as dotted-decimal-free raw bytes (first arc +
/// base-128 groups are kept encoded; callers compare the DER bytes).
pub fn oid_bytes<'a>(tlv: &Tlv<'a>) -> Result<&'a [u8]> {
    if tlv.tag != TAG_OID {
        return Err(DerError::UnexpectedTag {
            expected: TAG_OID,
            found: tlv.tag,
        });
    }
    Ok(tlv.value)
}

/// Parse a DER INTEGER into big-endian bytes without the sign byte.
pub fn integer_bytes<'a>(tlv: &Tlv<'a>) -> Result<&'a [u8]> {
    if tlv.tag != TAG_INTEGER {
        return Err(DerError::UnexpectedTag {
            expected: TAG_INTEGER,
            found: tlv.tag,
        });
    }
    let mut v = tlv.value;
    while v.len() > 1 && v[0] == 0 {
        v = &v[1..];
    }
    Ok(v)
}

/// Parse a DER BIT STRING, returning the payload bytes (unused bits must be 0).
pub fn bit_string_bytes<'a>(tlv: &Tlv<'a>) -> Result<&'a [u8]> {
    if tlv.tag != TAG_BIT_STRING {
        return Err(DerError::UnexpectedTag {
            expected: TAG_BIT_STRING,
            found: tlv.tag,
        });
    }
    // `value[0]` is the number of unused bits in the last byte (0..=7); it is
    // nonzero for bit strings such as keyUsage.
    if tlv.value.is_empty() || tlv.value[0] > 7 {
        return Err(DerError::Malformed);
    }
    Ok(&tlv.value[1..])
}

/// Parse a DER BOOLEAN (DER requires exactly 0x00 or 0xFF).
pub fn boolean(tlv: &Tlv<'_>) -> Result<bool> {
    if tlv.tag != TAG_BOOLEAN || tlv.value.len() != 1 {
        return Err(DerError::Malformed);
    }
    Ok(tlv.value[0] != 0)
}

/// Days from 1970-01-01 to `y-m-d` (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn digits(s: &[u8]) -> Result<i64> {
    if s.is_empty() || !s.iter().all(|b| b.is_ascii_digit()) {
        return Err(DerError::Malformed);
    }
    let mut v = 0i64;
    for b in s {
        v = v * 10 + (b - b'0') as i64;
    }
    Ok(v)
}

/// Parse a UTCTime (`YYMMDDHHMMSSZ`) or GeneralizedTime (`YYYYMMDDHHMMSSZ`) into
/// Unix seconds. Only the `Z` (UTC) form is accepted.
pub fn time_to_unix(tag: u8, s: &[u8]) -> Result<i64> {
    let (year, rest) = match tag {
        TAG_UTC_TIME => {
            if s.len() < 13 || *s.last().unwrap() != b'Z' {
                return Err(DerError::UnsupportedTime);
            }
            let yy = digits(&s[0..2])?;
            // RFC 5280: YY >= 50 means 19YY, otherwise 20YY.
            let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
            (year, &s[2..s.len() - 1])
        }
        TAG_GENERALIZED_TIME => {
            if s.len() < 15 || *s.last().unwrap() != b'Z' {
                return Err(DerError::UnsupportedTime);
            }
            let year = digits(&s[0..4])?;
            (year, &s[4..s.len() - 1])
        }
        _ => return Err(DerError::UnsupportedTime),
    };
    if rest.len() < 10 {
        return Err(DerError::Malformed);
    }
    let month = digits(&rest[0..2])? as u32;
    let day = digits(&rest[2..4])? as u32;
    let hour = digits(&rest[4..6])?;
    let minute = digits(&rest[6..8])?;
    let second = digits(&rest[8..10])?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return Err(DerError::Malformed);
    }
    Ok(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_parses_tlv_and_integers() {
        // SEQUENCE { INTEGER 5, INTEGER 0x0102 }
        let data = [
            0x30, 0x07, 0x02, 0x01, 0x05, 0x02, 0x02, 0x01, 0x02,
        ];
        let mut der = Der::new(&data);
        let mut seq = der.expect_sequence().unwrap();
        assert_eq!(integer_bytes(&seq.next_tlv().unwrap()).unwrap(), &[5]);
        assert_eq!(integer_bytes(&seq.next_tlv().unwrap()).unwrap(), &[1, 2]);
        assert!(seq.is_empty());
    }

    #[test]
    fn der_rejects_truncation_and_bad_tags() {
        assert!(matches!(Der::new(&[0x30, 0x05, 0x02]).next_tlv(), Err(DerError::Truncated)));
        let mut der = Der::new(&[0x02, 0x01, 0x00]);
        assert!(matches!(der.expect(TAG_SEQUENCE), Err(DerError::UnexpectedTag { .. })));
    }

    #[test]
    fn der_times_convert_to_unix() {
        // 2026-11-16T12:15:30Z (UTCTime and GeneralizedTime forms).
        assert_eq!(time_to_unix(TAG_UTC_TIME, b"261116121530Z").unwrap(), 1_794_831_330);
        assert_eq!(
            time_to_unix(TAG_GENERALIZED_TIME, b"20261116121530Z").unwrap(),
            1_794_831_330
        );
        // 1970-01-01T00:00:00Z
        assert_eq!(time_to_unix(TAG_UTC_TIME, b"700101000000Z").unwrap(), 0);
        assert!(time_to_unix(TAG_UTC_TIME, b"261116121530").is_err());
    }
}
