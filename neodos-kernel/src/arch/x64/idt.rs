use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::serial_println;
use crate::scheduler::{current_scheduler, ThreadState};
use crate::panic_classification::PanicClass;
use crate::trace::TraceEvent;
use crate::exception::{
    EXCEPTION_DIVIDE_ERROR, EXCEPTION_GPF, EXCEPTION_PAGE_FAULT,
    EXCEPTION_INVALID_OPCODE, EXCEPTION_OVERFLOW, EXCEPTION_BOUND_RANGE,
    EXCEPTION_DEVICE_NOT_AVAILABLE,
    DispatchResult,
    exception_dispatch,
};

core::arch::global_asm!(
    ".extern timer_handler_inner",
    ".global timer_handler_asm",
    "timer_handler_asm:",
    "push rbp",
    "push r15",
    "push r14",
    "push r13",
    "push r12",
    "push r11",
    "push r10",
    "push r9",
    "push r8",
    "push rdi",
    "push rsi",
    "push rdx",
    "push rcx",
    "push rbx",
    "push rax",
    "mov rdi, rsp",
    "call timer_handler_inner",
    "mov rsp, rax",
    "pop rax",
    "pop rbx",
    "pop rcx",
    "pop rdx",
    "pop rsi",
    "pop rdi",
    "pop r8",
    "pop r9",
    "pop r10",
    "pop r11",
    "pop r12",
    "pop r13",
    "pop r14",
    "pop r15",
    "pop rbp",
    "iretq"
);

core::arch::global_asm!(
    ".extern syscall_dispatch",
    ".extern syscall_try_resched",
    ".extern apc_dispatch_on_syscall_return",
    ".extern is_thread_terminated",
    ".global syscall_handler_asm",
    "syscall_handler_asm:",
    "push rbp",
    "push r15",
    "push r14",
    "push r13",
    "push r12",
    "push r11",
    "push r10",
    "push r9",
    "push r8",
    "push rdi",
    "push rsi",
    "push rdx",
    "push rcx",
    "push rbx",
    "push rax",
    "mov r15, [rsp]",
    "mov rdi, [rsp + 0]",
    "mov rsi, [rsp + 8]",
    "mov rdx, [rsp + 16]",
    "mov rcx, [rsp + 24]",
    "mov r8,  [rsp + 48]",
    "mov r9,  [rsp + 56]",
    "call syscall_dispatch",
    "mov [rsp + 0], rax",
    // Check if syscall number was 0 (exit) — if so, check per-CPU exit_now flag
    "test r15, r15",
    "jnz 1f",
    // Read per-CPU exit_now from KPRCB via GS segment
    "xor rax, rax",
    "mov al, gs:[0xB98]",                  // OFFSET_EXIT_NOW in KPRCB
    "test al, al",
    "jz 4f",
    // Clear exit_now and jump to exit_to_kernel
    "mov byte ptr gs:[0xB98], 0",          // OFFSET_EXIT_NOW
    ".extern exit_to_kernel",
    "jmp exit_to_kernel",
    // Non-last thread exit: check if thread was terminated
    "4:",
    "push rsp",
    "call is_thread_terminated",
    "add rsp, 8",
    "test rax, rax",
    "jz 1f",
    // Thread terminated but not last → reschedule (switch to next thread)
    "mov rdi, rsp",
    "call syscall_try_resched",
    "mov rsp, rax",
    "jmp 3f",
    "1:",
    // Check per-CPU NEED_RESCHED via GS segment (offset 0x015 in KPRCB)
    "xor rax, rax",
    "mov al, gs:[0x015]",                  // OFFSET_NEED_RESCHED in KPRCB
    "test al, al",
    "jz 2f",
    // Clear per-CPU NEED_RESCHED
    "mov byte ptr gs:[0x015], 0",          // OFFSET_NEED_RESCHED
    // Also clear the global NEED_RESCHED (backward compat) and do work
    "call clear_need_resched",
    "test al, al",
    "jz 2f",
    "mov rdi, rsp",
    "call syscall_try_resched",
    "mov rsp, rax",
    "2:",
    // A4.5: Dispatch pending APCs before returning to Ring 3
    "call apc_dispatch_on_syscall_return",
    "3:",
    "pop rax",
    "pop rbx",
    "pop rcx",
    "pop rdx",
    "pop rsi",
    "pop rdi",
    "pop r8",
    "pop r9",
    "pop r10",
    "pop r11",
    "pop r12",
    "pop r13",
    "pop r14",
    "pop r15",
    "pop rbp",

    "iretq"
);

