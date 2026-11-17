//! Ob create — session objects (`ObType::Session`).
//!
//! USR-P1a defines the `Session` object type and lets sessions be created and
//! destroyed through the Object Manager (`ob_create` / `ob_destroy`). The
//! session manager, tokens and the `\Session\{id}` layout are layered on top in
//! USR-P2a/P2b.

use crate::syscall::{err_to_u64, SyscallError};
use crate::scheduler;

pub(super) fn handles(obj_type: crate::object::ObType) -> bool {
    obj_type == crate::object::ObType::Session
}

pub(super) fn dispatch(
    obj_type: crate::object::ObType,
    path_str: &str,
    _fds_out: u64,
    _attrs: u64,
) -> u64 {
    match obj_type {
        crate::object::ObType::Session => {
            let ob_id = match crate::object::ob_create_object_path(path_str, obj_type, 0, None) {
                Ok(id) => id,
                Err(crate::object::ObError::AlreadyExists) => {
                    return err_to_u64(SyscallError::Exist)
                }
                Err(crate::object::ObError::InvalidType)
                | Err(crate::object::ObError::InvalidParam) => {
                    return err_to_u64(SyscallError::Inval)
                }
                Err(_) => return err_to_u64(SyscallError::NoMem),
            };
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
        _ => err_to_u64(SyscallError::Inval),
    }
}
