//! Ob create — dispatcher facade (split by object kind).
//!
//! `handler_ob_create` keeps the original path/type validation prologue and
//! routes each `ObType` to its owning module. Arm bodies were moved verbatim.

use crate::syscall::{err_to_u64, SyscallError};
use crate::syscall::util::copy_user_string;

mod process;
mod pipe;
mod directory;
mod ipc;
mod socket;
mod service;
mod driver;
mod session;

/// Map a user-supplied object-type discriminant (`ob_create` RCX) to a concrete
/// `ObType`. Only these types may be created from user mode.
pub(crate) fn create_obj_type(v: u32) -> Option<crate::object::ObType> {
    use crate::object::ObType;
    Some(match v {
        1 => ObType::Process,
        2 => ObType::Driver,
        4 => ObType::Pipe,
        11 => ObType::Directory,
        13 => ObType::Event,
        14 => ObType::Semaphore,
        15 => ObType::Timer,
        16 => ObType::Thread,
        17 => ObType::Section,
        18 => ObType::Socket,
        19 => ObType::Session,
        20 => ObType::Service,
        _ => return None,
    })
}

pub fn handler_ob_create(regs: crate::syscall::Registers) -> u64 {
    let path_ptr = regs.rbx;
    let obj_type_val = regs.rcx as u32;
    let fds_out = regs.rdx;
    let attrs = regs.r8;

    if path_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let path_str = match copy_user_string(path_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if path_str.is_empty() || !path_str.starts_with('\\') {
        return err_to_u64(SyscallError::Inval);
    }

    let obj_type = match create_obj_type(obj_type_val) {
        Some(t) => t,
        None => return err_to_u64(SyscallError::Inval),
    };

    match obj_type {
        _ if process::handles(obj_type) => process::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if pipe::handles(obj_type) => pipe::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if directory::handles(obj_type) => directory::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if ipc::handles(obj_type) => ipc::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if socket::handles(obj_type) => socket::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if service::handles(obj_type) => service::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if driver::handles(obj_type) => driver::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if session::handles(obj_type) => session::dispatch(obj_type, &path_str, fds_out, attrs),
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-012: ObQueryInfo — RAX=62
// ═══════════════════════════════════════════════════════════════════════
