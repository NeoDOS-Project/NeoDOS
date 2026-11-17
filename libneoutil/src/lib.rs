//! Pure userland text/path utilities shared by the filesystem tools.
//!
//! Host-testable: no syscalls, no allocation. The functions here were copied
//! byte-for-byte across a dozen `core*` tools before this crate existed.

#![cfg_attr(not(test), no_std)]

/// Prefix of every path in the Object Manager filesystem namespace.
pub const OB_FS_PREFIX: &[u8] = b"\\Global\\FileSystem\\";

/// Prepend the Ob filesystem prefix to `vfs` into `buf` (NUL-terminated) and
/// return the resulting `&str`. If it does not fit, `vfs` is returned unchanged.
pub fn to_ob_path<'a>(vfs: &'a str, buf: &'a mut [u8; 512]) -> &'a str {
    let total = OB_FS_PREFIX.len() + vfs.len();
    if total > 510 {
        return vfs;
    }
    buf[..OB_FS_PREFIX.len()].copy_from_slice(OB_FS_PREFIX);
    buf[OB_FS_PREFIX.len()..total].copy_from_slice(vfs.as_bytes());
    buf[total] = 0;
    unsafe { core::str::from_utf8_unchecked(&buf[..total]) }
}

/// Like [`to_ob_path`] but takes raw bytes; `None` if it does not fit.
pub fn to_ob_path_bytes<'a>(vfs: &[u8], buf: &'a mut [u8; 512]) -> Option<&'a str> {
    let total = OB_FS_PREFIX.len() + vfs.len();
    if total > 510 {
        return None;
    }
    buf[..OB_FS_PREFIX.len()].copy_from_slice(OB_FS_PREFIX);
    buf[OB_FS_PREFIX.len()..total].copy_from_slice(vfs);
    buf[total] = 0;
    Some(unsafe { core::str::from_utf8_unchecked(&buf[..total]) })
}

/// Canonical name of a negative kernel errno (`EINVAL`..`EBUSY`, else `UNKNOWN`).
pub fn errno_str(code: i64) -> &'static str {
    match code {
        -1 => "EINVAL",
        -2 => "ENOENT",
        -3 => "ENOMEM",
        -4 => "EACCES",
        -5 => "EBADF",
        -6 => "EFAULT",
        -7 => "ENOSYS",
        -8 => "EAGAIN",
        -9 => "EPIPE",
        -10 => "EEXIST",
        -11 => "ENOTDIR",
        -12 => "EISDIR",
        -13 => "EIO",
        -14 => "ENODEV",
        -15 => "EBUSY",
        _ => "UNKNOWN",
    }
}

/// NUL-terminated bytes → `&str` (empty on invalid UTF-8).
pub fn nul_str(buf: &[u8]) -> &str {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    core::str::from_utf8(&buf[..end]).unwrap_or("")
}

/// Parse an ASCII decimal `u32` from `bytes` (all digits required).
pub fn parse_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut n: u32 = 0;
    for &b in bytes {
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(n)
}

/// Split `s` at the first ASCII space; returns `(token, rest)` where `rest`
/// has the leading space skipped. With no space, `rest` is empty.
pub fn split_first_token(s: &[u8]) -> (&[u8], &[u8]) {
    match s.iter().position(|&b| b == b' ') {
        Some(i) => {
            let mut rest = &s[i..];
            while !rest.is_empty() && rest[0] == b' ' {
                rest = &rest[1..];
            }
            (&s[..i], rest)
        }
        None => (s, &[]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_ob_path_prefixes() {
        let mut buf = [0u8; 512];
        let p = to_ob_path("C:\\foo", &mut buf);
        assert_eq!(p, "\\Global\\FileSystem\\C:\\foo");
    }

    #[test]
    fn nul_str_trims_at_nul() {
        assert_eq!(nul_str(b"abc\0xyz"), "abc");
        assert_eq!(nul_str(b"abc"), "abc");
    }

    #[test]
    fn parse_u32_digits() {
        assert_eq!(parse_u32(b"0"), Some(0));
        assert_eq!(parse_u32(b"1234"), Some(1234));
        assert_eq!(parse_u32(b"12a"), None);
        assert_eq!(parse_u32(b""), None);
        assert_eq!(parse_u32(b"99999999999"), None); // overflow
    }

    #[test]
    fn errno_str_names() {
        assert_eq!(errno_str(-1), "EINVAL");
        assert_eq!(errno_str(-2), "ENOENT");
        assert_eq!(errno_str(-15), "EBUSY");
        assert_eq!(errno_str(0), "UNKNOWN");
    }

    #[test]
    fn to_ob_path_bytes_variant() {
        let mut buf = [0u8; 512];
        assert_eq!(to_ob_path_bytes(b"C:\\x", &mut buf), Some("\\Global\\FileSystem\\C:\\x"));
    }

    #[test]
    fn split_first_token_basic() {
        assert_eq!(split_first_token(b"cmd arg1 arg2"), (&b"cmd"[..], &b"arg1 arg2"[..]));
        assert_eq!(split_first_token(b"solo"), (&b"solo"[..], &b""[..]));
        assert_eq!(split_first_token(b"a   b"), (&b"a"[..], &b"b"[..]));
    }
}
