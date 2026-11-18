//! Ob create — namespace directory objects.

use crate::syscall::{err_to_u64, SyscallError};
use crate::scheduler;

pub(super) fn handles(obj_type: crate::object::ObType) -> bool {
    obj_type == crate::object::ObType::Directory
}

/// Dispatch the `directory` object kinds.
pub(super) fn dispatch(
    obj_type: crate::object::ObType,
    path_str: &str,
    _fds_out: u64,
    _attrs: u64,
) -> u64 {
    match obj_type {
        crate::object::ObType::Directory => {
            // VFS-3.3: VFS paths under \Global\FileSystem\ delegate to
            // vfs.mkdir and return a VFS-backed fd (no Ob namespace entry).
            if let Some(vfs_path) = path_str.strip_prefix("\\Global\\FileSystem\\") {
                if !vfs_path.is_empty() {
                    let token = crate::security::current_token();
                    let parent_descriptor = if let Some(parent) =
                        vfs_path.rsplit_once('\\').map(|(p, _)| p)
                    {
                        let info = crate::globals::with_vfs(|vfs| {
                            let (drive, node) = vfs.resolve_path(parent).ok()?;
                            let descriptor = vfs.security_bytes(drive, node.inode);
                            let allowed = crate::security::access::vfs_access_check(
                                &token,
                                descriptor.as_deref(),
                                crate::security::acl::ACCESS_WRITE
                                    | crate::security::acl::ACCESS_EXECUTE,
                            );
                            Some((allowed, descriptor))
                        });
                        match info {
                            Some((true, descriptor)) => descriptor,
                            Some((false, _)) => return err_to_u64(SyscallError::Perm),
                            None => None,
                        }
                    } else {
                        None
                    };
                    let node = match crate::globals::with_vfs(|vfs| vfs.mkdir(vfs_path)) {
                        Ok(n) => n,
                        Err(_) => return err_to_u64(SyscallError::Io),
                    };
                    let drive_idx = {
                        let b = vfs_path.as_bytes().first().copied().unwrap_or(b'C');
                        (b.to_ascii_uppercase() - b'A') as usize
                    };
                    if let Some(bytes) = crate::security::acl::serialize_security_descriptor(
                        &crate::security::access::child_descriptor(
                            parent_descriptor.as_deref(),
                            &token,
                        ),
                    ) {
                        let _ = crate::globals::with_vfs(|vfs| {
                            vfs.set_security_bytes(drive_idx, node.inode, &bytes)
                        });
                    }
                }
                let token = crate::hal::without_interrupts(|| {
                    let s = scheduler::current_scheduler();
                    let lock = s.lock();
                    lock.current_eprocess()
                        .map(|ep| ep.token.clone())
                        .unwrap_or(crate::security::DEFAULT_SYSTEM_TOKEN.clone())
                });
                return match crate::object::ob_open_path(
                    &path_str,
                    &token,
                    crate::security::acl::ACCESS_READ
                        | crate::security::acl::ACCESS_WRITE
                        | crate::security::acl::ACCESS_EXECUTE,
                ) {
                    Ok(ob_id) => {
                        let entry = crate::handle::HandleEntry::ob_object(
                            ob_id,
                            crate::security::acl::ACCESS_READ
                                | crate::security::acl::ACCESS_WRITE
                                | crate::security::acl::ACCESS_EXECUTE,
                        );
                        let fd = crate::hal::without_interrupts(|| {
                            let s = scheduler::current_scheduler();
                            let mut lock = s.lock();
                            if let Some(ep) = lock.current_eprocess_mut() {
                                crate::handle::alloc_handle(&mut ep.handle_table, entry)
                            } else {
                                None
                            }
                        });
                        match fd {
                            Some(fd) => fd as u64,
                            None => {
                                let _ = crate::object::ob_close_object(ob_id);
                                err_to_u64(SyscallError::NoMem)
                            }
                        }
                    }
                    Err(_) => err_to_u64(SyscallError::Io),
                };
            }
            let ob_id = match crate::object::ob_create_object(
                obj_type, &path_str, 0, 0, None,
            ) {
                Ok(id) => id,
                Err(_) => return err_to_u64(SyscallError::NoMem),
            };
            {
                let _ = crate::object::namespace::ob_create_directory_tree(&path_str);
            }
            let _ = crate::object::namespace::ob_insert_object(&path_str, ob_id);
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
            let fd = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    crate::handle::alloc_handle(&mut ep.handle_table, entry)
                } else {
                    None
                }
            });
            match fd {
                Some(fd) => {
                    fd as u64
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
