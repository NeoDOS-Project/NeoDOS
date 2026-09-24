//! Ob open — extracted from ob.rs
use alloc::string::{String, ToString};
use crate::scheduler::{self, ThreadState};
use crate::object::types::{ObInfoClass, ObSetInfoClass};
use crate::log::LogSubsys;
use crate::syscall::{current_handle_entry, copy_handle_entry_for_child, resolve_chdir_target, err_to_u64, ob_err_to_syscall, SyscallError};
use crate::syscall::util::{is_user_ptr_valid, copy_user_string};

pub fn handler_ob_open(regs: crate::syscall::Registers) -> u64 {
    let path_ptr = regs.rbx;
    let desired_access = regs.rcx as u32;

    if path_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let path_str = match copy_user_string(path_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    if path_str.is_empty() {
        return err_to_u64(SyscallError::NoEnt);
    }

    let path = path_str;

    let token = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        lock.current_eprocess()
            .map(|ep| ep.token.clone())
            .unwrap_or(crate::security::DEFAULT_ADMIN_TOKEN.clone())
    });

    let ob_id = match crate::object::ob_open_path(&path, &token, desired_access) {
        Ok(id) => {
            if path.contains("Network") {
                kdebug!(LogSubsys::Object, "ObOpen '{}' => ob_id={}", path, id);
            }
            id
        }
        Err(e) => {
            if path.contains("Network") {
                kdebug!(LogSubsys::Object, "ObOpen '{}' FAILED: {:?}", path, e);
            }
            return err_to_u64(ob_err_to_syscall(e));
        }
    };

    let entry = crate::handle::HandleEntry::ob_object(ob_id, desired_access);

    let fd = crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
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

// ═══════════════════════════════════════════════════════════════════════
// OB-011: ObCreate — RAX=61
// ═══════════════════════════════════════════════════════════════════════

