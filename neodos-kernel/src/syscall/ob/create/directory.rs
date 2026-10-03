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
                    match crate::globals::with_vfs(|vfs| vfs.mkdir(vfs_path)) {
                        Ok(_) => {},
                        Err(_) => return err_to_u64(SyscallError::Io),
                    }
                }
                let token = crate::hal::without_interrupts(|| {
                    let s = scheduler::current_scheduler();
                    let lock = s.lock();
                    lock.current_eprocess()
                        .map(|ep| ep.token.clone())
                        .unwrap_or(crate::security::DEFAULT_ADMIN_TOKEN.clone())
                });
                return match crate::object::ob_open_path(&path_str, &token, 0) {
                    Ok(ob_id) => {
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
