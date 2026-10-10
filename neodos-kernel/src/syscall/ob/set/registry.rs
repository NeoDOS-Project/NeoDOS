//! Ob set — registry key/value mutation.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError, copy_from_user};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::RegistryCreateKey as u32
        || info_class == ObSetInfoClass::RegistryDeleteKey as u32
        || info_class == ObSetInfoClass::RegistrySetValue as u32
        || info_class == ObSetInfoClass::RegistryDeleteValue as u32
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
        _ if info_class == ObSetInfoClass::RegistryCreateKey as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            let mut kbuf = alloc::vec::Vec::with_capacity(buf_size);
            kbuf.resize(buf_size, 0u8);
            if copy_from_user(&mut kbuf, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let name = {
                let mut s = alloc::string::String::new();
                for &c in kbuf.iter() {
                    if c == 0 { break; }
                    s.push(c as char);
                }
                s
            };
            if name.is_empty() { return err_to_u64(SyscallError::Inval); }
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            match crate::cm::cm_create_key(native_id, &name) {
                Ok(_) => 0,
                Err(()) => err_to_u64(SyscallError::Exist),
            }
        }
        // ── RegistryDeleteKey (24): delete a subkey (name in buf) ──
        _ if info_class == ObSetInfoClass::RegistryDeleteKey as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            // Fault-safe read of the name buffer (may be empty / NULL).
            let mut kbuf = alloc::vec::Vec::new();
            if buf_size > 0 {
                kbuf.resize(buf_size, 0u8);
                if copy_from_user(&mut kbuf, buf_ptr).is_err() {
                    return err_to_u64(SyscallError::Fault);
                }
            }
            // If buf is empty, delete the key itself (like handler_cm_delete_key)
            if buf_size == 0 || kbuf[0] == 0 {
                let native_id = match entry.native_id() {
                    Some(id) => id,
                    None => return err_to_u64(SyscallError::BadF),
                };
                match crate::cm::cm_delete_key(native_id) {
                    Ok(()) => {
                        let _ = crate::object::ob_destroy_object(entry.object_id);
                        0
                    }
                    Err(()) => err_to_u64(SyscallError::Inval),
                }
            } else {
                let name = {
                    let mut s = alloc::string::String::new();
                    for &c in kbuf.iter() {
                        if c == 0 { break; }
                        s.push(c as char);
                    }
                    s
                };
                let native_id = match entry.native_id() {
                    Some(id) => id,
                    None => return err_to_u64(SyscallError::BadF),
                };
                match crate::cm::cm_open_key(native_id, &name) {
                    Ok(subkey_native_id) => {
                        match crate::cm::cm_delete_key(subkey_native_id) {
                            Ok(()) => 0,
                            Err(()) => err_to_u64(SyscallError::Inval),
                        }
                    }
                    Err(()) => err_to_u64(SyscallError::NoEnt),
                }
            }
        }
        // ── RegistrySetValue (25): set a value on the key ──
        // buf = [name\0][value_type: u32 LE][data_len: u32 LE][data...]
        _ if info_class == ObSetInfoClass::RegistrySetValue as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            let mut base = alloc::vec::Vec::with_capacity(buf_size);
            base.resize(buf_size, 0u8);
            if copy_from_user(&mut base, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let mut name_end = 0;
            while name_end < buf_size && buf_size - name_end >= 4 {
                let c = base[name_end];
                if c == 0 { break; }
                name_end += 1;
            }
            if name_end == 0 || name_end >= buf_size - 8 {
                return err_to_u64(SyscallError::Inval);
            }
            let name_bytes = &base[..name_end];
            let name = core::str::from_utf8(name_bytes).unwrap_or("");
            if name.is_empty() { return err_to_u64(SyscallError::Inval); }
            let payload_start = name_end + 1;
            if payload_start + 8 > buf_size {
                return err_to_u64(SyscallError::Inval);
            }
            let value_type = u32::from_le_bytes([
                base[payload_start],
                base[payload_start + 1],
                base[payload_start + 2],
                base[payload_start + 3],
            ]);
            let data_len = u32::from_le_bytes([
                base[payload_start + 4],
                base[payload_start + 5],
                base[payload_start + 6],
                base[payload_start + 7],
            ]) as usize;
            let data_start = payload_start + 8;
            if data_start + data_len > buf_size {
                return err_to_u64(SyscallError::Inval);
            }
            let data = &base[data_start..data_start + data_len];
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            match crate::cm::cm_set_value(native_id, name, value_type, data) {
                Ok(()) => 0,
                Err(()) => err_to_u64(SyscallError::NoMem),
            }
        }
        // ── RegistryDeleteValue (26): delete a value by name (name in buf) ──
        _ if info_class == ObSetInfoClass::RegistryDeleteValue as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            let mut kbuf = alloc::vec::Vec::with_capacity(buf_size);
            kbuf.resize(buf_size, 0u8);
            if copy_from_user(&mut kbuf, buf_ptr).is_err() {
                return err_to_u64(SyscallError::Fault);
            }
            let name = {
                let mut s = alloc::string::String::new();
                for &c in kbuf.iter() {
                    if c == 0 { break; }
                    s.push(c as char);
                }
                s
            };
            if name.is_empty() { return err_to_u64(SyscallError::Inval); }
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            match crate::cm::cm_delete_value(native_id, &name) {
                Ok(()) => 0,
                Err(()) => err_to_u64(SyscallError::NoEnt),
            }
        }
        // ── SetNicIp (27): set NIC IP address and subnet mask ──
        _ => err_to_u64(SyscallError::Inval),
    }
}
