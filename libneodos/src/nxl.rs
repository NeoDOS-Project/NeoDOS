//! Userland NXL path resolution.
//!
//! Like Windows `KnownDlls`, the path of an NXL can be configured in the
//! Registry so a library can be relocated or versioned without rebuilding its
//! clients:
//!
//! ```text
//! \Registry\Machine\System\CurrentControlSet\Control\Library\<name>  (REG_SZ)
//! ```
//!
//! When the value is absent (or the Registry is unavailable) the default path
//! `C:\System\Libraries\<name>.nxl` is used.

use crate::syscall;

const LIBRARY_KEY: &str =
    "\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Library";
const FALLBACK_DIR: &str = "C:\\System\\Libraries\\";

/// Resolve NXL `name` (e.g. `"math"`) into `buf`, returning the byte length
/// (no NUL terminator). The Registry value wins; otherwise the default path.
pub fn library_path(name: &str, buf: &mut [u8]) -> Option<usize> {
    if let Some(n) = registry_path(name, buf) {
        return Some(n);
    }
    fallback_path(name, buf)
}

fn registry_path(name: &str, buf: &mut [u8]) -> Option<usize> {
    let fd = syscall::sys_cm_open_key(LIBRARY_KEY).ok()?;
    let mut raw = [0u8; 268];
    let total = match syscall::sys_cm_query_value(fd, name, &mut raw) {
        Ok(n) => n,
        Err(_) => {
            let _ = syscall::sys_close(fd);
            return None;
        }
    };
    let _ = syscall::sys_close(fd);
    if total < 8 {
        return None;
    }
    // Value layout: [type u32][len u32][data...]
    let len = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
    let avail = total.saturating_sub(8).min(raw.len() - 8);
    let src = &raw[8..8 + len.min(avail)];
    let end = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    if end == 0 {
        return None;
    }
    let n = end.min(buf.len());
    buf[..n].copy_from_slice(&src[..n]);
    Some(n)
}

fn fallback_path(name: &str, buf: &mut [u8]) -> Option<usize> {
    let mut pos = 0usize;
    let mut push = |bytes: &[u8]| {
        for &b in bytes {
            if pos < buf.len() {
                buf[pos] = b;
                pos += 1;
            }
        }
    };
    push(FALLBACK_DIR.as_bytes());
    push(name.as_bytes());
    push(b".nxl");
    if pos == 0 { None } else { Some(pos) }
}
