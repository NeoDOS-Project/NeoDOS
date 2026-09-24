// ── Unified Wait Engine (KWait)
//
// Replaces all ad-hoc blocking/wake mechanisms with a single abstraction.
// Every blocking operation (pipe read, IRP complete, thread join, child exit,
// event wait, timer) goes through KWait.
//
// ABI frozen at v0.42 — WaitReason variants and magic encoding must not change.

use crate::scheduler::{self, ThreadState};
use crate::hal::irql::{self, DISPATCH_LEVEL};

/// Magic number base for each wait reason type.
/// Upper 16 bits encode the reason, lower 16 bits carry the instance ID.
const MAGIC_PIPE_BASE: u32    = 0x0001_0000;
const MAGIC_IRP_BASE: u32     = 0x0002_0000;
const MAGIC_THREAD_BASE: u32  = 0x0003_0000;
const MAGIC_CHILD_BASE: u32   = 0x0004_0000;
const MAGIC_EVENT_BASE: u32   = 0x0005_0000;
const MAGIC_TIMER_BASE: u32   = 0x0006_0000;
const MAGIC_APC_BASE: u32     = 0x0007_0000;
const MAGIC_SEMAPHORE_BASE: u32 = 0x0008_0000;
const MAGIC_SOCKET_BASE: u32 = 0x0009_0000;

/// WaitReason encodes what a thread is waiting for.
/// Variants MUST NOT be reordered or removed (ABI freeze v0.42).
/// New variants may be appended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaitReason {
    /// Waiting for pipe data (pipe_id in low 16 bits)
    PipeRead { pipe_id: u16 },
    /// Waiting for an IRP to complete (irp_id)
    IrpComplete { irp_id: u32 },
    /// Waiting for a thread to exit (tid)
    ThreadJoin { tid: u32 },
    /// Waiting for a child process to exit (pid)
    ChildExit { pid: u32 },
    /// Waiting for an event bus event
    Event { event_type: u32 },
    /// Waiting for a timer to expire (timer_id)
    Timer { timer_id: u32 },
    /// Waiting for an APC to be delivered
    Alertable,
    /// Waiting on a semaphore (sem_id)
    Semaphore { sem_id: u32 },
    /// Waiting for socket data to be readable (socket_id)
    SocketRead { socket_id: u32 },
    /// Waiting for a socket connection to complete (socket_id)
    SocketConnect { socket_id: u32 },
    /// Waiting to accept a new connection (socket_id)
    SocketAccept { socket_id: u32 },
}

impl WaitReason {
    /// Encode this wait reason into a magic u32 for the scheduler.
    /// Upper bits: type tag. Lower bits: instance ID.
    pub fn encode_magic(&self) -> u32 {
        match *self {
            WaitReason::PipeRead { pipe_id }  => MAGIC_PIPE_BASE | pipe_id as u32,
            WaitReason::IrpComplete { irp_id } => MAGIC_IRP_BASE | (irp_id & 0xFFFF),
            WaitReason::ThreadJoin { tid }     => MAGIC_THREAD_BASE | (tid & 0xFFFF),
            WaitReason::ChildExit { pid }      => MAGIC_CHILD_BASE | (pid & 0xFFFF),
            WaitReason::Event { event_type }   => MAGIC_EVENT_BASE | (event_type & 0xFFFF),
            WaitReason::Timer { timer_id }      => MAGIC_TIMER_BASE | (timer_id & 0xFFFF),
            WaitReason::Alertable              => MAGIC_APC_BASE,
            WaitReason::Semaphore { sem_id }   => MAGIC_SEMAPHORE_BASE | (sem_id & 0xFFFF),
            WaitReason::SocketRead { socket_id }   => MAGIC_SOCKET_BASE | 0x1000 | (socket_id & 0xFFF),
            WaitReason::SocketConnect { socket_id } => MAGIC_SOCKET_BASE | 0x2000 | (socket_id & 0xFFF),
            WaitReason::SocketAccept { socket_id }  => MAGIC_SOCKET_BASE | 0x3000 | (socket_id & 0xFFF),
        }
    }

