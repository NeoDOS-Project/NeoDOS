use super::Registers;
use super::{err_to_u64, SyscallError};
use crate::cm;
use crate::hal;
use crate::handle::{HandleEntry, alloc_handle};
use crate::scheduler;
use crate::syscall::{copy_user_string, is_user_ptr_valid, current_handle_entry};

// ═══════════════════════════════════════════════════════════════════════
// Registry syscall handlers (Cm — Configuration Manager)
// RAX 67-76
// ═══════════════════════════════════════════════════════════════════════

/// RAX 67: cm_open_key(path_ptr) -> handle
/// Open a registry key by path under \Registry.
/// Returns fd (>=3) on success.
pub(super) fn handler_cm_open_key(regs: Registers) -> u64 {
    let path_ptr = regs.rbx;

    let path = match copy_user_string(path_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    // Look up via Ob namespace (full path like \Registry\Machine\System)
    let token = hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let lock = s.lock();
        lock.current_eprocess()
            .map(|ep| ep.token.clone())
            .unwrap_or(crate::security::DEFAULT_ADMIN_TOKEN.clone())
    });

    match crate::object::ob_open_path(&path, &token, 1) {
        Ok(ob_id) => {
            // The ob_id has native_id = encoded cell index
            let entry = HandleEntry::ob_object(ob_id, 1);
            let fd = hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    alloc_handle(&mut ep.handle_table, entry)
                } else {
                    None
                }
            });
            match fd {
                Some(f) => f as u64,
                None => err_to_u64(SyscallError::NoMem),
            }
        }
        Err(_) => err_to_u64(SyscallError::NoEnt),
    }
}

/// RAX 68: cm_create_key(fd, name_ptr) -> handle
/// Create a subkey under the key referenced by fd.
pub(super) fn handler_cm_create_key(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let name_ptr = regs.rcx;

    let name = match copy_user_string(name_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    // Get the ObObject's native_id (which encodes hive_idx + cell_idx)
    let native_id = match entry.native_id() {
        Some(id) => id,
        None => return err_to_u64(SyscallError::Inval),
    };

    match cm::cm_create_key(native_id, &name) {
        Ok(new_native_id) => {
            // Create a new ObObject for the subkey
            let ob_id = match crate::object::ob_create_object(
                crate::object::ObType::Key,
                &name,
                new_native_id,
                0,
                None,
            ) {
                Ok(id) => id,
                Err(_) => return err_to_u64(SyscallError::NoMem),
            };
            let new_entry = HandleEntry::ob_object(ob_id, 1);
            let new_fd = hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    alloc_handle(&mut ep.handle_table, new_entry)
                } else {
                    None
                }
            });
            match new_fd {
                Some(f) => f as u64,
                None => err_to_u64(SyscallError::NoMem),
            }
        }
        Err(_) => err_to_u64(SyscallError::Exist),
    }
}

