//! Ob set — process/thread priority, terminate and VT foreground.

use crate::object::types::ObSetInfoClass;
use crate::syscall::{err_to_u64, SyscallError};

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObSetInfoClass::ProcessPriority as u32
        || info_class == ObSetInfoClass::ThreadPriority as u32
        || info_class == ObSetInfoClass::ProcessTerminate as u32
        || info_class == ObSetInfoClass::SetForegroundProcess as u32
        || info_class == ObSetInfoClass::SetProcessVt as u32
}

/// Dispatch the `process` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
        _ if info_class == ObSetInfoClass::ProcessPriority as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 {
                return err_to_u64(SyscallError::Inval);
            }
            let priority = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            if priority > 3 {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                lock.set_process_priority(pid, priority as u8);
            });
            0
        }
        _ if info_class == ObSetInfoClass::ThreadPriority as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process && obj.obj_type != crate::object::ObType::Thread {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 {
                return err_to_u64(SyscallError::Inval);
            }
            let priority = unsafe { core::ptr::read_volatile(buf_ptr as *const u32) };
            if priority > 3 {
                return err_to_u64(SyscallError::Inval);
            }
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                if obj.obj_type == crate::object::ObType::Process {
                    let pid = obj.native_id as u32;
                    for k in lock.kthreads.iter_mut().flatten() {
                        if k.pid == pid {
                            k.priority = priority as u8;
                        }
                    }
                } else {
                    let tid = obj.native_id as u32;
                    if let Some(k) = lock.find_kthread_mut(tid) {
                        k.priority = priority as u8;
                    }
                }
            });
            0
        }
        _ if info_class == ObSetInfoClass::ProcessTerminate as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            if pid == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                if lock.kill_pid(pid) {
                    lock.wake_waiters(pid);
                    0
                } else {
                    err_to_u64(SyscallError::NoEnt)
                }
            })
        }
        _ if info_class == ObSetInfoClass::SetForegroundProcess as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            if pid == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            crate::input::manager::set_foreground_pid(pid);
            0
        }
        _ if info_class == ObSetInfoClass::SetProcessVt as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::Inval); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 11 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let new_vt = unsafe { core::ptr::read_volatile(buf_ptr as *const u8) };
            if new_vt >= crate::input::vt::VT_COUNT as u8 { return err_to_u64(SyscallError::Inval); }
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() { ep.vt_num = new_vt; }
            });
            0
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
