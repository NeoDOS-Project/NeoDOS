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
    test_case!("n474_ring0_preempt_only_publishes_dispatchable_frame", {
        // #474: the timer's Ring-0 preemption branch must not publish a user
        // thread interrupted inside a syscall (cs == 0x08). Its live `rsp` is a
        // transient kernel call frame, not a dispatch frame; publishing it let
        // a later iretq consume stack data as RIP/CS (wild RIP=0x148 on SMP2).
        use crate::scheduler::schedule::ring0_publish_is_dispatchable;

        // User thread interrupted in Ring 0 (inside a syscall) -> must defer.
        test_true!(!ring0_publish_is_dispatchable(0x08, false));
        // A user thread with a transient ring0 CS variant is likewise deferred.
        test_true!(!ring0_publish_is_dispatchable(0x10, false));
        // Genuine kernel/idle threads run Ring 0 by design -> publishable.
        test_true!(ring0_publish_is_dispatchable(0x08, true));
        // A Ring-3 interruption is dispatchable (handled by the user branch).
        test_true!(ring0_publish_is_dispatchable(0x1B, false));
        test_true!(ring0_publish_is_dispatchable(0x1B, true));

        // `frame_is_ring3` alone is insufficient here: a user thread's *stored*
        // dispatch frame may still be Ring 3 while its *live* rsp (the value
        // this branch would save) is a deep Ring-0 call frame. The gate must be
        // driven by the interrupted CS, which is what the helper encodes.
        use crate::scheduler::schedule::frame_is_ring3;
        use crate::scheduler::types::KERNEL_STACK_SIZE;
        let stack = crate::scheduler::stack::AlignedKStack::new_boxed();
        let top = stack.0.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let deep_rsp = top - 0x140;
        unsafe { *((deep_rsp + 128) as *mut u64) = 0x08; } // deep slot is Ring 0
        let mut k = Kthread::new_idle(2, 0, 0, top);
        k.rsp = deep_rsp;
        k.kernel_stack_top = top;
        test_true!(!frame_is_ring3(&k));
        let _ = &stack; // keep the allocation alive for the duration of the test
    });
    test_case!("n476_kstack_switch_out_conflict_detector", {
        // #476 H1 experiment: validate the switch-out kernel-stack detector
        // data path. `note` marks the stack of the thread a CPU is leaving;
        // `reclaim_conflict` must report it until `switch_out_clear` runs
        // (which the ASM does only after `mov rsp`).
        use crate::scheduler::diag::kstack;
        use crate::scheduler::types::KERNEL_STACK_SIZE;
        use core::sync::atomic::Ordering;

        let sa = crate::scheduler::stack::AlignedKStack::new_boxed();
        let sb = crate::scheduler::stack::AlignedKStack::new_boxed();
        let a_top = sa.0.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let b_top = sb.0.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let a = Kthread::new_idle(200, 0, 0, a_top);
        let b = Kthread::new_idle(201, 0, 0, b_top);

        kstack::switch_out_clear(); // start from a clean window
        let conflicts_before = kstack::CONFLICTS.load(Ordering::Relaxed);
        test_eq!(kstack::reclaim_conflict(a_top), None);
        test_eq!(kstack::reclaim_conflict(b_top), None);

        // CPU repoints KPRCB: it is about to abandon `a`'s stack.
        kstack::note(&a as *const _, &b as *const _, 0xBAD_F00D);
        let hit = kstack::reclaim_conflict(a_top);
        test_true!(hit.is_some());
        if let Some((_cpu, tid, pid, rsp, _nks)) = hit {
            test_eq!(tid, 200);
            test_eq!(pid, 0);
            test_eq!(rsp, 0xBAD_F00D);
        }
        // A different stack is not reported.
        test_eq!(kstack::reclaim_conflict(b_top), None);

        // The ASM clear (after `mov rsp`) closes the window.
        kstack::switch_out_clear();
        test_eq!(kstack::reclaim_conflict(a_top), None);
        test_eq!(kstack::CONFLICTS.load(Ordering::Relaxed), conflicts_before);
        let _ = &sa;
        let _ = &sb;
    });
    test_case!("n476_iretq_frame_validator", {
        // #476: pure validation logic for the frame consumed by `iretq`.
        use crate::scheduler::diag::iretq::{validate, Frame, IretqBad};
        let ks_base = 0x24b0000u64;
        let ks_top = 0x24b4000u64;
        let frame_addr = ks_base + 0x200; // 8-aligned, inside the stack
        let ring3_ok = Frame { rip: 0x40_1000, cs: 0x1B, rflags: 0x202, rsp: 0x10_0000, ss: 0x23 };
        let ring0_ok = Frame { rip: 0x40_1000, cs: 0x08, rflags: 0x202, rsp: 0, ss: 0 };
        test_eq!(validate(frame_addr, ks_base, ks_top, &ring3_ok, true), None);
        test_eq!(validate(frame_addr, ks_base, ks_top, &ring0_ok, false), None);
        // Frame not on the selected thread's kernel stack.
        test_eq!(validate(ks_top - 4, ks_base, ks_top, &ring3_ok, true), Some(IretqBad::FrameOutsideKstack));
        // Misaligned frame address.
        test_eq!(validate(frame_addr + 1, ks_base, ks_top, &ring3_ok, true), Some(IretqBad::FrameAlignment));
        // Bad CS selector.
        test_eq!(validate(frame_addr, ks_base, ks_top, &Frame { cs: 0x10, ..ring3_ok }, true), Some(IretqBad::InvalidCs));
        // Ring mismatch (kernel frame expected, user frame given and vice versa).
        test_eq!(validate(frame_addr, ks_base, ks_top, &ring0_ok, true), Some(IretqBad::RingMismatch));
        test_eq!(validate(frame_addr, ks_base, ks_top, &ring3_ok, false), Some(IretqBad::RingMismatch));
        // RFLAGS bit1 clear / reserved bits set.
        test_eq!(validate(frame_addr, ks_base, ks_top, &Frame { rflags: 0x0, ..ring3_ok }, true), Some(IretqBad::InvalidRflags));
        test_eq!(validate(frame_addr, ks_base, ks_top, &Frame { rflags: 0x202 | (1 << 22), ..ring3_ok }, true), Some(IretqBad::InvalidRflags));
        // Non-canonical / high-half RIP for a Ring-3 frame.
        test_eq!(validate(frame_addr, ks_base, ks_top, &Frame { rip: 0x0000_8000_0000_0000, ..ring3_ok }, true), Some(IretqBad::InvalidRip));
        // Bad SS / non-canonical user RSP.
        test_eq!(validate(frame_addr, ks_base, ks_top, &Frame { ss: 0x10, ..ring3_ok }, true), Some(IretqBad::InvalidSs));
        test_eq!(validate(frame_addr, ks_base, ks_top, &Frame { rsp: 0xFFFF_8000_0000_0000, ..ring3_ok }, true), Some(IretqBad::InvalidRsp));
    });
    test_case!("n476_on_timer_tick_rsp_ownership", {
        // #476: `on_timer_tick` may only save a live rsp that lies on the
        // current thread's own kernel stack; a foreign stack is rejected.
        use crate::scheduler::stack::rsp_in_kernel_stack;
        let top = 0x24b4000u64;
        let size = 0x4000usize;
        test_true!(rsp_in_kernel_stack(top, size, top - 8));         // just below top
        test_true!(rsp_in_kernel_stack(top, size, top - size as u64)); // bottom inclusive
        test_true!(!rsp_in_kernel_stack(top, size, top));            // at the top
        test_true!(!rsp_in_kernel_stack(top, size, top - size as u64 - 8)); // below the bottom
        test_true!(!rsp_in_kernel_stack(top, size, top + 0x8000));   // another kstack
        test_true!(rsp_in_kernel_stack(0, size, 0x1234));            // unknown: allow
    });
    test_case!("n354_fallback_selects_only_ring3_ready", {
        let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        let mut sched = Scheduler::new();

        // A Ready Ring-3-framed thread.
        let s1 = crate::scheduler::AlignedKStack::new_boxed();
        let t1 = s1.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let r1 = crate::scheduler::init_ring3_frame(t1, 0x400000, 0x800000);
        let mut k1 = Kthread::new_ring3_with_stack(10, 10, 0x400000, r1, t1, s1);
        k1.state = ThreadState::Ready;
        k1.priority = PRIORITY_NORMAL;
        k1.cpu = this_cpu;
        let i1 = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[i1] = Some(Box::new(k1));

        // A Ready Ring-0-framed thread: must be skipped by this fallback.
        let s2 = crate::scheduler::AlignedKStack::new_boxed();
        let t2 = s2.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let r2 = crate::scheduler::stack::init_ring0_frame(t2, 0x400000);
        let mut k2 = Kthread::new_ring3_with_stack(11, 11, 0x400000, r2, t2, s2);
        k2.state = ThreadState::Ready;
        k2.priority = PRIORITY_NORMAL;
        k2.cpu = this_cpu;
        let i2 = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[i2] = Some(Box::new(k2));

        test_eq!(sched.select_fallback_ring3(this_cpu), Some(i1));

        // With no Ready Ring-3 thread it selects nothing (idle fallback follows).
        sched.kthreads[i1].as_mut().unwrap().state = ThreadState::Running;
        test_eq!(sched.select_fallback_ring3(this_cpu), None);
    });
    test_case!("n355_kernel_thread_starvation_handoff", {
        let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        let mut sched = Scheduler::new();
        sched.next_tid = 100; // let the global scan cover our synthetic tids

        // Use the scheduler's own idle for this CPU (Scheduler::new registers
        // TID 1 on cpu 0); re-home it to whichever CPU runs the test.
        for k in sched.kthreads.iter_mut().flatten() {
            if k.is_idle { k.cpu = this_cpu; }
        }

        // Current Ring-3 thread, Running (the spinner that keeps the CPU Ring-3).
        let s3 = crate::scheduler::AlignedKStack::new_boxed();
        let t3 = s3.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let r3 = crate::scheduler::init_ring3_frame(t3, 0x400000, 0x800000);
        let mut k3 = Kthread::new_ring3_with_stack(10, 10, 0x400000, r3, t3, s3);
        k3.state = ThreadState::Running;
        k3.priority = PRIORITY_NORMAL;
        k3.cpu = this_cpu;
        let i3 = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[i3] = Some(Box::new(k3));
        sched.current_tid = 10;

        // A Ready Ring-0 kernel thread (no Eprocess -> kernel thread), starved.
        let sk = crate::scheduler::AlignedKStack::new_boxed();
        let tk = sk.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let rk = crate::scheduler::stack::init_ring0_frame(tk, 0x500000);
        let mut kk = Kthread::new_ring3_with_stack(11, 11, 0x500000, rk, tk, sk);
        kk.state = ThreadState::Ready;
        kk.priority = PRIORITY_NORMAL;
        kk.cpu = this_cpu;
        kk.ticks_since_scheduled = MAX_STARVATION_TICKS + 1;
        let ik = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[ik] = Some(Box::new(kk));

        // Ring-3 selection cannot commit the kernel thread -> hand off to idle.
        let next = sched.schedule_with_handoff(true, true);
        test_true!(unsafe { (*next).is_idle });
        test_true!(Scheduler::take_kernel_handoff(this_cpu));
        test_true!(!Scheduler::take_kernel_handoff(this_cpu)); // consumed once

        // From the Ring-0 (idle) context the kernel thread IS selectable.
        let next2 = sched.schedule_with(false);
        test_eq!(unsafe { (*next2).tid }, 11);
    });
    test_case!("n355_kernel_thread_selectable_from_ring3_context", {
        // Regression for the K355 starvation: a *Ready* genuine Ring-0 kernel
        // thread must be committable by the Ring-3-context selection path
        // (`require_ring3 = true`) even when it is NOT past the starvation
        // threshold. Before the fix it was only reachable via the 5000-tick
        // hand-off, so it ran once per window and the hand-off flooded.
        let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 100;
        for k in sched.kthreads.iter_mut().flatten() {
            if k.is_idle { k.cpu = this_cpu; }
        }

        // Current Ring-3 yielder, Running (the syscall-return context).
        let s3 = crate::scheduler::AlignedKStack::new_boxed();
        let t3 = s3.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let r3 = crate::scheduler::init_ring3_frame(t3, 0x400000, 0x800000);
        let mut k3 = Kthread::new_ring3_with_stack(10, 10, 0x400000, r3, t3, s3);
        k3.state = ThreadState::Running;
        k3.priority = PRIORITY_NORMAL;
        k3.base_priority = PRIORITY_NORMAL;
        k3.cpu = this_cpu;
        let i3 = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[i3] = Some(Box::new(k3));
        sched.current_tid = 10;

        // A genuine Ring-0 kernel thread: Ready, NOT starved (ticks == 0),
        // normal priority, enqueued on this CPU.
        let sk = crate::scheduler::AlignedKStack::new_boxed();
        let tk = sk.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let rk = crate::scheduler::stack::init_ring0_frame(tk, 0x500000);
        let mut kk = Kthread::new_ring3_with_stack(11, 11, 0x500000, rk, tk, sk);
        kk.is_kernel = true;
        kk.state = ThreadState::Ready;
        kk.priority = PRIORITY_NORMAL;
        kk.base_priority = PRIORITY_NORMAL;
        kk.ticks_since_scheduled = 0;
        kk.cpu = this_cpu;
        let ik = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[ik] = Some(Box::new(kk));
        Scheduler::enqueue_to_cpu_run_queue(sched.find_kthread(11).unwrap());

        // The Ring-3 syscall-return selection must commit the kernel thread
        // directly (not the idle fallback / hand-off).
        let next = sched.schedule_with_handoff(true, true);
        test_eq!(unsafe { (*next).tid }, 11);
        test_true!(unsafe { (*next).is_kernel });

        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
    });
    test_case!("n355_aging_boost_persists_until_dispatch", {
        // End-to-end aging lifecycle through the REAL scheduler path.
        // A boost must persist while the thread stays Ready (a counter reset
        // alone must not cancel it) and must end at dispatch, where the base
        // priority is restored and a fresh slice is granted.
        let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 100;
        for k in sched.kthreads.iter_mut().flatten() {
            if k.is_idle { k.cpu = this_cpu; }
        }
        let sk = crate::scheduler::AlignedKStack::new_boxed();
        let tk = sk.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let rk = crate::scheduler::stack::init_ring0_frame(tk, 0x500000);
        let mut kk = Kthread::new_ring3_with_stack(12, 12, 0x500000, rk, tk, sk);
        kk.is_kernel = true;
        kk.state = ThreadState::Ready;
        kk.priority = PRIORITY_NORMAL;
        kk.base_priority = PRIORITY_NORMAL;
        kk.ticks_since_scheduled = MAX_STARVATION_TICKS;
        kk.time_slice_remaining = 0;
        kk.cpu = this_cpu;
        let ik = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[ik] = Some(Box::new(kk));
        Scheduler::enqueue_to_cpu_run_queue(sched.find_kthread(12).unwrap());

        // 1st aging pass: boost.
        sched.apply_aging();
        test_eq!(sched.find_kthread(12).unwrap().priority, PRIORITY_ABOVE_NORMAL);

        // 2nd aging pass: the counter was reset, but the boost MUST persist
        // (it ends at dispatch, not at an arbitrary aging tick).
        sched.apply_aging();
        test_eq!(sched.find_kthread(12).unwrap().priority, PRIORITY_ABOVE_NORMAL);
        test_true!(sched.find_kthread(12).unwrap().ticks_since_scheduled > 0);

        // Queue/bitmap must be consistent at the boosted priority.
        test_true!(unsafe {
            crate::arch::x64::cpu_local::with_runqueue(this_cpu as usize, |rq| rq.contains(12))
        });
        test_true!(
            (crate::arch::x64::cpu_local::read_active_bitmap(this_cpu as usize)
                >> PRIORITY_ABOVE_NORMAL) & 1 == 1
        );

        // Dispatch through the real scheduler: base priority restored, slice
        // granted, starvation counter cleared, thread Running.
        let next = sched.schedule();
        test_eq!(unsafe { (*next).tid }, 12);
        let k = sched.find_kthread(12).unwrap();
        test_eq!(k.priority, PRIORITY_NORMAL);
        test_eq!(k.ticks_since_scheduled, 0);
        test_eq!(k.state, ThreadState::Running);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_NORMAL as usize]);

        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
    });
    test_case!("n355_timer_accepts_kernel_thread_dispatch", {
        // Timer user-preempt return path: `schedule_with_handoff(true, true)`
        // selects a genuine kernel thread, and the shared acceptance predicate
        // (used by both the timer and syscall return paths) accepts it while
        // rejecting idle-without-handoff and user threads.
        use crate::scheduler::schedule::accept_non_ring3_dispatch;
        let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 100;
        for k in sched.kthreads.iter_mut().flatten() {
            if k.is_idle { k.cpu = this_cpu; }
        }
        let sk = crate::scheduler::AlignedKStack::new_boxed();
        let tk = sk.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let rk = crate::scheduler::stack::init_ring0_frame(tk, 0x500000);
        let mut kk = Kthread::new_ring3_with_stack(11, 11, 0x500000, rk, tk, sk);
        kk.is_kernel = true;
        kk.state = ThreadState::Ready;
        kk.priority = PRIORITY_NORMAL;
        kk.base_priority = PRIORITY_NORMAL;
        kk.ticks_since_scheduled = 0;
        kk.cpu = this_cpu;
        let ik = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[ik] = Some(Box::new(kk));
        Scheduler::enqueue_to_cpu_run_queue(sched.find_kthread(11).unwrap());

        let next = sched.schedule_with_handoff(true, true);
        test_eq!(unsafe { (*next).tid }, 11);
        // Non-Ring-3 frame (Ring-0 kernel thread).
        test_true!(unsafe { *(((*next).rsp + 128) as *const u64) & 3 } != 3);
        // No #355 handoff: acceptance must come from the kernel-thread rule.
        test_true!(!crate::scheduler::Scheduler::take_kernel_handoff(this_cpu));
        test_true!(accept_non_ring3_dispatch(false, next));

        // idle without handoff is NOT accepted; with handoff it is.
        let idle = sched.find_idle_ptr(this_cpu);
        test_true!(!idle.is_null());
        test_true!(!accept_non_ring3_dispatch(false, idle));
        test_true!(accept_non_ring3_dispatch(true, idle));

        // A user thread with a Ring-0 frame (as if interrupted in a syscall)
        // must never be accepted as a kernel dispatch.
        let su = crate::scheduler::AlignedKStack::new_boxed();
        let tu = su.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let ru = crate::scheduler::stack::init_ring0_frame(tu, 0x400000);
        let ku = Kthread::new_ring3_with_stack(13, 13, 0x400000, ru, tu, su);
        test_true!(!ku.is_kernel);
        test_true!(!accept_non_ring3_dispatch(false, &ku as *const Kthread));

        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
    });
    test_case!("n355_smp_owned_kernel_thread_not_committed", {
        // A Ready kernel thread whose live execution context is still owned by
        // another CPU (KPRCB.current_thread) must be rejected, never committed
        // on the wrong CPU. `candidate_owned_elsewhere` is not weakened.
        let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 100;
        for k in sched.kthreads.iter_mut().flatten() {
            if k.is_idle { k.cpu = this_cpu; }
        }
        let sk = crate::scheduler::AlignedKStack::new_boxed();
        let tk = sk.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let rk = crate::scheduler::stack::init_ring0_frame(tk, 0x500000);
        let mut kk = Kthread::new_ring3_with_stack(11, 11, 0x500000, rk, tk, sk);
        kk.is_kernel = true;
        kk.state = ThreadState::Ready;
        kk.priority = PRIORITY_NORMAL;
        kk.base_priority = PRIORITY_NORMAL;
        kk.cpu = this_cpu;
        let ik = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[ik] = Some(Box::new(kk));
        Scheduler::enqueue_to_cpu_run_queue(sched.find_kthread(11).unwrap());

        let kptr = sched.find_kthread(11).unwrap() as *const Kthread;
        let other = if this_cpu == 0 { 1 } else { 0 };
        let other_kprcb = crate::arch::x64::cpu_local::kprcb_page(other as usize);
        test_true!(other_kprcb.is_some());
        if let Some(base) = other_kprcb {
            let slot =
                (base + crate::arch::x64::cpu_local::OFFSET_CURRENT_THREAD as u64) as *mut u64;
            let saved = unsafe { *slot };
            // Simulate the other CPU still owning this thread's live context.
            unsafe { *slot = kptr as u64; }
            test_eq!(
                crate::arch::x64::cpu_local::kthread_current_cpu(kptr),
                Some(other)
            );

            let next = sched.schedule_with_handoff(true, true);
            test_ne!(unsafe { (*next).tid }, 11);
            test_eq!(sched.find_kthread(11).unwrap().state, ThreadState::Ready);

            unsafe { *slot = saved; }
        }

        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
    });
    test_case!("n355_syscall_dispatch_grants_fresh_slice", {
        // F2: a kernel thread accepted through the syscall-return selection must
        // not be committed with `time_slice_remaining == 0` (which would
        // republish it after a single tick).
        let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 100;
        for k in sched.kthreads.iter_mut().flatten() {
            if k.is_idle { k.cpu = this_cpu; }
        }
        let sk = crate::scheduler::AlignedKStack::new_boxed();
        let tk = sk.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let rk = crate::scheduler::stack::init_ring0_frame(tk, 0x500000);
        let mut kk = Kthread::new_ring3_with_stack(11, 11, 0x500000, rk, tk, sk);
        kk.is_kernel = true;
        kk.state = ThreadState::Ready;
        kk.priority = PRIORITY_NORMAL;
        kk.base_priority = PRIORITY_NORMAL;
        kk.ticks_since_scheduled = 0;
        kk.time_slice_remaining = 0; // as left by a prior timer expiry
        kk.cpu = this_cpu;
        let ik = sched.alloc_kthread_slot().unwrap();
        sched.kthreads[ik] = Some(Box::new(kk));
        Scheduler::enqueue_to_cpu_run_queue(sched.find_kthread(11).unwrap());

        let next = sched.schedule_with_handoff(true, true);
        test_eq!(unsafe { (*next).tid }, 11);
        test_eq!(
            sched.find_kthread(11).unwrap().time_slice_remaining,
            TIME_SLICES[PRIORITY_NORMAL as usize]
        );

        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
    });
    test_case!("n382_fifo_fast_path_respects_priority", {
        let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        // Isolate the real per-CPU run queue for this CPU.
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 100;
        for k in sched.kthreads.iter_mut().flatten() {
            if k.is_idle { k.cpu = this_cpu; }
        }

        // Current thread: Running Ring-3, not enqueued.
        add_test_thread(&mut sched, 10, 10, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        sched.current_tid = 10;
        // Enqueue low priority FIRST so it is the FIFO head.
        add_test_thread(&mut sched, 12, 12, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // Then a higher-priority Ready thread behind it.
        add_test_thread(&mut sched, 11, 11, 0x400000, PRIORITY_HIGH, ThreadState::Ready);

        // The fast path pops tid 12 (low prio) but must fall through to the
        // priority scan and select tid 11.
        let next = sched.schedule_with(false);
        test_eq!(unsafe { (*next).tid }, 11);

        // Cleanup: leave the run queue empty for subsequent tests.
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(this_cpu as usize).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(this_cpu as usize).clear();
            }
        }
    });
    test_case!("n376_preempt_disable_keeps_lock_holder_running", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 3;
        let slot = sched.alloc_kthread_slot().unwrap();
        // pid 1 with no Eprocess => kernel thread (like boot/netpump).
        let mut k = Kthread::new_ring3(3, 1, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.time_slice_remaining = 1;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(Box::new(k));

        crate::scheduler::preempt_disable();
        test_true!(crate::scheduler::preempt_disabled());
        sched.on_timer_tick(0x700000, 0x08); // Ring-0 interrupt, slice exhausted
        let kk = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(kk.state, ThreadState::Running); // not published Ready
        test_eq!(kk.time_slice_remaining, TIME_SLICES[PRIORITY_NORMAL as usize]);
        crate::scheduler::preempt_enable();
        test_true!(!crate::scheduler::preempt_disabled());
    });
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
        // Fill thief CPU0 to capacity: RUNQUEUE_PRIO_CAP entries in each of the
        // 4 priority sub-queues. (Previously 60 = 4×15 matched the old
        // RUNQUEUE_PRIO_CAP=15; with CAP=64 the queue is only full at 4×64=256.)
        unsafe {
            let rq0 = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            let cap = crate::arch::x64::cpu_local::RUNQUEUE_PRIO_CAP;
            let total = (cap * 4) as u32;
            rq0.clear();
            for i in 0..total {
                let prio = (i as usize / cap) as u8;
                rq0.push_priority(1000 + i, prio);
            }
            test_eq!(rq0.len(), total as u16);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 1);
            let stolen = crate::arch::x64::cpu_local::steal_from_cpu_run_queue(1, rq0);
            // Destination full → stolen == 0, victim retains entry, no loss, push-back
            test_eq!(stolen, 0);
            test_eq!(rq0.len(), total as u16);
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
    test_case!("n338_ready_frame_must_be_ring3", {
        use crate::scheduler::schedule::{frame_is_ring3, thread_dispatch_frame_is_ring3};

        // A user (Ring 3) thread whose saved frame is Ring 3 → dispatchable.
        let user_stack = crate::scheduler::AlignedKStack::new_boxed();
        let user_ktop = user_stack.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let user_rsp = crate::scheduler::init_ring3_frame(user_ktop, 0x400000, 0x800000);
        let mut uk = Kthread::new_ring3_with_stack(7, 42, 0x400000, user_rsp, user_ktop, user_stack);
        uk.state = ThreadState::Ready;
        test_eq!(unsafe { *((uk.rsp + 128) as *const u64) & 3 }, 3);
        test_true!(frame_is_ring3(&uk));
        test_true!(thread_dispatch_frame_is_ring3(&uk, false));

        // A user thread preempted inside a syscall: saved frame is Ring 0
        // (cs == 0x08) and may NOT be published as a Ready dispatch frame.
        let kstack = crate::scheduler::AlignedKStack::new_boxed();
        let ktop = kstack.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let ring0_rsp = crate::scheduler::stack::init_ring0_frame(ktop, 0x400000);
        let mut kk = Kthread::new_ring3_with_stack(8, 43, 0x400000, ring0_rsp, ktop, kstack);
        kk.state = ThreadState::Ready;
        test_eq!(unsafe { *((kk.rsp + 128) as *const u64) & 3 }, 0);
        test_true!(!frame_is_ring3(&kk));
        test_true!(!thread_dispatch_frame_is_ring3(&kk, false));

        // Regression guard for the netd starvation bug: a *kernel* thread runs
        // in Ring 0 by design and its Ring-0 frame IS a valid dispatch frame.
        // `is_kernel_thread == true` exempts it even though its frame is Ring 0.
        // (netd has pid != 0, so the old `pid == 0` exemption stranded it.)
        let kt_stack = crate::scheduler::AlignedKStack::new_boxed();
        let kt_ktop = kt_stack.0.as_ptr() as u64 + crate::scheduler::KERNEL_STACK_SIZE as u64;
        let kt_rsp = crate::scheduler::stack::init_ring0_frame(kt_ktop, 0x500000);
        let mut kt = Kthread::new_ring3_with_stack(9, 1, 0x500000, kt_rsp, kt_ktop, kt_stack);
        kt.state = ThreadState::Running;
        test_true!(!frame_is_ring3(&kt));
        test_true!(thread_dispatch_frame_is_ring3(&kt, true));
    });
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
        test_true!(thread_dispatch_frame_is_ring3(sched.find_kthread(3).unwrap(), false));

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
        test_true!(!thread_dispatch_frame_is_ring3(sched.find_kthread(3).unwrap(), false));
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
                test_true!(!thread_dispatch_frame_is_ring3(k, false));
            }
            // ...the syscall return captures the Ring-3 frame, which IS valid.
            {
                let k = sched.find_kthread_mut(3).unwrap();
                k.rsp = ring3_rsp;
                test_true!(frame_is_ring3(k));
                test_true!(thread_dispatch_frame_is_ring3(k, false));
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
        // ── Regression guard: a kernel thread (netd, non-zero pid, no user
        //    image) interrupted in Ring 0 MUST still be published Ready. The
        //    earlier `pid == 0` exemption stranded netd (pid != 0). ──
        {
            let mut sched = Scheduler::new();
            sched.next_tid = 4;
            sched.current_tid = 3;
            let slot = sched.alloc_kthread_slot().unwrap();
            // Kthread with a real (non-zero) pid and a Ring-0 dispatch frame.
            let mut k = Kthread::new_ring3(3, 1, 0x400000, 0x800000);
            k.state = ThreadState::Running;
            k.time_slice_remaining = 1;
            k.priority = PRIORITY_NORMAL;
            // Emulate `spawn_kthread_named`: a kernel Eprocess (no user_slot).
            sched.kthreads[slot] = Some(Box::new(k));
            let ep = sched.alloc_eprocess_slot().unwrap();
            sched.eprocesses[ep] = Some(Eprocess::new_kernel(1));
            test_true!(sched.is_kernel_thread(sched.find_kthread(3).unwrap()));
            sched.on_timer_tick(0x700000, 0x08); // netd always interrupts Ring 0
            test_eq!(sched.kthreads[slot].as_ref().unwrap().state, ThreadState::Ready);
        }
        // ── Same thread class with a *user* Eprocess must NOT be published. ──
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
            test_true!(!sched.is_kernel_thread(sched.find_kthread(3).unwrap()));
            sched.on_timer_tick(0x700000, 0x08);
            test_eq!(sched.kthreads[slot].as_ref().unwrap().state, ThreadState::Running);
        }
    });

    // ── NEODOS-01 (#631): zombie queue observability and boundedness ─────────
    test_case!("neodos01_zombie_queue_dedup_is_single_entry", {
        use crate::scheduler::lifecycle::ZombieQueue;
        let mut q = ZombieQueue::new();
        test_true!(q.enqueue(42));
        test_true!(!q.enqueue(42)); // duplicate is refused
        test_true!(!q.enqueue(0));  // pid 0 is never enqueued
        test_eq!(q.len(), 1);
        test_true!(!q.is_empty());
    });
    test_case!("neodos01_zombie_reclaim_skips_running", {
        use crate::scheduler::lifecycle::ZombieQueue;
        let mut q = ZombieQueue::new();
        q.enqueue(1);
        q.enqueue(2);
        q.enqueue(3);
        // pid 2 still running; 1 and 3 are reclaimable.
        let pos = q.find_reclaimable(0, |p| p == 2).unwrap();
        test_eq!(q.take_at(pos).unwrap(), 1);
        let pos = q.find_reclaimable(0, |p| p == 2).unwrap();
        test_eq!(q.take_at(pos).unwrap(), 3);
        test_true!(q.find_reclaimable(0, |p| p == 2).is_none());
        test_eq!(q.len(), 1);
    });
    test_case!("neodos01_zombie_requeue_is_idempotent", {
        use crate::scheduler::lifecycle::ZombieQueue;
        let mut q = ZombieQueue::new();
        q.enqueue(7);
        test_eq!(q.take_at(0).unwrap(), 7);
        q.requeue(7);
        q.requeue(7); // must not duplicate
        test_eq!(q.len(), 1);
    });
    test_case!("neodos01_zombie_hard_cap_and_backpressure", {
        use crate::scheduler::lifecycle::{ZombieQueue, ZOMBIE_HARD_CAP, MAX_ZOMBIES};
        let mut q = ZombieQueue::new();
        // Fill to the soft watermark; every entry is "running".
        for p in 1..=MAX_ZOMBIES as u32 { q.enqueue(p); }
        test_eq!(q.len(), MAX_ZOMBIES);
        test_true!(!q.over_hard_cap());
        test_true!(q.backpressured(|_| true));
        // A single reclaimable entry clears backpressure.
        test_true!(!q.backpressured(|p| p == 1));
        // Cross the deterministic hard cap with unique PIDs.
        while q.len() < ZOMBIE_HARD_CAP {
            let next = (q.len() as u32) + 1000;
            q.enqueue(next);
        }
        test_true!(q.over_hard_cap());
        // Nothing reclaimable (all still running) -> no candidate.
        test_true!(q.find_reclaimable(0, |_| true).is_none());
    });
    test_case!("neodos01_zombie_stress_4x_max_no_slot_leak", {
        use crate::scheduler::lifecycle::{ZombieQueue, MAX_ZOMBIES};
        let mut sched = Scheduler::new();
        let base_pid: u32 = 10_000;
        let n = MAX_ZOMBIES * 4; // 4×MAX_ZOMBIES = 256 exits

        // 4×MAX real, already-terminated EPROCESS/KTHREAD pairs. `new_kernel`
        // owns no external resources (no user slot / heap / handles), so the
        // reaper only has to free the slot + kernel stack: exactly the leak
        // surface of the zombie path.
        for i in 0..n {
            let pid = base_pid + i as u32;
            let ep_slot = sched.alloc_eprocess_slot().unwrap();
            sched.eprocesses[ep_slot] = Some(Eprocess::new_kernel(pid));
            let th_slot = sched.alloc_kthread_slot().unwrap();
            let mut k = Kthread::new_ring3(pid, pid, 0x400000, 0x800000);
            k.state = ThreadState::Terminated;
            sched.kthreads[th_slot] = Some(Box::new(k));
        }
        test_eq!(sched.eprocesses.iter().flatten().count(), n + 1); // + boot (pid 0)

        // Feed all 256 dead PIDs through the zombie queue, then drain it exactly
        // like the reaper does (`recycle_terminated`).
        let mut q = ZombieQueue::new();
        for i in 0..n {
            q.enqueue(base_pid + i as u32);
        }
        let mut reclaimed = 0usize;
        while let Some(pos) = q.find_reclaimable(0, |_| false) {
            let pid = q.take_at(pos).unwrap();
            test_true!(sched.recycle_terminated(pid));
            reclaimed += 1;
        }
        test_eq!(reclaimed, n);
        test_eq!(q.len(), 0);

        // Zero slot leak: only the boot EPROCESS and the boot+idle KTHREADs
        // remain, and none of the 256 pids is still tracked.
        test_eq!(sched.eprocesses.iter().flatten().count(), 1);
        test_eq!(sched.kthreads.iter().flatten().count(), 2);
        for i in 0..n {
            let pid = base_pid + i as u32;
            test_true!(sched.find_eprocess(pid).is_none());
            test_true!(sched.thread_tids_for_pid(pid).is_empty());
        }
    });

    // ── NEODOS-02 (#632): current-identity reads are never silent ────────────
    test_case!("neodos02_current_identity_prefers_kprcb_and_is_never_silent", {
        // A local (non-global) Scheduler must keep using its own current_tid and
        // current_pid, and must not record a KPRCB identity fallback while AP
        // scheduling is inactive (the test/local-scheduler path). All the
        // "current" accessors now funnel through `current_tid_checked`, so a
        // production fallback is always counted, never silent.
        let before = crate::scheduler::diag::kprcb_fallback_count();
        let mut sched = Scheduler::new();
        sched.next_tid = 5;
        sched.current_tid = 3;
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(3, 7, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        sched.kthreads[slot] = Some(Box::new(k));

        test_eq!(sched.current_tid_for_this_cpu(), 3);
        test_eq!(sched.current_pid(), 7);
        test_true!(sched.current_kthread_mut().is_some());
        // No Eprocess for pid 7 → resolving the current Eprocess yields None,
        // not a wrong process from the global current_tid.
        test_true!(sched.current_eprocess_mut().is_none());
        test_true!(sched.current_eprocess().is_none());

        if !crate::scheduler::ap_sched_active() {
            test_eq!(crate::scheduler::diag::kprcb_fallback_count(), before);
        }
    });
}
