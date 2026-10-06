//! Ob query — registry key/value info.

use crate::object::types::ObInfoClass;
use crate::syscall::{err_to_u64, SyscallError};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::RegistryKey as u32
        || info_class == ObInfoClass::RegistryValue as u32
}

/// Dispatch the `registry` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObInfoClass::RegistryKey as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            // Decode hive and cell, query key info via cm
            let (hive_idx, cell_idx) = crate::cm::decode_cell(native_id);
            let cm_lock = crate::cm::CM_MANAGER.lock();
            if (hive_idx as usize) >= cm_lock.hives.len() {
                return err_to_u64(SyscallError::NoEnt);
            }
            let hm = &cm_lock.hives[hive_idx as usize];
            let subkey_count = hm.hive.key_count(cell_idx) as u32;
            let value_count = hm.hive.value_count(cell_idx) as u32;
            drop(cm_lock);
            // Write [subkey_count: u32, value_count: u32] = 8 bytes
            let header = [
                (subkey_count & 0xFF) as u8, ((subkey_count >> 8) & 0xFF) as u8,
                ((subkey_count >> 16) & 0xFF) as u8, ((subkey_count >> 24) & 0xFF) as u8,
                (value_count & 0xFF) as u8, ((value_count >> 8) & 0xFF) as u8,
                ((value_count >> 16) & 0xFF) as u8, ((value_count >> 24) & 0xFF) as u8,
            ];
            let sz = 8;
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(header.as_ptr(), buf_ptr as *mut u8, sz);
            }
            sz as u64
        }
        // ── RegistryValue (22): query a value by name (name in buf, output overwrites) ──
        _ if info_class == ObInfoClass::RegistryValue as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            // Read value name from buf (null-terminated)
            let base = buf_ptr as *const u8;
            let name = {
                let mut s = alloc::string::String::new();
                for i in 0..buf_size {
                    let c = unsafe { core::ptr::read_volatile(base.add(i)) };
                    if c == 0 { break; }
                    s.push(c as char);
                }
                s
            };
            if name.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            match crate::cm::cm_query_value(native_id, &name) {
                Ok(val) => {
                    let data = &val.data;
                    let total_size = 8 + data.len();
                    // Write [value_type: u32 LE, data_len: u32 LE, data...]
                    let header = [
                        (val.value_type & 0xFF) as u8, ((val.value_type >> 8) & 0xFF) as u8,
                        ((val.value_type >> 16) & 0xFF) as u8, ((val.value_type >> 24) & 0xFF) as u8,
                        (data.len() & 0xFF) as u8, ((data.len() >> 8) & 0xFF) as u8,
                        ((data.len() >> 16) & 0xFF) as u8, ((data.len() >> 24) & 0xFF) as u8,
                    ];
                    let copy_len = if buf_size >= total_size { total_size } else { buf_size };
                    unsafe {
                        core::ptr::copy_nonoverlapping(header.as_ptr(), buf_ptr as *mut u8, 8);
                        if copy_len > 8 {
                            let data_copy = &data[..core::cmp::min(data.len(), buf_size - 8)];
                            core::ptr::copy_nonoverlapping(
                                data_copy.as_ptr(), (buf_ptr + 8) as *mut u8, data_copy.len(),
                            );
                        }
                    }
                    total_size as u64
                }
                Err(()) => err_to_u64(SyscallError::NoEnt),
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
