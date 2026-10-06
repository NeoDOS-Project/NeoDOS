//! Ob query — process / thread info classes.

use crate::object::types::ObInfoClass;
use crate::syscall::{err_to_u64, SyscallError};
use crate::scheduler::ThreadState;
use crate::syscall::ob::types::{ObProcessInfo, ObThreadInfo};

/// Aggregate a process's thread states into the documented `ObProcessInfo`
/// state encoding (0 Ready, 1 Running, 2 Blocked, 3 Terminated):
///   1 Running    — at least one thread Running on some CPU
///   0 Ready      — no Running thread, at least one Ready
///   2 Blocked    — live threads exist, all Blocked/Suspended/Terminated
///   3 Terminated — no live thread exists
pub(super) fn process_state_aggregate(any_live: bool, any_running: bool, any_ready: bool) -> u8 {
    if !any_live {
        3
    } else if any_running {
        1
    } else if any_ready {
        0
    } else {
        2
    }
}

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::Process as u32
        || info_class == ObInfoClass::Thread as u32
        || info_class == ObInfoClass::ProcessId as u32
        || info_class == ObInfoClass::ProcessArgs as u32
        || info_class == ObInfoClass::ProcessShutdownState as u32
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
        _ if info_class == ObInfoClass::Process as u32 => {
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
            let pi = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let lock = s.lock();
                if let Some(ep) = lock.find_eprocess(pid) {
                    // Aggregate the process's threads into one documented state
                    // (layout/semantics compatible with ObProcessInfo::state):
                    //   1 Running  — at least one thread Running on some CPU
                    //   0 Ready    — no Running thread, at least one Ready
                    //   2 Blocked  — all live threads Blocked/Suspended
                    //   3 Terminated — no live thread exists
                    // This replaces the old `if thread_count == 0 { 1 } else { 0 }`
                    // which reported every live process as "Ready".
                    let mut prio = 2u8;
                    let mut found_thread = false;
                    let mut any_running = false;
                    let mut any_ready = false;
                    for k in lock.kthreads.iter().flatten() {
                        if k.pid != pid {
                            continue;
                        }
                        if !found_thread {
                            prio = k.priority;
                            found_thread = true;
                        }
                        match k.state {
                            ThreadState::Running => any_running = true,
                            ThreadState::Ready => any_ready = true,
                            _ => {}
                        }
                    }
                    let state = process_state_aggregate(found_thread, any_running, any_ready);
                    ObProcessInfo {
                        pid,
                        parent_pid: ep.parent_pid,
                        priority: prio,
                        thread_count: ep.thread_count,
                        state,
                        padding: [0u8; 2],
                    }
                } else {
                    ObProcessInfo {
                        pid, parent_pid: 0, priority: 0,
                        thread_count: 0, state: 0, padding: [0u8; 2],
                    }
                }
            });
            let sz = core::mem::size_of::<ObProcessInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &pi as *const ObProcessInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Thread as u32 => {
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
            let ti = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let lock = s.lock();
                let mut found = ObThreadInfo {
                    tid: 0, pid, state: 0, priority: 0, padding: [0u8; 2],
                };
                    for k in lock.kthreads.iter().flatten() {
                        if k.pid == pid {
                            found.tid = k.tid;
                            found.state = k.state.to_u8();
                            found.priority = k.priority;
                            break;
                        }
                    }
                found
            });
            let sz = core::mem::size_of::<ObThreadInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &ti as *const ObThreadInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::ProcessId as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 12 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 { return err_to_u64(SyscallError::Inval); }
            let pid = crate::hal::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid()
            });
            let bytes = (pid as u32).to_le_bytes();
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, 4);
            }
            4u64
        }
        _ if info_class == ObInfoClass::ProcessArgs as u32 => {
            // Per-process args buffer: return current process's args (fix 1.2)
            // Usable via any valid handle, or via \Global\Info\Process handle.
            // This isolates concurrent pipeline args — kernel copies from 0x41F000
            // at spawn time into the child's Eprocess.args.
            let args = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler().lock();
                if let Some(ep) = s.current_eprocess() {
                    ep.args
                } else {
                    [0u8; 256]
                }
            });
            let arg_len = args.iter().position(|&b| b == 0).unwrap_or(256);
            let copy_len = core::cmp::min(arg_len, buf_size.saturating_sub(1));
            unsafe {
                if copy_len > 0 {
                    core::ptr::copy_nonoverlapping(args.as_ptr(), buf_ptr as *mut u8, copy_len);
                }
                if buf_size > 0 {
                    (buf_ptr as *mut u8).add(copy_len).write(0u8);
                }
            }
            return arg_len as u64;
        }
        _ if info_class == ObInfoClass::ProcessShutdownState as u32 => {
            // #358: report whether a graceful shutdown has been requested for
            // the calling process (which must be a registered service).
            // Returns 1 byte: 0 = normal execution, 1 = shutdown requested.
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let pid = crate::hal::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid()
            });
            let requested = {
                let sm = crate::services::SERVICE_MANAGER.try_lock();
                match sm {
                    Some(sm) => sm
                        .find_by_pid(pid)
                        .map(|idx| sm.services[idx].shutdown_requested)
                        .unwrap_or(false),
                    // Contended: report "not requested" rather than blocking the
                    // service during a shutdown storm; it will re-query.
                    None => false,
                }
            };
            unsafe {
                (buf_ptr as *mut u8).write(requested as u8);
            }
            1u64
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}
