//! Pure diagnostics/table text helpers shared by the admin/diagnostic tools
//! (`ps`, `neotop`). Host-testable: no syscalls, no allocation.
//!
//! Only the common denominator lives here — the number of spaces needed to pad
//! a field, and left/right alignment into a caller buffer. Column widths and
//! per-table layout stay in each tool.

#![cfg_attr(not(test), no_std)]

/// Spaces required to pad a field of `len` bytes up to `width` (0 if already at
/// least `width`). Saturating by construction.
pub fn pad_width(len: usize, width: usize) -> usize {
    width.saturating_sub(len)
}

/// Right-align `text` in a `width`-wide field into `buf`; returns bytes written.
/// A value wider than `width` is written in full (never truncated).
pub fn pad_right(buf: &mut [u8], text: &[u8], width: usize) -> usize {
    let pad = pad_width(text.len(), width);
    let mut pos = 0usize;
    for _ in 0..pad {
        if pos < buf.len() { buf[pos] = b' '; pos += 1; }
    }
    for &b in text {
        if pos < buf.len() { buf[pos] = b; pos += 1; }
    }
    pos.min(buf.len())
}

/// Left-align `text` in a `width`-wide field into `buf`; returns bytes written.
pub fn pad_left(buf: &mut [u8], text: &[u8], width: usize) -> usize {
    let pad = pad_width(text.len(), width);
    let mut pos = 0usize;
    for &b in text {
        if pos < buf.len() { buf[pos] = b; pos += 1; }
    }
    for _ in 0..pad {
        if pos < buf.len() { buf[pos] = b' '; pos += 1; }
    }
    pos.min(buf.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_width_saturates() {
        assert_eq!(pad_width(0, 4), 4);
        assert_eq!(pad_width(3, 4), 1);
        assert_eq!(pad_width(4, 4), 0);
        assert_eq!(pad_width(9, 4), 0);
        assert_eq!(pad_width(usize::MAX, 3), 0);
    }

    #[test]
    fn pad_right_aligns() {
        let mut b = [0u8; 8];
        let n = pad_right(&mut b, b"12", 4);
        assert_eq!(&b[..n], b"  12");
    }

    #[test]
    fn pad_left_aligns() {
        let mut b = [0u8; 8];
        let n = pad_left(&mut b, b"ab", 4);
        assert_eq!(&b[..n], b"ab  ");
    }

    #[test]
    fn wider_value_not_truncated() {
        let mut b = [0u8; 8];
        let n = pad_right(&mut b, b"12345", 3);
        assert_eq!(&b[..n], b"12345");
    }
}
