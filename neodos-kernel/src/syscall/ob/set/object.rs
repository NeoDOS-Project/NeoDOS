//! Ob set — object name and security descriptor.

use crate::object::types::ObSetInfoClass;
use crate::security::acl::ACCESS_WRITE;
use crate::syscall::{err_to_u64, SyscallError, copy_from_user};
use crate::syscall::util::copy_user_string;

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::ObjectName as u32
        || info_class == ObSetInfoClass::Security as u32
}

/// Policy for userland `ObSetInfo(ObjectName)` (AUDIT-004 / #812).
///
/// Kernel-owned namespace objects cannot be renamed, the new name is a plain
/// leaf (no path separators or drive colon), and the caller needs a writable
/// handle unless it is an administrator.
pub(crate) fn object_rename_allowed(
    is_admin: bool,
    access: u32,
    current_name: &str,
    new_name: &str,
) -> Result<(), SyscallError> {
    if new_name.is_empty()
        || new_name.len() > 31
        || new_name.contains(['\\', '/', ':'])
    {
        return Err(SyscallError::Perm);
    }
    if crate::object::namespace::is_protected_path(current_name) {
        return Err(SyscallError::Perm);
    }
    if !is_admin && access & ACCESS_WRITE == 0 {
        return Err(SyscallError::Perm);
    }
    Ok(())
}

/// Dispatch the `object` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObSetInfoClass::ObjectName as u32 => {
            let name = match copy_user_string(buf_ptr) {
                Ok(s) => s,
                Err(_) => return err_to_u64(SyscallError::Fault),
            };
            if name.len() > 31 || name.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            // AUDIT-004 / #812: the name is later used to derive VFS paths in
            // FileDelete/VfsRename/ObDestroy. Do not let userland rename
            // namespace objects or turn a leaf into a path.
            if let Err(e) = object_rename_allowed(
                crate::syscall::is_current_admin(),
                entry.access,
                obj.name_str(),
                &name,
            ) {
                return err_to_u64(e);
            }
            match crate::object::ob_set_object_name(entry.object_id, &name) {
                Ok(_) => 0,
                Err(_) => err_to_u64(SyscallError::BadF),
            }
        }
        _ if info_class == ObSetInfoClass::Security as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            // AUDIT-009 / #817: only the owner or an administrator may replace
            // the DACL; objects without a descriptor are admin-only.
            let token = crate::security::current_token();
            if !crate::object::ob_can_set_security(entry.object_id, &token) {
                return err_to_u64(SyscallError::Perm);
            }
            // buf = [rev: u8, ace_count: u8, (ace_type: u8, flags: u8, access_mask: u32 LE, sid_cnt: u8, sid_auth: [u8;6], sid_subs: u32×cnt)...]
            if buf_size < 2 {
                return err_to_u64(SyscallError::Inval);
            }
            let mut kbuf = alloc::vec::Vec::with_capacity(buf_size);
            kbuf.resize(buf_size, 0u8);
            if copy_from_user(&mut kbuf, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let _sd_rev = kbuf[0];
            let ace_count = kbuf[1];
            let mut offset = 2usize;
            let mut acl = crate::security::acl::Acl::new();
            for _ in 0..ace_count {
                if offset + 7 > buf_size {
                    return err_to_u64(SyscallError::Inval);
                }
                let ace_type = kbuf[offset];
                let flags = kbuf[offset + 1];
                let access_mask = u32::from_le_bytes([
                    kbuf[offset + 2],
                    kbuf[offset + 3],
                    kbuf[offset + 4],
                    kbuf[offset + 5],
                ]);
                let sid_cnt = kbuf[offset + 6] as usize;
                if sid_cnt > crate::security::sid::MAX_SUB_AUTHORITIES {
                    return err_to_u64(SyscallError::Inval);
                }
                offset += 7;
                if offset + 6 + sid_cnt * 4 > buf_size {
                    return err_to_u64(SyscallError::Inval);
                }
                let mut sid_auth = [0u8; 6];
                for j in 0..6 {
                    sid_auth[j] = kbuf[offset + j];
                }
                offset += 6;
                let mut sid_subs = [0u32; crate::security::sid::MAX_SUB_AUTHORITIES];
                for j in 0..sid_cnt {
                    sid_subs[j] = u32::from_le_bytes([
                        kbuf[offset + j * 4],
                        kbuf[offset + j * 4 + 1],
                        kbuf[offset + j * 4 + 2],
                        kbuf[offset + j * 4 + 3],
                    ]);
                }
                offset += sid_cnt * 4;
                let sid = crate::security::sid::Sid::from_parts(1, &sid_auth, &sid_subs[..sid_cnt]);
                let ace = crate::security::acl::Ace { ace_type, flags, access_mask, sid };
                acl.insert_ace_canonical(ace);
            }
            let sd = crate::security::acl::SecurityDescriptor::new()
                .with_dacl(acl);
            match crate::object::ob_set_security(entry.object_id, sd) {
                Ok(()) => 0,
                Err(_) => err_to_u64(SyscallError::BadF),
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