    pub fn decode_magic(magic: u32) -> Option<WaitReason> {
        let tag = magic & 0xFFFF_0000;
        let id = magic & 0xFFFF;
        Some(match tag {
            MAGIC_PIPE_BASE   => WaitReason::PipeRead { pipe_id: id as u16 },
            MAGIC_IRP_BASE    => WaitReason::IrpComplete { irp_id: id },
            MAGIC_THREAD_BASE => WaitReason::ThreadJoin { tid: id },
            MAGIC_CHILD_BASE  => WaitReason::ChildExit { pid: id },
            MAGIC_EVENT_BASE  => WaitReason::Event { event_type: id },
            MAGIC_TIMER_BASE  => WaitReason::Timer { timer_id: id },
            MAGIC_APC_BASE    => WaitReason::Alertable,
            MAGIC_SEMAPHORE_BASE => WaitReason::Semaphore { sem_id: id },
            MAGIC_SOCKET_BASE => {
                let sub_type = id & 0xF000;
                let instance = id & 0xFFF;
                match sub_type {
                    0x1000 => WaitReason::SocketRead { socket_id: instance },
                    0x2000 => WaitReason::SocketConnect { socket_id: instance },
                    0x3000 => WaitReason::SocketAccept { socket_id: instance },
                    _ => return None,
                }
            }
            _ => return None,
        })
    }
}

/// Block the current thread with the given wait reason.
/// The thread will be in Blocked state until `kwait_wake` is called with
/// a matching magic value.
pub fn kwait_block(reason: WaitReason) {
    let magic = reason.encode_magic();
    let old_irql = unsafe { irql::raise_irql(DISPATCH_LEVEL) };
    let mut lock = scheduler::current_scheduler().lock();
    if let Some(k) = lock.current_kthread_mut() {
        let before = k.state.to_u8();
        scheduler::Scheduler::remove_from_run_queue(k);
        k.state = ThreadState::Blocked { waiting_for: magic };
        k.waiting_for = Some(magic);
        crate::trace_sched_state!(k.tid, before, k.state.to_u8(), 3u8); // KWAIT_BLOCK
    }
    crate::syscall::set_need_resched();
    drop(lock);
    unsafe { irql::lower_irql(old_irql) };
}

/// Wake ALL threads blocked on a specific wait reason.
/// The scheduler scans all threads and transitions Blocked → Ready
/// for those whose `waiting_for` matches the encoded magic.
pub fn kwait_wake(reason: &WaitReason) {
    let magic = reason.encode_magic();
    let old_irql = unsafe { irql::raise_irql(DISPATCH_LEVEL) };
    let mut scheduler = scheduler::current_scheduler().lock();
    for k in scheduler.kthreads.iter_mut().flatten() {
        if k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }) {
            k.waiting_for = None;
            scheduler::Scheduler::make_thread_ready(k);
            crate::syscall::set_need_resched();
        }
    }
    drop(scheduler);
    unsafe { irql::lower_irql(old_irql) };
}

// ── Tests ──

