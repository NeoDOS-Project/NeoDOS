//! Ob create — pipe objects (paired read/write handles).

use crate::syscall::{err_to_u64, SyscallError};
use crate::scheduler;
use crate::syscall::ob_err_to_syscall;
use crate::syscall::util::is_user_ptr_valid;

pub(super) fn handles(obj_type: crate::object::ObType) -> bool {
    obj_type == crate::object::ObType::Pipe
}

/// Dispatch the `pipe` object kinds.
pub(super) fn dispatch(
    obj_type: crate::object::ObType,
    path_str: &str,
    fds_out: u64,
    _attrs: u64,
) -> u64 {
    match obj_type {
        crate::object::ObType::Pipe => {
            if fds_out == 0 || !is_user_ptr_valid(fds_out, 16) {
                return err_to_u64(SyscallError::Fault);
            }
            let ob_id = match crate::object::ob_create_object_path(
                &path_str, obj_type, 0, Some(&crate::object::pipe::PIPE_OPS),
            ) {
                Ok(id) => id,
                Err(e) => return err_to_u64(ob_err_to_syscall(e)),
            };
            let obj = crate::object::ob_lookup(ob_id).unwrap();
            let pipe_id = obj.native_id as u8;
            let read_entry = crate::handle::HandleEntry {
                object_id: ob_id,
                offset: 0,
            };
            let write_entry = crate::handle::HandleEntry {
                object_id: ob_id,
                offset: 1,
            };
            let (rfd, wfd) = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    match crate::handle::alloc_two_handles(&mut ep.handle_table, read_entry, write_entry) {
                        Some((r, w)) => {
                            crate::object::pipe::PIPE_MANAGER.inc_read_ref(pipe_id);
                            crate::object::pipe::PIPE_MANAGER.inc_write_ref(pipe_id);
                            (r as u64, w as u64)
                        }
                        None => (0u64, 0u64)
                    }
                } else {
                    (0u64, 0u64)
                }
            });
            if rfd == 0 {
                let _ = crate::object::ob_close_object(ob_id);
                return err_to_u64(SyscallError::NoMem);
            }
            crate::object::ob_reference(ob_id).ok();
            crate::object::ob_reference(ob_id).ok();
            let _ = crate::object::ob_close_object(ob_id);
            unsafe {
                (fds_out as *mut u64).write(rfd);
                (fds_out as *mut u64).add(1).write(wfd);
            }
            rfd
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
