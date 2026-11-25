//! Ob create — driver objects.

use crate::log::LogSubsys;
use crate::syscall::{err_to_u64, SyscallError};

pub(super) fn handles(obj_type: crate::object::ObType) -> bool {
    obj_type == crate::object::ObType::Driver
}

/// Trusted location for drivers loaded through `ObCreate(Driver)`.
///
/// Boot uses its own loader; this syscall is the userland administration path
/// (#417) and must not become a generic "execute this file in Ring 0" gate.
/// Both `/` and `\` are accepted because the VFS treats them as equivalent.
pub(crate) fn is_allowed_driver_path(path: &str) -> bool {
    let normalized = path.replace('/', "\\");
    let vfs_path = normalized
        .strip_prefix("\\Global\\FileSystem\\")
        .unwrap_or(&normalized);
    // Accept both the legacy spelling and the canonical on-disk tree
    // (`C:\NeoDOS\System\drivers`, see `paths::DRIVERS_DIR`).
    let lower = match crate::paths::canonicalize(vfs_path) {
        Some(canonical) => canonical.to_ascii_lowercase(),
        None => vfs_path.to_ascii_lowercase(),
    };
    (lower.starts_with("c:\\neodos\\system\\drivers\\")
        || lower.starts_with("c:\\system\\drivers\\"))
        && lower.ends_with(".nem")
        && !lower.contains("..")
}

/// Authorization gate for `ObCreate(Driver)` (AUDIT-001).
///
/// Kept separate from the caller so the policy can be unit-tested without
/// fabricating a Ring-3 token: the dispatch site passes
/// `crate::syscall::is_current_admin()`.
pub(crate) fn authorize_driver_load(is_admin: bool, path: &str) -> Result<(), SyscallError> {
    if !is_admin || !is_allowed_driver_path(path) {
        return Err(SyscallError::Perm);
    }
    Ok(())
}

/// Dispatch the `driver` object kinds.
pub(super) fn dispatch(
    obj_type: crate::object::ObType,
    path_str: &str,
    _fds_out: u64,
    _attrs: u64,
) -> u64 {
    match obj_type {
        crate::object::ObType::Driver => {
            // AUDIT-001: loading a NEM driver executes native Ring-0 code and
            // applies attacker-controlled relocations. Require an administrator
            // and a trusted path before reading the file at all.
            let is_admin = crate::syscall::is_current_admin();
            if let Err(e) = authorize_driver_load(is_admin, path_str) {
                kwarn!(
                    LogSubsys::Nem,
                    "ObCreate(Driver) denied (admin={}, path='{}')",
                    is_admin,
                    path_str
                );
                return err_to_u64(e);
            }
            let driver_path = path_str.strip_prefix("\\Global\\FileSystem\\").unwrap_or(&path_str);
            match crate::drivers::nem::load_nem_driver(driver_path) {
                Ok(driver_id) => {
                    let driver_name = alloc::format!("driver/{}", driver_id);
                    let ob_id = match crate::object::ob_create_object(
                        crate::object::ObType::Driver, &driver_name,
                        driver_id as u64, 0, None,
                    ) {
                        Ok(id) => id,
                        Err(_) => return err_to_u64(SyscallError::Io),
                    };
                    let ns_path = alloc::format!("\\Driver\\{}", driver_id);
                    let _ = crate::object::namespace::ob_insert_object(&ns_path, ob_id);
                    ob_id as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
