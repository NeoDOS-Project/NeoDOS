//! Ob create — session objects (`ObType::Session`).
//!
//! USR-P1a defined the `Session` object type; USR-P2a (#37) adds the manager
//! hand-off. A session starts `LoggedOff` and only becomes usable after a
//! successful `ObSetInfo(SessionLogon)` bind.
//!
//! Path forms:
//! - `\Session` or `\Session\{id}` — `{id}` selects the session id, otherwise
//!   the manager assigns the next free one.

use crate::syscall::{err_to_u64, SyscallError};
use crate::scheduler;

/// Cleanup hook: `ob_destroy`/final close removes the session from the table.
struct SessionObOps;

impl crate::object::ObOperations for SessionObOps {
    fn on_destroy(&self, _id: crate::object::ObId, native_id: u64) {
        crate::security::session::destroy_session(native_id);
    }
}

static SESSION_OPS: SessionObOps = SessionObOps;

pub(super) fn handles(obj_type: crate::object::ObType) -> bool {
    obj_type == crate::object::ObType::Session
}

/// Split `\Session\{id}` into the requested id (`None` = auto-assign).
fn requested_session_id(path: &str) -> Result<Option<u32>, ()> {
    let normalized = crate::object::namespace::normalize_path(path);
    if normalized == "\\Session" || normalized == "\\Session\\" {
        return Ok(None);
    }
    let rest = normalized
        .strip_prefix("\\Session\\")
        .ok_or(())?;
    if rest.is_empty() || rest.contains('\\') {
        return Err(());
    }
    let id: u32 = rest.parse().map_err(|_| ())?;
    if id < crate::security::session::SESSION_ID_MIN {
        return Err(());
    }
    Ok(Some(id))
}

pub(super) fn dispatch(
    obj_type: crate::object::ObType,
    path_str: &str,
    _fds_out: u64,
    attrs: u64,
) -> u64 {
    match obj_type {
        crate::object::ObType::Session => {
            let requested = match requested_session_id(path_str) {
                Ok(id) => id,
                Err(_) => return err_to_u64(SyscallError::Inval),
            };
            let id = match requested {
                Some(id) => id,
                None => match crate::security::SESSIONS.lock().next_free_id() {
                    Some(id) => id,
                    None => return err_to_u64(SyscallError::NoMem),
                },
            };
            let vt_num = {
                let vt = (attrs & 0xFF) as u8;
                if vt == 0xFF {
                    crate::scheduler::current_vt_num()
                } else {
                    vt
                }
            };
            let path = alloc::format!("\\Session\\{}", id);
            let ob_id = match crate::object::ob_create_object_path(
                &path,
                obj_type,
                id,
                Some(&SESSION_OPS),
            ) {
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
            let owner_pid = crate::hal::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid()
            });
            if crate::security::SESSIONS
                .lock()
                .register(Some(id), ob_id, vt_num, owner_pid)
                .is_err()
            {
                let _ = crate::object::ob_destroy_object(ob_id);
                return err_to_u64(SyscallError::Exist);
            }
            crate::serial_println!(
                "[SEC] session {} created (ob_id={}, vt={})",
                id,
                ob_id,
                vt_num
            );

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
