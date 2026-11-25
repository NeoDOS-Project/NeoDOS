//! Ob set — login sessions (#37/#39/#40).
//!
//! `SessionLogon` authenticates the credentials against SAM and binds the
//! issued token to the session object. `SessionLock`/`SessionLogoff` are
//! restricted to an administrator or to the session's own identity.

use crate::object::types::ObSetInfoClass;
use crate::security::session::{self, SessionState};
use crate::syscall::ob::types::ObSessionCredentials;
use crate::syscall::{copy_from_user, err_to_u64, SyscallError};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::SessionLogon as u32
        || info_class == ObSetInfoClass::SessionLock as u32
        || info_class == ObSetInfoClass::SessionLogoff as u32
        || info_class == ObSetInfoClass::SessionUnlock as u32
}

fn session_id_of(entry: crate::handle::HandleEntry) -> Option<u32> {
    if entry.object_id == 0 {
        return None;
    }
    let obj = crate::object::ob_lookup(entry.object_id)?;
    if obj.obj_type != crate::object::ObType::Session {
        return None;
    }
    session::id_for_ob(entry.object_id)
}

fn caller_controls(session_id: u32) -> bool {
    let token = crate::security::current_token();
    token.is_admin_token() || token.session_id == session_id
}

/// Extract a NUL-terminated UTF-8 field from a fixed-size credentials buffer.
fn field_str(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..end]).unwrap_or("")
}

pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    let Some(session_id) = session_id_of(entry) else {
        return err_to_u64(SyscallError::Inval);
    };

    match info_class {
        _ if info_class == ObSetInfoClass::SessionLogon as u32 => {
            let size = core::mem::size_of::<ObSessionCredentials>();
            if buf_size < size {
                return err_to_u64(SyscallError::Inval);
            }
            let mut raw = alloc::vec![0u8; size];
            if copy_from_user(&mut raw, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let mut username = [0u8; 32];
            let mut password = [0u8; 64];
            username.copy_from_slice(&raw[..32]);
            password.copy_from_slice(&raw[32..96]);
            let username = field_str(&username);
            let password = field_str(&password);
            if username.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }

            match session::logon(session_id, username, password.as_bytes()) {
                Ok(token) => {
                    crate::serial_println!(
                        "[SEC] session {} logged on as {} (admin={})",
                        session_id,
                        token.sid,
                        token.is_admin_token()
                    );
                    0
                }
                Err(session::SessionError::NotFound) => err_to_u64(SyscallError::NoEnt),
                Err(session::SessionError::AlreadyActive) => err_to_u64(SyscallError::Exist),
                Err(session::SessionError::Auth(_)) => {
                    crate::serial_println!("[SEC] session {} logon failed", session_id);
                    err_to_u64(SyscallError::Perm)
                }
                Err(_) => err_to_u64(SyscallError::Inval),
            }
        }
        _ if info_class == ObSetInfoClass::SessionLock as u32 => {
            if !caller_controls(session_id) {
                return err_to_u64(SyscallError::Perm);
            }
            match session::set_state(session_id, SessionState::Locked) {
                Ok(()) => 0,
                Err(session::SessionError::NotFound) => err_to_u64(SyscallError::NoEnt),
                Err(_) => err_to_u64(SyscallError::Inval),
            }
        }
        _ if info_class == ObSetInfoClass::SessionLogoff as u32 => {
            if !caller_controls(session_id) {
                return err_to_u64(SyscallError::Perm);
            }
            match session::logoff(session_id) {
                Ok(()) => {
                    crate::serial_println!("[SEC] session {} logged off", session_id);
                    0
                }
                Err(session::SessionError::NotFound) => err_to_u64(SyscallError::NoEnt),
                Err(_) => err_to_u64(SyscallError::Inval),
            }
        }
        _ if info_class == ObSetInfoClass::SessionUnlock as u32 => {
            let size = core::mem::size_of::<ObSessionCredentials>();
            if buf_size < size {
                return err_to_u64(SyscallError::Inval);
            }
            let mut raw = alloc::vec![0u8; size];
            if copy_from_user(&mut raw, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let mut password = [0u8; 64];
            password.copy_from_slice(&raw[32..96]);
            let password = field_str(&password);
            match session::unlock(session_id, password.as_bytes()) {
                Ok(()) => {
                    crate::serial_println!("[SEC] session {} unlocked", session_id);
                    0
                }
                Err(session::SessionError::NotFound) => err_to_u64(SyscallError::NoEnt),
                Err(session::SessionError::Auth(_)) => err_to_u64(SyscallError::Perm),
                Err(_) => err_to_u64(SyscallError::Inval),
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
