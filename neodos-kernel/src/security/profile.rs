//! Per-user profile directories (#400).
//!
//! Each SAM account gets a profile tree on the system volume:
//!
//! ```text
//! C:\Users\<usuario>\Desktop
//!                   \Documents
//!                   \Downloads
//!                   \AppData\Local
//!                   \AppData\Roaming
//! ```
//!
//! The profile is **bound to the account**: its owner SID, username and path
//! are recorded in memory and in the registry under
//! `\Registry\Machine\Software\NeoDOS\Profiles\<usuario>`, so the filesystem
//! ACL support planned in Phase 3 of `docs/design/users-security-design.md`
//! can enforce ownership. This module deliberately does not enforce access
//! yet: `DirEntryV2` has no owner SID, so permissions are advisory until that
//! phase lands.
//!
//! Creation is idempotent and happens at boot for every account except
//! `SYSTEM` (a service account, which has no interactive profile). The NT
//! partitioning half of #400 moved to #499.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use lazy_static::lazy_static;
use spin::Mutex;

use crate::security::sid::Sid;

/// Root of every profile tree, on the system volume.
pub const PROFILE_ROOT: &str = "C:\\Users";
/// Registry key holding one subkey per profile.
pub const REGISTRY_BASE: &str = "Software\\NeoDOS\\Profiles";

/// Directories every profile contains, in creation order (parents first).
pub const STANDARD_DIRS: [&str; 6] = [
    "Desktop",
    "Documents",
    "Downloads",
    "AppData",
    "AppData\\Local",
    "AppData\\Roaming",
];

/// Longest accepted profile name (NT limits the account name to 20).
pub const MAX_PROFILE_NAME: usize = 20;

/// Device names that cannot be used as a directory name on NT.
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Why a profile could not be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileError {
    /// Empty, too long, or containing a character NT forbids in a name.
    InvalidName,
    /// A reserved device name (`CON`, `NUL`, `COM1`, …), case-insensitive.
    ReservedName,
    /// The directory tree could not be created.
    Vfs,
}

/// One account's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// SAM account name (the directory name).
    pub username: String,
    /// Owner SID, as recorded by SAM.
    pub sid: Sid,
    /// Absolute path of the profile root (`C:\Users\<usuario>`).
    pub path: String,
}

lazy_static! {
    /// Every known profile, in creation order. The runtime source of truth;
    /// the registry copy is for persistence and for other components.
    static ref PROFILES: Mutex<Vec<Profile>> = Mutex::new(Vec::new());
}

/// Validate `name` as a profile directory name.
///
/// Rejects the characters NT forbids in a path component (`\ / : * ? " < > |`),
/// control characters, a trailing dot or space, `.`/`..`, and the reserved
/// device names.
pub fn validate_profile_name(name: &str) -> Result<(), ProfileError> {
    if name.is_empty() || name.len() > MAX_PROFILE_NAME {
        return Err(ProfileError::InvalidName);
    }
    if name == "." || name == ".." {
        return Err(ProfileError::InvalidName);
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(ProfileError::InvalidName);
    }
    for ch in name.chars() {
        if ch.is_ascii_control() || matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            return Err(ProfileError::InvalidName);
        }
    }
    // The device-name rule applies to the part before the first dot.
    let stem = name.split('.').next().unwrap_or(name);
    if RESERVED_NAMES.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return Err(ProfileError::ReservedName);
    }
    Ok(())
}

/// Absolute profile path for `name`.
pub fn profile_path(name: &str) -> Result<String, ProfileError> {
    validate_profile_name(name)?;
    let mut path = String::from(PROFILE_ROOT);
    path.push('\\');
    path.push_str(name);
    Ok(path)
}

/// Directories to create for `username`, parents before children.
///
/// Pure: the caller (the profile service in `cm`) performs the `mkdir` calls.
pub fn plan(username: &str) -> Result<Vec<String>, ProfileError> {
    let path = profile_path(username)?;
    let mut dirs: Vec<String> = Vec::with_capacity(STANDARD_DIRS.len() + 1);
    dirs.push(String::from(PROFILE_ROOT));
    dirs.push(path.clone());
    for sub in STANDARD_DIRS {
        let mut dir = path.clone();
        dir.push('\\');
        dir.push_str(sub);
        dirs.push(dir);
    }
    Ok(dirs)
}

