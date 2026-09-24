//! Syscall reschedule path — extracted from mod.rs (mechanical split)
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use crate::log::LogSubsys;
use crate::scheduler::{self, ThreadState};

#[no_mangle]
static SAVED_USER_RSP: AtomicU64 = AtomicU64::new(0);
static SAVED_USER_RIP: AtomicU64 = AtomicU64::new(0);

#[no_mangle]
pub extern "C" fn syscall_trace_frame(frame_rsp: u64, phase: u64) {
    const CPU_FRAME: u64 = 15 * 8;
    let frame = frame_rsp.wrapping_add(CPU_FRAME) as *const u64;
    let (rip, cs, rflags, user_rsp) = unsafe {
        let rip = *frame.add(0);
        let cs = *frame.add(1);
        let rflags = *frame.add(2);
        let user_rsp = if cs & 3 == 3 { *frame.add(3) } else { 0 };
        (rip, cs, rflags, user_rsp)
    };
    let need = crate::syscall::NEED_RESCHED.load(Ordering::SeqCst);
    let (tid, pid, state) = crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let lock = s.lock();
        let tid = lock.current_tid_for_this_cpu();
        let pid = lock.current_pid();
        let state = lock.find_kthread(tid).map(|k| k.state.to_u8());
        (tid, pid, state)
    });

    if phase == 0 && cs & 3 == 3 {
        SAVED_USER_RSP.store(user_rsp, Ordering::Relaxed);
        SAVED_USER_RIP.store(rip, Ordering::Relaxed);
    }

    if phase == 1 && cs & 3 == 3 {
        let saved_rsp = SAVED_USER_RSP.load(Ordering::Relaxed);
        let saved_rip = SAVED_USER_RIP.load(Ordering::Relaxed);
        if saved_rsp != user_rsp || saved_rip != rip {
            crate::serial_println!(
                "[SYSCALL_CORRUPT] pid={} tid={} RIP saved=0x{:x} now=0x{:x} RSP saved=0x{:x} now=0x{:x}",
                pid, tid, saved_rip, rip, saved_rsp, user_rsp);
            kerror!(LogSubsys::Syscall,
                "[SYSCALL_CORRUPT] pid={} tid={} RIP saved=0x{:x} now=0x{:x} RSP saved=0x{:x} now=0x{:x}",
                pid, tid, saved_rip, rip, saved_rsp, user_rsp);
        }
    }

    // Solo en modo trazas (LOG_SYSCALL=TRACE) — evita spam en boot normal
    if crate::log::log_enabled(crate::log::LogSubsys::Syscall, crate::log::LogLevel::Trace) {
        crate::serial_println!(
            "[SYSCALL_FRAME] phase={} pid={} tid={} rip=0x{:x} cs=0x{:x} rsp=0x{:x} rflags=0x{:x} need_resched={} state={}",
            phase, pid, tid, rip, cs, user_rsp, rflags, need, state.unwrap_or(255));
    }
    ktrace!(LogSubsys::Syscall,
        "[SYSCALL_FRAME] phase={} pid={} tid={} rip=0x{:x} cs=0x{:x} rsp=0x{:x} rflags=0x{:x} need_resched={} state={}",
        phase, pid, tid, rip, cs, user_rsp, rflags, need, state.unwrap_or(255));
}