extern "C" {
    fn timer_handler_asm();
    fn syscall_handler_asm();
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.debug.set_handler_fn(debug_handler);
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.overflow.set_handler_fn(overflow_handler);
        idt.bound_range_exceeded.set_handler_fn(bounds_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available.set_handler_fn(device_not_available_handler);

        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(crate::arch::x64::gdt::DOUBLE_FAULT_IST_INDEX);
        }

        idt.invalid_tss.set_handler_fn(invalid_tss_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
        idt.general_protection_fault.set_handler_fn(gpf_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.x87_floating_point.set_handler_fn(x87_handler);
        idt.alignment_check.set_handler_fn(alignment_check_handler);
        idt.machine_check.set_handler_fn(machine_check_handler);
        idt.simd_floating_point.set_handler_fn(simd_handler);
        idt.virtualization.set_handler_fn(virtualization_handler);

        unsafe {
            idt[32].set_handler_addr(x86_64::VirtAddr::new(timer_handler_asm as *const () as u64));
        }
        idt[33].set_handler_fn(keyboard_handler);
        idt[36].set_handler_fn(serial_handler);
        idt[44].set_handler_fn(mouse_handler);

        unsafe {
            idt[0x80]
                .set_handler_addr(x86_64::VirtAddr::new(syscall_handler_asm as *const () as u64))
                .set_privilege_level(x86_64::PrivilegeLevel::Ring3)
                .disable_interrupts(true);
        }

        // IPI handler for per-CPU reschedule (vector 0xF0)
        unsafe {
            idt[0xF0]
                .set_handler_addr(x86_64::VirtAddr::new(ipi_reschedule_handler as *const () as u64));
        }

        // IPI handler for TLB shootdown (vector 0xF1)
        unsafe {
            idt[0xF1]
                .set_handler_addr(x86_64::VirtAddr::new(ipi_tlb_shootdown_handler as *const () as u64));
        }

        // IPI handler for cross-CPU function call (vector 0xF2)
        unsafe {
            idt[0xF2]
                .set_handler_addr(x86_64::VirtAddr::new(ipi_call_function_handler as *const () as u64));
        }

        idt
    };
}

macro_rules! panic_classified {
    ($class:expr, $($arg:tt)*) => {{
        crate::panic_classification::set_panic_class($class);
        panic!($($arg)*);
    }};
}

/// Helper: check if the exception was from user mode (Ring 3).
fn is_user_exception(frame: &InterruptStackFrame) -> bool {
    frame.code_segment == 0x1B
}

/// Helper: terminate the current user process (Ring 3 exception unhandled).
fn terminate_user_process() {
    use crate::scheduler::{current_scheduler, current_tid};
    use crate::syscall::set_need_resched;
    let tid = current_tid();
    if tid > 0 {
        let mut s = current_scheduler().lock();
        if let Some(k) = s.find_kthread_mut(tid) {
            k.state = ThreadState::Terminated;
        }
    }
    set_need_resched();
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();

    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(
            EXCEPTION_DIVIDE_ERROR, rip, rsp, 0, true, 0, 0,
        );
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => {
                terminate_user_process();
                return;
            }
            DispatchResult::Panic => {} // fall through to kernel panic
        }
    }

    crate::trace_event!(TraceEvent::Panic, 0, 0, 0, 0);
    panic_classified!(PanicClass::UnknownCpuException,
        "Divide error: rip={:#x}", rip);
}

