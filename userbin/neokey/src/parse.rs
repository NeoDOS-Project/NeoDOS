//! Argument parsing helpers for NEOKEY.

/// Parse a non-empty decimal string into a `u32`.
pub fn u32(s: &str) -> Option<u32> {
    if s.is_empty() { return None; }
    let mut v: u32 = 0;
    for &b in s.as_bytes() {
        if !b.is_ascii_digit() { return None; }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(v)
}

/// Parse a comma-separated LED list (`num`, `caps`, `scroll`, `none`).
pub fn led_mask(s: &str) -> Option<u8> {
    let mut bits = 0u8;
    let mut any = false;
    for tok in s.split(',') {
        if tok.is_empty() { continue; }
        any = true;
        if tok.eq_ignore_ascii_case("num") || tok.eq_ignore_ascii_case("numlock") {
            bits |= 1;
        } else if tok.eq_ignore_ascii_case("caps") || tok.eq_ignore_ascii_case("capslock") {
            bits |= 2;
        } else if tok.eq_ignore_ascii_case("scroll") || tok.eq_ignore_ascii_case("scrolllock") {
            bits |= 4;
        } else if tok.eq_ignore_ascii_case("none") || tok.eq_ignore_ascii_case("off") {
            // explicitly no LED
        } else {
            return None;
        }
    }
    if any { Some(bits) } else { None }
}
