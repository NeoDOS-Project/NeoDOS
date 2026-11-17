//! Ob set — timers, semaphores and sections.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError, copy_from_user, copy_to_user};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::TimerStart as u32
        || info_class == ObSetInfoClass::TimerCancel as u32
        || info_class == ObSetInfoClass::SemaphoreRelease as u32
        || info_class == ObSetInfoClass::SectionMapView as u32
        || info_class == ObSetInfoClass::SectionUnmapView as u32
}

/// Dispatch the `ipc` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObSetInfoClass::TimerStart as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Timer {
                return err_to_u64(SyscallError::Inval);
            }
            let timer_id = obj.native_id as u32;
            if crate::object::timer::start_timer(timer_id) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::TimerCancel as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Timer {
                return err_to_u64(SyscallError::Inval);
            }
            let timer_id = obj.native_id as u32;
            if crate::object::timer::cancel_timer(timer_id) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::SemaphoreRelease as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Semaphore {
                return err_to_u64(SyscallError::Inval);
            }
            let sem_id = obj.native_id as u32;
            let release_count = if buf_size >= 4 {
                let mut b = [0u8; 4];
                if copy_from_user(&mut b, buf_ptr).is_err() {
                    return err_to_u64(SyscallError::Fault);
                }
                u32::from_ne_bytes(b) as i32
            } else {
                1
            };
            if crate::object::semaphore::release_semaphore(sem_id, release_count) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ if info_class == ObSetInfoClass::SectionMapView as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Section {
                return err_to_u64(SyscallError::Inval);
            }
            let section_id = obj.native_id as u32;
            match crate::object::section::map_view(section_id) {
                Some(base) => {
                    if buf_size >= 8 {
                        if copy_to_user(buf_ptr, &base.to_ne_bytes()).is_err() {
                            return err_to_u64(SyscallError::Fault);
                        }
                    }
                    base
                }
                None => err_to_u64(SyscallError::NoMem),
            }
        }
        _ if info_class == ObSetInfoClass::SectionUnmapView as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Section {
                return err_to_u64(SyscallError::Inval);
            }
            let section_id = obj.native_id as u32;
            let base = if buf_size >= 8 {
                let mut b = [0u8; 8];
                if copy_from_user(&mut b, buf_ptr).is_err() {
                    return err_to_u64(SyscallError::Fault);
                }
                u64::from_ne_bytes(b)
            } else {
                return err_to_u64(SyscallError::Inval);
            };
            if crate::object::section::unmap_view(section_id, base) { 0 }
            else { err_to_u64(SyscallError::Inval) }
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
