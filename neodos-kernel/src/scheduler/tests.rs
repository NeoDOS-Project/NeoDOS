//! Scheduler tests — extracted from mod.rs (mechanical split)
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::scheduler::types::{Kthread, Eprocess, ThreadState, MmapRegion, PRIORITY_HIGH, PRIORITY_NORMAL, PRIORITY_IDLE, PRIORITY_ABOVE_NORMAL, TIME_SLICES, IDLE_TID, BOOT_TID, MAX_STARVATION_TICKS, AGING_INTERVAL_TICKS, IDLE_TIME_SLICE};
use crate::scheduler::Scheduler;
use crate::log::LogSubsys;

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;
    use crate::test_true;

    // ── Process tests ──

    test_case!("kthread_new_initial_state", {
        let k = Kthread::new_idle(1, 0, 0x400000, 0x800000);
        test_eq!(k.tid, 1);
        test_eq!(k.rip, 0x400000);
        test_eq!(k.state, ThreadState::Ready);
        test_eq!(k.cpu_ticks, 0);
        test_eq!(k.pid, 0);
        test_eq!(k.priority, PRIORITY_IDLE);
        test_eq!(k.time_slice_remaining, IDLE_TIME_SLICE);
    });

    test_case!("kthread_state_debug", {
        let mut k = Kthread::new_idle(1, 0, 0x400000, 0x800000);
        test_eq!(k.state, ThreadState::Ready);
        k.state = ThreadState::Running;
        test_eq!(k.state, ThreadState::Running);
        k.state = ThreadState::Blocked { waiting_for: 42 };
        test_eq!(k.state, ThreadState::Blocked { waiting_for: 42 });
        k.state = ThreadState::Terminated;
        test_eq!(k.state, ThreadState::Terminated);
    });

    test_case!("kthread_state_partial_eq", {
        let s1 = ThreadState::Ready;
        let s2 = ThreadState::Ready;
        test_eq!(s1, s2);
        test_ne!(ThreadState::Ready, ThreadState::Running);
        test_ne!(ThreadState::Blocked { waiting_for: 1 }, ThreadState::Blocked { waiting_for: 2 });
    });

    test_case!("eprocess_new_ring3", {
        let ep = Eprocess::new_ring3(42, 1, 2, "\\", 0x10000000, 0);
        test_eq!(ep.pid, 42);
        test_eq!(ep.heap_base, 0x10000000);
        test_eq!(ep.heap_break, 0x10000000);
        test_eq!(ep.thread_count, 1);
        test_eq!(ep.cwd_drive, 2);
    });

    // ── Scheduler priority tests ──

    fn add_test_thread(sched: &mut Scheduler, tid: u32, pid: u32, entry: u64, priority: u8, state: ThreadState) {
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

    fn set_test_current(sched: &mut Scheduler, tid: u32) {
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

    fn prepare_test_schedule(sched: &mut Scheduler) {
        if let Some(k) = sched.find_kthread_mut(sched.current_tid) {
            if k.state == ThreadState::Running {
                k.state = ThreadState::Blocked { waiting_for: 0 };
            }
        }
    }

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
        sched.on_timer_tick(0x700000);
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
        sched.on_timer_tick(0x700000);
        let state = sched.kthreads[slot].as_ref().unwrap().state;
        test_eq!(state, ThreadState::Ready);
    });

    test_case!("sched_aging_boosts_starved", {
        let mut sched = Scheduler::new();
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
            sched.on_timer_tick(0x700000);
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
        sched.next_tid = 4;
        sched.current_tid = 2;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        add_test_thread(&mut sched, 3, 3, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 1);
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
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Blocked { waiting_for: 0xFFFF_0000 });
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_IDLE, ThreadState::Running);
        sched.kthreads.iter_mut().find(|t| t.as_ref().is_some_and(|k| k.tid == 1))
            .and_then(|t| t.as_mut()).unwrap().state = ThreadState::Ready;
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 1);
    });

    // ── Mmap tests ──

    test_case!("mmap_region_create", {
        let r = MmapRegion {
            base: 0x20000000, len: 0x1000, prot: 3, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        test_eq!(r.base, 0x20000000);
        test_eq!(r.len, 0x1000);
        test_eq!(r.prot, 3);
        test_eq!(r.flags, 1);
    });

    test_case!("mmap_region_anonymous", {
        let r = MmapRegion {
            base: 0x20001000, len: 0x4000, prot: 1, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        test_true!((r.flags & 1) != 0);
        test_eq!(r.prot & 2, 0);
        test_eq!(r.prot & 1, 1);
    });

    test_case!("mmap_region_file_backed", {
        let r = MmapRegion {
            base: 0x20010000, len: 0x2000, prot: 3, flags: 0,
            drive: 2, inode: 42, file_size: 8192,
        };
        test_eq!(r.flags & 1, 0);
        test_eq!(r.drive, 2);
        test_eq!(r.inode, 42);
        test_eq!(r.file_size, 8192);
    });

    test_case!("mmap_region_contains", {
        let r = MmapRegion {
            base: 0x20000000, len: 0x10000, prot: 3, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        test_true!(0x20000000 >= r.base && 0x20000000 < r.base + r.len);
        test_true!(0x2000FFF0 >= r.base && 0x2000FFF0 < r.base + r.len);
        test_true!(!(0x20010000 >= r.base && 0x20010000 < r.base + r.len));
    });

    test_case!("mmap_is_mmap_virtual_addr", {
        test_true!(crate::arch::x64::paging::is_mmap_virtual_addr(0x20000000));
        test_true!(crate::arch::x64::paging::is_mmap_virtual_addr(0x21FFFFFF));
        test_true!(!crate::arch::x64::paging::is_mmap_virtual_addr(0x1FFFFFFF));
        test_true!(!crate::arch::x64::paging::is_mmap_virtual_addr(0x22000000));
    });

    test_case!("mmap_process_add_remove", {
        let mut ep = Eprocess::new_ring3(99, 0, 2, "\\", 0x10000000, 0);
        test_eq!(ep.mmap_regions.len(), 0);
        let r1 = MmapRegion {
            base: 0x20000000, len: 0x1000, prot: 3, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        ep.mmap_regions.push(r1);
        test_eq!(ep.mmap_regions.len(), 1);
        test_eq!(ep.mmap_regions[0].base, 0x20000000);
        let r2 = MmapRegion {
            base: 0x20001000, len: 0x2000, prot: 1, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        ep.mmap_regions.push(r2);
        test_eq!(ep.mmap_regions.len(), 2);
        let idx = ep.mmap_regions.iter().position(|r| r.base == 0x20000000);
        test_true!(idx.is_some());
        ep.mmap_regions.remove(idx.unwrap());
        test_eq!(ep.mmap_regions.len(), 1);
        test_eq!(ep.mmap_regions[0].base, 0x20001000);
    });

    // ── Scheduler stress ──

    test_case!("stress_sched_rapid_yield", {
        for i in 0..500 {
            crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
            let prev = crate::syscall::clear_need_resched();
            test_true!(prev);
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            let _ = i;
        }
    });

    test_case!("stress_sched_state_transitions", {
        let mut p = Kthread::new_idle(99, 0, 0x400000, 0x800000);
        test_eq!(p.state, ThreadState::Ready);
        for _ in 0..200 {
            p.state = ThreadState::Running;
            p.state = ThreadState::Ready;
        }
        p.state = ThreadState::Terminated;
        test_eq!(p.state, ThreadState::Terminated);
    });

    // ── Run queue invariant tests (P0-3) ──

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
            ThreadState::Blocked { waiting_for: 0x0005_0001 });
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
            ThreadState::Blocked { waiting_for: 0x0005_0001 });
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

    // ── P0-3 Priority scan regression tests ──

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
            // Make both threads Ready again
            {
                let k2 = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                k2.state = ThreadState::Ready;
                Scheduler::enqueue_to_cpu_run_queue(k2);
            }
            {
                let k3 = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 3).unwrap();
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

        sched.on_timer_tick(0x700000);
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
        sched.next_tid = 2;
        let tid = sched.spawn_kthread(0x400000, PRIORITY_NORMAL).unwrap();
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(tid));
        }
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
    });

    test_case!("rq_validator_rejects_invalid_current", {
        let mut sched = Scheduler::new();
        sched.current_tid = 999;
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_err());
    });

    // ── K17 Gap A/B: kwait_block / kwait_wake real path (isolated Scheduler) ──

    test_case!("k17_kwait_block_wake_single_entry", {
        // Running/Ready thread → kwait_block (remove+Blocked) → kwait_wake (make_ready) → single entry
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        // TID 2 Ready via helper (enqueued)
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // Make TID 2 current Running (remove from queue)
        set_test_current(&mut sched, 2);
        // Validate Running has 0 entries
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 0);

        // Simulate kwait_block: remove + Blocked
        let magic: u32 = 0x0005_0063; // Event 99
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Blocked { waiting_for: magic };
            k.waiting_for = Some(magic);
        }
        // Switch current to idle to allow validation (current must be Running)
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 0);
        test_eq!(sched.find_kthread(2).unwrap().state, ThreadState::Blocked { waiting_for: magic });

        // Simulate kwait_wake: scan + make_thread_ready
        {
            // replicate kwait_wake logic
            let m = magic;
            // collect tids to wake to avoid borrow issues
            let to_wake: Vec<u32> = sched.kthreads.iter().flatten()
                .filter(|k| k.waiting_for == Some(m) && matches!(k.state, ThreadState::Blocked { .. }))
                .map(|k| k.tid)
                .collect();
            for tid in to_wake {
                if let Some(k) = sched.find_kthread_mut(tid) {
                    k.waiting_for = None;
                    Scheduler::make_thread_ready(k);
                }
            }
        }
        // Ready must be exactly once
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        test_eq!(sched.find_kthread(2).unwrap().state, ThreadState::Ready);
        test_eq!(sched.find_kthread(2).unwrap().waiting_for, None);
    });

    test_case!("k17_kwait_double_wake_idempotent", {
        // Blocked → wake → wake again → still single entry
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: 0x0005_0063 });
        // manually set waiting_for to match blocked magic
        sched.find_kthread_mut(2).unwrap().waiting_for = Some(0x0005_0063);
        // first wake
        {
            let magic = 0x0005_0063;
            let tids: Vec<u32> = sched.kthreads.iter().flatten()
                .filter(|k| k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }))
                .map(|k| k.tid).collect();
            for tid in tids {
                if let Some(k) = sched.find_kthread_mut(tid) {
                    k.waiting_for = None;
                    Scheduler::make_thread_ready(k);
                }
            }
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        // second wake — should be no-op
        {
            let magic = 0x0005_0063;
            let tids: Vec<u32> = sched.kthreads.iter().flatten()
                .filter(|k| k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }))
                .map(|k| k.tid).collect();
            for tid in tids {
                if let Some(k) = sched.find_kthread_mut(tid) {
                    k.waiting_for = None;
                    Scheduler::make_thread_ready(k);
                }
            }
        }
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        // also direct make_thread_ready idempotent
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
    });

    test_case!("k17_kwait_wake_multiple_threads_same_magic", {
        // Two Blocked threads waiting on same magic → single wake wakes both
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        let magic: u32 = 0x0006_000A; // Timer 10
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: magic });
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: magic });
        sched.find_kthread_mut(2).unwrap().waiting_for = Some(magic);
        sched.find_kthread_mut(3).unwrap().waiting_for = Some(magic);
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 0);
        // wake all
        {
            let tids: Vec<u32> = sched.kthreads.iter().flatten()
                .filter(|k| k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }))
                .map(|k| k.tid).collect();
            for tid in tids {
                if let Some(k) = sched.find_kthread_mut(tid) {
                    k.waiting_for = None;
                    Scheduler::make_thread_ready(k);
                }
            }
        }
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 2);
        test_eq!(sched.find_kthread(2).unwrap().state, ThreadState::Ready);
        test_eq!(sched.find_kthread(3).unwrap().state, ThreadState::Ready);
    });

    // ── K17 Gap 2: Terminated lifecycle & stale entry ──

    test_case!("k17_terminated_stale_runqueue_detected_and_recycled", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, 2);
        // Simulate buggy exit: set Terminated WITHOUT removing from runqueue
        // First, make it Ready again so it's in queue
        {
            let k = sched.find_kthread_mut(2).unwrap();
            // already Running, make Ready to re-enqueue
            Scheduler::make_thread_ready(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        let ok = sched.validate_runqueue_invariants();
        test_true!(ok.is_ok());
        // Now fake bug: Terminated while still in queue (skip remove)
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.state = ThreadState::Terminated;
            // keep in queue intentionally — do NOT call remove
        }
        // Need a Running current for validation: idle is Running
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_err()); // stale non-Ready in queue must be detected
        // Correct path: remove and validate passes
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
        }
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 0);
        // recycle_thread must free slot and keep invariants
        let freed = sched.recycle_thread(2);
        test_true!(freed);
        test_true!(sched.find_kthread(2).is_none());
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
    });

    test_case!("k17_terminated_recycle_keeps_invariants", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, 2);
        // Terminate current correctly (remove then Terminated)
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1); // only TID3 remains Ready
        // recycle terminated thread
        test_true!(sched.recycle_thread(2));
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        test_true!(sched.find_kthread(2).is_none());
        // schedule should still pick TID3
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let tid = unsafe { (*next).tid };
        test_eq!(tid, 3);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
    });

    // ── K17 Gap 3: Same-priority round-robin sustained ──

    test_case!("k17_same_prio_round_robin_sustained_20", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        let mut picks: Vec<u32> = Vec::new();
        for _ in 0..20 {
            prepare_test_schedule(&mut sched);
            let next = sched.schedule();
            let tid = unsafe { (*next).tid };
            test_true!(tid == 2 || tid == 3);
            picks.push(tid);
            // validate Running not in queue
            let r = sched.validate_runqueue_invariants();
            test_true!(r.is_ok());
            // yield back to Ready (re-enqueue) and set idle as current for next iteration
            {
                let k = sched.find_kthread_mut(tid).unwrap();
                Scheduler::make_thread_ready(k);
            }
            set_test_current(&mut sched, IDLE_TID);
            let r = sched.validate_runqueue_invariants();
            test_true!(r.is_ok());
            test_eq!(r.unwrap(), 2);
        }
        // Both threads must have been scheduled at least 8 times (no starvation)
        let c2 = picks.iter().filter(|&&t| t == 2).count();
        let c3 = picks.iter().filter(|&&t| t == 3).count();
        test_true!(c2 >= 8);
        test_true!(c3 >= 8);
        // Must have alternated at least once (no permanent exclusion)
        let mut alternated = false;
        for w in picks.windows(2) {
            if w[0] != w[1] { alternated = true; break; }
        }
        test_true!(alternated);
    });

    test_case!("k17_same_prio_three_threads_round_robin", {
        let mut sched = Scheduler::new();
        sched.next_tid = 5;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 4, 3, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);
        let mut picks = Vec::new();
        for _ in 0..30 {
            prepare_test_schedule(&mut sched);
            let next = sched.schedule();
            let tid = unsafe { (*next).tid };
            test_true!(tid == 2 || tid == 3 || tid == 4);
            picks.push(tid);
            {
                let k = sched.find_kthread_mut(tid).unwrap();
                Scheduler::make_thread_ready(k);
            }
            set_test_current(&mut sched, IDLE_TID);
        }
        let c2 = picks.iter().filter(|&&t| t == 2).count();
        let c3 = picks.iter().filter(|&&t| t == 3).count();
        let c4 = picks.iter().filter(|&&t| t == 4).count();
        // Each at least 5 times in 30 picks
        test_true!(c2 >= 5);
        test_true!(c3 >= 5);
        test_true!(c4 >= 5);
    });

    // ── K18 Gap 4: Work stealing / cross-CPU runqueue ──
    // Helpers for cross-CPU queue manipulation (avoid IPI side-effects in tests)
    // These tests deliberately use unsafe cpu_run_queue_mut and direct state
    // mutation to isolate the work-stealing contract without modifying production.

    test_case!("k18_steal_drains_victim_to_thief_preserves_order", {
        // Setup: victim CPU1 with 3 Ready threads, thief CPU0 empty
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(0).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            }
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 6;
        // Create 3 threads affine to CPU1 (manual cpu override after helper)
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 4, 3, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // Move them to CPU1: remove from CPU0, set cpu=1, push to CPU1
        for tid in [2u32, 3, 4] {
            if let Some(k) = sched.find_kthread_mut(tid) {
                unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, tid); }
                k.cpu = 1;
                Scheduler::enqueue_to_cpu_run_queue(k);
            }
        }
        // Verify victim has 3, thief empty
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 3);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 0);
        }
        // Steal: drain victim 1 into thief 0 via production steal_from_cpu_run_queue
        let stolen = unsafe {
            let dst = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            crate::arch::x64::cpu_local::steal_from_cpu_run_queue(1, dst)
        };
        test_eq!(stolen, 3);
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 0);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 3);
            // Order preserved: victim pushed 2,3,4 head->tail so thief should pop 2 first
            let rq0 = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            test_eq!(rq0.pop(), Some(2));
            test_eq!(rq0.pop(), Some(3));
            test_eq!(rq0.pop(), Some(4));
        }
        // Cleanup invariant: restore queues empty and threads Ready but not queued
        // Need to re-enqueue for validation? Instead remove and set state
        for tid in [2u32, 3, 4] {
            if let Some(k) = sched.find_kthread_mut(tid) {
                // after pop they are not in queue; ensure state still Ready
                test_eq!(k.state, ThreadState::Ready);
                // reset cpu to 0 for next tests
                k.cpu = 0;
                unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, tid); }
                k.state = ThreadState::Blocked { waiting_for: 0 };
            }
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
        }
        // restore terminated for isolation
        for tid in [2u32, 3, 4] {
            sched.recycle_thread(tid);
        }
    });

    test_case!("k18_steal_affinity_mismatch_stale_cpu_detected", {
        // K20 regression: stolen thread must have k.cpu updated to thief,
        // so validate passes. Previously this test demonstrated stale cpu bug.
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(0).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            }
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        // Move TID2 to CPU1
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);

        // Scheduler-level steal must migrate ownership: use try_work_steal path
        // which now updates Kthread.cpu atomically.
        // Call steal_and_migrate directly to isolate migration without dequeue
        let stolen = unsafe { sched.steal_and_migrate(1, 0) };
        test_eq!(stolen, 1);
        // Now TID2 in CPU0 queue and k.cpu==0 → validate must pass
        test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(2));
            test_true!(!crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
        }

        // Pop and become Running on CPU0 → validate still passes
        let tid = unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(0).pop().unwrap() };
        test_eq!(tid, 2);
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.state = ThreadState::Running;
        }
        sched.current_tid = 2;
        test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());

        // Cleanup
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.state = ThreadState::Terminated;
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });

    test_case!("k18_schedule_via_steal_sets_current_tid_and_removes", {
        // Verify schedule() steal path (try_dequeue_local fails, try_work_steal succeeds)
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = IDLE_TID;
        // Prepare idle as Blocked so schedule must pick something else
        prepare_test_schedule(&mut sched); // idle Running -> Blocked
        // Create victim thread on CPU1
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            // Push directly to CPU1 to avoid IPI in test setup
            unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(1).push(2); }
        }
        // Ensure local queue empty
        unsafe { test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 0); }
        // schedule should steal from CPU1 to CPU0 and return TID2 as Running
        // Note: schedule's try_work_steal drains all from victim then pops from thief.
        // After steal, thief has the entry, pop yields TID2, state becomes Running.
        let next = sched.schedule();
        let tid = unsafe { (*next).tid };
        test_eq!(tid, 2);
        test_eq!(sched.current_tid, 2);
        test_eq!(unsafe { (*next).state }, ThreadState::Running);
        // Victim emptied
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 0);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 0);
        }
        // K20 fixed: k.cpu must have been migrated to thief (0) and validate passes
        test_eq!(unsafe { (*next).cpu }, 0);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        // Cleanup: make Running thread Ready (should stay on thief) then remove
        {
            let k = sched.find_kthread_mut(2).unwrap();
            // schedule already removed from queue
            k.state = ThreadState::Ready;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        // cleanup
        if let Some(k) = sched.find_kthread_mut(2) {
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });

    test_case!("k18_cross_cpu_enqueue_respects_thread_cpu_and_validate", {
        // enqueue_to_cpu_run_queue must place thread on its cpu's queue, not current cpu's
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Blocked { waiting_for: 99 });
        // Make it Ready with cpu=1; enqueue should go to CPU1
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.cpu = 1;
            Scheduler::make_thread_ready(k);
        }
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
            test_true!(!crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(2));
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        // Now change cpu to 0 but leave entry on CPU1 → validate must detect
        sched.find_kthread_mut(2).unwrap().cpu = 0;
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_err());
        // fix and cleanup
        sched.find_kthread_mut(2).unwrap().cpu = 1;
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
        }
    });

    test_case!("k18_steal_empty_victim_returns_zero_and_leaves_local_intact", {
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // TID2 is on CPU0 (default), CPU1 empty
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 0);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 1);
        }
        let stolen = unsafe {
            let dst = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            crate::arch::x64::cpu_local::steal_from_cpu_run_queue(1, dst)
        };
        test_eq!(stolen, 0);
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 1);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 0);
        }
        // cleanup
        sched.find_kthread_mut(2).unwrap().state = ThreadState::Terminated;
        unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
        sched.recycle_thread(2);
        set_test_current(&mut sched, IDLE_TID);
        unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear(); }
    });

    test_case!("k18_steal_then_requeue_bounces_to_victim_cpu", {
        // K20 regression: after scheduler-level steal, requeue must stay on thief (0), not bounce to victim (1)
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // Move to CPU1
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        // Steal via scheduler (migrates ownership)
        let stolen = unsafe { sched.steal_and_migrate(1, 0) };
        test_eq!(stolen, 1);
        test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
        let tid = unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(0).pop().unwrap() };
        test_eq!(tid, 2);
        // Simulate Running on thief
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.state = ThreadState::Running;
            sched.current_tid = 2;
        }
        test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
        // Yield: Running → Ready should stay on thief (0)
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(2));
            test_true!(!crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        // cleanup
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });

    // ── K19: destination-full and repeated-steal evidence ──

    test_case!("k19_dest_full_steal_pushback_no_loss", {
        // Fill thief CPU0 to capacity (64), victim CPU1 has 1, steal must not lose TID
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(0).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            }
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // Move TID2 to victim CPU1
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        // Fill thief CPU0 with 64 dummy TIDs (no Kthread backing, just queue entries)
        // Use raw queue API to avoid needing Kthreads; validate is not used for these dummies
        // Instead fill with 64 entries that are not part of scheduler, then attempt steal
        // To keep validation valid, we fill with fake tids that are NOT in scheduler table
        // but we will clear afterwards. For this test we instead fill thief via direct push
        // of valid tids? Simpler: fill thief with 64 copies of a valid TID2 duplicate check
        // will prevent duplicates, so we directly manipulate queue without scheduler threads:
        unsafe {
            let rq0 = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            // Ensure empty then fill with distinct dummy tids 1000..1063
            rq0.clear();
            for i in 0..64u32 {
                rq0.push(1000 + i);
            }
            test_eq!(rq0.len(), 64);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 1);
            let stolen = crate::arch::x64::cpu_local::steal_from_cpu_run_queue(1, rq0);
            // Destination full → stolen == 0, victim retains entry, no loss, push-back
            test_eq!(stolen, 0);
            test_eq!(rq0.len(), 64);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 1);
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
            // Cleanup
            rq0.clear();
            crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
        }
        // Validate scheduler still consistent (TID2 Ready on CPU1, no entry on full thief after clear)
        // Need to re-ensure TID2 on CPU1 for validation
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(1).push(2);
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        // cleanup
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.cpu = 0;
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });

    test_case!("k19_repeated_steal_requeue_bounce_5_cycles", {
        // 5 cycles: victim→thief→Running(cpu=1)→Ready→bounce to victim, repeat
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(0).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            }
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        for _ in 0..5 {
            // steal via scheduler (migrates ownership to thief)
            let stolen = unsafe { sched.steal_and_migrate(1, 0) };
            test_eq!(stolen, 1);
            test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
            // pop and run on thief
            let tid = unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(0).pop().unwrap() };
            test_eq!(tid, 2);
            {
                let k = sched.find_kthread_mut(2).unwrap();
                k.state = ThreadState::Running;
            }
            sched.current_tid = 2;
            test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
            // yield back → should stay on thief (0) after fix
            {
                let k = sched.find_kthread_mut(2).unwrap();
                Scheduler::make_thread_ready(k);
            }
            unsafe {
                test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(2));
                test_true!(!crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
            }
            set_test_current(&mut sched, IDLE_TID);
            let r = sched.validate_runqueue_invariants();
            test_true!(r.is_ok());
        }
        // cleanup
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.cpu = 0;
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });
}
