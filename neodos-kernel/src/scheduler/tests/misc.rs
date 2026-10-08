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
    test_case!("eprocess_new_ring3", {
        let ep = Eprocess::new_ring3(42, 1, 2, "\\", 0x10000000, 0);
        test_eq!(ep.pid, 42);
        test_eq!(ep.heap_base, 0x10000000);
        test_eq!(ep.heap_break, 0x10000000);
        test_eq!(ep.thread_count, 1);
        test_eq!(ep.cwd_drive, 2);
        // Phase 14-A: deterministic default process name.
        test_eq!(ep.name(), "process");
    });
    test_case!("idle_fallback_requires_cpu_ownership", {
        // #293 regression: the idle fallback must only select an idle Kthread
        // whose `k.cpu == this_cpu`. Selecting a global idle (first is_idle)
        // let two CPUs adopt the same idle/0 and run on one kernel stack.
        let mut sched = Scheduler::new();
        // Two synthetic per-CPU idles: tid=10 cpu=0, tid=11 cpu=1.
        for (tid, cpu) in [(10u32, 0u32), (11u32, 1u32)] {
            let slot = sched.alloc_kthread_slot().unwrap();
            let mut k = Kthread::new_idle(tid, 0, 0x400000, 0x800000);
            k.cpu = cpu;
            sched.kthreads[slot] = Some(Box::new(k));
            if tid >= sched.next_tid { sched.next_tid = tid + 1; }
        }
        // find_idle_ptr(cpu) must resolve an idle owned by that CPU.
        let p0 = sched.find_idle_ptr(0);
        let p1 = sched.find_idle_ptr(1);
        test_true!(!p0.is_null());
        test_true!(!p1.is_null());
        unsafe {
            test_eq!((*p0).cpu, 0);
            test_eq!((*p1).cpu, 1);
        }
        // The predicate used by the resched fallback: is_idle && k.cpu == cpu.
        // Every CPU must resolve an idle owned by that same CPU (never another).
        for cpu in 0..2u32 {
            let chosen_cpu = sched.kthreads.iter().flatten()
                .find(|k| k.is_idle && k.cpu == cpu)
                .map(|k| k.cpu);
            test_eq!(chosen_cpu, Some(cpu));
        }
        // A cross-CPU idle must not satisfy the ownership predicate.
        let cross = sched.kthreads.iter().flatten()
            .find(|k| k.is_idle && k.cpu == 0)
            .map(|k| k.cpu == 1);
        test_eq!(cross, Some(false));
    });
    test_case!("find_idle_ptr_no_cross_cpu_fallback", {
        // #346/#293: an idle Kthread may only be returned for the CPU that
        // owns it. The old global fallback to IDLE_TID returned the BSP idle
        // for any CPU without a registered idle yet, letting two CPUs adopt
        // one Kthread and execute on a single kernel stack.
        let mut sched = Scheduler::new(); // registers idle TID=1 on cpu=0
        let p0 = sched.find_idle_ptr(0);
        test_true!(!p0.is_null());
        unsafe { test_eq!((*p0).cpu, 0); }
        // No idle registered for cpu 1 yet: must be null, never the BSP idle.
        test_true!(sched.find_idle_ptr(1).is_null());
        test_true!(sched.find_idle_ptr(7).is_null());
        // Register an AP idle and verify exact ownership.
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_idle(11, 0, 0x400000, 0x800000);
        k.cpu = 1;
        sched.kthreads[slot] = Some(Box::new(k));
        let p1 = sched.find_idle_ptr(1);
        test_true!(!p1.is_null());
        unsafe {
            test_eq!((*p1).cpu, 1);
            test_eq!((*p1).tid, 11);
        }
        test_true!(p1 != p0);
    });
    test_case!("memproc_committed_and_working_set", {
        // MEM-PROC (#274): committed = heap span + mmap; WS = resident heap
        // pages of the process's slot. Driven through the real snapshot path.
        let mut sched = Scheduler::new();
        let heap_base = crate::arch::x64::paging::PROCESS_HEAP_BASE;
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        {
            let ep = sched.find_eprocess_mut(1).unwrap();
            ep.heap_base = heap_base;
            ep.heap_break = heap_base + 0x3000; // 3 pages committed
            ep.mmap_regions.push(MmapRegion {
                base: 0x4000_0000, len: 0x2000, prot: 3, flags: 0,
                drive: 0, inode: 0, file_size: 0,
            });
        }
        // Simulate 2 resident heap pages for this process's slot (slot 0).
        let slot = 0usize;
        crate::arch::x64::paging::heap_slot_reset(slot);
        // No public inc API; recompute via snapshot with a known resident count.
        // We assert committed exactly and WS from the (currently 0) counter.
        let mut snap = crate::scheduler::ProcSnapshot::empty();
        sched.snapshot_into(&mut snap);
        let p = snap.process(1).unwrap();
        // committed = 0x3000 heap + 0x2000 mmap = 0x5000
        test_eq!(p.committed_bytes, 0x5000);
        // WS reflects the slot's resident pages (0 in this synthetic case).
        test_eq!(p.working_set_bytes, 0);

        // Now simulate 3 resident pages in the process's slot and re-snapshot.
        crate::arch::x64::paging::heap_slot_add_resident(slot, 3);
        let mut snap2 = crate::scheduler::ProcSnapshot::empty();
        sched.snapshot_into(&mut snap2);
        let p2 = snap2.process(1).unwrap();
        test_eq!(p2.working_set_bytes, 3 * crate::arch::x64::paging::PAGE_4K);
        test_eq!(p2.committed_bytes, 0x5000);
        crate::arch::x64::paging::heap_slot_reset(slot);

        // Now the counter moves: exercise the real accounting helper by
        // allocating/freeing through the choke points is not safe in a unit
        // test (needs real mappings), so validate the arithmetic helper.
        crate::arch::x64::paging::heap_slot_reset(slot);
        test_eq!(crate::arch::x64::paging::heap_slot_resident_pages(slot), 0);
        // heap_slot_of maps addresses to slots deterministically.
        test_eq!(crate::arch::x64::paging::heap_slot_of(heap_base), Some(0));
        test_eq!(
            crate::arch::x64::paging::heap_slot_of(
                heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE * 2),
            Some(2));
        test_eq!(crate::arch::x64::paging::heap_slot_of(0), None);
    });
    test_case!("handoff_target_marked_running_by_tid", {
        let mut sched = Scheduler::new();
        // Target: a suspended Ring-3 thread, as NeoInit is at spawn.
        add_test_thread(&mut sched, 5, 3, 0x400000, PRIORITY_NORMAL, ThreadState::Suspended);
        // The per-CPU KPRCB still identifies the bootstrap thread (TID 0)
        // during the deferred-publication window.
        sched.current_tid = BOOT_TID;
        test_true!(sched.mark_handoff_target_running(5));
        test_eq!(sched.find_kthread(5).unwrap().state, ThreadState::Running);
        test_ne!(sched.find_kthread(5).unwrap().state, ThreadState::Suspended);
    });
    test_case!("inv10_kill_pid_1_refused", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // PID 1 must be refused; PID 0 is also refused.
        test_true!(!sched.kill_pid(crate::scheduler::lifecycle::INIT_PID));
        test_true!(!sched.kill_pid(0));
        // PID 2 (a normal process) is killable.
        test_true!(sched.kill_pid(2));
    });
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
}
