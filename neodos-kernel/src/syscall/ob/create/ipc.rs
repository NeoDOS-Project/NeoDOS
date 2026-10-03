//! Ob create — event, semaphore, timer and section objects.

use crate::syscall::{err_to_u64, SyscallError};
use crate::scheduler;
use crate::syscall::ob_err_to_syscall;

pub(super) fn handles(obj_type: crate::object::ObType) -> bool {
    obj_type == crate::object::ObType::Event
        || obj_type == crate::object::ObType::Semaphore
        || obj_type == crate::object::ObType::Timer
        || obj_type == crate::object::ObType::Section
}

/// Dispatch the `ipc` object kinds.
pub(super) fn dispatch(
    obj_type: crate::object::ObType,
    path_str: &str,
    _fds_out: u64,
    attrs: u64,
) -> u64 {
    match obj_type {
        crate::object::ObType::Event => {
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0, None,
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
            let fd = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    crate::handle::alloc_handle(&mut ep.handle_table, entry)
                } else {
                    None
                }
            });
            match fd {
                Some(fd) => {
                    fd as u64
                }
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Semaphore => {
            let initial_count = (attrs & 0xFFFF) as i32;
            let max_count = ((attrs >> 16) & 0xFFFF) as i32;
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, attrs as u32,
                Some(&crate::object::semaphore::SEMAPHORE_OPS),
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let sem_id = match crate::object::semaphore::alloc_semaphore(ob_id, initial_count, max_count) {
                Some(id) => id,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    return err_to_u64(SyscallError::Inval);
                }
            };
            {
                let mut table = crate::object::OB_TABLE.lock();
                if let Some(obj) = table.lookup_mut(ob_id) {
                    obj.native_id = sem_id as u64;
                }
            }
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
            let fd = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    crate::handle::alloc_handle(&mut ep.handle_table, entry)
                } else {
                    None
                }
            });
            match fd {
                Some(fd) => fd as u64,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Timer => {
            let period_ms = attrs & 0x7FFFFFFF;
            let periodic = (attrs >> 31) & 1 != 0;
            if period_ms == 0 || period_ms > 3600000 {
                return err_to_u64(SyscallError::Inval);
            }
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0,
                Some(&crate::object::timer::TIMER_OPS),
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let timer_id = match crate::object::timer::alloc_timer(ob_id, period_ms, periodic) {
                Some(id) => id,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    return err_to_u64(SyscallError::NoMem);
                }
            };
            {
                let mut table = crate::object::OB_TABLE.lock();
                if let Some(obj) = table.lookup_mut(ob_id) {
                    obj.native_id = timer_id as u64;
                }
            }
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
            let fd = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    crate::handle::alloc_handle(&mut ep.handle_table, entry)
                } else {
                    None
                }
            });
            match fd {
                Some(fd) => fd as u64,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        crate::object::ObType::Section => {
            let size = attrs & 0xFFFF_FFFF;
            let prot = ((attrs >> 32) & 0xFF) as u32;
            if size == 0 || size > 0x100000 || prot == 0 || prot > 3 {
                return err_to_u64(SyscallError::Inval);
            }
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0,
                Some(&crate::object::section::SECTION_OPS),
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let section_id = match crate::object::section::alloc_section(ob_id, size, prot) {
                Some(id) => id,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    return err_to_u64(SyscallError::NoMem);
                }
            };
            {
                let mut table = crate::object::OB_TABLE.lock();
                if let Some(obj) = table.lookup_mut(ob_id) {
                    obj.native_id = section_id as u64;
                }
            }
            let entry = crate::handle::HandleEntry::ob_object(ob_id, 0);
            let fd = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    crate::handle::alloc_handle(&mut ep.handle_table, entry)
                } else {
                    None
                }
            });
            match fd {
                Some(fd) => fd as u64,
                None => {
                    let _ = crate::object::ob_close_object(ob_id);
                    err_to_u64(SyscallError::NoMem)
                }
            }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