/// RAX 69: cm_query_value(fd, name_ptr, buf_ptr, buf_len) -> size
/// Query a value on the key referenced by fd.
pub(super) fn handler_cm_query_value(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let name_ptr = regs.rcx;
    let buf_ptr = regs.rdx;
    let buf_len = regs.r8 as usize;

    let name = match copy_user_string(name_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let native_id = match entry.native_id() {
        Some(id) => id,
        None => return err_to_u64(SyscallError::Inval),
    };

    match cm::cm_query_value(native_id, &name) {
        Ok(val) => {
            let data = &val.data;
            let total_size = 8 + data.len(); // type(u32) + len(u32) + data

            if buf_ptr != 0 && buf_len >= 8 {
                // Write value type + data size to the user buffer
                let header = [
                    (val.value_type & 0xFF) as u8,
                    ((val.value_type >> 8) & 0xFF) as u8,
                    ((val.value_type >> 16) & 0xFF) as u8,
                    ((val.value_type >> 24) & 0xFF) as u8,
                    (data.len() & 0xFF) as u8,
                    ((data.len() >> 8) & 0xFF) as u8,
                    ((data.len() >> 16) & 0xFF) as u8,
                    ((data.len() >> 24) & 0xFF) as u8,
                ];

                let copy_len = if buf_len >= total_size { total_size } else { buf_len };

                if is_user_ptr_valid(buf_ptr, copy_len as u64) {
                    unsafe {
                        core::ptr::copy_nonoverlapping(header.as_ptr(), buf_ptr as *mut u8, 8);
                        if copy_len > 8 && buf_len >= 8 + data.len() {
                            let data_copy = &data[..core::cmp::min(data.len(), buf_len - 8)];
                            core::ptr::copy_nonoverlapping(
                                data_copy.as_ptr(),
                                (buf_ptr + 8) as *mut u8,
                                data_copy.len(),
                            );
                        }
                    }
                }
            }

            total_size as u64
        }
        Err(_) => err_to_u64(SyscallError::NoEnt),
    }
}

/// RAX 70: cm_set_value(fd, name_ptr, value_type, data_ptr, data_len)
/// Set a value on the key referenced by fd.
pub(super) fn handler_cm_set_value(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let name_ptr = regs.rcx;
    let value_type = regs.rdx as u32;
    let data_ptr = regs.r8;
    let data_len = regs.r9 as usize;

    let name = match copy_user_string(name_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let native_id = match entry.native_id() {
        Some(id) => id,
        None => return err_to_u64(SyscallError::Inval),
    };

    // Validate user data pointer
    if data_ptr != 0 && data_len > 0 && !is_user_ptr_valid(data_ptr, data_len as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let mut data = alloc::vec::Vec::with_capacity(data_len);
    if data_len > 0 {
        unsafe {
            let slice = core::slice::from_raw_parts(data_ptr as *const u8, data_len);
            data.extend_from_slice(slice);
        }
    }

    match cm::cm_set_value(native_id, &name, value_type, &data) {
        Ok(()) => 0,
        Err(_) => err_to_u64(SyscallError::NoMem),
    }
}

/// RAX 71: cm_enum_key(fd, index, buf_ptr) -> 0 or error
/// Enumerate subkeys of the key referenced by fd.
pub(super) fn handler_cm_enum_key(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let index = regs.rcx as u32;
    let buf_ptr = regs.rdx;

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let native_id = match entry.native_id() {
        Some(id) => id,
        None => return err_to_u64(SyscallError::Inval),
    };

    match cm::cm_enum_key(native_id, index) {
        Ok(name) => {
            let bytes = name.as_bytes();
            let len = bytes.len().min(255);
            if buf_ptr != 0 {
                if is_user_ptr_valid(buf_ptr, (len + 1) as u64) {
                    unsafe {
                        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, len);
                        (buf_ptr as *mut u8).add(len).write(0u8);
                    }
                    return (len + 1) as u64;
                }
            }
            0
        }
        Err(_) => 0, // No more entries
    }
}

/// RAX 72: cm_enum_value(fd, index, buf_ptr) -> 0 or error
/// Enumerate values of the key referenced by fd.
pub(super) fn handler_cm_enum_value(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;
    let index = regs.rcx as u32;
    let buf_ptr = regs.rdx;

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let native_id = match entry.native_id() {
        Some(id) => id,
        None => return err_to_u64(SyscallError::Inval),
    };

    match cm::cm_enum_value(native_id, index) {
        Ok(name) => {
            let bytes = name.as_bytes();
            let len = bytes.len().min(255);
            if buf_ptr != 0 {
                if is_user_ptr_valid(buf_ptr, (len + 1) as u64) {
                    unsafe {
                        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, len);
                        (buf_ptr as *mut u8).add(len).write(0u8);
                    }
                    return (len + 1) as u64;
                }
            }
            0
        }
        Err(_) => 0,
    }
}

/// RAX 73: cm_delete_key(fd)
pub(super) fn handler_cm_delete_key(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let native_id = match entry.native_id() {
        Some(id) => id,
        None => return err_to_u64(SyscallError::Inval),
    };

    match cm::cm_delete_key(native_id) {
        Ok(()) => 0,
        Err(_) => err_to_u64(SyscallError::Inval),
    }
}

/// RAX 74: cm_flush_key(fd)
pub(super) fn handler_cm_flush_key(regs: Registers) -> u64 {
    let fd = regs.rbx as u8;

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    let native_id = match entry.native_id() {
        Some(id) => id,
        None => return err_to_u64(SyscallError::Inval),
    };

    match cm::cm_flush_key(native_id) {
        Ok(()) => 0,
        Err(_) => err_to_u64(SyscallError::Io),
    }
}

/// RAX 75: cm_load_hive(name_ptr, mount_point_ptr) [admin]
pub(super) fn handler_cm_load_hive(regs: Registers) -> u64 {
    let name_ptr = regs.rbx;
    let mount_ptr = regs.rcx;

    let name = match copy_user_string(name_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };
    let mount = match copy_user_string(mount_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    match cm::cm_load_hive(&name, &mount) {
        Ok(()) => 0,
        Err(_) => err_to_u64(SyscallError::Exist),
    }
}

/// RAX 76: cm_unload_hive(mount_point_ptr) [admin]
pub(super) fn handler_cm_unload_hive(regs: Registers) -> u64 {
    let mount_ptr = regs.rbx;

    let mount = match copy_user_string(mount_ptr) {
        Ok(s) => s,
        Err(_) => return err_to_u64(SyscallError::Fault),
    };

    match cm::cm_unload_hive(&mount) {
        Ok(()) => 0,
        Err(_) => err_to_u64(SyscallError::NoEnt),
    }
}
