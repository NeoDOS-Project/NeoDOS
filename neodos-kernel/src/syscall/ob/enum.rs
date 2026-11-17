//! Ob enum — extracted from ob.rs
use alloc::string::{String, ToString};
use crate::scheduler::{self, ThreadState};
use crate::object::types::{ObInfoClass, ObSetInfoClass};
use crate::log::LogSubsys;
use crate::syscall::{current_handle_entry, copy_handle_entry_for_child, resolve_chdir_target, err_to_u64, ob_err_to_syscall, SyscallError, copy_to_user};
use crate::syscall::util::{is_user_ptr_valid};

pub fn handler_ob_enum(regs: crate::syscall::Registers) -> u64 {
    let dir_fd = regs.rbx as u8;
    let buf_ptr = regs.rcx;
    let max_entries = regs.rdx as usize;

    if buf_ptr == 0 || max_entries == 0 {
        return err_to_u64(SyscallError::Inval);
    }

    let entry_size = core::mem::size_of::<crate::object::ObEnumEntry>() as u64;
    if !is_user_ptr_valid(buf_ptr, entry_size.saturating_mul(max_entries as u64)) {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(dir_fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let use_vfs = if entry.object_id != 0 {
        matches!(entry.obj_type(), Some(crate::object::ObType::Filesystem) | Some(crate::object::ObType::Directory))
            && crate::object::ob_lookup(entry.object_id).is_some_and(|obj| {
                let s = obj.name_str();
                s.starts_with("\\Global\\FileSystem\\") || s.starts_with("dir/") || s.starts_with("file/")
            })
    } else {
        false
    };
    if use_vfs {
        let drv = entry.drive().unwrap_or(0);
        let nid = entry.native_id().unwrap_or(0);
        let drive_idx = drv as usize;
        let dir_inode = nid as u32;
        let mut entries = alloc::vec::Vec::new();
        let result: Result<(), ()> = crate::globals::with_vfs(|vfs| {
            let mut idx = 0usize;
            loop {
                match vfs.readdir(drive_idx, dir_inode, idx) {
                    Ok(Some(vfs_entry)) => {
                        let name_bytes = vfs_entry.name.as_bytes();
                        let mut name_arr = [0u8; 32];
                        let len = name_bytes.len().min(31);
                        name_arr[..len].copy_from_slice(&name_bytes[..len]);
                        let obj_type = if (vfs_entry.node.mode & crate::fs::vfs::MODE_DIR) != 0 {
                            crate::object::ObType::Directory
                        } else {
                            crate::object::ObType::Filesystem
                        };
                        entries.push(crate::object::ObEnumEntry {
                            id: vfs_entry.node.inode as u64,
                            obj_type: obj_type as u32,
                            name: name_arr,
                            mode: vfs_entry.node.mode,
                            _pad: [0u8; 2],
                            size: vfs_entry.node.size,
                        });
                        idx += 1;
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            Ok(())
        });
        return match result {
            Ok(()) => {
                let count = core::cmp::min(max_entries, entries.len());
                let esize = core::mem::size_of::<crate::object::ObEnumEntry>();
                let mut out = alloc::vec::Vec::with_capacity(count * esize);
                for raw in entries.iter().take(count) {
                    let bytes = unsafe {
                        core::slice::from_raw_parts(
                            raw as *const crate::object::ObEnumEntry as *const u8,
                            esize,
                        )
                    };
                    out.extend_from_slice(bytes);
                }
                if copy_to_user(buf_ptr, &out).is_err() {
                    return err_to_u64(SyscallError::Fault);
                }
                count as u64
            }
            Err(_) => err_to_u64(SyscallError::Inval),
        };
    }

    let path = if entry.object_id != 0 {
        crate::object::namespace::ob_find_path_by_id(entry.object_id)
    } else {
        None
    };
    let dir_path = match path {
        Some(p) => p,
        None => return err_to_u64(SyscallError::Inval),
    };
    let ob_entries = match crate::object::ob_enum_directory(&dir_path) {
        Ok(e) => e,
        Err(_) => return err_to_u64(SyscallError::Inval),
    };
    let count = core::cmp::min(max_entries, ob_entries.len());
    let esize = core::mem::size_of::<crate::object::ObEnumEntry>();
    let mut out = alloc::vec::Vec::with_capacity(count * esize);
    for raw in ob_entries.iter().take(count) {
        let bytes = unsafe {
            core::slice::from_raw_parts(
                raw as *const crate::object::ObEnumEntry as *const u8,
                esize,
            )
        };
        out.extend_from_slice(bytes);
    }
    if copy_to_user(buf_ptr, &out).is_err() {
        return err_to_u64(SyscallError::Fault);
    }
    count as u64
}

// ═══════════════════════════════════════════════════════════════════════
// OB-020: ObWait — RAX=65
// ═══════════════════════════════════════════════════════════════════════

