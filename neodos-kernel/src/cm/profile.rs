//! Profile service: materializes the per-user tree (#400).
//!
//! `security::profile` owns the policy (name rules, layout, owner binding);
//! this module performs the effects — `mkdir` through the VFS and the registry
//! record — because `security/` must not depend on `cm` or `fs` (rule 12).
//!
//! Layout and lifecycle are documented in `docs/design/users-security-design.md`
//! §6.1.1; enforcement waits for the filesystem ACL work (Phase 3).

use alloc::string::String;
use alloc::vec::Vec;

use crate::security::profile::{self, Profile, ProfileError};
use crate::security::sid::Sid;

/// Create one directory, treating "already exists" as success.
///
/// Uses `resolve_path` for the existence check so this module needs no `fs`
/// types: a path that resolves is a path we can leave alone.
fn make_dir(path: &str) -> bool {
    crate::globals::with_vfs(|vfs| {
        if vfs.resolve_path(path).is_ok() {
            return true;
        }
        match vfs.mkdir(path) {
            Ok(_) => true,
            // Lost a race with another creator: it exists now.
            Err(_) => {
                let now = vfs.resolve_path(path).is_ok();
                if !now {
                    kerror!(
                        crate::log::LogSubsys::Security,
                        "profile mkdir '{}' failed",
                        path
                    );
                }
                now
            }
        }
    })
}

/// Record the profile in the registry (best effort: a read-only or minimal
/// image still works, the in-memory table is authoritative at runtime).
fn persist(profile: &Profile) {
    let mut key = crate::cm::encode_cell(0, 0);
    for component in profile::REGISTRY_BASE.split('\\') {
        key = match crate::cm::cm_open_key(key, component) {
            Ok(k) => k,
            Err(_) => match crate::cm::cm_create_key(key, component) {
                Ok(k) => k,
                Err(_) => return,
            },
        };
    }
    let key = match crate::cm::cm_open_key(key, &profile.username) {
        Ok(k) => k,
        Err(_) => match crate::cm::cm_create_key(key, &profile.username) {
            Ok(k) => k,
            Err(_) => return,
        },
    };
    // REG_SZ values are NUL-terminated.
    let mut path = profile.path.as_bytes().to_vec();
    path.push(0);
    let mut sid = profile.sid.format_string().into_bytes();
    sid.push(0);
    let mut name = profile.username.as_bytes().to_vec();
    name.push(0);
    let _ = crate::cm::cm_set_value(key, "ProfilePath", 1, &path);
    let _ = crate::cm::cm_set_value(key, "OwnerSid", 1, &sid);
    let _ = crate::cm::cm_set_value(key, "Username", 1, &name);
    let _ = crate::cm::cm_flush_key(key);
}

/// Create the profile tree for `username`/`sid` (idempotent).
pub fn ensure_profile(username: &str, sid: &Sid) -> Result<Profile, ProfileError> {
    let dirs = profile::plan(username)?;
    for dir in &dirs {
        if !make_dir(dir) {
            return Err(ProfileError::Vfs);
        }
    }
    let profile = Profile {
        username: String::from(username),
        sid: *sid,
        path: profile::profile_path(username)?,
    };
    profile::register(profile.clone());
    persist(&profile);
    Ok(profile)
}

/// Create a profile for every SAM account that should have one.
///
/// `SYSTEM` is skipped: it is the operating-system account and has no
/// interactive session. Returns how many profiles are now known.
pub fn ensure_all() -> usize {
    let accounts: Vec<(String, Sid)> = {
        let db = crate::security::SAM_DB.lock();
        db.entries
            .iter()
            .filter(|e| !e.username.eq_ignore_ascii_case("SYSTEM"))
            .map(|e| (e.username.clone(), e.sid))
            .collect()
    };
    for (name, sid) in accounts {
        if let Err(e) = ensure_profile(&name, &sid) {
            kwarn!(
                crate::log::LogSubsys::Security,
                "profile for '{}': {:?}",
                name,
                e
            );
        }
    }
    profile::all().len()
}

pub fn register_profile_service_tests() {
    use crate::{test_eq, test_true};
    use crate::test_case;

    test_case!("security_profiles_cover_sam_accounts", {
        // `ensure_all` runs during boot, after the system volume is mounted.
        let accounts: Vec<(String, Sid)> = {
            let db = crate::security::SAM_DB.lock();
            db.entries
                .iter()
                .map(|e| (e.username.clone(), e.sid))
                .collect()
        };
        for (name, _sid) in accounts {
            if name.eq_ignore_ascii_case("SYSTEM") {
                test_true!(profile::lookup(&name).is_none());
                continue;
            }
            let found = match profile::lookup(&name) {
                Some(p) => p,
                None => return Err("a SAM account has no profile"),
            };
            test_true!(found.path.ends_with(&name));
            test_eq!(
                profile::lookup_sid(&found.sid).map(|p| p.username),
                Some(found.username.clone())
            );
        }
    });

    test_case!("vfs_can_create_a_root_directory", {
        // The profile service relies on this; a failure here would break every
        // profile created at runtime.
        const PROBE: &str = "C:\\__mkdir_probe";
        let created = crate::globals::with_vfs(|vfs| vfs.mkdir(PROBE).is_ok());
        test_true!(created);
        let visible = crate::globals::with_vfs(|vfs| vfs.resolve_path(PROBE).is_ok());
        test_true!(visible);
        let _ = crate::globals::with_vfs(|vfs| vfs.remove_dir(PROBE));
    });

    test_case!("security_profile_tree_exists", {
        // The directories must really be on the system volume, not just in the
        // in-memory table.
        for p in profile::all() {
            test_true!(
                crate::globals::with_vfs(|vfs| vfs.resolve_path(&p.path).is_ok()),
            );
        }
    });
}