extern "x86-interrupt" fn debug_handler(frame: InterruptStackFrame) {
    ktrace!(crate::log::LogSubsys::Exception, "Debug exception @ {:#x}", frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn nmi_handler(stack_frame: InterruptStackFrame) {
    crate::trace_event!(TraceEvent::Panic, 1, 0, 0, 0);
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    crate::crash::dump_nmi(rip, rsp);
    panic_classified!(PanicClass::UnknownCpuException, "Non-maskable interrupt");
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    ktrace!(crate::log::LogSubsys::Exception, "Breakpoint: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(EXCEPTION_OVERFLOW, rip, rsp, 0, true, 0, 0);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => { terminate_user_process(); return; }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Overflow: rip={:#x}", rip);
}

extern "x86-interrupt" fn bounds_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(EXCEPTION_BOUND_RANGE, rip, rsp, 0, true, 0, 0);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => { terminate_user_process(); return; }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Bound range: rip={:#x}", rip);
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(EXCEPTION_INVALID_OPCODE, rip, rsp, 0, true, 0, 0);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => { terminate_user_process(); return; }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Invalid opcode: rip={:#x}", rip);
}

extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(EXCEPTION_DEVICE_NOT_AVAILABLE, rip, rsp, 0, true, 0, 0);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => { terminate_user_process(); return; }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Device not available: rip={:#x}", rip);
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) -> ! {
    crate::trace_event!(TraceEvent::Panic, 2, error_code, 0, 0);
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    // Capture crash dump before panic (will write to serial and RAM buffer)
    crate::crash::dump_double_fault(rip, rsp, error_code);
    panic_classified!(PanicClass::DoubleFault,
        "Double fault: rip={:#x} rsp={:#x} error={:#x}",
        rip, rsp, error_code);
}

extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic_classified!(PanicClass::InvalidContextSwitch,
        "Invalid TSS: rip={:#x} rsp={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        error_code);
}

extern "x86-interrupt" fn segment_not_present_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic_classified!(PanicClass::MemoryCorruption,
        "Segment not present: rip={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(), error_code);
}

extern "x86-interrupt" fn stack_segment_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic_classified!(PanicClass::StackCorruption,
        "Stack segment fault: rip={:#x} rsp={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        error_code);
}

extern "x86-interrupt" fn gpf_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    // Read actual GS selector directly from the CPU register to
    // determine if the fault is from a bad GS load.
    let gs: u16;
    unsafe { core::arch::asm!("mov {0:x}, gs", out(reg) gs, options(nomem, nostack)); }
    kerror!(crate::log::LogSubsys::Exception,
        "GPF: error={:#x} rip={:#x} cs={:#x} rflags={:#x} rsp={:#x} tick={} GS={:#x}",
        error_code, rip,
        stack_frame.code_segment,
        stack_frame.cpu_flags, rsp,
        crate::hal::get_ticks(), gs,
    );

    // Try user-mode dispatch first
    let in_user_window = (crate::arch::x64::paging::USER_BASE..crate::arch::x64::paging::USER_LIMIT).contains(&rip);
    if is_user_exception(&stack_frame) || in_user_window {
        // For GPF, the fault_addr is typically RIP (null deref) or a selector
        let fault_addr = rip;
        let result = exception_dispatch(EXCEPTION_GPF, rip, rsp, error_code, true, fault_addr, error_code);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => {
                terminate_user_process();
                return;
            }
            DispatchResult::Panic => {} // fall through
        }
    }

    let class = if error_code == 0x15c {
        PanicClass::InvalidIretq
    } else if rsp & 0xFFF < 0x100 || rsp & 0xFFF > 0xF00 {
        // RSP near page boundary — possible stack overflow
        PanicClass::StackCorruption
    } else {
        PanicClass::Gpf
    };
    crate::trace_event!(TraceEvent::Panic, 3, error_code, rip, rsp);
    panic_classified!(class,
        "GPF: error={:#x} rip={:#x}", error_code, rip);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    // INV-14: Page fault at IRQL >= DISPATCH is fatal (bugcheck).
    let irql = unsafe { crate::arch::x64::cpu_local::this_cpu_irql() };
    if irql >= crate::hal::irql::DISPATCH_LEVEL {
        let rip = stack_frame.instruction_pointer.as_u64();
        let virt = crate::hal::read_cr2();
        panic_classified!(PanicClass::PageFault,
            "BUGCHECK KI_EXCEPTION_ACCESS_VIOLATION: page fault at IRQL {} (>= DISPATCH) \
             @ {:#x} virt={:#x}",
            irql, rip, virt);
    }

    let virt = crate::hal::read_cr2();
    let is_user = error_code.contains(PageFaultErrorCode::USER_MODE);
    let is_write = error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE);
    let is_not_present = !error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION);

    if is_user && is_not_present {
        if crate::arch::x64::paging::handle_heap_page_fault(virt, true, is_write) {
            return;
        }
        if crate::arch::x64::paging::handle_mmap_page_fault(virt, true, is_write) {
            return;
        }
        // Also handle TEB demand paging (TEB at 0x7000)
        if crate::arch::x64::paging::handle_teb_page_fault(virt) {
            return;
        }
    }

    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();

    // A3.4: Try user-mode SEH dispatch before panic
    if is_user {
        let fault_code = if is_not_present { 0u64 } else { 1u64 }; // 0=not-present, 1=protection
        let result = exception_dispatch(
            EXCEPTION_PAGE_FAULT, rip, rsp, 0, true, virt, fault_code,
        );
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => {
                terminate_user_process();
                return;
            }
            DispatchResult::Panic => {} // fall through
        }
    }

    let class = if !is_not_present {
        PanicClass::PageTableCorruption
    } else {
        PanicClass::PageFault
    };
    crate::trace_event!(TraceEvent::Panic, 4, virt, rip, is_write as u64);
    panic_classified!(class,
        "Page fault @ {:#x} (user={}, write={}, np={}) rip={:#x}",
        virt, is_user, is_write, is_not_present, rip);
}

