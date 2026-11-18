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

use alloc::string::String;

use lazy_static::lazy_static;

use crate::security::access::se_access_check;
use crate::security::acl::{
    Acl, Ace, SecurityDescriptor, ACCESS_ALL, ACCESS_DELETE, ACCESS_READ, ACCESS_WRITE,
};
use crate::security::sid::{sid_builtin_admin, sid_world};
use crate::security::token::Token;

use super::manager::{decode_cell, CM_MANAGER};

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
        dacl.insert_ace_canonical(Ace::allow(sid_builtin_admin(), ACCESS_ALL));
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

/// Full Ob path of a Cm key from its encoded `(hive, cell)` native id.
fn key_path(native_id: u64) -> Option<String> {
    let (hive_idx, cell) = decode_cell(native_id);
    let cm = CM_MANAGER.lock();
    let mount = cm.hives.get(hive_idx as usize)?;
    let relative = mount.hive.key_path(cell)?;
    if relative.is_empty() {
        Some(mount.mount_path.clone())
    } else {
        Some(alloc::format!("{}\\{}", mount.mount_path, relative))
    }
}

/// Path of a subkey that is about to be created under `parent_native_id`.
fn new_key_path(parent_native_id: u64, name: &str) -> Option<String> {
    let parent = key_path(parent_native_id)?;
    Some(alloc::format!("{}\\{}", parent, name))
}

fn deny(path: &str, access: u32) {
    kwarn!(
        crate::log::LogSubsys::Security,
        "cm: access denied path='{}' access={:#x}",
        path,
        access
    );
}

/// Authorize mutating the key identified by `native_id`.
pub fn authorize_key(native_id: u64, access: u32) -> bool {
    // Token first (SCHEDULER) then the key path (CM_MANAGER), matching the
    // syscall handlers' lock order.
    let token = crate::security::current_token();
    let Some(path) = key_path(native_id) else {
        return true; // invalid key: the operation will fail on its own
    };
    let ok = authorize_token(&token, &path, access);
    if !ok {
        deny(&path, access);
    }
    ok
}

/// Authorize creating `name` under `parent_native_id`.
pub fn authorize_new_key(parent_native_id: u64, name: &str) -> bool {
    let token = crate::security::current_token();
    let Some(path) = new_key_path(parent_native_id, name) else {
        return true;
    };
    let ok = authorize_token(&token, &path, ACCESS_WRITE);
    if !ok {
        deny(&path, ACCESS_WRITE);
    }
    ok
}

/// Authorize deleting the key identified by `native_id`, including the case
/// where it is an ancestor of the trust store.
pub fn authorize_delete_key(native_id: u64) -> bool {
    let token = crate::security::current_token();
    let Some(path) = key_path(native_id) else {
        return true;
    };
    let ok = authorize_delete_token(&token, &path);
    if !ok {
        deny(&path, ACCESS_DELETE);
    }
    ok
}
