//! Ob query — file, cwd, volume label and FSCK status.

use crate::object::types::ObInfoClass;
use crate::syscall::{err_to_u64, SyscallError, copy_to_user};
use crate::scheduler;
use crate::syscall::ob::types::ObFileInfo;

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::File as u32
        || info_class == ObInfoClass::ReadContent as u32
        || info_class == ObInfoClass::VolumeLabel as u32
        || info_class == ObInfoClass::Cwd as u32
        || info_class == ObInfoClass::FsckStatus as u32
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
        _ if info_class == ObInfoClass::File as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive = entry.drive().unwrap_or(0);
            let inode = entry.native_id().unwrap_or(0) as u32;
            let size = crate::globals::with_vfs(|vfs| {
                vfs.stat(drive as usize, inode).map(|n| n.size).unwrap_or(0)
            });
            let fi = ObFileInfo {
                size: size as u64,
                drive,
                inode,
                padding: [0u8; 3],
            };
            let sz = core::mem::size_of::<ObFileInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let bytes = unsafe {
                core::slice::from_raw_parts(&fi as *const ObFileInfo as *const u8, sz)
            };
            if copy_to_user(buf_ptr, bytes).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::ReadContent as u32 => {
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
            let result = crate::globals::with_vfs(|vfs| {
                vfs.read(drive_idx, inode_num, handle_offset, &mut temp_buf)
            });
            match result {
                Ok(bytes_read) => {
                    if copy_to_user(buf_ptr, &temp_buf[..bytes_read]).is_err() {
                        return err_to_u64(SyscallError::Fault);
                    }
                    crate::hal::without_interrupts(|| {
                        let s = scheduler::current_scheduler();
                        let mut lock = s.lock();
                        if let Some(ep) = lock.current_eprocess_mut() {
                            ep.handle_table[fd as usize].offset += bytes_read as u64;
                        }
                    });
                    bytes_read as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObInfoClass::VolumeLabel as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_byte = entry.drive().unwrap_or(0xFF);
            if drive_byte == 0xFF {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_char = (b'A' + drive_byte) as char;
            let result = crate::globals::with_vfs(|vfs| {
                vfs.volume_label(drive_char)
            });
            match result {
                Ok(label) => {
                    let bytes = label.as_bytes();
                    let copy_len = bytes.len().min(buf_size.saturating_sub(1));
                    let mut out = alloc::vec::Vec::with_capacity(copy_len + 1);
                    out.extend_from_slice(&bytes[..copy_len]);
                    out.push(0);
                    if copy_to_user(buf_ptr, &out).is_err() {
                        return err_to_u64(SyscallError::Fault);
                    }
                    copy_len as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObInfoClass::Cwd as u32 => {
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
            let (drive, path) = crate::scheduler::get_current_cwd();
            let full = alloc::format!("{}:{}", (b'A' + drive) as char, path);
            let bytes = full.as_bytes();
            let copy_len = bytes.len().min(buf_size.saturating_sub(1));
            let mut out = alloc::vec::Vec::with_capacity(copy_len + 1);
            out.extend_from_slice(&bytes[..copy_len]);
            out.push(0);
            if copy_to_user(buf_ptr, &out).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            copy_len as u64
        }
        _ if info_class == ObInfoClass::FsckStatus as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_byte = entry.drive().unwrap_or(0xFF);
            if drive_byte == 0xFF {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_char = (b'A' + drive_byte) as char;
            let stat = crate::globals::with_vfs(|vfs| {
                let mut result = crate::fs::fsck::FsckStatsRaw {
                    total_blocks: 0, used_blocks: 0, free_blocks: 0,
                    total_nodes: 0, total_dirs: 0, total_files: 0,
                    errors: 0, warnings: 0, repaired: 0,
                };
                let drive_idx = match crate::fs::vfs::Vfs::drive_index(drive_char) {
                    Some(idx) => idx,
                    None => return result,
                };
                if let Some(fs) = vfs.drives[drive_idx].as_mut() {
                    let _ = fs.fsck(false, false, &mut result);
                }
                result
            });
            let sz = core::mem::size_of::<crate::fs::fsck::FsckStatsRaw>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let bytes = unsafe {
                core::slice::from_raw_parts(&stat as *const _ as *const u8, sz)
            };
            if copy_to_user(buf_ptr, bytes).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            sz as u64
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
