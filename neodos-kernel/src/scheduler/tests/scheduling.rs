#![allow(unused_imports)]
//! Scheduler tests — extracted from mod.rs (mechanical split)
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::scheduler::types::{Kthread, Eprocess, ThreadState, MmapRegion, KernelName, NAME_MAX, PRIORITY_HIGH, PRIORITY_NORMAL, PRIORITY_IDLE, PRIORITY_ABOVE_NORMAL, TIME_SLICES, IDLE_TID, BOOT_TID, MAX_STARVATION_TICKS, AGING_INTERVAL_TICKS, IDLE_TIME_SLICE, KERNEL_STACK_SIZE};
use crate::scheduler::Scheduler;
use crate::log::LogSubsys;


use super::{add_test_thread, prepare_test_schedule, set_test_current};
pub fn register() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;
    use crate::test_true;
    test_case!("sched_yield_intent_not_published_until_ready", {
        // Phase 13-A regression contract: a running thread that asks to yield
        // stays Running with `yield_requested` set; it is only published as
        // Ready (enqueued) by the switch-out/wake path, which also consumes
        // the flag. Publishing a still-running thread with a stale `rsp` is
        // what let two CPUs run the same KTHREAD on one kernel stack.
        // BOOT_TID/idle is used so the transition does not touch any runqueue.
        let mut k = Kthread::new_idle(BOOT_TID, 0, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.yield_requested = true;
        test_eq!(k.state, ThreadState::Running);
        test_true!(k.yield_requested);
        Scheduler::make_thread_ready(&mut k);
        test_eq!(k.state, ThreadState::Ready);
        test_eq!(k.yield_requested, false);
    });
    test_case!("sched_keep_current_recovery_identity", {
        // #346 regression: a user thread whose timeslice expires inside a
        // syscall is published `Ready` with a Ring-0 frame (cs=0x08). On the
        // syscall return, `schedule_with(require_ring3=true)` rejects it and
        // falls through to the idle Kthread. The recovery must keep the current
        // thread Running, out of the run queue, and restore the scheduler/CPU
        // identity to it. The missing restore left the CPU's KPRCB pointing at
        // the idle Kthread - the writer that corrupted TID=1's frame.
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        set_test_current(&mut sched, 3);
        // Make the saved dispatch frame Ring 0 (interrupted inside a syscall).
        {
            let k = sched.find_kthread_mut(3).unwrap();
            let cs_slot = (k.rsp + 15 * 8 + 8) as *mut u64;
            unsafe { core::ptr::write_volatile(cs_slot, 0x08); }
        }
        // Publish Ready as `on_timer_tick` does on timeslice expiry.
        {
            let k = sched.find_kthread_mut(3).unwrap();
            k.state = ThreadState::Ready;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        // require_ring3 must not commit the non-Ring3 candidate.
        let next = sched.schedule_with(true);
        test_ne!(unsafe { (*next).tid }, 3);
        // The caller rejects `next` and resumes thread 3.
        let info = sched.resume_current_after_rejected_dispatch(3);
        test_true!(info.is_some());
        let (ptr, pid, ks_top) = info.unwrap();
        test_true!(!ptr.is_null());
        test_eq!(pid, 2);
        test_true!(ks_top != 0);
        let k = sched.find_kthread(3).unwrap();
        test_eq!(k.state, ThreadState::Running);
        test_eq!(sched.current_tid, 3);
        let queued = crate::arch::x64::cpu_local::with_runqueue(k.cpu as usize, |rq| rq.contains(3));
        test_eq!(queued, false);
    });
    test_case!("sched_priority_high_picked_first", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        let next = sched.schedule();
        let picked_tid = unsafe { (*next).tid };
        test_eq!(picked_tid, 2);
    });
    test_case!("sched_priority_round_robin_same_level", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        let first = sched.schedule();
        let first_tid = unsafe { (*first).tid };
        test_ne!(first_tid, 0);
        let second = sched.schedule();
        let second_tid = unsafe { (*second).tid };
        test_ne!(second_tid, first_tid);
    });
    test_case!("sched_priority_idle_last", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 2);
    });
    test_case!("sched_time_slice_default_values", {
        let k = Kthread::new_ring3(1, 1, 0x400000, 0x800000);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_NORMAL as usize]);
        test_eq!(k.priority, PRIORITY_NORMAL);
    });
    test_case!("sched_on_timer_tick_decrements_slice", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;  // skip TID 0 (boot) and TID 1 (idle)
        sched.current_tid = 3;
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(3, 2, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.time_slice_remaining = 5;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(Box::new(k));
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        sched.on_timer_tick(0x700000, 0x1B); // Ring-3 interrupt (timer preempts user code)
        let remaining = sched.kthreads[slot].as_ref().unwrap().time_slice_remaining;
        test_eq!(remaining, 4);
    });
    test_case!("sched_on_timer_tick_expire_yields", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;  // skip TID 0 (boot) and TID 1 (idle)
        sched.current_tid = 3;
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(3, 2, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.time_slice_remaining = 1;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(Box::new(k));
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        sched.on_timer_tick(0x700000, 0x1B); // Ring-3 interrupt (timer preempts user code)
        let state = sched.kthreads[slot].as_ref().unwrap().state;
        test_eq!(state, ThreadState::Ready);
    });
    test_case!("sched_activate_suspended_process_idempotent", {
        let mut sched = Scheduler::new();
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Suspended);

        // First activation publishes the initial thread Ready.
        test_true!(sched.activate_suspended_process(2));
        test_eq!(sched.find_kthread(3).unwrap().state, ThreadState::Ready);
        // Second activation must not re-publish (no double Ready / double enqueue).
        test_true!(!sched.activate_suspended_process(2));
        test_eq!(sched.find_kthread(3).unwrap().state, ThreadState::Ready);
        // Unknown pid is a no-op.
        test_true!(!sched.activate_suspended_process(99));
    });
    test_case!("sched_cpu_time_dispatch_arms_non_idle", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 3, 3, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 3);
        let k = sched.find_kthread(3).unwrap();
        // Dispatch must not credit any execution on its own: it only arms the
        // base against the current per-CPU clock (or parks with the sentinel if
        // no clock exists yet, as on the host target).
        test_eq!(k.cpu_time, 0);
        match crate::scheduler::accounting::per_cpu_tick_base() {
            Some(b) => test_eq!(k.cpu_time_base, b),
            None => test_eq!(k.cpu_time_base, Kthread::CPU_TIME_UNSET),
        }
    });
    test_case!("sched_cpu_time_tick_accumulates_when_armed", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 3;
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(3, 2, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.time_slice_remaining = 50;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(Box::new(k));
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        // Without a per-CPU base the tick is a no-op; the counter stays put.
        sched.on_timer_tick(0x700000, 0x1B); // Ring-3 interrupt (timer preempts user code)
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.cpu_time, 0);
        // cpu_ticks (the legacy tick count) still advances.
        test_eq!(k.cpu_ticks, 1);
    });
    test_case!("sched_cpu_time_migration_preserves_counter", {
        // A thread that ran on CPU0 keeps its accumulated total when it is
        // re-dispatched on CPU1: accounting follows execution, never the CPU.
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 3, 3, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        {
            let k = sched.find_kthread_mut(3).unwrap();
            k.cpu_time = 7; // pretend 7 intervals ran on CPU0
            k.cpu_time_base = Kthread::CPU_TIME_UNSET;
        }
        // Re-dispatch arms a fresh base but must not reset the total.
        let next = sched.schedule();
        test_eq!(unsafe { (*next).tid }, 3);
        let k = sched.find_kthread(3).unwrap();
        test_eq!(k.cpu_time, 7);
    });
    test_case!("sched_cpu_time_blocked_thread_not_charged", {
        // A Blocked thread is never the current thread, so `on_timer_tick`
        // cannot charge it: the transition Running -> Blocked stops the clock.
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 2;
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Blocked { waiting_for: 1 });
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(3, 3, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(Box::new(k));
        sched.on_timer_tick(0x700000, 0x1B); // Ring-3 interrupt (timer preempts user code)
        let blocked = sched.find_kthread(2).unwrap();
        test_eq!(blocked.cpu_time, 0);
        test_eq!(blocked.cpu_ticks, 0);
    });
    test_case!("sched_cpu_time_idle_excluded", {
        // The idle thread must never accumulate CPU time.
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 3;
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_idle(3, 0, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.cpu = 0;
        sched.kthreads[slot] = Some(Box::new(k));
        for _ in 0..10 {
            sched.on_timer_tick(0x700000, 0x1B); // Ring-3 interrupt (timer preempts user code)
        }
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.cpu_time, 0);
    });
    test_case!("sched_snapshot_process_cpu_time_sums_threads", {
        // Process CPU time is derived: it is the sum of its threads' counters,
        // and idle threads contribute nothing.
        let mut sched = Scheduler::new();
        sched.next_tid = 10;
        // Two threads of PID 5.
        add_test_thread(&mut sched, 5, 5, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 6, 5, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        {
            let a = sched.find_kthread_mut(5).unwrap();
            a.cpu_time = 300;
            a.cpu_time_base = Kthread::CPU_TIME_UNSET;
            let b = sched.find_kthread_mut(6).unwrap();
            b.cpu_time = 700;
            b.cpu_time_base = Kthread::CPU_TIME_UNSET;
        }
        let mut snap = crate::scheduler::ProcSnapshot::empty();
        sched.snapshot_into(&mut snap);
        let p = snap.process(5).expect("pid 5 present");
        test_eq!(p.cpu_time, 1000);
        let a = snap.thread(5).unwrap();
        let b = snap.thread(6).unwrap();
        test_eq!(a.cpu_time, 300);
        test_eq!(b.cpu_time, 700);
    });
    test_case!("sched_cpu_time_monotonic_under_repeated_ticks", {
        // The counter is monotonic: it only grows across ticks. (Arithmetic is
        // exercised here with a manual base; the base logic in `accounting.rs`
        // is unit-tested with explicit higher bases.)
        let mut k = Kthread::new_ring3(9, 9, 0x400000, 0x800000);
        k.cpu_time = 0;
        k.cpu_time_base = Kthread::CPU_TIME_UNSET;
        let mut last = 0u64;
        for _ in 0..5 {
            k.cpu_time = k.cpu_time.saturating_add(1);
            test_true!(k.cpu_time >= last);
            last = k.cpu_time;
        }
        test_eq!(k.cpu_time, 5);
    });
    test_case!("sched_aging_boosts_starved", {        let mut sched = Scheduler::new();
        sched.next_tid = 4;  // skip TID 0 (boot) and TID 1 (idle)
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(3, 2, 0x400000, 0x800000);
        k.state = ThreadState::Ready;
        k.priority = PRIORITY_IDLE;
        k.ticks_since_scheduled = MAX_STARVATION_TICKS + 1;
        k.time_slice_remaining = 50;
        sched.kthreads[slot] = Some(Box::new(k));
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        for _ in 0..AGING_INTERVAL_TICKS + 5 {
            sched.on_timer_tick(0x700000, 0x1B); // Ring-3 interrupt (timer preempts user code)
        }
        let boosted = sched.kthreads[slot].as_ref().unwrap();
        test_true!(boosted.priority < PRIORITY_IDLE);
    });
    test_case!("sched_set_process_priority", {
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(1, 1, 0x400000, 0x800000);
        k.state = ThreadState::Ready;
        sched.kthreads[slot] = Some(Box::new(k));
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(1, 0, 2, "\\", 0x10000000, 0));
        test_true!(sched.set_process_priority(1, PRIORITY_HIGH));
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.priority, PRIORITY_HIGH);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_HIGH as usize]);
        test_true!(sched.set_process_priority(1, PRIORITY_IDLE));
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.priority, PRIORITY_IDLE);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_IDLE as usize]);
        test_true!(!sched.set_process_priority(1, 99));
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.priority, PRIORITY_IDLE);
        test_true!(!sched.set_process_priority(999, PRIORITY_HIGH));
    });
    test_case!("sched_priority_preempt_higher_ready", {
        let mut sched = Scheduler::new();
        // Use TIDs 5,6,7 to avoid colliding with reserved BOOT_TID=0 and IDLE_TID=1
        // and with existing test TIDs 2,3. next_tid must be > max used.
        sched.next_tid = 8;
        // Create high priority Ready thread that should preempt current
        add_test_thread(&mut sched, 5, 5, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        // Create current Running thread (normal priority)
        add_test_thread(&mut sched, 6, 6, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        set_test_current(&mut sched, 6);
        // Create idle priority Ready thread
        add_test_thread(&mut sched, 7, 7, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 5);
    });
    test_case!("sched_priority_blocked_ignored", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 2;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Blocked { waiting_for: 99 });
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        add_test_thread(&mut sched, 3, 3, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 3);
    });
    test_case!("sched_priority_unblock_picks_higher", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        sched.current_tid = 2;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Blocked { waiting_for: 0xFFFF_0000u64 });
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_IDLE, ThreadState::Running);
        sched.kthreads.iter_mut().find(|t| t.as_ref().is_some_and(|k| k.tid == 1))
            .and_then(|t| t.as_mut()).unwrap().state = ThreadState::Ready;
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 1);
    });
    test_case!("rq_invariant_enqueue_once", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });
    test_case!("rq_invariant_ready_to_blocked", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Blocked { waiting_for: 99 };
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });
    test_case!("rq_invariant_blocked_to_ready", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: 0x0005_0000_0001u64 });
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });
    test_case!("rq_invariant_double_wake", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: 0x0005_0000_0001u64 });
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            k.waiting_for = None;
            Scheduler::make_thread_ready(k);
        }
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });
    test_case!("rq_invariant_suspended_to_ready", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Suspended);
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });
    test_case!("rq_invariant_running_no_entry", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        set_test_current(&mut sched, 2);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });
    test_case!("rq_invariant_terminated_no_entry", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Terminated);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });
    test_case!("rq_invariant_stress_mixed_transitions", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        for _ in 0..1000 {
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                k.state = ThreadState::Running;
            }
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                Scheduler::make_thread_ready(k);
            }
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                Scheduler::remove_from_run_queue(k);
                k.state = ThreadState::Blocked { waiting_for: 99 };
            }
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                Scheduler::make_thread_ready(k);
            }
        }
        set_test_current(&mut sched, IDLE_TID);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });
    test_case!("rq_invariant_multi_thread", {
        let mut sched = Scheduler::new();
        sched.next_tid = 6;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 4, 3, 0x400000, PRIORITY_NORMAL, ThreadState::Blocked { waiting_for: 42 });
        add_test_thread(&mut sched, 5, 4, 0x400000, PRIORITY_IDLE, ThreadState::Running);
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Running;
        }
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 3).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Blocked { waiting_for: 43 };
        }
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 4).unwrap();
            Scheduler::make_thread_ready(k);
        }
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 5).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        set_test_current(&mut sched, 2);
        let result = sched.validate_runqueue_invariants();
        match result {
            Ok(count) => test_eq!(count, 1),
            Err(msg) => {
                crate::serial_println!("rq_invariant_multi_thread: {}", msg);
                return Err(msg);
            }
        }
    });
    test_case!("rq_invariant_cpu_runqueue_remove", {
        use crate::arch::x64::cpu_local::CpuRunQueue;
        let mut rq = CpuRunQueue::new();
        rq.push(10);
        rq.push(20);
        rq.push(30);
        test_eq!(rq.len(), 3);
        test_true!(rq.contains(20));
        test_true!(rq.remove(20));
        test_eq!(rq.len(), 2);
        test_true!(!rq.contains(20));
        test_true!(rq.contains(10));
        test_true!(rq.contains(30));
        test_true!(rq.remove(10));
        test_eq!(rq.len(), 1);
        test_true!(rq.contains(30));
        test_true!(rq.remove(30));
        test_eq!(rq.len(), 0);
        test_true!(!rq.contains(30));
        test_true!(!rq.remove(99));
    });
    test_case!("rq_invariant_full_regression", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        let result = sched.validate_runqueue_invariants();
        if let Err(msg) = result {
            crate::serial_println!("rq_invariant_full_regression: {}", msg);
            return Err(msg);
        }

        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked).unwrap();
            Scheduler::make_thread_ready(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        let result = sched.validate_runqueue_invariants();

        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Blocked { waiting_for: 99 };
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());

        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked).unwrap();
            k.waiting_for = None;
            Scheduler::make_thread_ready(k);
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());

        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
    });
    test_case!("rq_priority_scan_removes_from_runqueue", {
        // Test: Priority scan must remove thread from runqueue before setting to Running.
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 0;
        // Add a high-priority thread (TID 2) and a normal thread (TID 1).
        // TID 0 is boot, TID 1 is idle, so we start from TID 2.
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        // Verify initial invariants
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 2); // 2 Ready threads in runqueue

        // Schedule — should pick TID 2 (high priority) via priority scan
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked_tid = unsafe { (*next).tid };
        test_eq!(picked_tid, 2);

        // Verify invariant: Running thread must have 0 runqueue entries
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1); // Only TID 3 remains in runqueue
    });
    test_case!("rq_priority_scan_stress_100_iterations", {
        // Stress test: Repeat priority scan 100 times, verify invariant each time.
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        for i in 0..100 {
            // Make both threads Ready again (remove before enqueue to avoid FIFO ordering bug from leftover)
            {
                let k2 = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                Scheduler::remove_from_run_queue(k2);
                k2.state = ThreadState::Ready;
                Scheduler::enqueue_to_cpu_run_queue(k2);
            }
            {
                let k3 = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 3).unwrap();
                Scheduler::remove_from_run_queue(k3);
                k3.state = ThreadState::Ready;
                Scheduler::enqueue_to_cpu_run_queue(k3);
            }

            // Schedule — should pick TID 2 (high priority)
            prepare_test_schedule(&mut sched);
            let next = sched.schedule();
            let picked_tid = unsafe { (*next).tid };
            test_eq!(picked_tid, 2);

            // Verify invariant: Running thread must have 0 runqueue entries
            let result = sched.validate_runqueue_invariants();
            test_true!(result.is_ok());
            let _ = i; // suppress unused warning
        }
    });
    test_case!("rq_priority_scan_return_to_ready", {
        // Test: After priority scan selects X, X can yield back to Ready,
        // then be scheduled again via priority scan with invariant preserved.
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        // Schedule TID 2
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked_tid = unsafe { (*next).tid };
        test_eq!(picked_tid, 2);

        // Verify invariant: Running => runqueue_count == 0
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());

        // Simulate yield: Running -> Ready (enqueue)
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        set_test_current(&mut sched, IDLE_TID);

        // Verify invariant: Ready => runqueue_count == 1
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);

        // Schedule again
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked_tid2 = unsafe { (*next).tid };
        test_eq!(picked_tid2, 2);

        // Verify invariant: Running => runqueue_count == 0
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });
    test_case!("rq_priority_scan_multiple_threads", {
        // Test: Multiple threads with different priorities, verify invariant
        // holds for all threads after each schedule.
        let mut sched = Scheduler::new();
        sched.next_tid = 6;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 4, 3, 0x400000, PRIORITY_ABOVE_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 5, 4, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        // Schedule 4 times, each time verify invariants
        for _ in 0..4 {
            prepare_test_schedule(&mut sched);
            let next = sched.schedule();
            let picked_tid = unsafe { (*next).tid };

            // Verify invariant
            let result = sched.validate_runqueue_invariants();
            test_true!(result.is_ok());

            // Mark as Ready again for next iteration
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked_tid).unwrap();
                Scheduler::make_thread_ready(k);
            }
        }
    });
    test_case!("rq_priority_scan_duplicate_protection", {
        // Test: Fix doesn't break duplicate enqueue protection.
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);

        // Try to enqueue twice
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::enqueue_to_cpu_run_queue(k);
        }

        // Verify only 1 entry
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });
    test_case!("rq_priority_scan_idle_thread_no_runqueue", {
        // Test: Idle thread is never in runqueue by design.
        // Verify it can be scheduled via priority scan without issues.
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        sched.current_tid = 0;

        // The idle thread (TID 1) is created in Scheduler::new() with state=Ready
        // but is NOT in any runqueue by design.
        // Schedule — should fall back to idle thread
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked_tid = unsafe { (*next).tid };
        test_eq!(picked_tid, 1); // IDLE_TID

        // Verify invariant: idle thread (Running) must have 0 runqueue entries
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });
    test_case!("rq_timer_expiration_requeues_once", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        set_test_current(&mut sched, 2);
        sched.kthreads.iter_mut().flatten()
            .find(|k| k.tid == 2).unwrap().time_slice_remaining = 1;

        sched.on_timer_tick(0x700000, 0x1B); // Ring-3 interrupt (timer preempts user code)
        let k = sched.find_kthread(2).unwrap();
        test_eq!(k.state, ThreadState::Ready);
        test_eq!(k.rsp, 0x700000);
        unsafe {
            let rq = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            test_eq!(rq.len(), 1);
            test_true!(rq.contains(2));
        }
        set_test_current(&mut sched, IDLE_TID);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });
    test_case!("rq_spawn_kthread_enqueues_once", {
        let mut sched = Scheduler::new();
        // Phase 14-B: minimal/empty-state snapshot (boot + idle only).
        {
            let fresh = Scheduler::new();
            let mut s0 = alloc::boxed::Box::new(crate::scheduler::ProcSnapshot::empty());
            fresh.snapshot_into(&mut s0);
            test_true!(s0.process_count >= 1);
            test_true!(s0.thread_count >= 2);
            test_eq!(s0.truncated, false);
        }
        sched.next_tid = 2;
        let tid = sched.spawn_kthread(0x400000, PRIORITY_NORMAL).unwrap();
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(tid));
        }
        // Phase 14-A: default kernel-thread name is preserved; PID/TID unchanged.
        test_eq!(sched.find_kthread(tid).unwrap().name(), "kthread");
        test_eq!(sched.find_kthread(tid).unwrap().tid, tid);
        // Named variant applies the supplied bounded name and keeps TIDs distinct.
        let named = sched.spawn_kthread_named(0x400100, PRIORITY_NORMAL, "worker").unwrap();
        test_ne!(named, tid);
        test_eq!(sched.find_kthread(named).unwrap().name(), "worker");

        // Phase 14-B: scheduler-consistent snapshot over the logical registry.
        let mut snap = alloc::boxed::Box::new(crate::scheduler::ProcSnapshot::empty());
        sched.snapshot_into(&mut snap);
        // Process enumeration: boot (pid 0) + two spawned kernel processes.
        test_true!(snap.process_count >= 3);
        test_true!(snap.process(0).is_some());
        // Thread enumeration + Phase 14-A names.
        let t_main = snap.thread(tid).unwrap();
        test_eq!(t_main.name.as_str(), "kthread");
        test_eq!(t_main.tid, tid);
        let t_named = snap.thread(named).unwrap();
        test_eq!(t_named.name.as_str(), "worker");
        // Ownership: every thread maps to an enumerated process.
        for t in snap.threads[..snap.thread_count].iter() {
            test_true!(snap.process(t.pid).is_some());
        }
        // Idle remains a distinct, flagged thread (not collapsed).
        test_true!(snap.threads[..snap.thread_count].iter().any(|t| t.idle));
        // Deterministic ordering: processes by pid, threads by tid.
        let mut ordered = true;
        for w in snap.processes[..snap.process_count].windows(2) {
            if w[0].pid > w[1].pid { ordered = false; }
        }
        for w in snap.threads[..snap.thread_count].windows(2) {
            if w[0].tid > w[1].tid { ordered = false; }
        }
        test_true!(ordered);
        // Snapshot lifetime: detached from live state, repeatable for stable state.
        let mut snap2 = alloc::boxed::Box::new(crate::scheduler::ProcSnapshot::empty());
        sched.snapshot_into(&mut snap2);
        test_eq!(snap.thread_count, snap2.thread_count);
        test_true!(snap.threads[..snap.thread_count] == snap2.threads[..snap2.thread_count]);
        let mut snap3 = alloc::boxed::Box::new(crate::scheduler::ProcSnapshot::empty());
        sched.snapshot_into(&mut snap3);
        test_true!(snap.threads[..snap.thread_count] == snap3.threads[..snap3.thread_count]);
    });
    test_case!("rq_add_thread_enqueues_once", {
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(42, 0, 2, "\\", 0x10000000, 0));
        let tid = sched.add_thread_to_process(42, 0x400000, 0x800000).unwrap();
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(tid));
        }
        // Phase 14-A: additional threads get the documented default name.
        test_eq!(sched.find_kthread(tid).unwrap().name(), "thread");
    });
    test_case!("rq_validator_rejects_invalid_current", {
        let mut sched = Scheduler::new();
        sched.current_tid = 999;
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_err());
    });
}
