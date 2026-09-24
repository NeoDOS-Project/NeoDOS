//! Ob destroy — extracted from ob.rs
use alloc::string::{String, ToString};
use crate::scheduler::{self, ThreadState};
use crate::object::types::{ObInfoClass, ObSetInfoClass};
use crate::log::LogSubsys;
use crate::syscall::util::{is_user_ptr_valid, copy_user_string};
use crate::syscall::{current_handle_entry, copy_handle_entry_for_child, resolve_chdir_target, err_to_u64, ob_err_to_syscall, SyscallError};

pub fn handler_ob_destroy(regs: crate::syscall::Registers) -> u64 {
    let fd = regs.rbx as u8;

    let (object_id, obj_type, name) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            let entry = ep.handle_table[fd as usize];
            if !entry.is_open() {
                return (0, crate::object::ObType::Unknown, alloc::string::String::new());
            }
            let oid = entry.object_id;
            let ot = entry.obj_type().unwrap_or(crate::object::ObType::Unknown);
            if oid == 0 {
                return (0, crate::object::ObType::Unknown, alloc::string::String::new());
            }
            let obj_name = match crate::object::ob_lookup(oid) {
                Some(o) => o.name_str().to_string(),
                None => alloc::string::String::new(),
            };
            (oid, ot, obj_name)
        } else {
            (0, crate::object::ObType::Unknown, alloc::string::String::new())
        }
    });

    if object_id == 0 {
        return err_to_u64(SyscallError::BadF);
    }

    if obj_type == crate::object::ObType::Directory && name.starts_with("\\Global\\FileSystem\\") {
        let vfs_path = &name["\\Global\\FileSystem\\".len()..];
        if !vfs_path.is_empty() {
            let _ = crate::globals::with_vfs(|vfs| vfs.remove_dir(vfs_path));
        }
    } else if obj_type == crate::object::ObType::Driver {
        let driver_name = if name.ends_with('\0') {
            &name[..name.len() - 1]
        } else {
            &name
        };
        let _ = crate::drivers::hotreload::unload_driver(driver_name, false);
    }

    if name.starts_with("\\Global\\FileSystem\\") {
        let _ = crate::object::namespace::ob_remove_object(&name);
    }

    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut lock = s.lock();
        if let Some(ep) = lock.current_eprocess_mut() {
            ep.handle_table[fd as usize].close();
        }
    });
    0
}

// ═══════════════════════════════════════════════════════════════════════
// OB-021: ObService — RAX=47
// ═══════════════════════════════════════════════════════════════════════

const SERVICE_CONTROL_START: u32 = 0;
const SERVICE_CONTROL_STOP: u32 = 1;
const SERVICE_CONTROL_RESTART: u32 = 2;
const SERVICE_CONTROL_QUERY_STATUS: u32 = 3;
const SERVICE_CONTROL_SET_CONFIG: u32 = 4;

// ═══════════════════════════════════════════════════════════════════════
// OB-077: ObSnapshot — RAX=48
// ═══════════════════════════════════════════════════════════════════════

const SNAPSHOT_OP_CREATE: u32 = 0;
const SNAPSHOT_OP_RESTORE: u32 = 1;
const SNAPSHOT_OP_LIST: u32 = 2;
const SNAPSHOT_OP_PURGE: u32 = 3;

/// Resolve drive index from a filesystem root handle.
pub(crate) fn resolve_handle_drive(fd: u8) -> Result<usize, u64> {
    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return Err(err_to_u64(SyscallError::BadF));
    }

    let obj = match crate::object::ob_lookup(entry.object_id) {
        Some(o) => o,
        None => return Err(err_to_u64(SyscallError::BadF)),
    };

    if obj.obj_type != crate::object::ObType::Directory {
        return Err(err_to_u64(SyscallError::Inval));
    }

    let drive_idx = if let Some(d) = entry.drive() {
        d as usize
    } else {
        return Err(err_to_u64(SyscallError::Inval));
    };

    Ok(drive_idx)
}

