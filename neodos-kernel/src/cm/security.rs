//! Per-key authorization for the Configuration Manager.
//!
//! Cm keys are persisted in hives, not represented by long-lived Ob objects,
//! so an `ObObject` DACL cannot protect them: every open creates a fresh,
//! ephemeral `ObType::Key`. The mutation paths in [`super::api`] therefore
//! check a real ACL here, against the full key path, before touching the hive.
//!
//! The protected subtree is the system trust store. Reads stay open (public
//! certificates are public); create/delete/write require an administrator or a
//! token whose groups contain the administrator SID.

use alloc::string::{String, ToString};

use lazy_static::lazy_static;

use crate::security::access::se_access_check;
use crate::security::acl::{
    Acl, Ace, SecurityDescriptor, ACCESS_ALL, ACCESS_DELETE, ACCESS_READ, ACCESS_WRITE,
};
use crate::security::sid::{sid_builtin_administrators, sid_builtin_system, sid_world};
use crate::security::token::Token;

use super::hive::Hive;

/// Registry subtree that owns the system trust store.
pub const TRUST_STORE_KEY: &str = "\\Registry\\Machine\\Software\\NeoDOS\\Trust";

lazy_static! {
    /// DACL of [`TRUST_STORE_KEY`]:
    /// * `Everyone` can read public anchors.
    /// * Administrators/SYSTEM can do everything (the token admin flag also
    ///   grants access through `se_access_check`).
    static ref TRUST_SD: SecurityDescriptor = {
        let mut dacl = Acl::new();
        dacl.insert_ace_canonical(Ace::allow(sid_world(), ACCESS_READ));
        dacl.insert_ace_canonical(Ace::allow(sid_builtin_system(), ACCESS_ALL));
        dacl.insert_ace_canonical(Ace::allow(sid_builtin_administrators(), ACCESS_ALL));
        SecurityDescriptor::new().with_dacl(dacl)
    };
}

/// True when `path` is exactly the trust root or a descendant of it.
///
/// Comparison is ASCII case-insensitive (the registry is case-insensitive)
/// and requires a component boundary, so `...\TrustEvil` is not protected.
pub fn path_is_protected(path: &str) -> bool {
    let prefix = TRUST_STORE_KEY.as_bytes();
    let bytes = path.as_bytes();
    if bytes.len() < prefix.len() || !bytes[..prefix.len()].eq_ignore_ascii_case(prefix) {
        return false;
    }
    bytes.len() == prefix.len() || bytes[prefix.len()] == b'\\'
}

/// True when `path` is the trust root, one of its ancestors, or a descendant.
///
/// Deleting an ancestor (e.g. `...\Software\NeoDOS`) would recursively destroy
/// the protected subtree, so it needs the same authorization as the subtree
/// itself.
pub fn path_contains_trust(path: &str) -> bool {
    let trust = TRUST_STORE_KEY.as_bytes();
    let bytes = path.as_bytes();
    if bytes.len() <= trust.len() {
        return bytes.eq_ignore_ascii_case(&trust[..bytes.len()])
            && (bytes.len() == trust.len() || trust[bytes.len()] == b'\\');
    }
    path_is_protected(path)
}

/// Check an open/write against the trust DACL. Paths outside the trust subtree
/// are unprotected (as before).
pub fn authorize_token(token: &Token, path: &str, access: u32) -> bool {
    if !path_is_protected(path) {
        return true;
    }
    se_access_check(token, Some(&TRUST_SD), access)
}

/// Check a key deletion. Ancestors of the trust root are protected too,
/// because deleting them deletes the store.
pub fn authorize_delete_token(token: &Token, path: &str) -> bool {
    if !path_contains_trust(path) {
        return true;
    }
    se_access_check(token, Some(&TRUST_SD), ACCESS_DELETE)
}

/// Full Ob path of a key relative to the hive root. Pure: the caller holds
/// `CM_MANAGER`, so authorization and mutation share one critical section.
pub(crate) fn full_path(mount_path: &str, hive: &Hive, cell: u32) -> Option<String> {
    let relative = hive.key_path(cell)?;
    if relative.is_empty() {
        Some(mount_path.to_string())
    } else {
        Some(alloc::format!("{}\\{}", mount_path, relative))
    }
}

/// Path of a subkey that is about to be created.
pub(crate) fn new_key_path(
    mount_path: &str,
    hive: &Hive,
    parent_cell: u32,
    name: &str,
) -> Option<String> {
    let parent = full_path(mount_path, hive, parent_cell)?;
    Some(alloc::format!("{}\\{}", parent, name))
}

pub(crate) fn deny(path: &str, access: u32) {
    kwarn!(
        crate::log::LogSubsys::Security,
        "cm: access denied path='{}' access={:#x}",
        path,
        access
    );
}
