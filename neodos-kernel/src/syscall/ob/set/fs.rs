//! Ob set — VFS rename/write/cwd/volume-label, file create/delete, FSCK.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError, copy_from_user, copy_to_user};
use crate::scheduler;
use crate::syscall::resolve_chdir_target;
use crate::syscall::util::copy_user_string;
use alloc::string::ToString;

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::VfsRename as u32
        || info_class == ObSetInfoClass::WriteContent as u32
        || info_class == ObSetInfoClass::SetCwd as u32
        || info_class == ObSetInfoClass::SetVolumeLabel as u32
        || info_class == ObSetInfoClass::FileCreate as u32
        || info_class == ObSetInfoClass::FileDelete as u32
        || info_class == ObSetInfoClass::FsckRepair as u32
}

/// Dispatch the `fs` info classes.
pub(super) fn dispatch(
    info_class: u32,
    fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObSetInfoClass::VfsRename as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            let obj_name = obj.name_str();
            if !obj_name.starts_with("\\Global\\FileSystem\\") {
                return err_to_u64(SyscallError::Inval);
            }
            let old_vfs_path = &obj_name["\\Global\\FileSystem\\".len()..];
            if old_vfs_path.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            let new_path = {
                let mut tmp = [0u8; 256];
                let copy_len = buf_size.min(255);
                if copy_from_user(&mut tmp[..copy_len], buf_ptr).is_err() {
                    return err_to_u64(SyscallError::Fault);
                }
                match core::str::from_utf8(&tmp[..copy_len]) {
                    Ok(s) => s.to_string(),
                    Err(_) => return err_to_u64(SyscallError::Inval),
                }
            };
            if new_path.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            match crate::globals::with_vfs(|vfs| vfs.rename(old_vfs_path, &new_path)) {
                Ok(_) => {
                    let _ = crate::object::namespace::ob_remove_object(obj_name);
                    let new_ob_name = alloc::format!("\\Global\\FileSystem\\{}", new_path);
                    let _ = crate::object::ob_set_object_name(entry.object_id, &new_ob_name);
                    {
                        let _ = crate::object::namespace::ob_create_directory_tree(&new_ob_name);
                    }
                    let _ = crate::object::namespace::ob_insert_object(&new_ob_name, entry.object_id);
                    0
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObSetInfoClass::WriteContent as u32 => {
            let (drive_idx, inode_num, handle_offset) = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    let e = ep.handle_table[fd as usize];
                    if e.has_ob_object() {
                        if let Some(obj) = crate::object::ob_lookup(e.object_id) {
                            if obj.obj_type == crate::object::ObType::Filesystem {
                                return (obj.flags as usize, obj.native_id as u32, e.offset);
                            }
                        }
                    }
                    if let Some(ot) = e.obj_type() {
                        if ot == crate::object::ObType::Filesystem {
                            return (e.drive().unwrap_or(0) as usize, e.native_id().unwrap_or(0) as u32, e.offset);
                        }
                    }
                }
                (usize::MAX, 0, 0)
            });
            if drive_idx == usize::MAX {
                return err_to_u64(SyscallError::Inval);
            }
            let mut temp_buf = alloc::vec![0u8; buf_size];
            if copy_from_user(&mut temp_buf, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let result = crate::globals::with_vfs(|vfs| {
                vfs.write(drive_idx, inode_num, handle_offset, &temp_buf)
            });
            match result {
                Ok(bytes_written) => {
                    crate::hal::without_interrupts(|| {
                        let s = scheduler::current_scheduler();
                        let mut lock = s.lock();
                        if let Some(ep) = lock.current_eprocess_mut() {
                            ep.handle_table[fd as usize].offset += bytes_written as u64;
                        }
                    });
                    bytes_written as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObSetInfoClass::SetCwd as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 8 {
                return err_to_u64(SyscallError::Inval);
            }
            let path_str = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if path_str.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            match resolve_chdir_target(path_str) {
                Ok((new_drive, new_cwd_path)) => {
                    crate::scheduler::set_current_cwd(new_drive, &new_cwd_path);
                    0
                }
                Err(_) => err_to_u64(SyscallError::NoEnt),
            }
        }
        _ if info_class == ObSetInfoClass::SetVolumeLabel as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_byte = entry.drive().unwrap_or(0xFF);
            if drive_byte == 0xFF {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_char = (b'A' + drive_byte) as char;
            let label = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if label.len() > 31 || label.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            match crate::globals::with_vfs(|vfs| vfs.set_volume_label(drive_char, &label)) {
                Ok(_) => 0,
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObSetInfoClass::FileCreate as u32 => {
            if buf_size < 3 { return err_to_u64(SyscallError::Inval); }
            let path_str = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if !path_str.contains(':') { return err_to_u64(SyscallError::Inval); }
            let node = match crate::globals::with_vfs(|vfs| vfs.create(&path_str)) {
                Ok(n) => n,
                Err(_) => return err_to_u64(SyscallError::Io),
            };
            let drive_idx = {
                let drive_letter = path_str.as_bytes()[0].to_ascii_uppercase();
                (drive_letter - b'A') as usize
            };
            let inode = node.inode;
            let ob_name = alloc::format!("\\Global\\FileSystem\\{}", path_str);
            let ob_id = match crate::object::ob_create_object(
                crate::object::ObType::Filesystem, &ob_name,
                inode as u64, drive_idx as u32, None,
            ) {
                Ok(id) => id,
                Err(_) => return err_to_u64(SyscallError::NoMem),
            };
            {
                let _ = crate::object::namespace::ob_create_directory_tree(&ob_name);
            }
            let _ = crate::object::namespace::ob_insert_object(&ob_name, ob_id);
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
            let fd = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    crate::handle::alloc_handle(&mut ep.handle_table, entry)
                } else { None }
            });
            match fd {
                Some(fd_val) => {
                    if buf_size >= 1 {
                        if copy_to_user(buf_ptr, &[fd_val]).is_err() {
                            return err_to_u64(SyscallError::Fault);
                        }
                    }
                    fd_val as u64
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        _ if info_class == ObSetInfoClass::FileDelete as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Filesystem {
                return err_to_u64(SyscallError::Inval);
            }
            let obj_name = obj.name_str();
            if !obj_name.starts_with("\\Global\\FileSystem\\") {
                return err_to_u64(SyscallError::Inval);
            }
            let vfs_path = &obj_name["\\Global\\FileSystem\\".len()..];
            if vfs_path.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            let _ = crate::globals::with_vfs(|vfs| vfs.remove_file(vfs_path));
            let _ = crate::object::namespace::ob_remove_object(obj_name);
            crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    ep.handle_table[fd as usize].close();
                }
            });
            0
        }
        _ if info_class == ObSetInfoClass::FsckRepair as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_byte = entry.drive().unwrap_or(0xFF);
            if drive_byte == 0xFF {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_char = (b'A' + drive_byte) as char;
            let repair = if buf_size >= 1 {
                let mut b = [0u8; 1];
                if copy_from_user(&mut b, buf_ptr).is_err() {
                    return err_to_u64(SyscallError::Fault);
                }
                b[0] != 0
            } else { false };
            crate::globals::with_vfs(|vfs| {
                let mut result = crate::fs::fsck::FsckStatsRaw {
                    total_blocks: 0, used_blocks: 0, free_blocks: 0,
                    total_nodes: 0, total_dirs: 0, total_files: 0,
                    errors: 0, warnings: 0, repaired: 0,
                };
                let drive_idx = match crate::fs::vfs::Vfs::drive_index(drive_char) {
                    Some(idx) => idx,
                    None => return,
                };
                if let Some(fs) = vfs.drives[drive_idx].as_mut() {
                    let _ = fs.fsck(repair, false, &mut result);
                }
            });
            0
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
