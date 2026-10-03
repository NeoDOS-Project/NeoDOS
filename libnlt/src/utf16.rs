//! UTF-16LE ↔ UTF-8 helpers (`I18N-P8`).
//!
//! NLTv3 can store the string blob as UTF-16LE. The runtime transcodes to
//! UTF-8 on load and rewrites the index offsets so all later lookups are
//! ordinary UTF-8. Source `.toml` files may also carry a BOM; the compiler
//! detects it and decodes accordingly.

use crate::{read_index, write_index_v3, ENTRY_FLAG_PLURAL, VERSION_V3};

/// UTF-8 byte-order mark.
pub const BOM_UTF8: [u8; 3] = [0xEF, 0xBB, 0xBF];
/// UTF-16 little-endian byte-order mark.
pub const BOM_UTF16LE: [u8; 2] = [0xFF, 0xFE];

/// Detect a leading BOM. Returns `Some("utf-8")`, `Some("utf-16le")` or `None`.
pub fn detect_bom(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&BOM_UTF8) {
        Some("utf-8")
    } else if data.starts_with(&BOM_UTF16LE) {
        Some("utf-16le")
    } else {
        None
    }
}

/// Decode a little-endian UTF-16 code unit at `units[i]`, advancing `i`.
fn next_unit(src: &[u8], i: &mut usize) -> Option<u16> {
    if *i + 2 > src.len() {
        return None;
    }
    let u = u16::from_le_bytes([src[*i], src[*i + 1]]);
    *i += 2;
    Some(u)
}

/// Transcode a single NUL-terminated UTF-16LE string starting at `src[start..]`
/// into `dst`, returning the number of UTF-8 bytes written.
pub fn transcode_one(src: &[u8], start: usize, dst: &mut [u8]) -> Option<usize> {
    let mut i = start;
    let mut out = 0usize;
    loop {
        let u = next_unit(src, &mut i)?;
        if u == 0 {
            return Some(out);
        }
        let mut buf = [0u8; 4];
        let n = if (0xD800..0xDC00).contains(&u) {
            // high surrogate — needs a low surrogate
            let lo = next_unit(src, &mut i)?;
            if !(0xDC00..0xE000).contains(&lo) {
                return None;
            }
            let cp = 0x1_0000
                + (((u as u32) - 0xD800) << 10)
                + ((lo as u32) - 0xDC00);
            put_utf8(cp, &mut buf)
        } else if (0xDC00..0xE000).contains(&u) {
            return None; // lone low surrogate
        } else {
            put_utf8(u as u32, &mut buf)
        };
        if out + n > dst.len() {
            return None;
        }
        dst[out..out + n].copy_from_slice(&buf[..n]);
        out += n;
    }
}

fn put_utf8(cp: u32, buf: &mut [u8; 4]) -> usize {
    if cp < 0x80 {
        buf[0] = cp as u8;
        1
    } else if cp < 0x800 {
        buf[0] = (0xC0 | (cp >> 6)) as u8;
        buf[1] = (0x80 | (cp & 0x3F)) as u8;
        2
    } else if cp < 0x1_0000 {
        buf[0] = (0xE0 | (cp >> 12)) as u8;
        buf[1] = (0x80 | ((cp >> 6) & 0x3F)) as u8;
        buf[2] = (0x80 | (cp & 0x3F)) as u8;
        3
    } else {
        buf[0] = (0xF0 | (cp >> 18)) as u8;
        buf[1] = (0x80 | ((cp >> 12) & 0x3F)) as u8;
        buf[2] = (0x80 | ((cp >> 6) & 0x3F)) as u8;
        buf[3] = (0x80 | (cp & 0x3F)) as u8;
        4
    }
}

