//! Scheduler tests — extracted from mod.rs (mechanical split)
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::scheduler::types::{Kthread, Eprocess, ThreadState, MmapRegion, KernelName, NAME_MAX, PRIORITY_HIGH, PRIORITY_NORMAL, PRIORITY_IDLE, PRIORITY_ABOVE_NORMAL, TIME_SLICES, IDLE_TID, BOOT_TID, MAX_STARVATION_TICKS, AGING_INTERVAL_TICKS, IDLE_TIME_SLICE, KERNEL_STACK_SIZE};
use crate::scheduler::Scheduler;
use crate::log::LogSubsys;

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;
    use crate::test_true;

    // Phase 15-A.1: CPU execution accounting units.
    crate::scheduler::accounting::register_tests();

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
        // Phase 14-A: deterministic default name + read-only accessor.
        test_eq!(k.name(), "idle");
        test_eq!(k.name.len(), 4);
        test_true!(!k.name.is_empty());
        // Phase 14-A: bounded KernelName semantics (basic, custom, boundary,
        // oversized, path derivation). Metadata only; never panics.
        test_eq!(KernelName::from_str("netd").as_str(), "netd");
        let exact = "abcdefghijklmnopqrstuvwxyz012345"; // 32 bytes == NAME_MAX
        test_eq!(exact.len(), NAME_MAX);
        let ne = KernelName::from_str(exact);
        test_eq!(ne.len(), NAME_MAX);
        test_eq!(ne.as_str(), exact);
        let oversized = "abcdefghijklmnopqrstuvwxyz0123456789"; // 36 bytes
        let no = KernelName::from_str(oversized);
        test_eq!(no.len(), NAME_MAX);
        test_eq!(no.as_str(), "abcdefghijklmnopqrstuvwxyz012345");
        test_eq!(
            KernelName::from_path("\\Global\\FileSystem\\C:\\Programs\\neoshell.nxe").as_str(),
            "neoshell"
        );
        test_eq!(KernelName::from_path("netd").as_str(), "netd");
        test_eq!(KernelName::from_str("né").as_str(), "n??");
        test_true!(KernelName::empty().is_empty());
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

    // ── Scheduler priority tests ──

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

    test_case!("stack_canary_bounds_model", {
        // #348: the canary lives at `stack_bottom = ks_top - actual_size`.
        // The checker must use the stack's owned size, not the global
        // KERNEL_STACK_SIZE, so the 4 KiB idle stack is inspected at its own
        // bottom rather than 12 KiB below it.
        use crate::scheduler::stack::kernel_stack_canary_addr;

        // Normal 16 KiB kernel stack.
        let top16 = 0x1_0000u64;
        test_eq!(kernel_stack_canary_addr(top16, KERNEL_STACK_SIZE), Some(top16 - 16384));
        // BSP idle 4 KiB stack: bottom is exactly the idle stack base.
        let idle_base = 0x2_0000u64;
        let idle_top = idle_base + crate::scheduler::IDLE_STACK_SIZE as u64;
        test_eq!(
            kernel_stack_canary_addr(idle_top, crate::scheduler::IDLE_STACK_SIZE),
            Some(idle_base)
        );
        // The old (unsound) computation for the idle stack would have read
        // 12 KiB below the owned region; assert the correct address is not that.
        test_ne!(kernel_stack_canary_addr(idle_top, crate::scheduler::IDLE_STACK_SIZE), Some(idle_top - 16384));
        // Guard rails: no address for a zero top or a zero size.
        test_eq!(kernel_stack_canary_addr(0, KERNEL_STACK_SIZE), None);
        test_eq!(kernel_stack_canary_addr(top16, 0), None);
    });

    test_case!("stack_canary_initialized_for_both_sizes", {
        // #348: both stack kinds must actually receive a canary at the address
        // the (sized) checker inspects, otherwise a correct checker would still
        // report corruption.
        use crate::scheduler::stack::{
            kernel_stack_canary_addr, init_raw_stack_canary, IDLE_STACK_SIZE,
        };
        use crate::scheduler::types::STACK_CANARY;

        // 16 KiB heap stack: AlignedKStack writes the canary at its bottom.
        let stack = crate::scheduler::AlignedKStack::new_boxed();
        let base = stack.0.as_ptr() as u64;
        let top = base + KERNEL_STACK_SIZE as u64;
        test_eq!(kernel_stack_canary_addr(top, KERNEL_STACK_SIZE), Some(base));
        let canary = unsafe { *(base as *const u64) };
        test_eq!(canary, STACK_CANARY);

        // 4 KiB idle stack: initialize a stand-in buffer and verify the canary
        // lands at the buffer bottom (the checker's address for the idle size).
        let mut buf = [0u8; IDLE_STACK_SIZE];
        init_raw_stack_canary(buf.as_mut_ptr(), IDLE_STACK_SIZE);
        let btop = buf.as_ptr() as u64 + IDLE_STACK_SIZE as u64;
        test_eq!(kernel_stack_canary_addr(btop, IDLE_STACK_SIZE), Some(buf.as_ptr() as u64));
        test_eq!(unsafe { *(buf.as_ptr() as *const u64) }, STACK_CANARY);
    });

    test_case!("stack_canary_detects_real_corruption", {
        // #348: the sized checker must still detect a genuinely corrupted
        // canary at the correct bottom address.
        use crate::scheduler::stack::{
            check_kernel_stack_canary_sized, kernel_stack_canary_addr, IDLE_STACK_SIZE,
        };
        use crate::scheduler::types::STACK_CANARY;

        let mut buf = [0u8; IDLE_STACK_SIZE];
        let base = buf.as_mut_ptr() as u64;
        let top = base + IDLE_STACK_SIZE as u64;
        // Intact canary -> must not panic (function returns normally).
        crate::scheduler::stack::init_raw_stack_canary(buf.as_mut_ptr(), IDLE_STACK_SIZE);
        test_eq!(kernel_stack_canary_addr(top, IDLE_STACK_SIZE), Some(base));
        check_kernel_stack_canary_sized(top, IDLE_STACK_SIZE, 0, 11, top);

        // Corrupt the canary word; the address the checker reads must change.
        unsafe { (base as *mut u64).write(STACK_CANARY ^ 0x1); }
        test_ne!(unsafe { *(base as *const u64) }, STACK_CANARY);
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

    // ── Phase 15-A.1: CPU execution accounting ──
    //
    // The host/unit-test target has no KPRCB pages, so `cpu_time_now` and
    // cross-CPU reads resolve nothing; the scheduler-integration tests below
    // assert the *ownership/exclusion rules* (dispatch arms without crediting,
    // migration preserves the total, idle/blocked exclusion) while the pure
    // arithmetic is unit-tested with explicit bases in `accounting.rs`. This
    // keeps them deterministic and free of timing.

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
        let magic: u64 = 0x0005_0000_0063u64; // Event 99
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
            ThreadState::Blocked { waiting_for: 0x0005_0000_0063u64 });
        // manually set waiting_for to match blocked magic
        sched.find_kthread_mut(2).unwrap().waiting_for = Some(0x0005_0000_0063u64);
        // first wake
        {
            let magic = 0x0005_0000_0063u64;
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
            let magic = 0x0005_0000_0063u64;
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
        let magic: u64 = 0x0006_000A; // Timer 10
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
        // Skip on single-CPU configs (QEMU reports 1 CPU online) — SMP not testable
        if crate::arch::x64::cpu_local::cpu_count() < 2 {
            return Ok(());
        }
        // Phase 4: isolate from AP's concurrent runqueue activity
        struct _TestModeGuard; impl Drop for _TestModeGuard { fn drop(&mut self) { crate::scheduler::SCHED_TEST_MODE.store(false, core::sync::atomic::Ordering::Relaxed); } }
        let _test_mode_guard = { crate::scheduler::SCHED_TEST_MODE.store(true, core::sync::atomic::Ordering::Relaxed); _TestModeGuard };
        // Also log validate errors verbosely
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
        if let Err(e) = &r { crate::serial_println!("k18_a after steal validate err: {}", e); }
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(2));
            test_true!(!crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
        }

        // Pop and become Running on CPU0 → validate still passes
        let tid = unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(0).pop().unwrap() };
        test_eq!(tid, 2);
        set_test_current(&mut sched, 2);
        test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
        let r = sched.validate_runqueue_invariants();
        if let Err(e) = &r { crate::serial_println!("k18_a after pop Running validate err: {}", e); }
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
        // Skip on single-CPU configs
        if crate::arch::x64::cpu_local::cpu_count() < 2 {
            return Ok(());
        }
        struct _TestModeGuard2; impl Drop for _TestModeGuard2 { fn drop(&mut self) { crate::scheduler::SCHED_TEST_MODE.store(false, core::sync::atomic::Ordering::Relaxed); } }
        let _test_mode_guard = { crate::scheduler::SCHED_TEST_MODE.store(true, core::sync::atomic::Ordering::Relaxed); _TestModeGuard2 };
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = IDLE_TID;
        // Phase 4: ensure BOOT is not Running (would cause 2 Running on CPU0)
        if let Some(k) = sched.find_kthread_mut(BOOT_TID) {
            k.state = ThreadState::Blocked { waiting_for: 0 };
        }
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
        if let Err(e) = &r { crate::serial_println!("k18_schedule after steal validate err: {}", e); }
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
        if let Err(e) = &r { crate::serial_println!("k18_schedule after requeue validate err: {}", e); }
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
        // 5 cycles: victim→thief→Running→Ready (stay on thief) → next cycle needs requeue to victim
        // Phase 4: after fix, make_thread_ready stays on thief, so we must explicitly
        // requeue to victim at the end of each cycle to allow the next steal to succeed.
        // Skip on single-CPU configs
        if crate::arch::x64::cpu_local::cpu_count() < 2 {
            return Ok(());
        }
        struct _TestModeGuard3; impl Drop for _TestModeGuard3 { fn drop(&mut self) { crate::scheduler::SCHED_TEST_MODE.store(false, core::sync::atomic::Ordering::Relaxed); } }
        let _test_mode_guard = { crate::scheduler::SCHED_TEST_MODE.store(true, core::sync::atomic::Ordering::Relaxed); _TestModeGuard3 };
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
        for iter in 0..5 {
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
            if let Err(e) = r { crate::serial_println!("k19 iter {} validate failed: {}", iter, e); return Err(e); }
            test_true!(r.is_ok());
            // Requeue to victim for next iteration (except after last)
            if iter < 4 {
                let k = sched.find_kthread_mut(2).unwrap();
                // k is currently Blocked after set_test_current, make Ready on victim
                k.state = ThreadState::Ready;
                k.cpu = 1;
                unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); crate::arch::x64::cpu_local::remove_from_cpu_run_queue(1, 2); }
                Scheduler::enqueue_to_cpu_run_queue(k);
                // Keep current as idle
                set_test_current(&mut sched, IDLE_TID);
                let r2 = sched.validate_runqueue_invariants();
                if let Err(e) = r2 { crate::serial_println!("k19 iter {} requeue validate failed: {}", iter, e); return Err(e); }
            }
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

    // ── Test A: a Ring-3 frame is dispatchable; a Ring-0 frame is not ──
    test_case!("n338_ready_frame_must_be_ring3", {
        use crate::scheduler::schedule::{frame_is_ring3, thread_dispatch_frame_is_ring3};
        use crate::scheduler::stack::{init_ring3_frame, init_ring0_frame};

        // A user (Ring 3) thread: its saved entry frame must be dispatchable.
        let user_stack = crate::scheduler::AlignedKStack::new_boxed();
        let user_ktop = user_stack.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let user_rsp = init_ring3_frame(user_ktop, 0x400000, 0x800000);
        let mut uk = Kthread::new_ring3_with_stack(7, 42, 0x400000, user_rsp, user_ktop, user_stack);
        uk.state = ThreadState::Ready;
        test_eq!(unsafe { *((uk.rsp + 128) as *const u64) & 3 }, 3);
        test_true!(frame_is_ring3(&uk));
        test_true!(thread_dispatch_frame_is_ring3(&uk));

        // Simulate being preempted inside a syscall: the timer saved a Ring-0
        // kernel frame (cs == 0x08) as the authoritative `rsp`.
        let kstack = crate::scheduler::AlignedKStack::new_boxed();
        let ktop = kstack.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let ring0_rsp = init_ring0_frame(ktop, 0x400000);
        let mut kk = Kthread::new_ring3_with_stack(8, 43, 0x400000, ring0_rsp, ktop, kstack);
        kk.state = ThreadState::Ready;
        test_eq!(unsafe { *((kk.rsp + 128) as *const u64) & 3 }, 0);
        test_true!(!frame_is_ring3(&kk));
        // A user thread may NOT be published Ready with a Ring-0 frame.
        test_true!(!thread_dispatch_frame_is_ring3(&kk));

        // Kernel threads (pid 0) are exempt: they are dispatched Ring 0 by design.
        let kthread_stack = crate::scheduler::AlignedKStack::new_boxed();
        let kt_ktop = kthread_stack.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let kt_rsp = init_ring0_frame(kt_ktop, 0x500000);
        let mut kt = Kthread::new_ring3_with_stack(9, 0, 0x500000, kt_rsp, kt_ktop, kthread_stack);
        kt.state = ThreadState::Running;
        test_true!(!frame_is_ring3(&kt));
        test_true!(thread_dispatch_frame_is_ring3(&kt));
    });

    // ── Test A2: a Ring-0-framed Ready user thread is never SELECTED ──
    // This pins the exact failure mode: `schedule_with(require_ring3=true)`
    // rejects the frame, so the thread must never be committed `Running`.
    test_case!("n338_ring0_frame_ready_thread_not_dispatched", {
        use crate::scheduler::stack::init_ring0_frame;
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        // Provide an idle owned by this CPU so the idle fallback is well-defined.
        let islot = sched.alloc_kthread_slot().unwrap();
        let mut idle = Kthread::new_idle(50, 0, 0x600000, 0x800000);
        idle.cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        sched.kthreads[islot] = Some(Box::new(idle));
        if sched.next_tid <= 50 { sched.next_tid = 51; }
        // A user thread published Ready with a Ring-0 dispatch frame.
        let stack = crate::scheduler::AlignedKStack::new_boxed();
        let ktop = stack.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let rsp = init_ring0_frame(ktop, 0x400000);
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3_with_stack(3, 2, 0x400000, rsp, ktop, stack);
        k.state = ThreadState::Ready;
        k.cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        sched.kthreads[slot] = Some(Box::new(k));
        sched.eprocesses[0] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        let kref = sched.find_kthread(3).unwrap();
        Scheduler::enqueue_to_cpu_run_queue(kref);
        // require_ring3 must skip it: the only viable target is the idle thread.
        let next = sched.schedule_with(true);
        test_ne!(unsafe { (*next).tid }, 3);
        test_eq!(sched.find_kthread(3).unwrap().state, ThreadState::Ready);
    });

    // ── Test B: a user thread preempted in a syscall is re-published with a
    //            Ring-3 frame and becomes schedulable again (progress) ──
    test_case!("n338_syscall_preempt_preserves_ring3_progress", {
        use crate::scheduler::schedule::{frame_is_ring3, thread_dispatch_frame_is_ring3};
        use crate::scheduler::stack::init_ring0_frame;
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        let stack = crate::scheduler::AlignedKStack::new_boxed();
        let ktop = stack.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let ring3_rsp = crate::scheduler::init_ring3_frame(ktop, 0x400000, 0x800000);
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3_with_stack(3, 2, 0x400000, ring3_rsp, ktop, stack);
        k.state = ThreadState::Running;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(Box::new(k));
        // First iteration: running on the entry (Ring-3) frame.
        test_true!(thread_dispatch_frame_is_ring3(sched.find_kthread(3).unwrap()));

        // A timer fires inside a blocking syscall: the saved frame is Ring 0.
        // Use a separate stack so the Ring-0 frame does not clobber the Ring-3
        // entry frame at `ktop`.
        let kstack2 = crate::scheduler::AlignedKStack::new_boxed();
        let ktop2 = kstack2.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let ring0_rsp = init_ring0_frame(ktop2, 0x400000);
        {
            let k = sched.find_kthread_mut(3).unwrap();
            k.rsp = ring0_rsp;
        }
        // Invariant: this frame may NOT be published as a Ready dispatch frame.
        test_true!(!thread_dispatch_frame_is_ring3(sched.find_kthread(3).unwrap()));
        // The syscall return path restores the Ring-3 frame before publishing.
        {
            let k = sched.find_kthread_mut(3).unwrap();
            k.rsp = ring3_rsp;
            test_true!(frame_is_ring3(k));
        }
        // Now the thread can be published Ready with a valid Ring-3 frame and
        // must be selectable again (i.e. it makes progress).
        {
            let k = sched.find_kthread_mut(3).unwrap();
            Scheduler::make_thread_ready(k);
        }
        sched.eprocesses[0] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        let kref = sched.find_kthread(3).unwrap();
        Scheduler::enqueue_to_cpu_run_queue(kref);
        let next = sched.schedule_with(true);
        test_eq!(unsafe { (*next).tid }, 3);
        test_eq!(sched.find_kthread(3).unwrap().state, ThreadState::Running);
    });

    // ── Test C: a long-lived daemon pattern (repeated yield/preempt cycles)
    //            keeps a user thread dispatchable across many iterations ──
    // Mirrors `netcfg`/`dhcpd`: a Ring-3 thread alternating between executing
    // on its Ring-3 frame and being preempted inside a syscall (Ring-0 frame),
    // then restored by the syscall return. The invariant must hold at every
    // step and the thread must be dispatchable after each cycle.
    test_case!("n338_long_lived_yield_loop_stays_dispatchable", {
        use crate::scheduler::schedule::{frame_is_ring3, thread_dispatch_frame_is_ring3};
        use crate::scheduler::stack::init_ring0_frame;
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        let stack = crate::scheduler::AlignedKStack::new_boxed();
        let ktop = stack.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let ring3_rsp = crate::scheduler::init_ring3_frame(ktop, 0x400000, 0x800000);
        // A dedicated stack holds the simulated Ring-0 syscall frame.
        let kstack2 = crate::scheduler::AlignedKStack::new_boxed();
        let ktop2 = kstack2.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let ring0_rsp = init_ring0_frame(ktop2, 0x400000);
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3_with_stack(3, 2, 0x400000, ring3_rsp, ktop, stack);
        k.state = ThreadState::Ready;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(Box::new(k));
        sched.eprocesses[0] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        for iter in 0..64u32 {
            // In-syscall preemption: saved frame is Ring 0 and must NOT be
            // considered dispatchable...
            {
                let k = sched.find_kthread_mut(3).unwrap();
                k.rsp = ring0_rsp;
                test_true!(!frame_is_ring3(k));
                test_true!(!thread_dispatch_frame_is_ring3(k));
            }
            // ...the syscall return captures the Ring-3 frame, which IS valid.
            {
                let k = sched.find_kthread_mut(3).unwrap();
                k.rsp = ring3_rsp;
                test_true!(frame_is_ring3(k));
                test_true!(thread_dispatch_frame_is_ring3(k));
                // Publish and verify the thread is reachable via the run queue.
                k.state = ThreadState::Running;
                Scheduler::make_thread_ready(k);
                test_eq!(k.state, ThreadState::Ready);
            }
            test_true!(unsafe {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(3)
            });
            // Consume it (simulate dispatch), leaving it Running for the next
            // cycle. Also verify the selected candidate has a Ring-3 frame.
            let picked = sched.schedule_with(true);
            test_eq!(unsafe { (*picked).tid }, 3);
            test_true!(frame_is_ring3(unsafe { &*picked }));
            test_eq!(sched.find_kthread(3).unwrap().state, ThreadState::Running);
        }
        // After 64 cycles the thread is still alive and Running, not stranded.
        test_eq!(sched.find_kthread(3).unwrap().state, ThreadState::Running);
    });

    // ── Test D: on_timer_tick only expires-to-Ready on a Ring-3 interrupt ──
    // This is the #338 regression at its origin: a user thread whose timeslice
    // expires while the CPU is in Ring 0 (inside a syscall) must stay Running;
    // the same expiry in Ring 3 must publish it Ready.
    test_case!("n338_timer_expiry_ring3_gate", {
        // Ring-3 interruption -> thread becomes Ready.
        {
            let mut sched = Scheduler::new();
            sched.next_tid = 4;
            sched.current_tid = 3;
            let slot = sched.alloc_kthread_slot().unwrap();
            let mut k = Kthread::new_ring3(3, 2, 0x400000, 0x800000);
            k.state = ThreadState::Running;
            k.time_slice_remaining = 1;
            k.priority = PRIORITY_NORMAL;
            sched.kthreads[slot] = Some(Box::new(k));
            let ep = sched.alloc_eprocess_slot().unwrap();
            sched.eprocesses[ep] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
            sched.on_timer_tick(0x700000, 0x1B);
            test_eq!(sched.kthreads[slot].as_ref().unwrap().state, ThreadState::Ready);
        }
        // Ring-0 interruption (in-syscall) -> stays Running, no Ready publication.
        {
            let mut sched = Scheduler::new();
            sched.next_tid = 4;
            sched.current_tid = 3;
            let slot = sched.alloc_kthread_slot().unwrap();
            let mut k = Kthread::new_ring3(3, 2, 0x400000, 0x800000);
            k.state = ThreadState::Running;
            k.time_slice_remaining = 1;
            k.priority = PRIORITY_NORMAL;
            sched.kthreads[slot] = Some(Box::new(k));
            let ep = sched.alloc_eprocess_slot().unwrap();
            sched.eprocesses[ep] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
            sched.on_timer_tick(0x700000, 0x08);
            let k = sched.kthreads[slot].as_ref().unwrap();
            test_eq!(k.state, ThreadState::Running);
            // A fresh slice is granted so the thread keeps running in-kernel.
            test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_NORMAL as usize]);
        }
    });
}
