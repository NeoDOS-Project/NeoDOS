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
    let key = crate::registry::RegistryKey::open(LIBRARY_KEY).ok()?;
    let n = key.query_string(name, buf);
    if n > 0 { Some(n) } else { None }
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
