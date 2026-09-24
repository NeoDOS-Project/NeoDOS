//! Ob wait — extracted from ob.rs
use alloc::string::{String, ToString};
use crate::scheduler::{self, ThreadState};
use crate::object::types::{ObInfoClass, ObSetInfoClass};
use crate::log::LogSubsys;
use crate::syscall::util::{is_user_ptr_valid, copy_user_string};
use crate::syscall::{current_handle_entry, copy_handle_entry_for_child, resolve_chdir_target, err_to_u64, ob_err_to_syscall, SyscallError};

pub fn handler_ob_wait(regs: crate::syscall::Registers) -> u64 {
    let handle_count = regs.rbx as usize;
    let handles_ptr = regs.rcx;
    let wait_type = regs.rdx as u32;
    let _timeout_ms = regs.r8;

    if handle_count == 0 || handles_ptr == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(handles_ptr, (handle_count as u64) * 8) {
        return err_to_u64(SyscallError::Fault);
    }
    if handle_count > 1 {
        return err_to_u64(SyscallError::NoSys);
    }
    if wait_type > 1 {
        return err_to_u64(SyscallError::Inval);
    }

    let fd = unsafe { (handles_ptr as *const u64).read() } as u8;
    let entry = current_handle_entry(fd);

    if entry.object_id == 0 {
        return err_to_u64(SyscallError::BadF);
    }

    let obj = match crate::object::ob_lookup(entry.object_id) {
        Some(o) => o,
        None => return err_to_u64(SyscallError::BadF),
    };

    let reason = match obj.obj_type {
        crate::object::ObType::Process => {
            let pid = obj.native_id as u32;
            crate::kwait::WaitReason::ChildExit { pid }
        }
        crate::object::ObType::Pipe => {
            let pipe_id = obj.native_id as u8;
            if let Some(true) = crate::object::pipe::pipe_peek_read_ready(pipe_id) {
                return 0;
            }
            crate::kwait::WaitReason::PipeRead { pipe_id: pipe_id as u16 }
        }
        crate::object::ObType::Event => {
            let event_type = obj.native_id as u32;
            crate::kwait::WaitReason::Event { event_type }
        }
        crate::object::ObType::Timer => {
            let timer_id = obj.native_id as u32;
            crate::kwait::WaitReason::Timer { timer_id }
        }
        crate::object::ObType::Semaphore => {
            let sem_id = obj.native_id as u32;
            if crate::object::semaphore::try_wait_semaphore(sem_id) {
                return 0;
            }
            crate::kwait::WaitReason::Semaphore { sem_id }
        }
        crate::object::ObType::Thread => {
            let tid = obj.native_id as u32;
            crate::kwait::WaitReason::ThreadJoin { tid }
        }
        _ => return err_to_u64(SyscallError::NoSys),
    };
    // OB-046 fix: cleanup_terminated_process is now handled by handler_exit
    // via work queue. We only clean up here if the child died before we block.
    // The check-and-block must be atomic to avoid a race where the child exits
    // between the check and the block (leaving the parent blocked forever).
    if obj.obj_type == crate::object::ObType::Process {
        let pid = obj.native_id as u32;
        if pid > 0 {
            crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let mut lock = s.lock();
                    crate::serial_println!("[OB_WAIT] entry object_type={:?} native_id={} pid_param={}", obj.obj_type, obj.native_id, pid);
                    let already_dead = lock.find_eprocess(pid).is_none_or(|ep| ep.thread_count == 0);
                    crate::serial_println!("[OB_WAIT] pid={} already_dead={} thread_count={} found={}", pid, already_dead, lock.find_eprocess(pid).map(|ep| ep.thread_count).unwrap_or(999), lock.find_eprocess(pid).is_some());
                    if already_dead {
                    drop(lock);
                    crate::scheduler::cleanup_terminated_process(pid);
                } else {
                    // ObCreate leaves new processes Suspended until their
                    // inherited handles are installed. ObWait is the
                    // hand-off point: activate the child before blocking
                    // the parent, otherwise the child is never schedulable.
                    let mut activated = false;
                    for child in lock.kthreads.iter_mut().flatten() {
                        if child.pid == pid && child.state == ThreadState::Suspended {
                            crate::scheduler::Scheduler::make_thread_ready(child);
                            activated = true;
                        }
                    }
                    if activated {
                        crate::serial_println!(
                            "[OB_WAIT] activated child pid={} before blocking parent pid={}",
                            pid, lock.current_pid());
                    }
                    // Atomically check-and-block: we hold the lock so the child
                    // cannot exit (modify thread_count) between check and block.
                    if let Some(k) = lock.current_kthread_mut() {
                        crate::scheduler::Scheduler::remove_from_run_queue(k);
                        let magic = reason.encode_magic();
                        k.state = ThreadState::Blocked { waiting_for: magic };
                        k.waiting_for = Some(magic);
                    }
                    crate::syscall::set_need_resched();
                }
            });
            return 0;
        }
    }
    crate::kwait::kwait_block(reason);
    0
}

// ═══════════════════════════════════════════════════════════════════════
// OB-066: ObDestroy — RAX=66
// ═══════════════════════════════════════════════════════════════════════