#[no_mangle]
pub extern "C" fn syscall_try_resched(current_rsp: u64) -> u64 {
    if cfg!(feature = "validation") && crate::invariants::is_in_timer_irq() {
        kdebug!(LogSubsys::Syscall, "resched called from timer IRQ context!");
    }

    let has_non_idle = crate::hal::without_interrupts(|| {
        let scheduler = scheduler::current_scheduler().lock();
        scheduler.has_non_idle_threads()
    });

    if !has_non_idle {
        return current_rsp;
    }

    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut scheduler = s.lock();

        let tid = scheduler.current_tid_for_this_cpu();
        let pid = scheduler.current_pid();
        kdebug!(LogSubsys::Syscall,
            "[SYSCALL_RESCHED] before pid={} tid={} current_rsp=0x{:x}", pid, tid, current_rsp);
        crate::serial_println!("[SYSCALL_RESCHED] before pid={} tid={} current_rsp=0x{:x}", pid, tid, current_rsp);
        if tid > 0 {
            if let Some(k) = scheduler.current_kthread_mut() {
                k.rsp = current_rsp;
                if k.state == ThreadState::Running {
                    scheduler::Scheduler::make_thread_ready(k);
                } else if cfg!(feature = "validation") {
                    kdebug!(LogSubsys::Syscall, "Context switch from non-Running state: {:?}", k.state);
                }
            }
        }

        let next = scheduler.schedule();
        if next.is_null() {
            return current_rsp;
        }
        let next_ks_top = unsafe { (*next).kernel_stack_top };
        let next_rsp = unsafe { (*next).rsp };
        if next_rsp == 0 {
            panic!("syscall_try_resched: next TID={} has rsp=0 (prev={}, ks_top=0x{:x})",
                unsafe { (*next).tid }, tid, next_ks_top);
        }
        let next_tid = unsafe { (*next).tid };
        let next_pid = unsafe { (*next).pid };
        let next_frame = (next_rsp + 15 * 8) as *const u64;
        let (next_rip, next_cs, next_rflags) = unsafe {
            (*next_frame.add(0), *next_frame.add(1), *next_frame.add(2))
        };

        // A syscall entered from Ring 3 must return to a user frame.  The
        // normal scheduler also contains kernel threads (notably netd), and
        // the old path could select one here and iretq directly into its
        // Ring-0 frame.  That left NeoInit Ready while netd ran forever in
        // its polling loop, so the instruction after INT 0x80 was never
        // reached.  Leave kernel-thread dispatch to timer preemption and
        // keep this syscall return on the original user thread.
        // FIX: Preserve Blocked state (e.g., NeoInit in sys_ob_wait). If the
        // current thread is Blocked, do NOT restore it to Running; instead
        // search for a Ring3 Ready thread (NeoShell) or fall back to idle.
        if next_cs & 3 != 3 {
            let current_is_blocked = scheduler.find_kthread(tid)
                .map(|k| matches!(k.state, ThreadState::Blocked { .. }))
                .unwrap_or(false);
            let current_is_terminated = scheduler.find_kthread(tid)
                .map(|k| k.state == ThreadState::Terminated)
                .unwrap_or(false);
            unsafe {
                (*next).state = ThreadState::Ready;
                scheduler::Scheduler::enqueue_to_cpu_run_queue(&*next);
            }
            // TERMINATED has precedence over KEEP_CURRENT (fix 02e74d3)
            if current_is_terminated {
                // Fall through to alternative search / idle — never KEEP_CURRENT
            } else if !current_is_blocked {
                let old_ks_top = scheduler.find_kthread(tid)
                    .map(|k| k.kernel_stack_top)
                    .unwrap_or(next_ks_top);
                scheduler.current_tid = tid;
                if let Some(current) = scheduler.find_kthread_mut(tid) {
                    current.state = ThreadState::Running;
                }
                unsafe { crate::arch::x64::gdt::prepare_ring3_return(old_ks_top, tid, pid); }
                return current_rsp;
            }
            // Unified fallback for BLOCKED and TERMINATED: search Ring3 Ready, else idle.
            // This reuses the existing idle mechanism (no new idle design).
            {
                let mut chosen_ptr: *mut scheduler::Kthread = core::ptr::null_mut();
                let mut chosen_rsp = 0u64;
                let mut chosen_ks_top = 0u64;
                let mut chosen_tid = 0u32;
                let mut chosen_pid = 0u32;
                for prio in 0..scheduler::PRIORITY_COUNT {
                    for k_opt in scheduler.kthreads.iter_mut() {
                        if let Some(k) = k_opt {
                            if k.state == ThreadState::Ready && k.priority == prio && k.rsp != 0 {
                                let cs_val = unsafe { *((k.rsp + 15 * 8 + 8) as *const u64) };
                                if (cs_val & 3) == 3 {
                                    chosen_ptr = &mut **k as *mut scheduler::Kthread;
                                    chosen_rsp = k.rsp;
                                    chosen_ks_top = k.kernel_stack_top;
                                    chosen_tid = k.tid;
                                    chosen_pid = k.pid;
                                    break;
                                }
                            }
                        }
                    }
                    if !chosen_ptr.is_null() { break; }
                }
                if !chosen_ptr.is_null() {
                    unsafe {
                        scheduler::Scheduler::remove_from_run_queue(&*chosen_ptr);
                        (*chosen_ptr).state = ThreadState::Running;
                    }
                    scheduler.current_tid = chosen_tid;
                    scheduler::check_kernel_stack_canary(chosen_ks_top, chosen_pid, chosen_tid, chosen_rsp);
                    unsafe { crate::arch::x64::gdt::prepare_ring3_return(chosen_ks_top, chosen_tid, chosen_pid); }
                    unsafe {
                        crate::arch::x64::cpu_local::this_cpu_set_current_thread(chosen_ptr);
                        crate::arch::x64::cpu_local::this_cpu_set_current_pid(chosen_pid);
                        crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
                    }
                    unsafe {
                        let frame = (chosen_rsp + 15 * 8) as *const u64;
                        let rip = *frame.add(0);
                        let cs = *frame.add(1);
                        kdebug!(LogSubsys::Syscall,
                            "[SYSCALL_RESCHED] after (blocked/terminated) old_pid={} old_tid={} next_pid={} next_tid={} next_rsp=0x{:x} next_rip=0x{:x} next_cs=0x{:x}",
                            pid, tid, chosen_pid, chosen_tid, chosen_rsp, rip, cs);
                    }
                    crate::trace_cswitch!(tid as u64, chosen_tid as u64);
                    return chosen_rsp;
                } else {
                    // No Ring3 Ready — switch to idle (existing mechanism, reused verbatim).
                    let mut idle_ptr: *mut scheduler::Kthread = core::ptr::null_mut();
                    for k_opt in scheduler.kthreads.iter_mut() {
                        if let Some(k) = k_opt {
                            if k.tid == scheduler::IDLE_TID {
                                idle_ptr = &mut **k as *mut scheduler::Kthread;
                                break;
                            }
                        }
                    }
                    if !idle_ptr.is_null() {
                        unsafe {
                            let idle = &mut *idle_ptr;
                            scheduler::Scheduler::remove_from_run_queue(idle);
                            if idle.state != ThreadState::Terminated {
                                scheduler.current_tid = scheduler::IDLE_TID;
                                idle.state = ThreadState::Running;
                                idle.time_slice_remaining = scheduler::IDLE_TIME_SLICE;
                                crate::arch::x64::cpu_local::this_cpu_set_current_thread(idle_ptr);
                                crate::arch::x64::cpu_local::this_cpu_set_current_pid((*idle_ptr).pid);
                                crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
                                return idle.rsp;
                            }
                        }
                    }
                    if current_is_terminated {
                        panic!("[SYSCALL_RESCHED] Terminated tid={} has no Ring3 target and idle unavailable", tid);
                    } else {
                        panic!("[SYSCALL_RESCHED] Blocked tid={} has no Ring3 target and idle unavailable", tid);
                    }
                }
            }
        }

        // ── BUGCHECK: Pre-iretq diagnostic ──
        if next_ks_top == 0 {
            crate::serial_println!(
                "\n!!! BUGCHECK: kernel_stack_top=0 for next TID={} (would triple-fault) !!!\n\
                 old_tid={} next_pid={} next_rsp=0x{:x} next_rip=0x{:x} next_cs=0x{:x}",
                next_tid, tid, next_pid, next_rsp, next_rip, next_cs);
            panic!("BUGCHECK: next TID={} has kernel_stack_top=0", next_tid);
        }
        if next_rsp == 0 {
            crate::serial_println!(
                "\n!!! BUGCHECK: rsp=0 for next TID={} (would triple-fault) !!!\n\
                 old_tid={} next_pid={} next_ks_top=0x{:x} next_rip=0x{:x} next_cs=0x{:x}",
                next_tid, tid, next_pid, next_ks_top, next_rip, next_cs);
            panic!("BUGCHECK: next TID={} has rsp=0", next_tid);
        }
        if next_cs & 3 != 3 {
            crate::serial_println!(
                "\n!!! BUGCHECK: next TID={} has non-Ring3 CS=0x{:x} (would iretq to Ring 0) !!!",
                next_tid, next_cs);
            panic!("BUGCHECK: next TID={} CS is not Ring 3", next_tid);
        }
        // Validate that the iretq frame has correct segment selectors
        let (rrsp, rss) = unsafe {
            let frame = (next_rsp + 15 * 8) as *const u64;
            let rsp = *frame.add(3);
            let ss  = *frame.add(4);
            (rsp, ss)
        };
        if rss != 0x23 {
            crate::serial_println!(
                "\n!!! BUGCHECK: next TID={} has SS=0x{:x} (expected 0x23) !!!",
                next_tid, rss);
        }
        crate::serial_println!(
            "[RING3_SWITCH] tid={}→{} pid={} ks_top=0x{:x} rsp=0x{:x} rip=0x{:x} cs=0x{:x} ss=0x{:x} user_rsp=0x{:x}",
            tid, next_tid, next_pid, next_ks_top, next_rsp, next_rip, next_cs, rss, rrsp);

        scheduler::check_kernel_stack_canary(next_ks_top, next_pid, next_tid, next_rsp);
        unsafe { crate::arch::x64::gdt::prepare_ring3_return(next_ks_top, next_tid, next_pid); }
        // Keep the per-CPU view in sync with Scheduler. Timer-driven
        // switches update KPRCB, but syscall-return switches previously
        // updated only `current_tid` and RSP0.
        unsafe {
            crate::arch::x64::cpu_local::this_cpu_set_current_thread(next);
            crate::arch::x64::cpu_local::this_cpu_set_current_pid(next_pid);
            crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
        }
        kdebug!(LogSubsys::Syscall,
            "[SYSCALL_RESCHED] after old_pid={} old_tid={} next_pid={} next_tid={} next_rsp=0x{:x} next_rip=0x{:x} next_cs=0x{:x} next_rflags=0x{:x}",
            pid, tid, next_pid, next_tid, next_rsp,
            next_rip, next_cs, next_rflags);
        crate::serial_println!(
            "[SYSCALL_RESCHED] after old_pid={} old_tid={} next_pid={} next_tid={} next_rsp=0x{:x} next_rip=0x{:x} next_cs=0x{:x}",
            pid, tid, next_pid, next_tid, next_rsp, next_rip, next_cs);
        crate::trace_cswitch!(tid as u64, unsafe { (*next).tid } as u64);
        next_rsp
    })
}

