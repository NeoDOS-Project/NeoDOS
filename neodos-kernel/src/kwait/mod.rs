// ── Unified Wait Engine (KWait)
//
// Replaces all ad-hoc blocking/wake mechanisms with a single abstraction.
// Every blocking operation (pipe read, IRP complete, thread join, child exit,
// event wait, timer) goes through KWait.
//
// ABI frozen at v0.42 — WaitReason variants and magic encoding must not change.

use crate::scheduler::{self, ThreadState};
use crate::hal::irql::{self, DISPATCH_LEVEL};

/// Magic number base for each wait reason type — F-03: full-width 64-bit.
/// High 32 bits encode the reason tag, low 32 bits carry the full instance ID
/// (no 16-bit truncation). This eliminates ABA collisions like PID 1 vs 65537.
const TAG_PIPE: u32      = 0x0001;
const TAG_IRP: u32       = 0x0002;
const TAG_THREAD: u32    = 0x0003;
const TAG_CHILD: u32     = 0x0004;
const TAG_EVENT: u32     = 0x0005;
const TAG_TIMER: u32     = 0x0006;
const TAG_APC: u32       = 0x0007;
const TAG_SEMAPHORE: u32 = 0x0008;
const TAG_SOCKET_READ: u32    = 0x0009;
const TAG_SOCKET_CONNECT: u32 = 0x000A;
const TAG_SOCKET_ACCEPT: u32  = 0x000B;

#[inline(always)]
fn make_magic(tag: u32, id: u32) -> u64 {
    ((tag as u64) << 32) | (id as u64)
}
#[inline(always)]
fn magic_tag(magic: u64) -> u32 { (magic >> 32) as u32 }
#[inline(always)]
fn magic_id(magic: u64) -> u32 { (magic & 0xFFFF_FFFF) as u32 }

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
    /// Encode this wait reason into a magic u64 for the scheduler — F-03 full-width.
    /// High 32: tag, Low 32: full instance ID (no truncation).
    pub fn encode_magic(&self) -> u64 {
        match *self {
            WaitReason::PipeRead { pipe_id }  => make_magic(TAG_PIPE, pipe_id as u32),
            WaitReason::IrpComplete { irp_id } => make_magic(TAG_IRP, irp_id),
            WaitReason::ThreadJoin { tid }     => make_magic(TAG_THREAD, tid),
            WaitReason::ChildExit { pid }      => make_magic(TAG_CHILD, pid),
            WaitReason::Event { event_type }   => make_magic(TAG_EVENT, event_type),
            WaitReason::Timer { timer_id }      => make_magic(TAG_TIMER, timer_id),
            WaitReason::Alertable              => make_magic(TAG_APC, 0),
            WaitReason::Semaphore { sem_id }   => make_magic(TAG_SEMAPHORE, sem_id),
            WaitReason::SocketRead { socket_id }   => make_magic(TAG_SOCKET_READ, socket_id),
            WaitReason::SocketConnect { socket_id } => make_magic(TAG_SOCKET_CONNECT, socket_id),
            WaitReason::SocketAccept { socket_id }  => make_magic(TAG_SOCKET_ACCEPT, socket_id),
        }
    }

    pub fn decode_magic(magic: u64) -> Option<WaitReason> {
        let tag = magic_tag(magic);
        let id = magic_id(magic);
        Some(match tag {
            TAG_PIPE      => WaitReason::PipeRead { pipe_id: id as u16 },
            TAG_IRP       => WaitReason::IrpComplete { irp_id: id },
            TAG_THREAD    => WaitReason::ThreadJoin { tid: id },
            TAG_CHILD     => WaitReason::ChildExit { pid: id },
            TAG_TIMER     => WaitReason::Timer { timer_id: id },
            TAG_EVENT     => WaitReason::Event { event_type: id },
            TAG_APC       => WaitReason::Alertable,
            TAG_SEMAPHORE => WaitReason::Semaphore { sem_id: id },
            TAG_SOCKET_READ    => WaitReason::SocketRead { socket_id: id },
            TAG_SOCKET_CONNECT => WaitReason::SocketConnect { socket_id: id },
            TAG_SOCKET_ACCEPT  => WaitReason::SocketAccept { socket_id: id },
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
            let m = r.encode_magic() & 0xFFFF_FFFF_0000_0000;
            test_true!(!magics.contains(&m));
            magics.push(m);
        }
    });

    test_case!("kwait_decode_nonexistent", {
        let result = WaitReason::decode_magic(0xDEAD_0000_0000_0000);
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
        test_eq!(a.encode_magic() & 0xFFFF_FFFF_0000_0000, make_magic(TAG_TIMER, 0) & 0xFFFF_FFFF_0000_0000);
    });

    test_case!("kwait_semaphore_instance_magic", {
        let a = WaitReason::Semaphore { sem_id: 5 };
        let b = WaitReason::Semaphore { sem_id: 5 };
        test_eq!(a.encode_magic(), b.encode_magic());
        let c = WaitReason::Semaphore { sem_id: 6 };
        test_ne!(a.encode_magic(), c.encode_magic());
    });

    // F-03: ABA collision must not happen — PID 1 vs 65537 (0x1_0001) previously collided on 16-bit
    test_case!("kwait_aba_pid_full_width", {
        let a = WaitReason::ChildExit { pid: 1 };
        let b = WaitReason::ChildExit { pid: 65537 }; // 1 + 0x10000
        test_ne!(a.encode_magic(), b.encode_magic());
        let c = WaitReason::ThreadJoin { tid: 1 };
        let d = WaitReason::ThreadJoin { tid: 65537 };
        test_ne!(c.encode_magic(), d.encode_magic());
        // Large PID near u32::MAX
        let e = WaitReason::ChildExit { pid: 0xFFFF_FFFE };
        let f = WaitReason::ChildExit { pid: 0xFFFF_FFFF };
        test_ne!(e.encode_magic(), f.encode_magic());
        test_eq!(WaitReason::decode_magic(a.encode_magic()).unwrap(), a);
        test_eq!(WaitReason::decode_magic(b.encode_magic()).unwrap(), b);
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
        // Second wake is idempotent — should remain Ready or may become Running if scheduled
        test_true!(state2 == crate::scheduler::ThreadState::Ready || state2 == crate::scheduler::ThreadState::Running);
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