/// Decode a whole UTF-16LE buffer (already BOM-stripped) into UTF-8.
pub fn utf16le_bytes_to_utf8(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    let mut i = 0usize;
    let mut out = 0usize;
    while i + 2 <= src.len() {
        let u = next_unit(src, &mut i)?;
        let mut buf = [0u8; 4];
        let n = if (0xD800..0xDC00).contains(&u) {
            let lo = next_unit(src, &mut i)?;
            if !(0xDC00..0xE000).contains(&lo) {
                return None;
            }
            let cp = 0x1_0000 + (((u as u32) - 0xD800) << 10) + ((lo as u32) - 0xDC00);
            put_utf8(cp, &mut buf)
        } else if (0xDC00..0xE000).contains(&u) {
            return None;
        } else {
            put_utf8(u as u32, &mut buf)
        };
        if out + n > dst.len() {
            return None;
        }
        dst[out..out + n].copy_from_slice(&buf[..n]);
        out += n;
    }
    Some(out)
}

/// Rewrite a decoded UTF-16LE payload into a UTF-8 payload with adjusted
/// index offsets.
///
/// The input payload is `[index][UTF-16LE blob]`; the output is
/// `[index][UTF-8 blob]`. Plural groups are transcoded form-by-form.
/// Returns the number of bytes written to `out`.
pub fn rewrite_index_utf16(
    payload: &[u8],
    version: u16,
    entry_count: u32,
    out: &mut [u8],
) -> Option<usize> {
    if version != VERSION_V3 {
        return None;
    }
    let index_size = entry_count as usize * crate::ENTRY_V3_SIZE;
    if out.len() < index_size {
        return None;
    }
    let mut cursor = index_size;

    for i in 0..entry_count as usize {
        let (id, old_off, flags) = read_index(payload, version, i)?;
        if flags & ENTRY_FLAG_PLURAL != 0 {
            let base = old_off as usize;
            let count = *payload.get(base)? as usize;
            if count == 0 || count > 8 {
                return None;
            }
            let entry_base = cursor;
            cursor += 1 + count;
            if out.len() < cursor {
                return None;
            }
            for c in 0..count {
                let rel = *payload.get(base + 1 + c)? as usize;
                let start = base + rel;
                let n = transcode_one(payload, start, &mut out[cursor..])?;
                let new_rel = cursor - entry_base;
                if new_rel > 255 {
                    return None;
                }
                out[entry_base + 1 + c] = new_rel as u8;
                cursor += n;
            }
            out[entry_base] = count as u8;
            write_index_v3(out, i, id, entry_base as u32, flags);
        } else {
            let start = old_off as usize;
            let n = transcode_one(payload, start, &mut out[cursor..])?;
            write_index_v3(out, i, id, cursor as u32, flags);
            cursor += n;
        }
    }
    Some(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_detection() {
        assert_eq!(detect_bom(&BOM_UTF8), Some("utf-8"));
        assert_eq!(detect_bom(&BOM_UTF16LE), Some("utf-16le"));
        assert_eq!(detect_bom(b"plain"), None);
    }

    #[test]
    fn transcode_ascii() {
        let src = [b'h', 0, b'i', 0, 0, 0];
        let mut dst = [0u8; 8];
        assert_eq!(transcode_one(&src, 0, &mut dst), Some(2));
        assert_eq!(&dst[..2], b"hi");
    }

    #[test]
    fn transcode_emoji_surrogate_pair() {
        // U+1F600 = D83D DE00
        let src = [0x3D, 0xD8, 0x00, 0xDE, 0x00, 0x00];
        let mut dst = [0u8; 8];
        let n = transcode_one(&src, 0, &mut dst).unwrap();
        assert_eq!(&dst[..n], "\u{1F600}".as_bytes());
    }

    #[test]
    fn whole_buffer() {
        let src = [b'a', 0, b'b', 0];
        let mut dst = [0u8; 8];
        let n = utf16le_bytes_to_utf8(&src, &mut dst).unwrap();
        assert_eq!(&dst[..n], b"ab");
    }
}
