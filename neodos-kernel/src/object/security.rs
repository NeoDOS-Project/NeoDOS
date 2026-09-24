//! Object security — extracted from mod.rs (ob_open_path with SeAccessCheck)
use alloc::vec::Vec;
use crate::object::types::{ObError, ObId, ObType};
use crate::object::table::{OB_TABLE, OB_SECURITY, ob_lookup, ob_create_object, ob_reference, ob_destroy_object, ob_close_object, FILE_HANDLE_OPS};

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
    // First try a regular lookup (finds object entries).
    if let Ok(kobj_id) = crate::object::namespace::ob_lookup_path(path_str) {
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
            let result = crate::globals::with_vfs(|vfs| vfs.resolve_path(vfs_path));
            if let Ok((drive_idx, node)) = result {
                let is_dir = (node.mode & crate::fs::vfs::MODE_DIR) != 0;
                let obj_type = if is_dir { ObType::Directory } else { ObType::Filesystem };
                let obj_id = ob_create_object(obj_type, path_str, node.inode as u64, drive_idx as u32, Some(&FILE_HANDLE_OPS))?;
                ob_reference(obj_id)?;
                return Ok(obj_id);
            }
        }
    }

    Err(ObError::NotFound)
}

