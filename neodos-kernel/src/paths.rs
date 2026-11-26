#![allow(dead_code)] // layout reference: not every path has a consumer yet

//! On-disk layout paths (#398).
//!
//! The image migrated from the ad-hoc `\System` + `\Programs` split to the
//! NT-style tree described in `docs/architecture/disk-layout.md`:
//!
//! ```text
//! \NeoDOS\System\            .NXE de sistema (binarios)
//! \NeoDOS\System\config\     hives del registro
//! \NeoDOS\System\drivers\    .NEM
//! \NeoDOS\System\Keyboard\   layouts .kbd
//! \NeoDOS\System\Globalization\<lang>\ *.nlt
//! \NeoDOS\Libraries\         .NXL
//! \Program Files\            aplicaciones
//! \ProgramData\              datos machine-wide
//! \Users\<usuario>\          perfiles (#400)
//! ```
//!
//! Binaries stay split between `\NeoDOS\System` and `\NeoDOS\System\Tools`
//! because a NE2 directory leaf holds ~28 entries and multi-block directories
//! are not implemented yet; `System` keeps the shell/init core reachable and
//! `Tools` the rest.
//!
//! ## Transitional mapping
//!
//! Consumers still spell several paths the old way (`C:\System\Libraries\…`,
//! `C:\Programs\…`, `C:\System\Registry\SYSTEM.hiv`). [`canonical_candidates`]
//! returns the new spelling(s) to try **before** the literal one, so an image
//! built with the new tree keeps working while the call sites are migrated one
//! by one. The mapping is pure and unit-tested.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// System root on the system volume.
pub const SYSTEM_ROOT: &str = "C:\\NeoDOS\\System";
/// Registry hives.
pub const CONFIG_DIR: &str = "C:\\NeoDOS\\System\\config";
/// Driver metadata (`.INF`).
pub const INF_DIR: &str = "C:\\NeoDOS\\System\\INF";
/// Kernel/driver logs and crash dumps.
pub const LOGS_DIR: &str = "C:\\NeoDOS\\System\\Logs";
/// Scratch space.
pub const TEMP_DIR: &str = "C:\\NeoDOS\\System\\Temp";
/// Boot-time `.NEM` drivers.
pub const DRIVERS_DIR: &str = "C:\\NeoDOS\\System\\drivers";
/// Keyboard layouts (`.kbd`).
pub const KEYBOARD_DIR: &str = "C:\\NeoDOS\\System\\Keyboard";
/// Localization data (`*.nlt`).
pub const GLOBALIZATION_DIR: &str = "C:\\NeoDOS\\System\\Globalization";
/// Fonts.
pub const FONTS_DIR: &str = "C:\\NeoDOS\\System\\Fonts";
/// Shared libraries (`.NXL`).
pub const LIBRARIES_DIR: &str = "C:\\NeoDOS\\Libraries";
/// Installed applications.
pub const PROGRAM_FILES: &str = "C:\\Program Files";
/// Machine-wide application data.
pub const PROGRAM_DATA: &str = "C:\\ProgramData";
/// Trust anchors for libtls (concatenated DER certificates, D8). Packed by
/// NeoDev from the host CA bundle; see `libneodos::paths::TRUST_BUNDLE`.
pub const TRUST_BUNDLE: &str = "C:\\NeoDOS\\System\\config\\roots.der";
/// User profiles (#400).
pub const USERS_ROOT: &str = "C:\\Users";

/// Legacy prefix → new prefix, longest first so `System\Libraries` is not
/// shadowed by `System`.
const LEGACY_MAP: [(&str, &str); 7] = [
    ("C:\\System\\Libraries\\", LIBRARIES_DIR),
    ("C:\\System\\Tools\\", "C:\\NeoDOS\\System\\Tools\\"),
    ("C:\\System\\Registry\\", CONFIG_DIR),
    ("C:\\System\\Driver\\", DRIVERS_DIR),
    ("C:\\System\\Drivers\\", DRIVERS_DIR),
    ("C:\\System\\Keyboard\\", KEYBOARD_DIR),
    ("C:\\System\\Locale\\", GLOBALIZATION_DIR),
];

/// Prefixes that map to the system root as a whole.
const LEGACY_ROOTS: [(&str, &str); 2] = [
    ("C:\\System\\", SYSTEM_ROOT),
    ("C:\\Programs\\", SYSTEM_ROOT),
];

/// Map a legacy path to its canonical spelling, or `None` when it is already
/// canonical (or has no mapping).
pub fn canonicalize(path: &str) -> Option<String> {
    match_legacy(path, &LEGACY_MAP).or_else(|| match_legacy(path, &LEGACY_ROOTS))
}

/// Match `path` against a prefix table. The table entries end in `\\`; a path
/// that names the directory itself (`C:\\System\\Drivers`, no trailing
/// separator) maps to the directory, not to `…\\Drivers\\`.
fn match_legacy(path: &str, table: &[(&str, &str)]) -> Option<String> {
    for (old, new) in table {
        let dir = &old[..old.len() - 1];
        // Table entries keep their trailing separator; path constants do not.
        // Joining blindly produced `C:\NeoDOS\System\Tools\\x.nxe`.
        let base = new.trim_end_matches('\\');
        if let Some(rest) = strip_prefix_ci(path, old) {
            let mut out = String::from(base);
            out.push('\\');
            out.push_str(rest);
            return Some(out);
        }
        if path.eq_ignore_ascii_case(dir) {
            return Some(String::from(base));
        }
    }
    None
}

