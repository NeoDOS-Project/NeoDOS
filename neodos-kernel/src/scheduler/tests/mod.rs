//! Scheduler tests — extracted from mod.rs (mechanical split)
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::scheduler::types::{Kthread, Eprocess, ThreadState, MmapRegion, KernelName, NAME_MAX, PRIORITY_HIGH, PRIORITY_NORMAL, PRIORITY_IDLE, PRIORITY_ABOVE_NORMAL, TIME_SLICES, IDLE_TID, BOOT_TID, MAX_STARVATION_TICKS, AGING_INTERVAL_TICKS, IDLE_TIME_SLICE, KERNEL_STACK_SIZE};
use crate::scheduler::Scheduler;
use crate::log::LogSubsys;


mod scheduling;
mod regressions;
mod mmap;
mod threads;
mod misc;

// Shared test helpers (module-level so the group files can call them).
pub(crate) fn add_test_thread(sched: &mut Scheduler, tid: u32, pid: u32, entry: u64, priority: u8, state: ThreadState) {
    let slot = sched.alloc_kthread_slot().unwrap();
    let mut k = Kthread::new_ring3(tid, pid, entry, 0x800000);
    k.state = state;
    k.priority = priority;
    k.time_slice_remaining = TIME_SLICES[priority as usize];
    sched.kthreads[slot] = Some(Box::new(k));
    if sched.find_eprocess(pid).is_none() {
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(pid, 0, 2, "\\", 0x10000000, 0));
    }
    if tid >= sched.next_tid {
        sched.next_tid = tid + 1;
    }
    let k = sched.kthreads.iter().flatten().find(|k| k.tid == tid).unwrap();
    Scheduler::remove_from_run_queue(k);
    if state == ThreadState::Ready {
        Scheduler::enqueue_to_cpu_run_queue(k);
    }
}

pub(crate) fn set_test_current(sched: &mut Scheduler, tid: u32) {
    let previous = sched.current_tid;
    if previous != tid {
        if let Some(k) = sched.find_kthread_mut(previous) {
            if k.state == ThreadState::Running {
                k.state = ThreadState::Blocked { waiting_for: 0 };
            }
        }
    }
    sched.current_tid = tid;
    let k = sched.find_kthread_mut(tid).unwrap();
    Scheduler::remove_from_run_queue(k);
    k.state = ThreadState::Running;
}

pub(crate) fn prepare_test_schedule(sched: &mut Scheduler) {
    if let Some(k) = sched.find_kthread_mut(sched.current_tid) {
        if k.state == ThreadState::Running {
            k.state = ThreadState::Blocked { waiting_for: 0 };
        }
    }
}

pub fn register_tests() {

    // Phase 15-A.1: CPU execution accounting units.
    crate::scheduler::accounting::register_tests();

    // ── Process tests ──






    // ── Scheduler priority tests ──














    // #501: the Ring-3 bootstrap hand-off must mark the *target* thread
    // Running by TID. Resolving via `current_kthread_mut()` is wrong: after
    // #482 the per-CPU KPRCB still points at the bootstrap thread until the
    // deferred publication, so NeoInit was left `Suspended` and never
    // scheduled again (the shell never started).

    // INV-10 (source-of-truth.md §INV-10): NeoInit (PID 1) must never be killed.






    // ── #375: single activation path (ObWait hand-off / Service Manager) ──


    // ── #354: the syscall-return fallback selects only Ready Ring-3 threads ──


    // ── #355: a starved Ring-0 kernel thread is reached via an idle hand-off ──


    // ── #382: FIFO fast path must not starve a higher-priority Ready thread ──

    // ── #376: a thread inside a preempt-disabled critical section (FS spinlock)
    //    must not be descheduled by the timer, even as a kernel thread. ──

    // ── Phase 15-A.1: CPU execution accounting ──
    //
    // The host/unit-test target has no KPRCB pages, so `cpu_time_now` and
    // cross-CPU reads resolve nothing; the scheduler-integration tests below
    // assert the *ownership/exclusion rules* (dispatch arms without crediting,
    // migration preserves the total, idle/blocked exclusion) while the pure
    // arithmetic is unit-tested with explicit bases in `accounting.rs`. This
    // keeps them deterministic and free of timing.













    // ── Mmap tests ──







    // ── Scheduler stress ──



    // ── Run queue invariant tests (P0-3) ──












    // ── P0-3 Priority scan regression tests ──











    // ── K17 Gap A/B: kwait_block / kwait_wake real path (isolated Scheduler) ──




    // ── K17 Gap 2: Terminated lifecycle & stale entry ──



    // ── K17 Gap 3: Same-priority round-robin sustained ──



    // ── K18 Gap 4: Work stealing / cross-CPU runqueue ──
    // Helpers for cross-CPU queue manipulation (avoid IPI side-effects in tests)
    // These tests deliberately use unsafe cpu_run_queue_mut and direct state
    // mutation to isolate the work-stealing contract without modifying production.







    // ── K19: destination-full and repeated-steal evidence ──



    // ═══════════════════════════════════════════════════════════════════
    // #338: Ring-0 preemption of a user thread must never publish a
    // non-Ring-3 dispatch frame as `Ready`.
    //
    // A user thread interrupted inside a syscall runs on its Ring-0 kernel
    // stack (`cs == 0x08`). The invariant is:
    //
    //     Ready + saved dispatch frame  =>  cs & 3 == 3
    //
    // Tests A/B/C below are deterministic and host-independent: they exercise
    // the predicates and transitions the timer/resched paths use, without
    // depending on IRQ timing.
    // ═══════════════════════════════════════════════════════════════════

    // ── Test A: a Ring-3 frame is dispatchable; a Ring-0 frame is not, unless
    //            the thread is a genuine kernel thread (netd regression) ──

    // ── Test A2: a Ring-0-framed Ready user thread is never SELECTED ──
    // This pins the exact failure mode: `schedule_with(require_ring3=true)`
    // rejects the frame, so the thread must never be committed `Running`.

    // ── Test B: a user thread preempted in a syscall is re-published with a
    //            Ring-3 frame and becomes schedulable again (progress) ──

    // ── Test C: a long-lived daemon pattern (repeated yield/preempt cycles)
    //            keeps a user thread dispatchable across many iterations ──
    // Mirrors `netapplier`/`dhcpd`: a Ring-3 thread alternating between executing
    // on its Ring-3 frame and being preempted inside a syscall (Ring-0 frame),
    // then restored by the syscall return. The invariant must hold at every
    // step and the thread must be dispatchable after each cycle.

    // ── Test D: on_timer_tick only expires-to-Ready on a Ring-3 interrupt ──
    // This is the #338 regression at its origin: a user thread whose timeslice
    // expires while the CPU is in Ring 0 (inside a syscall) must stay Running;
    // the same expiry in Ring 3 must publish it Ready.
    scheduling::register();
    regressions::register();
    mmap::register();
    threads::register();
    misc::register();
}
