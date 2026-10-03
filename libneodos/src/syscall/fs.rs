//! Filesystem Ob wrappers (file create/delete) and FSCK.

use super::{EINVAL, sys_ob_query_info, sys_ob_set_info, ObInfoClass, ObSetInfoClass};

/// ob_file_create: create a file via ob_set_info(FileCreate).
/// Calls sys_ob_set_info on the process context with FileCreate class.
/// Returns the new file fd on success.
pub fn ob_file_create(path: &str) -> Result<u8, i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr() as u64;
    let len = bytes.len() as u64;
    let r = unsafe { ob_syscall_4!(43, 1u64, ObSetInfoClass::FileCreate as u32 as u64, ptr, len) };
    if r < 0 { Err(r) } else { Ok(r as u8) }
}

/// ob_file_delete: delete a file by fd via ob_set_info(FileDelete).
/// Calls sys_ob_set_info(fd, FileDelete, null, 0).
pub fn ob_file_delete(fd: u8) -> Result<(), i64> {
    let dummy: u64 = 0;
    let r = unsafe { ob_syscall_4!(43, fd as u64, ObSetInfoClass::FileDelete as u32 as u64, &dummy as *const u64 as u64, 8u64) };
    if r < 0 { Err(r) } else { Ok(()) }
}

/// FsckStats — mirrors kernel's FsckStatsRaw.
#[repr(C)]
pub struct FsckStats {
    pub total_blocks: u64,
    pub used_blocks: u64,
    pub free_blocks: u64,
    pub total_nodes: u64,
    pub total_dirs: u64,
    pub total_files: u64,
    pub errors: u32,
    pub warnings: u32,
    pub repaired: u32,
}

/// ob_fsck_status: run read-only fsck check via ob_query_info(FsckStatus).
/// `drive_fd` = fd from ob_open on a file in the target filesystem.
pub fn ob_fsck_status(drive_fd: u8) -> Result<FsckStats, i64> {
    let mut stats = FsckStats {
        total_blocks: 0, used_blocks: 0, free_blocks: 0,
        total_nodes: 0, total_dirs: 0, total_files: 0,
        errors: 0, warnings: 0, repaired: 0,
    };
    let buf = unsafe {
        core::slice::from_raw_parts_mut(
            &mut stats as *mut FsckStats as *mut u8,
            core::mem::size_of::<FsckStats>(),
        )
    };
    sys_ob_query_info(drive_fd, ObInfoClass::FsckStatus, buf).map(|_| stats)
}

/// ob_fsck_repair: run fsck with repair via ob_set_info(FsckRepair).
/// `drive_fd` = fd from ob_open on a file in the target filesystem.
/// `repair` = true to attempt fixes, false for read-only check.
pub fn ob_fsck_repair(drive_fd: u8, repair: bool) -> Result<(), i64> {
    let flag = [if repair { 1u8 } else { 0u8 }];
    sys_ob_set_info(drive_fd, ObSetInfoClass::FsckRepair, &flag)
}