extern "x86-interrupt" fn x87_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "x87 FP: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn alignment_check_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic_classified!(PanicClass::MemoryCorruption,
        "Alignment check: rip={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(), error_code);
}

extern "x86-interrupt" fn machine_check_handler(stack_frame: InterruptStackFrame) -> ! {
    panic_classified!(PanicClass::UnknownCpuException,
        "Machine check: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn simd_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "SIMD FP: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn virtualization_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "Virtualization: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

/// Read CS selector from the interrupt stack frame.
/// The timer_handler_asm pushes 15 GPRs, then the iretq frame starts
/// at current_rsp + 120 (= 15 * 8).  CS is at +128.
/// 0x08 = Ring 0 (kernel), 0x1B = Ring 3 (user).
unsafe fn read_cs_from_stack(current_rsp: u64) -> u16 {
    ((current_rsp + 128) as *const u16).read()
}

/// Return true if the interrupted context was Ring 3 (user mode),
/// meaning it is safe to preempt and context-switch.
unsafe fn is_user_mode_interrupt(current_rsp: u64) -> bool {
    read_cs_from_stack(current_rsp) == 0x1B
}

#[no_mangle]
pub extern "C" fn timer_handler_inner(current_rsp: u64) -> u64 {
    crate::invariants::timer_irq_enter();
    crate::invariants::irq_enter_check(32);

    crate::hal::increment_ticks();
    crate::console::cursor_timer_tick();
    let current_tick = crate::hal::get_ticks();

    // Increment per-CPU timer tick count
    unsafe { crate::arch::x64::cpu_local::this_cpu_inc_timer_tick_count(); }

    crate::trace_event!(TraceEvent::IrqTimerTick, current_tick, current_rsp, 0, 0);

    // A3.3: Watchdog pet + check on every timer tick
    crate::watchdog::watchdog_pet();

    // v0.46: Timer Object tick — decrement running timers
    crate::object::timer::tick();
    if crate::watchdog::watchdog_check() {
        crate::watchdog::watchdog_trigger();
    }

    {
        use core::sync::atomic::Ordering;
        let last_flush = crate::globals::LAST_FLUSH_TICK.load(Ordering::Relaxed);
        if current_tick.saturating_sub(last_flush) >= crate::globals::FLUSH_INTERVAL_TICKS {
            crate::globals::NEED_CACHE_FLUSH.store(true, Ordering::Relaxed);
        }
    }

    let scheduler_mutex = current_scheduler();
    let mut scheduler = scheduler_mutex.lock();

    scheduler.on_timer_tick();

    let tid = scheduler.current_tid;

    // ── Preemptive context switch (all threads except idle) ──
    if tid != crate::scheduler::IDLE_TID {
        let should_preempt = scheduler.current_kthread_mut()
            .is_some_and(|k| k.state == ThreadState::Ready && k.tid == tid);

        if should_preempt {
            kdebug!(crate::log::LogSubsys::Sched, "[SCHED] PREEMPT tid={} reason=timeslice_expired", tid);
            // Save the current thread's RSP
            if let Some(k) = scheduler.current_kthread_mut() {
                k.rsp = current_rsp;
            }

            // Pick next thread
            let next = scheduler.schedule();
            let next_tid = unsafe { (*next).tid };

            // Safety: if the next thread has rsp==0, proceeding would
            // cause a triple fault (push at address 0).  Log and panic
            // instead so we can identify the root cause.
            let next_rsp = unsafe { (*next).rsp };
            if next_rsp == 0 {
                panic!("timer_handler: next TID={} has rsp=0 (prev={}, cpl=0, kernel_stack_top=0x{:x})",
                    next_tid, tid, unsafe { (*next).kernel_stack_top });
            }

            // If schedule() returned the same thread (no other Ready threads),
            // skip the full context switch: just reset the time slice and stay
            // on the current stack.  This avoids the expensive register
            // save/restore + serial log output when there is nothing else to run.
            if next_tid == tid {
                unsafe {
                    (*next).state = ThreadState::Running;
                    (*next).time_slice_remaining =
                        crate::scheduler::TIME_SLICES[((*next).priority as usize).min(
                            crate::scheduler::PRIORITY_COUNT as usize - 1,
                        )];
                    (*next).ticks_since_scheduled = 0;
                }
                crate::hal::ack_irq(32);
                crate::invariants::timer_irq_exit();
                crate::invariants::irq_exit_clear();
                return current_rsp;
            }

            // Reset the NEXT thread's time slice
            unsafe {
                let nt = &mut *next;
                let idx = (nt.priority as usize).min(crate::scheduler::PRIORITY_COUNT as usize - 1);
                nt.time_slice_remaining = if nt.tid == crate::scheduler::IDLE_TID {
                    crate::scheduler::IDLE_TIME_SLICE
                } else {
                    crate::scheduler::TIME_SLICES[idx]
                };
                nt.ticks_since_scheduled = 0;
            }

            // Switch TSS.RSP0 to the new thread's kernel stack
            let next_ks_top = unsafe { (*next).kernel_stack_top };
            crate::arch::x64::gdt::set_kernel_stack(next_ks_top);

            // Update per-CPU current thread and PID
            unsafe {
                crate::arch::x64::cpu_local::this_cpu_set_current_thread(next);
                crate::arch::x64::cpu_local::this_cpu_set_current_pid((*next).pid);
                crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
            }

            let next_rsp = unsafe { (*next).rsp };
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();

            // Push TimerTick event
            let _ = crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_TIMER_TICK,
                crate::eventbus::SOURCE_HAL,
                1,
                current_tick,
                0,
                0,
            );

            crate::trace_cswitch!(tid as u64, unsafe { (*next).tid } as u64);
            return next_rsp;
        }

        // Thread alive and time slice not expired — just set per-CPU NEED_RESCHED
        let alive = scheduler.current_kthread_mut()
            .is_some_and(|k| k.state != ThreadState::Terminated);
        if alive {
            unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            let _ = crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_TIMER_TICK,
                crate::eventbus::SOURCE_HAL,
                1,
                current_tick,
                0,
                0,
            );
            return current_rsp;
        }
    } else {
        // ── Idle thread (TID 1) preemption ──────────────────────
        let should_preempt = scheduler.current_kthread_mut()
            .is_some_and(|k| k.state == ThreadState::Ready);
        if should_preempt && scheduler.has_non_idle_threads() {
            kdebug!(crate::log::LogSubsys::Sched, "[SCHED] PREEMPT tid={} reason=idle_preempt (has_non_idle={})",
                tid, scheduler.has_non_idle_threads());
            // Save idle thread's RSP
            if let Some(k) = scheduler.current_kthread_mut() {
                k.rsp = current_rsp;
            }
            let next = scheduler.schedule();
            unsafe {
                let nt = &mut *next;
                let idx = (nt.priority as usize).min(crate::scheduler::PRIORITY_COUNT as usize - 1);
                nt.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
                nt.ticks_since_scheduled = 0;
            }
            let next_ks_top = unsafe { (*next).kernel_stack_top };
            crate::arch::x64::gdt::set_kernel_stack(next_ks_top);
            unsafe {
                crate::arch::x64::cpu_local::this_cpu_set_current_thread(next);
                crate::arch::x64::cpu_local::this_cpu_set_current_pid((*next).pid);
                crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
            }
            let next_rsp = unsafe { (*next).rsp };
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            let _ = crate::eventbus::EVENT_BUS.push_event(
                crate::eventbus::EVENT_TIMER_TICK,
                crate::eventbus::SOURCE_HAL,
                1,
                current_tick,
                0,
                0,
            );

            crate::trace_cswitch!(tid as u64, unsafe { (*next).tid } as u64);
            return next_rsp;
        }
    }

    // ── Kernel mode interrupt OR idle ──────────────────────────
    if tid > 0 {
        let alive = scheduler.current_kthread_mut()
            .is_some_and(|k| k.state != ThreadState::Terminated);
        if alive {
            unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            return current_rsp;
        }
        unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
    }

    // ── Idle thread (TID 0) ─────────────────────────────────────
    if scheduler.has_non_idle_threads() {
        unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
    }
    crate::hal::ack_irq(32);
    crate::invariants::timer_irq_exit();
    crate::invariants::irq_exit_clear();

    // Push TimerTick event (lock‑free, IRQ‑safe)
    let _ = crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_TIMER_TICK,
        crate::eventbus::SOURCE_HAL,
        1,
        current_tick,
        0,
        0,
    );

    // A2.5: DPC dispatch — process deferred procedures at DISPATCH_LEVEL
    // after device IRQ handling. This is the DIRQL→DISPATCH transition point.
    crate::dpc::dpc_dispatch_pending();

    current_rsp
}

