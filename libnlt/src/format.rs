//! Placeholder formatting for translated strings (`I18N-P4`).
//!
//! Templates use positional placeholders:
//!
//! ```text
//! {0}       string argument 0
//! {0:d}     argument 0 parsed as a signed decimal integer
//! {0:u}     argument 0 parsed as an unsigned decimal integer
//! {0:x}     argument 0 parsed as hexadecimal (lower case)
//! {0:X}     argument 0 parsed as hexadecimal (upper case)
//! {{        literal '{'
//! }}        literal '}'
//! ```
//!
//! Formatting is allocation-free: output is written into a caller-provided
//! slice and the number of bytes written is returned.

/// Format `template` with `args` into `out`.
///
/// Returns the number of bytes written. Unknown numbers or bad specifiers are
/// emitted verbatim so a missing translation never panics.
pub fn format_template(template: &str, args: &[&str], out: &mut [u8]) -> usize {
    let t = template.as_bytes();
    let mut pos = 0usize;
    let mut i = 0usize;

    while i < t.len() {
        match t[i] {
            b'{' => {
                // Escaped '{{'
                if i + 1 < t.len() && t[i + 1] == b'{' {
                    pos = put(out, pos, &[b'{']);
                    i += 2;
                    continue;
                }
                // Find closing brace
                if let Some(rel_end) = t[i + 1..].iter().position(|&b| b == b'}') {
                    let inner = &t[i + 1..i + 1 + rel_end];
                    if let Some((index, spec)) = parse_placeholder(inner) {
                        if let Some(arg) = args.get(index) {
                            pos = emit(out, pos, arg.as_bytes(), spec, &mut i, rel_end);
                            continue;
                        }
                    }
                    // Malformed / out of range: copy verbatim
                    pos = put(out, pos, &t[i..i + rel_end + 2]);
                    i += rel_end + 2;
                    continue;
                }
                pos = put(out, pos, &[b'{']);
                i += 1;
            }
            b'}' => {
                if i + 1 < t.len() && t[i + 1] == b'}' {
                    i += 2;
                } else {
                    i += 1;
                }
                pos = put(out, pos, &[b'}']);
            }
            b => {
                pos = put(out, pos, &[b]);
                i += 1;
            }
        }
    }
    pos
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Spec {
    Str,
    Signed,
    Unsigned,
    HexLower,
    HexUpper,
}

fn parse_placeholder(inner: &[u8]) -> Option<(usize, Spec)> {
    let s = core::str::from_utf8(inner).ok()?;
    let (num, spec) = match s.split_once(':') {
        Some((n, sp)) => (n, sp),
        None => (s, ""),
    };
    let index = parse_usize(num)?;
    let spec = match spec {
        "" => Spec::Str,
        "d" => Spec::Signed,
        "u" => Spec::Unsigned,
        "x" => Spec::HexLower,
        "X" => Spec::HexUpper,
        _ => return None,
    };
    Some((index, spec))
}

fn parse_usize(s: &str) -> Option<usize> {
    if s.is_empty() {
        return None;
    }
    let mut v: usize = 0;
    for &b in s.as_bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as usize)?;
    }
    Some(v)
}

/// Emit the formatted argument. `i` is advanced past the placeholder.
fn emit(
    out: &mut [u8],
    pos: usize,
    arg: &[u8],
    spec: Spec,
    i: &mut usize,
    rel_end: usize,
) -> usize {
    *i += rel_end + 2;
    match spec {
        Spec::Str => put(out, pos, arg),
        Spec::Signed => {
            let v = parse_i64(arg).unwrap_or(0);
            put_num(out, pos, v, false, false)
        }
        Spec::Unsigned => {
            let v = parse_u64(arg).unwrap_or(0);
            put_num(out, pos, v as i64, false, false)
        }
        Spec::HexLower => {
            let v = parse_u64(arg).unwrap_or(0);
            put_num(out, pos, v as i64, true, false)
        }
        Spec::HexUpper => {
            let v = parse_u64(arg).unwrap_or(0);
            put_num(out, pos, v as i64, true, true)
        }
    }
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

/// Write an integer into `out`, honouring an optional sign for signed values.
fn put_num(out: &mut [u8], pos: usize, v: i64, hex: bool, upper: bool) -> usize {
    let mut buf = [0u8; 24];
    let neg = v < 0;
    // Use u64 magnitude so i64::MIN is handled.
    let mut mag = if neg { (v as i128).unsigned_abs() as u64 } else { v as u64 };
    let mut idx = buf.len();
    if mag == 0 {
        idx -= 1;
        buf[idx] = b'0';
    } else if hex {
        const LOWER: &[u8; 16] = b"0123456789abcdef";
        const UPPER: &[u8; 16] = b"0123456789ABCDEF";
        let digits = if upper { UPPER } else { LOWER };
        while mag > 0 {
            idx -= 1;
            buf[idx] = digits[(mag & 0xF) as usize];
            mag >>= 4;
        }
    } else {
        while mag > 0 {
            idx -= 1;
            buf[idx] = b'0' + (mag % 10) as u8;
            mag /= 10;
        }
    }
    if neg {
        idx -= 1;
        buf[idx] = b'-';
    }
    put(out, pos, &buf[idx..])
}

fn parse_u64(s: &[u8]) -> Option<u64> {
    let s = core::str::from_utf8(s).ok()?;
    s.trim().parse::<u64>().ok()
}

fn parse_i64(s: &[u8]) -> Option<i64> {
    let s = core::str::from_utf8(s).ok()?;
    s.trim().parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(template: &str, args: &[&str]) -> String {
        let mut buf = [0u8; 256];
        let n = format_template(template, args, &mut buf);
        String::from_utf8(buf[..n].to_vec()).unwrap()
    }

    #[test]
    fn simple_substitution() {
        assert_eq!(fmt("Hello, {0}!", &["World"]), "Hello, World!");
    }

    #[test]
    fn repeated_placeholders() {
        assert_eq!(fmt("{0}-{1}-{0}", &["a", "b"]), "a-b-a");
    }

    #[test]
    fn no_placeholders() {
        assert_eq!(fmt("plain text", &[]), "plain text");
    }

    #[test]
    fn decimal_specifier() {
        assert_eq!(fmt("{0:d} files", &["42"]), "42 files");
        assert_eq!(fmt("{0:d}", &["-7"]), "-7");
    }

    #[test]
    fn hex_specifier() {
        assert_eq!(fmt("{0:x}", &["255"]), "ff");
        assert_eq!(fmt("{0:X}", &["255"]), "FF");
    }

    #[test]
    fn escaped_braces() {
        assert_eq!(fmt("{{0}}", &[]), "{0}");
    }

    #[test]
    fn out_of_range_is_verbatim() {
        assert_eq!(fmt("{3}", &["a"]), "{3}");
    }
}