/// Register a profile in the in-memory table (idempotent by username).
pub fn register(profile: Profile) {
    let mut table = PROFILES.lock();
    match table.iter_mut().find(|p| p.username.eq_ignore_ascii_case(&profile.username)) {
        Some(existing) => *existing = profile,
        None => table.push(profile),
    }
}

/// Look up a profile by account name (case-insensitive).
pub fn lookup(username: &str) -> Option<Profile> {
    PROFILES
        .lock()
        .iter()
        .find(|p| p.username.eq_ignore_ascii_case(username))
        .cloned()
}

/// Look up a profile by owner SID.
pub fn lookup_sid(sid: &Sid) -> Option<Profile> {
    PROFILES.lock().iter().find(|p| p.sid == *sid).cloned()
}

/// Every known profile (diagnostics, tests, a future `whoami`/logon path).
pub fn all() -> Vec<Profile> {
    PROFILES.lock().clone()
}

pub fn register_profile_tests() {
    use crate::{test_case, test_eq, test_true};

    test_case!("security_profile_name_validation", {
        test_true!(validate_profile_name("Administrator").is_ok());
        test_true!(validate_profile_name("jose.luis").is_ok());
        test_true!(validate_profile_name("a").is_ok());
        test_eq!(validate_profile_name(""), Err(ProfileError::InvalidName));
        test_eq!(
            validate_profile_name("aaaaaaaaaaaaaaaaaaaaa"),
            Err(ProfileError::InvalidName)
        );
        for bad in ["a\\b", "a/b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "x.", "x "] {
            test_eq!(validate_profile_name(bad), Err(ProfileError::InvalidName));
        }
        test_eq!(validate_profile_name("."), Err(ProfileError::InvalidName));
        test_eq!(validate_profile_name(".."), Err(ProfileError::InvalidName));
        for reserved in ["CON", "nul", "Com1", "LPT9", "con.txt"] {
            test_eq!(validate_profile_name(reserved), Err(ProfileError::ReservedName));
        }
    });

    test_case!("security_profile_paths", {
        test_eq!(
            profile_path("Administrator").as_deref(),
            Ok("C:\\Users\\Administrator")
        );
        test_true!(profile_path("bad/name").is_err());
        // Every standard directory is present, and `AppData` precedes its
        // children (a parent must exist before `mkdir` creates the child).
        for expected in ["Desktop", "Documents", "Downloads", "AppData", "AppData\\Local", "AppData\\Roaming"] {
            test_true!(STANDARD_DIRS.iter().any(|d| *d == expected));
        }
        let appdata = STANDARD_DIRS.iter().position(|d| *d == "AppData").unwrap();
        let local = STANDARD_DIRS.iter().position(|d| *d == "AppData\\Local").unwrap();
        let roaming = STANDARD_DIRS.iter().position(|d| *d == "AppData\\Roaming").unwrap();
        test_true!(appdata < local);
        test_true!(appdata < roaming);
    });

    test_case!("security_profile_plan_orders_parents_first", {
        let dirs = plan("Administrator").unwrap();
        test_eq!(dirs.len(), 8);
        test_eq!(dirs[0].as_str(), "C:\\Users");
        test_eq!(dirs[1].as_str(), "C:\\Users\\Administrator");
        test_eq!(dirs[2].as_str(), "C:\\Users\\Administrator\\Desktop");
        // `AppData` is created before its children.
        let appdata = dirs.iter().position(|d| d.ends_with("\\AppData")).unwrap();
        let local = dirs.iter().position(|d| d.ends_with("AppData\\Local")).unwrap();
        test_true!(appdata < local);
        test_eq!(dirs[dirs.len() - 1].as_str(), "C:\\Users\\Administrator\\AppData\\Roaming");
        test_true!(plan("bad/name").is_err());
    });

}