/// IPI handler for per-CPU reschedule (vector 0xF0).
/// Called when a remote CPU wakes a thread on this CPU.
extern "x86-interrupt" fn ipi_reschedule_handler(_: InterruptStackFrame) {
    unsafe {
        crate::arch::x64::cpu_local::this_cpu_set_need_resched(true);
    }
    crate::hal::ack_irq(crate::arch::x64::ipi::IPI_RESCHEDULE);
}

/// IPI handler for TLB shootdown (vector 0xF1).
/// Invalidates TLB entries for a virtual address range and sends ACK.
extern "x86-interrupt" fn ipi_tlb_shootdown_handler(_: InterruptStackFrame) {
    crate::arch::x64::ipi::ipi_tlb_shootdown_handler_impl();
    crate::hal::ack_irq(crate::arch::x64::ipi::IPI_TLB_SHOOTDOWN);
}

/// IPI handler for cross-CPU function call (vector 0xF2).
/// Executes a registered function on the receiving CPU and sends ACK.
extern "x86-interrupt" fn ipi_call_function_handler(_: InterruptStackFrame) {
    crate::arch::x64::ipi::ipi_call_function_handler_impl();
    crate::hal::ack_irq(crate::arch::x64::ipi::IPI_CALL_FUNCTION);
}

