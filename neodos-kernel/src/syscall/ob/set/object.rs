//! Ob set — object name and security descriptor.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError};
use crate::syscall::util::copy_user_string;

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::ObjectName as u32
        || info_class == ObSetInfoClass::Security as u32
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
            match crate::object::ob_set_object_name(entry.object_id, &name) {
                Ok(_) => 0,
                Err(_) => err_to_u64(SyscallError::BadF),
            }
        }
        _ if info_class == ObSetInfoClass::Security as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            // buf = [rev: u8, ace_count: u8, (ace_type: u8, flags: u8, access_mask: u32 LE, sid_cnt: u8, sid_auth: [u8;6], sid_subs: u32×cnt)...]
            if buf_size < 2 {
                return err_to_u64(SyscallError::Inval);
            }
            let base = buf_ptr as *const u8;
            let _sd_rev = unsafe { core::ptr::read_volatile(base) };
            let ace_count = unsafe { core::ptr::read_volatile(base.add(1)) };
            let mut offset = 2usize;
            let mut acl = crate::security::acl::Acl::new();
            for _ in 0..ace_count {
                if offset + 7 > buf_size {
                    return err_to_u64(SyscallError::Inval);
                }
                let ace_type = unsafe { core::ptr::read_volatile(base.add(offset)) };
                let flags = unsafe { core::ptr::read_volatile(base.add(offset + 1)) };
                let access_mask = unsafe {
                    u32::from_le_bytes([
                        core::ptr::read_volatile(base.add(offset + 2)),
                        core::ptr::read_volatile(base.add(offset + 3)),
                        core::ptr::read_volatile(base.add(offset + 4)),
                        core::ptr::read_volatile(base.add(offset + 5)),
                    ])
                };
                let sid_cnt = unsafe { core::ptr::read_volatile(base.add(offset + 6)) } as usize;
                if sid_cnt > crate::security::sid::MAX_SUB_AUTHORITIES {
                    return err_to_u64(SyscallError::Inval);
                }
                offset += 7;
                if offset + 6 + sid_cnt * 4 > buf_size {
                    return err_to_u64(SyscallError::Inval);
                }
                let mut sid_auth = [0u8; 6];
                for j in 0..6 {
                    sid_auth[j] = unsafe { core::ptr::read_volatile(base.add(offset + j)) };
                }
                offset += 6;
                let mut sid_subs = [0u32; crate::security::sid::MAX_SUB_AUTHORITIES];
                for j in 0..sid_cnt {
                    sid_subs[j] = unsafe {
                        u32::from_le_bytes([
                            core::ptr::read_volatile(base.add(offset + j * 4)),
                            core::ptr::read_volatile(base.add(offset + j * 4 + 1)),
                            core::ptr::read_volatile(base.add(offset + j * 4 + 2)),
                            core::ptr::read_volatile(base.add(offset + j * 4 + 3)),
                        ])
                    };
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
