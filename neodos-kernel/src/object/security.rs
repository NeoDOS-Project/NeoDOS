//! Object security — extracted from mod.rs (ob_open_path with SeAccessCheck)
use alloc::vec::Vec;
use crate::object::types::{ObError, ObId, ObType};
use crate::object::table::{OB_TABLE, OB_SECURITY, ob_lookup, ob_create_object, ob_reference, ob_destroy_object, ob_close_object, FILE_HANDLE_OPS};

/// `MODE_DIR` from the VFS (kept local to avoid object -> fs).
const MODE_DIR: u16 = 0x40;

/// May `token` replace the security descriptor of `id`?
///
/// NT semantics: the owner or an administrator. Objects without a descriptor
/// (most kernel-created namespace objects) are administrator-only, so holding
/// a handle is not enough to rewrite their DACL (AUDIT-009 / #817).
pub fn ob_can_set_security(
    id: ObId,
    token: &crate::security::token::Token,
) -> bool {
    if token.is_admin_token() {
        return true;
    }
    let sd = OB_SECURITY.lock().get(&id).cloned();
    matches!(sd, Some(sd) if sd.owner.as_ref() == Some(&token.sid))
}

#[derive(Clone, Copy)]
enum VfsOpenError {
    NotFound,
    Denied,
}

/// Open an object by Ob namespace path.
/// 1. Resolves `path_str` through the Ob namespace
/// 2. Verifies the object exists in the Object Manager
/// 3. Performs security access check against `token` with `desired_access`
/// 4. References the object (caller must later dereference via ob_close_object)
///
/// Returns the ObId on success, or an ObError.
pub fn ob_open_path(
    path_str: &str,
    token: &crate::security::token::Token,
    desired_access: u32,
) -> Result<ObId, ObError> {
    ktrace!(crate::log::LogSubsys::Object, "ob_open_path '{}': begin", path_str);
    // First try a regular lookup (finds object entries).
    if let Ok(kobj_id) = crate::object::namespace::ob_lookup_path(path_str) {
        ktrace!(crate::log::LogSubsys::Object, "ob_open_path '{}': namespace id={}", path_str, kobj_id);
        if let Some(_obj) = ob_lookup(kobj_id) {
            let sd = OB_SECURITY.lock().get(&kobj_id).cloned();
            if !crate::security::access::se_access_check(token, sd.as_ref(), desired_access) {
                return Err(ObError::AccessDenied);
            }
            ob_reference(kobj_id)?;
            return Ok(kobj_id);
        }
        // VFS-1.2: Namespace entry exists but object is gone.
        // This only happens for non-FS paths (e.g. devices, registry roots).
        // File handles under \Global\FileSystem\ no longer create namespace entries.
        let _ = crate::object::namespace::ob_remove_object(path_str);
    }

    // If not found as an object entry, check if it's a namespace directory
    // that exists but has no object entry yet. If so, create a directory
    // object for it on the fly.
    // Skip this for \Global\FileSystem\ paths — they must go through VFS
    // resolution below which sets the correct drive index (flags).
    let is_global_fs = path_str.starts_with("\\Global\\FileSystem\\");
    if !is_global_fs && crate::object::namespace::ob_is_directory(path_str) {
        let dir_id = ob_create_object(ObType::Directory, path_str, 0, 0, None)?;
        // Attempt atomic insert; if another thread created the same entry first,
        // destroy our object and use the existing one.
        let id = match crate::object::namespace::ob_insert_object(path_str, dir_id) {
            Ok(_) => dir_id,
            Err(_) => {
                let _ = ob_destroy_object(dir_id);
                match crate::object::namespace::ob_lookup_path(path_str) {
                    Ok(existing_id) => existing_id,
                    Err(_) => return Err(ObError::NotFound),
                }
            }
        };
        let sd = OB_SECURITY.lock().get(&id).cloned();
        if !crate::security::access::se_access_check(token, sd.as_ref(), desired_access) {
            let _ = ob_destroy_object(id);
            return Err(ObError::AccessDenied);
        }
        ob_reference(id)?;
        return Ok(id);
    }

    // VFS-1.2: Resolve paths under \Global\FileSystem\ via VFS.
    // Creates an ephemeral ObObject with FileHandleOps (no namespace entry).
    // Every open() creates a fresh handle instance — no orphaned namespace entries.
    if let Some(vfs_path) = path_str.strip_prefix("\\Global\\FileSystem\\") {
        if !vfs_path.is_empty() && vfs_path.contains(':') {
            let result = crate::globals::with_vfs(|vfs| {
                let bytes = vfs_path.as_bytes();
                if bytes.len() < 2 || bytes[1] != b':' {
                    return Err(VfsOpenError::NotFound);
                }
                let rest = &vfs_path[2..];
                let mut prefix = alloc::string::String::from(&vfs_path[..2]);
                let parts: alloc::vec::Vec<&str> = rest
                    .split('\\')
                    .filter(|p| !p.is_empty())
                    .collect();
                for part in parts.iter().take(parts.len().saturating_sub(1)) {
                    if !prefix.ends_with('\\') {
                        prefix.push('\\');
                    }
                    prefix.push_str(part);
                    let Ok((drive, node)) = vfs.resolve_path(&prefix) else {
                        return Err(VfsOpenError::NotFound);
                    };
                    if node.mode & MODE_DIR == 0 {
                        return Err(VfsOpenError::NotFound);
                    }
                    let descriptor = vfs.security_bytes(drive, node.inode);
                    if !crate::security::access::vfs_access_check(
                        token,
                        descriptor.as_deref(),
                        crate::security::acl::ACCESS_EXECUTE,
                    ) {
                        return Err(VfsOpenError::Denied);
                    }
                }
                let (drive_idx, node) = vfs
                    .resolve_path(vfs_path)
                    .map_err(|_| VfsOpenError::NotFound)?;
                let descriptor = vfs.security_bytes(drive_idx, node.inode);
                Ok((drive_idx, node, descriptor))
            });
            match result {
                Ok((drive_idx, node, descriptor)) => {
                    // NT DACL check at the access point, before creating a handle.
                    if !crate::security::access::vfs_access_check(
                        token,
                        descriptor.as_deref(),
                        desired_access,
                    ) {
                        return Err(ObError::AccessDenied);
                    }
                    let is_dir = (node.mode & MODE_DIR) != 0;
                    let obj_type = if is_dir { ObType::Directory } else { ObType::Filesystem };
                    let obj_id = ob_create_object(obj_type, path_str, node.inode as u64, drive_idx as u32, Some(&FILE_HANDLE_OPS))?;
                    ob_reference(obj_id)?;
                    return Ok(obj_id);
                }
                Err(VfsOpenError::Denied) => return Err(ObError::AccessDenied),
                Err(VfsOpenError::NotFound) => {}
            }
        }
    }

    Err(ObError::NotFound)
}

