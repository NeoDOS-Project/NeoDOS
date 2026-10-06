//! Ob query — dispatcher facade (split by domain).
//!
//! `handler_ob_query_info` performs argument/buffer validation and routes each
//! `ObInfoClass` to its owning domain module. ABI and behavior are unchanged;
//! the original arm bodies were moved verbatim.

use crate::syscall::{current_handle_entry, err_to_u64, SyscallError};
use crate::syscall::util::is_user_ptr_valid;

mod process;
mod stats;
mod net;
mod registry;
mod time;
mod devices;
mod fs;
mod session;

pub use stats::register_ob_stats_tests;

pub fn handler_ob_query_info(regs: crate::syscall::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let info_class = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    if buf_ptr == 0 || buf_size == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(buf_ptr, buf_size as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    match info_class {
        _ if process::handles(info_class) => process::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if stats::handles(info_class) => stats::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if net::handles(info_class) => net::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if registry::handles(info_class) => registry::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if time::handles(info_class) => time::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if devices::handles(info_class) => devices::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if fs::handles(info_class) => fs::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if session::handles(info_class) => session::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ => err_to_u64(SyscallError::Inval),
    }
}
