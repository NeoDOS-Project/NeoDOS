//! Userland path helpers shared by the filesystem tools.
//!
//! `normalize_path` resolves a user-supplied path to an absolute,
//! NUL-terminated buffer (256 bytes + NUL). Absolute paths (leading `\` or
//! containing `:`) are copied as-is; relative paths are joined to the current
//! working directory, falling back to `C:\`. These were duplicated across six
//! `core*` tools before this module existed.

use crate::syscall;

fn cwd_into(buf: &mut [u8; 260], pos: &mut usize, add_sep: bool) {
    let mut cwd_buf = [0u8; 256];
    match syscall::sys_getcwd(&mut cwd_buf) {
        Ok(n) if n > 0 => {
            for &b in &cwd_buf[..n] {
                if *pos < 259 { buf[*pos] = b; *pos += 1; }
            }
            if add_sep && *pos > 0 && buf[*pos - 1] != b'\\' {
                if *pos < 259 { buf[*pos] = b'\\'; *pos += 1; }
            }
        }
        _ => {
            buf[..3].copy_from_slice(b"C:\\");
            *pos = 3;
        }
    }
}

/// Resolve `input` to an absolute path. Empty input resolves to the cwd.
pub fn normalize_path(input: &[u8]) -> [u8; 260] {
    let path_str = core::str::from_utf8(input).unwrap_or("");
    if path_str.is_empty() {
        let mut buf = [0u8; 260];
        let mut pos = 0;
        cwd_into(&mut buf, &mut pos, false);
        if pos < 259 { buf[pos] = 0; }
        return buf;
    }
    normalize_absolute(path_str.as_bytes())
}

/// Like [`normalize_path`] but empty input yields an empty buffer.
pub fn normalize_path_required(input: &[u8]) -> [u8; 260] {
    if input.is_empty() {
        return [0u8; 260];
    }
    let path_str = core::str::from_utf8(input).unwrap_or("");
    normalize_absolute(path_str.as_bytes())
}

fn normalize_absolute(bytes: &[u8]) -> [u8; 260] {
    let mut buf = [0u8; 260];
    if bytes[0] == b'\\' || bytes.contains(&b':') {
        let n = bytes.len().min(259);
        buf[..n].copy_from_slice(&bytes[..n]);
    } else {
        let mut pos = 0;
        cwd_into(&mut buf, &mut pos, true);
        for &b in bytes {
            if pos < 259 { buf[pos] = b; pos += 1; }
        }
    }
    buf
}
