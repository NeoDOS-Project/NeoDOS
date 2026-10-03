//! Ob create — driver objects.

use crate::syscall::{err_to_u64, SyscallError};

pub(super) fn handles(obj_type: crate::object::ObType) -> bool {
    obj_type == crate::object::ObType::Driver
}

/// Dispatch the `driver` object kinds.
pub(super) fn dispatch(
    obj_type: crate::object::ObType,
    path_str: &str,
    _fds_out: u64,
    _attrs: u64,
) -> u64 {
    match obj_type {
        crate::object::ObType::Driver => {
            let driver_path = path_str.strip_prefix("\\Global\\FileSystem\\").unwrap_or(&path_str);
            match crate::drivers::nem::load_nem_driver(driver_path) {
                Ok(driver_id) => {
                    let driver_name = alloc::format!("driver/{}", driver_id);
                    let ob_id = match crate::object::ob_create_object(
                        crate::object::ObType::Driver, &driver_name,
                        driver_id as u64, 0, None,
                    ) {
                        Ok(id) => id,
                        Err(_) => return err_to_u64(SyscallError::Io),
                    };
                    let ns_path = alloc::format!("\\Driver\\{}", driver_id);
                    let _ = crate::object::namespace::ob_insert_object(&ns_path, ob_id);
                    ob_id as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
