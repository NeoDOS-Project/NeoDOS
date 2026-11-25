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
mod keyboard;
mod random;
mod power;

pub use stats::register_ob_stats_tests;

/// Upper bound for a single `ob_query_info`/`ob_set_info` payload. Prevents a
/// Ring-3 caller from sizing a kernel allocation to the whole user window
/// (heap-exhaustion DoS, #764). Individual classes remain free to impose
/// smaller limits.
pub const MAX_OB_IO_BUF: usize = 64 * 1024;

/// True if `info_class` is routed by some query domain. Completeness guard for
/// ObInfoClass (NEODOS-06/#636, CH-06/#510, AUD-006/#526).
pub fn info_class_handled(info_class: u32) -> bool {
    process::handles(info_class)
        || stats::handles(info_class)
        || net::handles(info_class)
        || registry::handles(info_class)
        || time::handles(info_class)
        || devices::handles(info_class)
        || fs::handles(info_class)
        || session::handles(info_class)
        || keyboard::handles(info_class)
        || random::handles(info_class)
        || power::handles(info_class)
}

pub fn handler_ob_query_info(regs: crate::syscall::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let info_class = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    if buf_ptr == 0 || buf_size == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if buf_size > MAX_OB_IO_BUF {
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
        _ if keyboard::handles(info_class) => keyboard::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if random::handles(info_class) => random::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ if power::handles(info_class) => power::dispatch(info_class, fd, entry, buf_ptr, buf_size),
        _ => err_to_u64(SyscallError::Inval),
    }
}