extern "x86-interrupt" fn keyboard_handler(_: InterruptStackFrame) {
    // Read scancode directly from PS/2 controller
    let status: u8 = crate::hal::inb(0x64);
    let scancode = if (status & 0x01) != 0 {
        Some(crate::hal::inb(0x60))
    } else {
        None
    };

    if let Some(scancode) = scancode {
        // Lock-free: push scancode to NeoKBD via Event Bus
        // NeoKBD processes it during dispatch (safe, no lock held).
        let _ = crate::eventbus::EVENT_BUS.push_event(
            crate::eventbus::EVENT_KEYBOARD_INPUT,
            crate::eventbus::SOURCE_HAL,
            3,
            scancode as u64,
            0,
            0,
        );
    }
    crate::hal::ack_irq(33);
}

extern "x86-interrupt" fn serial_handler(_: InterruptStackFrame) {
    while crate::hal::inb(0x3FD) & 1 != 0 {
        let byte = crate::hal::inb(0x3F8);
        let _ = crate::eventbus::EVENT_BUS.push_event(
            crate::eventbus::EVENT_SERIAL_DATA,
            crate::eventbus::SOURCE_HAL,
            2,
            byte as u64,
            0,
            0,
        );
    }
    crate::hal::ack_irq(36);
}

extern "x86-interrupt" fn mouse_handler(_: InterruptStackFrame) {
    let status: u8 = crate::hal::inb(0x64);
    if (status & 0x01) != 0 {
        let byte = crate::hal::inb(0x60);
        let _ = crate::eventbus::EVENT_BUS.push_event(
            crate::eventbus::EVENT_MOUSE_INPUT,
            crate::eventbus::SOURCE_HAL,
            4,
            byte as u64,
            0,
            0,
        );
    }
    crate::hal::ack_irq(44);
}