pub fn register_kwait_tests() {
    use crate::{test_case, test_eq, test_ne, test_true};

    test_case!("kwait_magic_pipe_read", {
        let r = WaitReason::PipeRead { pipe_id: 3 };
        let m = r.encode_magic();
        let d = WaitReason::decode_magic(m).unwrap();
        test_eq!(d, r);
    });

    test_case!("kwait_magic_irp_complete", {
        let r = WaitReason::IrpComplete { irp_id: 42 };
        let m = r.encode_magic();
        let d = WaitReason::decode_magic(m).unwrap();
        test_eq!(d, r);
    });

    test_case!("kwait_magic_thread_join", {
        let r = WaitReason::ThreadJoin { tid: 7 };
        let m = r.encode_magic();
        let d = WaitReason::decode_magic(m).unwrap();
        test_eq!(d, r);
    });

    test_case!("kwait_magic_child_exit", {
        let r = WaitReason::ChildExit { pid: 1 };
        let m = r.encode_magic();
        let d = WaitReason::decode_magic(m).unwrap();
        test_eq!(d, r);
    });

    test_case!("kwait_magic_event", {
        let r = WaitReason::Event { event_type: 5 };
        let m = r.encode_magic();
        let d = WaitReason::decode_magic(m).unwrap();
        test_eq!(d, r);
    });

    test_case!("kwait_magic_alertable", {
        let r = WaitReason::Alertable;
        let m = r.encode_magic();
        let d = WaitReason::decode_magic(m).unwrap();
        test_eq!(d, r);
    });

    test_case!("kwait_magic_timer", {
        let r = WaitReason::Timer { timer_id: 5 };
        let m = r.encode_magic();
        let d = WaitReason::decode_magic(m).unwrap();
        test_eq!(d, r);
    });

    test_case!("kwait_magic_semaphore", {
        let r = WaitReason::Semaphore { sem_id: 3 };
        let m = r.encode_magic();
        let d = WaitReason::decode_magic(m).unwrap();
        test_eq!(d, r);
    });

    test_case!("kwait_magic_unique_tags", {
        let reasons = [
            WaitReason::PipeRead { pipe_id: 1 },
            WaitReason::IrpComplete { irp_id: 2 },
            WaitReason::ThreadJoin { tid: 3 },
            WaitReason::ChildExit { pid: 4 },
            WaitReason::Event { event_type: 5 },
            WaitReason::Alertable,
            WaitReason::Semaphore { sem_id: 1 },
        ];
        let mut magics = alloc::vec::Vec::new();
        for r in &reasons {
            let m = r.encode_magic() & 0xFFFF_0000;
            test_true!(!magics.contains(&m));
            magics.push(m);
        }
    });

    test_case!("kwait_decode_nonexistent", {
        let result = WaitReason::decode_magic(0xDEAD_0000);
        test_true!(result.is_none());
    });

    test_case!("kwait_same_instance_same_magic", {
        let a = WaitReason::PipeRead { pipe_id: 5 };
        let b = WaitReason::PipeRead { pipe_id: 5 };
        test_eq!(a.encode_magic(), b.encode_magic());
    });

    test_case!("kwait_different_instance_different_magic", {
        let a = WaitReason::PipeRead { pipe_id: 1 };
        let b = WaitReason::PipeRead { pipe_id: 2 };
        test_ne!(a.encode_magic(), b.encode_magic());
    });

    test_case!("kwait_timer_instance_magic", {
        let a = WaitReason::Timer { timer_id: 1 };
        let b = WaitReason::Timer { timer_id: 2 };
        test_ne!(a.encode_magic(), b.encode_magic());
        test_eq!(a.encode_magic() & 0xFFFF0000, MAGIC_TIMER_BASE);
    });

    test_case!("kwait_semaphore_instance_magic", {
        let a = WaitReason::Semaphore { sem_id: 5 };
        let b = WaitReason::Semaphore { sem_id: 5 };
        test_eq!(a.encode_magic(), b.encode_magic());
        let c = WaitReason::Semaphore { sem_id: 6 };
        test_ne!(a.encode_magic(), c.encode_magic());
    });

    // ── K17 Gap 1: real kwait_block/kwait_wake functional (global scheduler, BOOT_TID) ──
    test_case!("k17_kwait_block_wake_real_transitions", {
        // Save original BOOT_TID state
        let orig = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            let k = s.kthreads[0].as_ref().unwrap();
            (k.state, k.waiting_for, s.current_tid)
        });
        // Ensure current is BOOT_TID and Running
        crate::hal::without_interrupts(|| {
            let mut s = crate::scheduler::current_scheduler().lock();
            s.current_tid = crate::scheduler::BOOT_TID;
            if let Some(k) = s.kthreads[0].as_mut() {
                crate::scheduler::Scheduler::remove_from_run_queue(k);
                k.state = crate::scheduler::ThreadState::Running;
                k.waiting_for = None;
            }
        });
        let reason = WaitReason::Event { event_type: 77 };
        let magic = reason.encode_magic();
        crate::kwait::kwait_block(reason);
        let after_block = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            let k = s.kthreads[0].as_ref().unwrap();
            (k.state, k.waiting_for)
        });
        test_eq!(after_block.0, crate::scheduler::ThreadState::Blocked { waiting_for: magic });
        test_eq!(after_block.1, Some(magic));
        // First wake → Ready
        crate::kwait::kwait_wake(&reason);
        let after_wake = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            s.kthreads[0].as_ref().unwrap().state
        });
        test_eq!(after_wake, crate::scheduler::ThreadState::Ready);
        // Second wake → still Ready (or Running if scheduled), still no waiting_for, idempotent
        crate::kwait::kwait_wake(&reason);
        let after_second = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            let k = s.kthreads[0].as_ref().unwrap();
            (k.state, k.waiting_for)
        });
        // After second wake, thread must not be Blocked and waiting_for must be None.
        // It may be Ready or Running depending on scheduler activity between wakes.
        test_true!(after_second.0 == crate::scheduler::ThreadState::Ready || after_second.0 == crate::scheduler::ThreadState::Running);
        test_eq!(after_second.1, None);
        // Restore original state
        crate::hal::without_interrupts(|| {
            let mut s = crate::scheduler::current_scheduler().lock();
            if let Some(k) = s.kthreads[0].as_mut() {
                k.state = orig.0;
                k.waiting_for = orig.1;
                // BOOT_TID never in queue, ensure removed
                crate::scheduler::Scheduler::remove_from_run_queue(k);
                if orig.0 == crate::scheduler::ThreadState::Running {
                    // keep Running
                } else if orig.0 == crate::scheduler::ThreadState::Ready {
                    // shouldn't happen for boot at test time, but handle
                    k.state = crate::scheduler::ThreadState::Running;
                }
            }
            s.current_tid = orig.2;
        });
    });

    test_case!("k17_kwait_double_wake_real_idempotent_global", {
        let reason = WaitReason::Timer { timer_id: 42 };
        let magic = reason.encode_magic();
        // Save
        let orig = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            let k = s.kthreads[0].as_ref().unwrap();
            (k.state, k.waiting_for, s.current_tid)
        });
        crate::hal::without_interrupts(|| {
            let mut s = crate::scheduler::current_scheduler().lock();
            s.current_tid = crate::scheduler::BOOT_TID;
            if let Some(k) = s.kthreads[0].as_mut() {
                crate::scheduler::Scheduler::remove_from_run_queue(k);
                k.state = crate::scheduler::ThreadState::Running;
                k.waiting_for = None;
            }
        });
        crate::kwait::kwait_block(reason);
        crate::kwait::kwait_wake(&reason);
        let state1 = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            s.kthreads[0].as_ref().unwrap().state
        });
        test_eq!(state1, crate::scheduler::ThreadState::Ready);
        crate::kwait::kwait_wake(&reason);
        let state2 = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler().lock();
            s.kthreads[0].as_ref().unwrap().state
        });
        test_eq!(state2, crate::scheduler::ThreadState::Ready);
        // restore
        crate::hal::without_interrupts(|| {
            let mut s = crate::scheduler::current_scheduler().lock();
            if let Some(k) = s.kthreads[0].as_mut() {
                k.state = orig.0;
                k.waiting_for = orig.1;
                crate::scheduler::Scheduler::remove_from_run_queue(k);
                if orig.0 == crate::scheduler::ThreadState::Ready {
                    k.state = crate::scheduler::ThreadState::Running;
                }
            }
            s.current_tid = orig.2;
        });
        let _ = magic;
    });
}
