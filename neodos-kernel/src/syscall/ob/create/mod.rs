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

    let obj_type = match obj_type_val {
        1 => crate::object::ObType::Process,
        2 => crate::object::ObType::Driver,
        4 => crate::object::ObType::Pipe,
        11 => crate::object::ObType::Directory,
        13 => crate::object::ObType::Event,
        14 => crate::object::ObType::Semaphore,
        15 => crate::object::ObType::Timer,
        16 => crate::object::ObType::Thread,
         17 => crate::object::ObType::Section,
         18 => crate::object::ObType::Socket,
         20 => crate::object::ObType::Service,
        _ => return err_to_u64(SyscallError::Inval),
    };

    match obj_type {
        _ if process::handles(obj_type) => process::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if pipe::handles(obj_type) => pipe::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if directory::handles(obj_type) => directory::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if ipc::handles(obj_type) => ipc::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if socket::handles(obj_type) => socket::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if service::handles(obj_type) => service::dispatch(obj_type, &path_str, fds_out, attrs),
        _ if driver::handles(obj_type) => driver::dispatch(obj_type, &path_str, fds_out, attrs),
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OB-012: ObQueryInfo — RAX=62
// ═══════════════════════════════════════════════════════════════════════
