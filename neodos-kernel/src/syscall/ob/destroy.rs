//! Ob destroy — extracted from ob.rs
use alloc::string::ToString;
use crate::scheduler::{self};
use crate::syscall::{current_handle_entry, err_to_u64, SyscallError};

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

// Snapshot/service op constants are defined in super::ob (mod.rs) to remain
// alongside handler_ob_snapshot/handler_ob_service as in the original ob.rs.

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