/// Paths to try, in order, for `path`: the canonical spelling first (when one
/// exists) and then the original.
/// Parent directory of a DOS path, accepting both `\` and `/`.
///
/// The VFS treats both separators as equivalent, so authorization must too
/// (AUDIT-008 / #816). A path without a separator after the drive colon is
/// relative to the volume root (`C:\`). Returns `None` when there is no drive.
pub fn parent_dir(path: &str) -> Option<String> {
    let colon = path.find(':')?;
    // DOS drive letter only (`C:`), never an Ob namespace path.
    if colon != 1 || colon + 1 >= path.len() {
        return None;
    }
    let mut last_sep = None;
    for (i, b) in path.bytes().enumerate() {
        if i > colon && (b == b'\\' || b == b'/') {
            last_sep = Some(i);
        }
    }
    match last_sep {
        Some(i) if i > colon + 1 => Some(String::from(&path[..i])),
        Some(i) => Some(String::from(&path[..=i])),
        None => Some(format!("{}\\", &path[..=colon])),
    }
}

pub fn canonical_candidates(path: &str) -> Vec<String> {
    match canonicalize(path) {
        Some(canon) => {
            let mut v = Vec::with_capacity(2);
            v.push(canon);
            v.push(String::from(path));
            v
        }
        None => {
            let mut v = Vec::with_capacity(1);
            v.push(String::from(path));
            v
        }
    }
}

/// Case-insensitive ASCII prefix strip that keeps the remainder verbatim.
fn strip_prefix_ci<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let bytes = path.as_bytes();
    if bytes.len() < prefix.len() {
        return None;
    }
    if !bytes[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()) {
        return None;
    }
    Some(&path[prefix.len()..])
}

pub fn register_tests() {
    use crate::{test_case, test_eq};

    test_case!("paths_legacy_mapping", {
        test_eq!(
            canonicalize("C:\\Programs\\neoshell.nxe").as_deref(),
            Some("C:\\NeoDOS\\System\\neoshell.nxe")
        );
        test_eq!(
            canonicalize("C:\\System\\Libraries\\net.nxl").as_deref(),
            Some("C:\\NeoDOS\\Libraries\\net.nxl")
        );
        test_eq!(
            canonicalize("C:\\System\\Registry\\SYSTEM.hiv").as_deref(),
            Some("C:\\NeoDOS\\System\\config\\SYSTEM.hiv")
        );
        test_eq!(
            canonicalize("C:\\System\\Drivers\\ahci.nem").as_deref(),
            Some("C:\\NeoDOS\\System\\drivers\\ahci.nem")
        );
        test_eq!(
            canonicalize("C:\\System\\Keyboard\\en-US.kbd").as_deref(),
            Some("C:\\NeoDOS\\System\\Keyboard\\en-US.kbd")
        );
        test_eq!(
            canonicalize("C:\\System\\Locale\\es-ES").as_deref(),
            Some("C:\\NeoDOS\\System\\Globalization\\es-ES")
        );
        test_eq!(
            canonicalize("C:\\System\\Tools\\ntpd.nxe").as_deref(),
            Some("C:\\NeoDOS\\System\\Tools\\ntpd.nxe")
        );
        test_eq!(
            canonicalize("c:\\system\\Libraries\\fs.nxl").as_deref(),
            Some("C:\\NeoDOS\\Libraries\\fs.nxl")
        );
        test_eq!(
            canonicalize("C:\\System\\Drivers").as_deref(),
            Some("C:\\NeoDOS\\System\\drivers")
        );
        test_eq!(
            canonicalize("C:\\System\\Registry").as_deref(),
            Some("C:\\NeoDOS\\System\\config")
        );
        test_eq!(
            canonicalize("C:\\Programs").as_deref(),
            Some("C:\\NeoDOS\\System")
        );
        // Already canonical, or unrelated: no mapping.
        test_eq!(canonicalize("C:\\NeoDOS\\System\\neoshell.nxe"), None);
        test_eq!(canonicalize("C:\\Users\\Administrator"), None);
        test_eq!(canonicalize("A:\\EFI\\BOOT"), None);
    });

    test_case!("paths_parent_dir_accepts_both_separators", {
        test_eq!(parent_dir("C:\\Users\\NeoDOS\\x").as_deref(), Some("C:\\Users\\NeoDOS"));
        test_eq!(parent_dir("C:/Users/NeoDOS/x").as_deref(), Some("C:/Users/NeoDOS"));
        test_eq!(parent_dir("C:\\x").as_deref(), Some("C:\\"));
        test_eq!(parent_dir("C:/x").as_deref(), Some("C:/"));
        test_eq!(parent_dir("C:x").as_deref(), Some("C:\\"));
        test_eq!(parent_dir("\\Global\\FileSystem\\C:\\x"), None);
        test_eq!(parent_dir(""), None);
    });

    test_case!("paths_candidates_put_canonical_first", {
        let c = canonical_candidates("C:\\Programs\\neoinit.nxe");
        test_eq!(c.len(), 2);
        test_eq!(c[0].as_str(), "C:\\NeoDOS\\System\\neoinit.nxe");
        test_eq!(c[1].as_str(), "C:\\Programs\\neoinit.nxe");

        let c = canonical_candidates("C:\\Users\\Administrator\\Desktop");
        test_eq!(c.len(), 1);
        test_eq!(c[0].as_str(), "C:\\Users\\Administrator\\Desktop");
    });
}