pub fn init() {
    IDT.load();
}

// ─────────────────────────────────────────────────────────────────────────────
// Dynamic MSI handler registration
// ─────────────────────────────────────────────────────────────────────────────
//
// The IDT is a lazy_static — its entries cannot be changed after it is loaded.
// To support dynamic MSI vector allocation without rebuilding the IDT, we use
// a secondary dispatch table: every MSI-capable vector (48..=255) points to
// the same `msi_generic_handler` entry in the IDT, which then calls the
// per-vector function stored in MSI_HANDLER_TABLE.

/// Maximum number of IDT entries (vectors 0-255).
const IDT_SIZE: usize = 256;
/// First vector available for MSI allocation (after legacy IRQ remaps 32-47).
const MSI_VECTOR_BASE: usize = 48;

type MsiHandlerFn = fn(vector: u8);

/// Per-vector handler table.  Index == vector number.  `None` means the
/// vector is not yet claimed.
static MSI_HANDLER_TABLE: spin::Mutex<[Option<MsiHandlerFn>; IDT_SIZE]> =
    spin::Mutex::new([None; IDT_SIZE]);

/// Register a handler function for an already-allocated MSI vector.
/// Panics if the vector is out of the MSI range or already registered.
pub fn msi_register_handler(vector: u8, handler: MsiHandlerFn) {
    assert!(
        (vector as usize) >= MSI_VECTOR_BASE,
        "msi_register_handler: vector {} is in the legacy range",
        vector
    );
    let mut table = MSI_HANDLER_TABLE.lock();
    assert!(
        table[vector as usize].is_none(),
        "msi_register_handler: vector {} already has a handler",
        vector
    );
    table[vector as usize] = Some(handler);
}

/// Unregister the handler for an MSI vector (call before freeing the vector).
pub fn msi_unregister_handler(vector: u8) {
    if (vector as usize) < MSI_VECTOR_BASE {
        return;
    }
    MSI_HANDLER_TABLE.lock()[vector as usize] = None;
}

/// Generic MSI dispatch — called from the IDT stub for every MSI vector.
/// It looks up the per-vector handler in MSI_HANDLER_TABLE and calls it.
/// If no handler is registered the spurious interrupt is silently discarded.
#[no_mangle]
pub extern "C" fn msi_dispatch(vector: u8) {
    let handler = {
        let table = MSI_HANDLER_TABLE.lock();
        table[vector as usize]
    };
    if let Some(f) = handler {
        f(vector);
    } else {
        ktrace!(crate::log::LogSubsys::Interrupts, "Spurious interrupt on vector {}", vector);
    }
    // Send EOI via the HAL (no-op if APIC is not configured yet).
    crate::hal::ack_irq(vector);
}
