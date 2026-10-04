//! x86_64 Interrupt Descriptor Table, exception/timer/IPI handlers.
//!
//! Diagnostic rings/dumps live in the `diag` submodule; handlers and the IDT
//! itself remain here.

use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
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


mod diag;

pub use diag::timer::timer_diag_dump;
pub use diag::netd::{netd_record_create, netd_record_select, frame_dump, netd_diag_dump, final_iretq_dump};
pub use diag::gpf::{gdt_dump, decode_gpf_error};
pub use diag::kbd::{kbd_irq_reset, kbd_irq_dump};

use diag::timer::{td_push, TimerDiagEntry};
use diag::kbd::{KBD_IRQ_RING_SIZE, KbdIrqEntry, KBD_IRQ_SEQ, KBD_IRQ_HEAD, KBD_IRQ_RING, KBD_IRQ_CNT};


core::arch::global_asm!(
    ".extern timer_handler_inner",
    ".extern timer_trace_iretq_frame",
    ".extern switch_out_clear",
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
    // Fase 3: trace iretq frame on old stack (stack-based, no r12 dependency)
    "push rax",
    "mov rdi, rax",
    "add rdi, 120",
    "call timer_trace_iretq_frame",
    "pop rax",
    "mov rsp, rax",
    // #476: the old stack is physically abandoned here.
    "call switch_out_clear",
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
    ".extern syscall_resched_enabled",
    ".extern apc_dispatch_on_syscall_return",
    ".extern is_thread_terminated",
    ".extern switch_out_clear",
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
    // The 15 saved GPRs occupy 120 bytes.  The CPU's INT 0x80 return
    // frame starts at rsp+120 (RIP, CS, RFLAGS, RSP, SS).
    ".extern syscall_trace_frame",
    "mov rdi, rsp",
    "xor rsi, rsi",                       // phase 0 = handler entry
    "call syscall_trace_frame",
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
    "call switch_out_clear",
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
    "call syscall_resched_enabled",
    "test rax, rax",
    "jz 2f",
    "mov rdi, rsp",
    "call syscall_try_resched",
    "mov rsp, rax",
    "call switch_out_clear",
    "2:",
    // A4.5: Dispatch pending APCs before returning to Ring 3
    "call apc_dispatch_on_syscall_return",
    "3:",
    "mov rdi, rsp",
    "mov rsi, 1",                         // phase 1 = frame about to be iretq'd
    "call syscall_trace_frame",
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
/// Uses centralized lifecycle termination so Eprocess/thread_count, handles, and ChildExit waiters are correctly handled.
/// Defer EPROCESS reclaim via zombie list to avoid use-after-free on current stack.
///
/// F-02: This function NEVER returns via `iretq` to the faulting RIP.
/// It terminates the current thread and immediately reschedules to the next
/// thread's saved context (via direct RSP switch + iretq), guaranteeing:
/// `Terminated -> never resumes at faulting RIP`. The old `return;` path
/// that did `iretq` to the faulting RIP is removed.
fn terminate_user_process() -> ! {
    use crate::scheduler::current_scheduler;
    let pid = crate::hal::without_interrupts(|| {
        let mut s = current_scheduler().lock();
        s.terminate_current(-1)
    });
    if pid.is_none() {
        // Fallback: at least terminate thread
        let tid = crate::scheduler::current_tid();
        if tid > 0 {
            let mut s = current_scheduler().lock();
            if let Some(k) = s.find_kthread_mut(tid) {
                k.state = ThreadState::Terminated;
            }
        }
    }
    // F-02: immediate reschedule — do not return to faulting frame.
    exception_do_resched()
}

/// F-02: immediate context switch from exception path.
/// Picks next thread via `schedule()` and switches RSP + iretq to its
/// saved frame. Never returns to the faulting thread's `InterruptStackFrame`.
fn exception_do_resched() -> ! {
    use crate::scheduler::current_scheduler;
    let next_rsp = crate::hal::without_interrupts(|| {
        let mut sched = current_scheduler().lock();
        let next = sched.schedule_with(true);
        if next.is_null() {
            panic!("exception_do_resched: no next thread (idle unavailable)");
        }
        let pid = unsafe { (*next).pid };
        let tid = unsafe { (*next).tid };
        let ks_top = unsafe { (*next).kernel_stack_top };
        let rsp = unsafe { (*next).rsp };
        if rsp == 0 {
            panic!("exception_do_resched: next TID={} has rsp=0", tid);
        }
        // Keep per-CPU and TSS in sync (also done in timer/syscall paths)
        unsafe {
            crate::arch::x64::cpu_local::this_cpu_set_current_thread_site(next, crate::scheduler::diag::SITE_SET_IDT);
            crate::arch::x64::cpu_local::this_cpu_set_current_pid(pid);
            crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
            crate::arch::x64::gdt::prepare_ring3_return(ks_top, tid, pid);
        }
        rsp
    });
    unsafe {
        core::arch::asm!(
            "mov rsp, {0}",
            // #476: old stack abandoned; clear its switch-out marker.
            "call {clr}",
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
            "iretq",
            in(reg) next_rsp,
            clr = sym crate::scheduler::diag::kstack::switch_out_clear,
            options(noreturn)
        );
    }
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    crate::raw_serial_println!("[FAULT] v=0 DIVIDE rip=0x{:x} cs=0x{:x} rsp=0x{:x} cpu={}",
        rip, stack_frame.code_segment, rsp,
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() });

    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(
            EXCEPTION_DIVIDE_ERROR, rip, rsp, 0, true, 0, 0,
        );
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => {
                terminate_user_process();
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
    crate::raw_serial_println!("[FAULT] v=2 NMI rip=0x{:x} cs=0x{:x} rsp=0x{:x} cpu={}",
        rip, stack_frame.code_segment, rsp,
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() });
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
            DispatchResult::Terminated => { terminate_user_process(); }
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
            DispatchResult::Terminated => { terminate_user_process(); }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Bound range: rip={:#x}", rip);
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    crate::raw_serial_println!("[FAULT] v=6 INVALID_OPCODE rip=0x{:x} cs=0x{:x} rsp=0x{:x} cpu={}",
        rip, stack_frame.code_segment, rsp,
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() });
    if is_user_exception(&stack_frame) {
        let result = exception_dispatch(EXCEPTION_INVALID_OPCODE, rip, rsp, 0, true, 0, 0);
        match result {
            DispatchResult::Handled => return,
            DispatchResult::Terminated => { terminate_user_process(); }
            DispatchResult::Panic => {}
        }
    }
    // #476: raw dump of the Ring-0 handler stack (includes the CPU-pushed
    // exception frame) so a wild control transfer is reconstructable even if
    // the regular logger/diagnostics are unavailable.
    {
        let base = unsafe { crate::hal::raw::raw_read_rsp() };
        crate::raw_serial_println!("[#UD_STACK] handler_rsp=0x{:x} frame_rip=0x{:x} frame_cs=0x{:x}",
            base, rip, stack_frame.code_segment);
        let mut off: u64 = 0;
        while off < 0x180 {
            let addr = base + off;
            let v = unsafe { core::ptr::read_volatile(addr as *const u64) };
            crate::raw_serial_println!("[#UD_STACK] [0x{:x}] = 0x{:x}", addr, v);
            off += 8;
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
            DispatchResult::Terminated => { terminate_user_process(); }
            DispatchResult::Panic => {}
        }
    }
    panic_classified!(PanicClass::UnknownCpuException, "Device not available: rip={:#x}", rip);
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) -> ! {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    crate::raw_serial_println!(
        "[FAULT] v=8 DOUBLE-FAULT err={:#x} rip={:#x} cs={:#x} rsp={:#x} cr2={:#x} cpu={}",
        error_code, rip, stack_frame.code_segment, rsp,
        crate::hal::read_cr2(), unsafe { crate::arch::x64::cpu_local::this_cpu_id() },
    );
    crate::crash::dump_double_fault(rip, rsp, error_code);
    panic_classified!(PanicClass::DoubleFault,
        "Double fault: rip={:#x} rsp={:#x} error={:#x}",
        rip, rsp, error_code);
}

extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    crate::raw_serial_println!("[FAULT] v=10 INVALID_TSS err=0x{:x} rip=0x{:x} cs=0x{:x} rsp=0x{:x} cpu={}",
        error_code, stack_frame.instruction_pointer.as_u64(), stack_frame.code_segment,
        stack_frame.stack_pointer.as_u64(),
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() });
    panic_classified!(PanicClass::InvalidContextSwitch,
        "Invalid TSS: rip={:#x} rsp={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        error_code);
}

extern "x86-interrupt" fn segment_not_present_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    crate::raw_serial_println!("[FAULT] v=11 SEGMENT_NOT_PRESENT err=0x{:x} rip=0x{:x} cs=0x{:x} rsp=0x{:x} cpu={}",
        error_code, stack_frame.instruction_pointer.as_u64(), stack_frame.code_segment,
        stack_frame.stack_pointer.as_u64(),
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() });
    panic_classified!(PanicClass::MemoryCorruption,
        "Segment not present: rip={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(), error_code);
}

extern "x86-interrupt" fn stack_segment_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    crate::raw_serial_println!("[FAULT] v=12 STACK_SEGMENT err=0x{:x} rip=0x{:x} cs=0x{:x} rsp=0x{:x} cpu={}",
        error_code, stack_frame.instruction_pointer.as_u64(), stack_frame.code_segment,
        stack_frame.stack_pointer.as_u64(),
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() });
    panic_classified!(PanicClass::StackCorruption,
        "Stack segment fault: rip={:#x} rsp={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        error_code);
}

extern "x86-interrupt" fn gpf_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    // Lock-free first report: guarantees the fault is captured even if the
    // regular logger deadlocks on the SERIAL1 spinlock.
    crate::raw_serial_println!(
        "[FAULT] v=13 GPF err={:#x} rip={:#x} cs={:#x} rsp={:#x} rflags={:#x} cr2={:#x} cpu={}",
        error_code, rip, stack_frame.code_segment, rsp, stack_frame.cpu_flags,
        crate::hal::read_cr2(), unsafe { crate::arch::x64::cpu_local::this_cpu_id() },
    );
    crate::scheduler::diag::dump_raw();
    crate::scheduler::diag::sys_dump_raw();
    crate::scheduler::diag::frame_dump_raw();
    crate::scheduler::diag::ctx_dump_raw();
    crate::scheduler::diag::rsp_dump_raw();
    crate::scheduler::diag::dr_dump_raw();
    crate::scheduler::diag::kcpu_dump_raw();
    crate::raw_serial_println!("[CORRELATION] last_DOUBLE_RUNNING_seq={}", crate::scheduler::diag::dr_last_seq());
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
            }
            DispatchResult::Panic => {} // fall through
        }
    }

    // Fase 3 P2-P6: dump diagnostico FRAME/TD/NETD/FINAL/GDT antes de panic para capturar first invalid
    crate::serial_println!("[GPF_DIAG] tick={} tid={} rip=0x{:x} rsp=0x{:x} err=0x{:x}", crate::hal::get_ticks(), crate::scheduler::current_tid(), rip, rsp, error_code);
    frame_dump();
    netd_diag_dump();
    final_iretq_dump();
    gdt_dump();
    decode_gpf_error(error_code as u16);
    // TD_RING dump via secondary (no cli/sti double)
    crate::arch::x64::idt::timer_diag_dump();

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
            }
            DispatchResult::Panic => {} // fall through
        }
    }

    let class = if !is_not_present {
        PanicClass::PageTableCorruption
    } else {
        PanicClass::PageFault
    };
    crate::raw_serial_println!(
        "[FAULT] v=14 PAGE-FAULT user={} write={} np={} virt={:#x} rip={:#x} cs={:#x} rsp={:#x} cpu={}",
        is_user, is_write, is_not_present, virt, rip, stack_frame.code_segment, rsp,
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() },
    );
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
    crate::raw_serial_println!("[FAULT] v=17 ALIGNMENT err=0x{:x} rip=0x{:x} cpu={}",
        error_code, stack_frame.instruction_pointer.as_u64(),
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() });
    panic_classified!(PanicClass::MemoryCorruption,
        "Alignment check: rip={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(), error_code);
}

extern "x86-interrupt" fn machine_check_handler(stack_frame: InterruptStackFrame) -> ! {
    crate::raw_serial_println!("[FAULT] v=18 MACHINE_CHECK rip=0x{:x} cpu={}",
        stack_frame.instruction_pointer.as_u64(),
        unsafe { crate::arch::x64::cpu_local::this_cpu_id() });
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

unsafe fn prepare_timer_return(next: *mut crate::scheduler::Kthread) {
    let next_rsp = (*next).rsp;
    let next_cs = *((next_rsp + 128) as *const u64);
    if next_cs & 3 == 3 {
        crate::arch::x64::gdt::prepare_ring3_return(
            (*next).kernel_stack_top, (*next).tid, (*next).pid);
    }
}

#[no_mangle]
pub extern "C" fn timer_handler_inner(current_rsp: u64) -> u64 {
    crate::invariants::timer_irq_enter();
    crate::invariants::irq_enter_check(32);

    // Phase 13: global tick/time side effects stay on the BSP. APs run the
    // scheduler tick only (per-CPU), otherwise the global tick counter would
    // advance once per CPU and break all timing assumptions.
    let is_bsp = unsafe { crate::arch::x64::cpu_local::this_cpu_id() == 0 };
    if is_bsp {
        crate::hal::increment_ticks();
        crate::console::cursor_timer_tick();
    }
    let current_tick = crate::hal::get_ticks();

    // Increment per-CPU timer tick count
    unsafe { crate::arch::x64::cpu_local::this_cpu_inc_timer_tick_count(); }

    crate::trace_event!(TraceEvent::IrqTimerTick, current_tick, current_rsp, 0, 0);

    if is_bsp {
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
    }

    // Phase 13: APs stay out of the scheduler until the BSP enables AP
    // scheduling after the boot test suite. Check BEFORE taking the scheduler
    // lock so parked/idle APs never contend on it during the tests.
    if !is_bsp && !crate::scheduler::ap_sched_active() {
        crate::hal::ack_irq(32);
        crate::invariants::timer_irq_exit();
        crate::invariants::irq_exit_clear();
        return current_rsp;
    }

    let scheduler_mutex = current_scheduler();
    let mut scheduler = scheduler_mutex.lock();

    // Decode the interrupted CS before the tick: `on_timer_tick` needs it to
    // decide whether an expired thread may be published `Ready` (#338). A user
    // thread interrupted inside a syscall is in Ring 0 (`cs == 0x08`) and must
    // not be enqueued with that non-dispatchable frame.
    let interrupted_cs = unsafe { *((current_rsp + 128) as *const u64) };
    let is_user_mode = (interrupted_cs & 3) == 3;

    scheduler.on_timer_tick(current_rsp, interrupted_cs);
    scheduler.consistency_check("timer");

    let tid = scheduler.current_tid_for_this_cpu();

    // ── Preemptive context switch ──
    // Rule: Ring 3 threads may be preempted on timeslice expiry.
    // Ring 0 kernel threads (netd, boot) that are in Ready state (via yield
    // or timeslice expiry) are also preempted if another non-idle thread
    // is available — the "idle preemption" path below handles this.

    let has_non_idle = scheduler.has_non_idle_threads();
    let current_state = scheduler.current_kthread_mut().map(|k| k.state);
    let current_is_idle = scheduler.current_kthread_mut().map(|k| k.is_idle).unwrap_or(false);
    // Phase 13-A: a cooperative yield from a still-running kernel thread sets
    // this flag instead of making it Ready. Treat it as a preemption request so
    // the switch-out path below saves `rsp` before publishing the thread.
    let current_yield = scheduler.current_kthread_mut().map(|k| k.yield_requested).unwrap_or(false);

    // Diagnostic entry: capture state before preemption decision
    td_push(TimerDiagEntry {
        tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
        cur_cs: interrupted_cs,
        flags: (is_user_mode as u8)
            | (((current_state == Some(ThreadState::Ready)) as u8) << 1)
            | ((has_non_idle as u8) << 2),
        path: 3, next_tid: 0, next_rsp: 0, next_ks_top: 0, returned_rsp: 0,
        frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 0,
    });

    if is_user_mode && !current_is_idle {
        let should_preempt = current_state == Some(ThreadState::Ready) || current_yield;

        crate::trace_timer_irq!(
            if should_preempt { 1u8 } else { 3u8 },
            tid, interrupted_cs, has_non_idle as u8);

        if should_preempt {
            kdebug!(crate::log::LogSubsys::Sched, "[SCHED] PREEMPT tid={} reason=timeslice_expired", tid);
            // Save the current thread's RSP, then publish it. `make_thread_ready`
            // is a no-op when the timeslice path already enqueued it.
            if let Some(k) = scheduler.current_kthread_mut() {
                crate::scheduler::diag::rsp_ev(crate::scheduler::diag::SITE_RSP_IDT_USER, k, current_rsp);
                k.rsp = current_rsp;
                k.yield_requested = false;
                // The CPU actually executing the thread owns its re-enqueue.
                k.cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
                crate::scheduler::diag::ev(
                    crate::scheduler::diag::EV_TIMER_SAVE, k.cpu, k.tid, k.rsp,
                    k.state.to_u8() as u64);
                // #338: publish the thread `Ready` only when the saved frame is
                // actually dispatchable back to Ring 3. This branch runs only
                // for user-mode interruptions (`is_user_mode && !current_is_idle`),
                // so a Ring-0 frame here means the user thread was preempted
                // inside a syscall. Marking it `Ready` would strand it forever:
                // `schedule_with(true)` rejects every non-Ring-3 frame. The
                // interrupted Ring-0 frame is still preserved in `rsp` for
                // accounting, and the in-flight syscall return captures the real
                // Ring-3 frame before this thread is re-enqueued.
                if k.state == ThreadState::Running
                    && crate::scheduler::schedule::frame_is_ring3(k)
                {
                    crate::scheduler::Scheduler::make_thread_ready(k);
                }
            }

            // Pick next thread
            let next = scheduler.schedule_with_handoff(true, true);
            let next_tid = unsafe { (*next).tid };

            // Safety: if the next thread has rsp==0, proceeding would
            // cause a triple fault (push at address 0).  Log and panic
            // instead so we can identify the root cause.
            let next_rsp = unsafe { (*next).rsp };
            let next_ks = unsafe { (*next).kernel_stack_top };
            if (tid == 5 || next_tid == 5) && crate::scheduler::sched_forensic_verbose() {
                let __cs = if next_rsp != 0 { unsafe { *((next_rsp + 128) as *const u64) } } else { 0 };
                crate::serial_println!("[T5_TM] userpreempt cur={} next={} rsp=0x{:x} cs=0x{:x} sched.current={} kprcb={:?}",
                    tid, next_tid, next_rsp, __cs, scheduler.current_tid,
                    crate::arch::x64::cpu_local::try_per_cpu_tid());
            }
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs,
                flags: 0x01, path: 0,
                next_tid, next_rsp, next_ks_top: next_ks, returned_rsp: 0,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 1,
            });
            netd_record_select(next as *const _ as u64, next_rsp, next_ks);
            if next_rsp == 0 {
                if unsafe { (*next).is_idle } {
                    crate::hal::ack_irq(32);
                    crate::invariants::timer_irq_exit();
                    crate::invariants::irq_exit_clear();
                    return current_rsp;
                }
                panic!("timer_handler: next TID={} has rsp=0 (prev={}, cpl=0, kernel_stack_top=0x{:x})",
                    next_tid, tid, unsafe { (*next).kernel_stack_top });
            }

            // A Ring 3 interrupt may dequeue a Ready kernel thread. Its Ring
            // 0 frame cannot be returned through this Ring 3 interrupt frame.
            let next_cs = unsafe { *((next_rsp + 128) as *const u64) };
            if next_cs & 3 != 3 {
                // #355: accept the scheduler's idle hand-off (a Ring-0 kernel
                // thread is starved). `schedule_with` already committed idle.
                let this_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
                if crate::scheduler::Scheduler::take_kernel_handoff(this_cpu) {
                    crate::serial_println!("[K355] timer handoff -> idle tid={}", next_tid);
                    unsafe { prepare_timer_return(next); }
                    unsafe {
                        crate::arch::x64::cpu_local::this_cpu_set_current_thread_site(
                            next, crate::scheduler::diag::SITE_SET_IDT);
                        crate::arch::x64::cpu_local::this_cpu_set_current_pid((*next).pid);
                        crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
                    }
                    crate::hal::ack_irq(32);
                    crate::invariants::timer_irq_exit();
                    crate::invariants::irq_exit_clear();
                    return next_rsp;
                }
                if (tid == 5 || next_tid == 5) && crate::scheduler::sched_forensic_verbose() {
                    crate::serial_println!("[T5_TM] userpreempt REVERT next={} cs=0x{:x} (non-Ring3) keep cur={}",
                        next_tid, next_cs, tid);
                }
                unsafe {
                    (*next).state = ThreadState::Ready;
                    crate::scheduler::Scheduler::enqueue_to_cpu_run_queue(&*next);
                }
                if let Some(current) = scheduler.find_kthread_mut(tid) {
                    crate::scheduler::Scheduler::remove_from_run_queue(current);
                    crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_IDT_REVERT, current);
                    current.state = ThreadState::Running;
                }
                scheduler.current_tid = tid;
                crate::hal::ack_irq(32);
                crate::invariants::timer_irq_exit();
                crate::invariants::irq_exit_clear();
                return current_rsp;
            }

            if next_tid == tid {
                unsafe {
                    crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_IDT_SAME, &*next);
                    (*next).state = ThreadState::Running;
                    (*next).time_slice_remaining =
                        crate::scheduler::TIME_SLICES[((*next).priority as usize).min(
                            crate::scheduler::PRIORITY_COUNT as usize - 1,
                        )];
                    (*next).ticks_since_scheduled = 0;
                    // INVARIANT: TSS.RSP0 must always match the current thread.
                    // Even when schedule() returns the same thread, we must
                    // update RSP0 because a PREVIOUS context switch may have
                    // left it pointing to another thread's kernel stack.
                    let ks_top = (*next).kernel_stack_top;
                    prepare_timer_return(next);
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
                nt.time_slice_remaining = if nt.is_idle {
                    crate::scheduler::IDLE_TIME_SLICE
                } else {
                    crate::scheduler::TIME_SLICES[idx]
                };
                nt.ticks_since_scheduled = 0;
            }

            // Switch TSS.RSP0 to the new thread's kernel stack
            let next_ks_top = unsafe { (*next).kernel_stack_top };
            if next_ks_top == 0 {
                panic!("timer IRQ: next TID={} has kernel_stack_top=0 (triple fault on Ring 3 entry)",
                    unsafe { (*next).tid });
            }
            unsafe {
                prepare_timer_return(next);
            }

            // Update per-CPU current thread and PID
            unsafe {
                crate::arch::x64::cpu_local::this_cpu_set_current_thread_site(next, crate::scheduler::diag::SITE_SET_IDT);
                crate::arch::x64::cpu_local::this_cpu_set_current_pid((*next).pid);
                crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
            }

            let next_rsp = unsafe { (*next).rsp };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs,
                flags: 0x01, path: 0,
                next_tid: unsafe { (*next).tid }, next_rsp,
                next_ks_top: unsafe { (*next).kernel_stack_top },
                returned_rsp: next_rsp,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 2,
            });
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();

            if is_bsp {
                let _ = crate::eventbus::EVENT_BUS.push_event(
                    crate::eventbus::EVENT_TIMER_TICK,
                    crate::eventbus::SOURCE_HAL,
                    1,
                    current_tick,
                    0,
                    0,
                );
            }

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
            if is_bsp {
                let _ = crate::eventbus::EVENT_BUS.push_event(
                    crate::eventbus::EVENT_TIMER_TICK,
                    crate::eventbus::SOURCE_HAL,
                    1,
                    current_tick,
                    0,
                    0,
                );
            }
            return current_rsp;
        }
    } else if current_is_idle {
        // ── Idle thread preemption ──────────────────────
        // Preempt idle if any other thread is ready
        let should_preempt = current_state == Some(ThreadState::Ready);

        crate::trace_timer_irq!(if should_preempt { 2u8 } else { 3u8 },
            tid, interrupted_cs, has_non_idle as u8);

        if should_preempt && has_non_idle {
            kdebug!(crate::log::LogSubsys::Sched, "[SCHED] PREEMPT idle tid={} has_non_idle={}",
                tid, has_non_idle);
            if let Some(k) = scheduler.current_kthread_mut() {
                crate::scheduler::diag::rsp_ev(crate::scheduler::diag::SITE_RSP_IDT_IDLE, k, current_rsp);
                k.rsp = current_rsp;
            }
            let next = scheduler.schedule();
            let next_ks = unsafe { (*next).kernel_stack_top };
            let next_rsp_tmp = unsafe { (*next).rsp };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs, flags: 0x02, path: 1,
                next_tid: unsafe { (*next).tid }, next_rsp: next_rsp_tmp,
                next_ks_top: next_ks, returned_rsp: 0,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 1,
            });
            netd_record_select(next as *const _ as u64, next_rsp_tmp, next_ks);
            unsafe {
                let nt = &mut *next;
                let idx = (nt.priority as usize).min(crate::scheduler::PRIORITY_COUNT as usize - 1);
                nt.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
                nt.ticks_since_scheduled = 0;
            }
            let next_ks_top = unsafe { (*next).kernel_stack_top };
            if next_ks_top == 0 {
                panic!("timer idle-preempt: next TID={} has kernel_stack_top=0",
                    unsafe { (*next).tid });
            }
            unsafe {
                prepare_timer_return(next);
                crate::arch::x64::cpu_local::this_cpu_set_current_thread_site(next, crate::scheduler::diag::SITE_SET_IDT);
                crate::arch::x64::cpu_local::this_cpu_set_current_pid((*next).pid);
                crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
            }
            let next_rsp = unsafe { (*next).rsp };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs, flags: 0x02, path: 1,
                next_tid: unsafe { (*next).tid }, next_rsp,
                next_ks_top: unsafe { (*next).kernel_stack_top }, returned_rsp: next_rsp,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 2,
            });
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            if is_bsp {
                let _ = crate::eventbus::EVENT_BUS.push_event(
                    crate::eventbus::EVENT_TIMER_TICK,
                    crate::eventbus::SOURCE_HAL,
                    1,
                    current_tick,
                    0,
                    0,
                );
            }

            crate::trace_cswitch!(tid as u64, unsafe { (*next).tid } as u64);
            return next_rsp;
        }
    } else {
        // ── Kernel thread (Ring 0, non-idle) preemption ──
        // Kernel threads are NOT preempted on every tick even if their
        // timeslice expired, because they may hold kernel locks.
        // However, if the thread yielded (state=Ready or yield_requested),
        // we DO preempt if another thread can run — but only for genuine
        // kernel/idle threads.
        //
        // #474: this branch is documented as kernel-thread-only, but a *user*
        // thread interrupted in Ring 0 (inside a syscall) also lands here
        // (is_user_mode is false because cs == 0x08). Its live `rsp` is a deep
        // kernel call frame, not a dispatch frame; publishing it Ready would
        // let a later `mov rsp,next_rsp; pop 15; iretq` consume arbitrary stack
        // data as RIP/CS (wild RIP -> INVALID_OPCODE). Such a thread must be
        // deferred to its syscall-return path, which saves the real Ring-3
        // frame; the fall-through below sets NEED_RESCHED so it is rescheduled.
        let current_is_kernel_thread = scheduler
            .find_kthread(tid)
            .map(|k| scheduler.is_kernel_thread(k))
            .unwrap_or(true);
        let should_preempt = crate::scheduler::schedule::ring0_publish_is_dispatchable(
            interrupted_cs,
            current_is_kernel_thread,
        ) && (current_state == Some(ThreadState::Ready) || current_yield);

        crate::trace_timer_irq!(if should_preempt { 2u8 } else { 0u8 },
            tid, interrupted_cs, has_non_idle as u8);

        if should_preempt && has_non_idle {
            kdebug!(crate::log::LogSubsys::Sched, "[SCHED] PREEMPT kernel tid={} reason=yield_or_expired has_non_idle={}",
                tid, has_non_idle);
            if let Some(k) = scheduler.current_kthread_mut() {
                crate::scheduler::diag::rsp_ev(crate::scheduler::diag::SITE_RSP_IDT_KERNEL, k, current_rsp);
                k.rsp = current_rsp;
                k.yield_requested = false;
                // The CPU actually executing the thread owns its re-enqueue.
                k.cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
                crate::scheduler::diag::ev(
                    crate::scheduler::diag::EV_TIMER_SAVE, k.cpu, k.tid, k.rsp,
                    k.state.to_u8() as u64);
                // Phase 13-A: publish only after the live context is saved.
                // For a timeslice expiry `on_timer_tick` already enqueued it;
                // `make_thread_ready` is then a no-op.
                //
                // #338: this branch serves non-user, non-idle threads — kernel
                // threads like netd, which run in Ring 0 **by design**. Their
                // Ring-0 dispatch frame is legitimate (they are dispatched via
                // `schedule()`, `require_ring3=false`), so the Ring-3 publication
                // gate must NOT be applied here: doing so starves netd, which
                // always interrupts in Ring 0. The gate is applied only to the
                // user-preempt branch above and to `on_timer_tick`.
                if k.state == ThreadState::Running {
                    crate::scheduler::Scheduler::make_thread_ready(k);
                }
            }
            let next = scheduler.schedule();
            let next_tid = unsafe { (*next).tid };
            let next_rsp = unsafe { (*next).rsp };
            let next_ks = unsafe { (*next).kernel_stack_top };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs, flags: 0x04, path: 2,
                next_tid, next_rsp, next_ks_top: next_ks, returned_rsp: 0,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 1,
            });
            netd_record_select(next as *const _ as u64, next_rsp, next_ks);
            if next_rsp == 0 {
                if unsafe { (*next).is_idle } {
                    crate::hal::ack_irq(32);
                    crate::invariants::timer_irq_exit();
                    crate::invariants::irq_exit_clear();
                    return current_rsp;
                }
                panic!("timer_handler: kernel preempt next TID={} has rsp=0", next_tid);
            }
            if next_tid == tid {
                // Same thread: no context switch needed
                if let Some(k) = scheduler.current_kthread_mut() {
                    crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_IDT_SAME, k);
                    k.state = ThreadState::Running;
                    let idx = (k.priority as usize).min(crate::scheduler::PRIORITY_COUNT as usize - 1);
                    k.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
                    k.ticks_since_scheduled = 0;
                }
                crate::hal::ack_irq(32);
                crate::invariants::timer_irq_exit();
                crate::invariants::irq_exit_clear();
                return current_rsp;
            }
            unsafe {
                let nt = &mut *next;
                let idx = (nt.priority as usize).min(crate::scheduler::PRIORITY_COUNT as usize - 1);
                nt.time_slice_remaining = crate::scheduler::TIME_SLICES[idx];
                nt.ticks_since_scheduled = 0;
            }
            let next_ks_top = unsafe { (*next).kernel_stack_top };
            if next_ks_top == 0 {
                panic!("timer kernel-preempt: next TID={} has kernel_stack_top=0",
                    unsafe { (*next).tid });
            }
            unsafe {
                crate::arch::x64::gdt::prepare_ring3_return(
                    next_ks_top, (*next).tid, (*next).pid);
                crate::arch::x64::cpu_local::this_cpu_set_current_thread_site(next, crate::scheduler::diag::SITE_SET_IDT);
                crate::arch::x64::cpu_local::this_cpu_set_current_pid((*next).pid);
                crate::arch::x64::cpu_local::this_cpu_inc_context_switch_count();
            }
            let next_rsp = unsafe { (*next).rsp };
            td_push(TimerDiagEntry {
                tick: current_tick, cur_tid: tid, cur_rsp: current_rsp,
                cur_cs: interrupted_cs, flags: 0x04, path: 2,
                next_tid: unsafe { (*next).tid }, next_rsp,
                next_ks_top: unsafe { (*next).kernel_stack_top }, returned_rsp: next_rsp,
                frame_rip: 0, frame_cs: 0, frame_rflags: 0, phase: 2,
            });
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            if is_bsp {
                let _ = crate::eventbus::EVENT_BUS.push_event(
                    crate::eventbus::EVENT_TIMER_TICK,
                    crate::eventbus::SOURCE_HAL,
                    1,
                    current_tick,
                    0,
                    0,
                );
            }
            crate::trace_cswitch!(tid as u64, unsafe { (*next).tid } as u64);
            return next_rsp;
        }
    }

    // ── Kernel mode interrupt (no preemption) ──
    // Per Source of Truth §6.2: Ring 0 code runs to completion.
    // If on_timer_tick() set state to Ready (timeslice expired),
    // restore it to Running because no context switch occurred.
    if tid > 0 {
        let alive = scheduler.current_kthread_mut()
            .is_some_and(|k| k.state != ThreadState::Terminated);
        if alive {
            if let Some(k) = scheduler.current_kthread_mut() {
                if k.state == ThreadState::Ready && k.tid == tid {
                    crate::scheduler::Scheduler::remove_from_run_queue(k);
                    crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_IDT_RESTORE, k);
                    k.state = ThreadState::Running;
                    if k.tid == 5 && crate::scheduler::sched_forensic_verbose() {
                        crate::serial_println!("[T5_TM] KERNEL-MODE restore Running tid=5 sched.current={} kprcb={:?}",
                            scheduler.current_tid, crate::arch::x64::cpu_local::try_per_cpu_tid());
                    }
                }
            }
            unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            return current_rsp;
        }
        unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
    }

    // ── Idle thread (TID 0/1) — no timeslice to manage ──
    if has_non_idle {
        unsafe { crate::arch::x64::cpu_local::this_cpu_set_need_resched(true); }
    }
    crate::hal::ack_irq(32);
    crate::invariants::timer_irq_exit();
    crate::invariants::irq_exit_clear();

    // Push TimerTick event (lock‑free, IRQ‑safe). BSP only: the event bus and
    // DPC queue are global and must have a single driver (Phase 13 SMP).
    if is_bsp {
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
    }

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
        // Phase 6: record ownership BEFORE decoder (lock-free, no serial).
        let cpu = if crate::hal::safe::GsBase::read() == 0 { 0 } else { unsafe { crate::arch::x64::cpu_local::this_cpu_id() } };
        let apic = if crate::hal::safe::GsBase::read() == 0 { 0 } else { unsafe { crate::arch::x64::cpu_local::this_cpu_apic_id() } };
        let tid = crate::scheduler::current_tid();
        let seq = KBD_IRQ_SEQ.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if (cpu as usize) < 16 {
            KBD_IRQ_CNT[cpu as usize].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
        let idx = KBD_IRQ_HEAD.fetch_add(1, core::sync::atomic::Ordering::Relaxed) % KBD_IRQ_RING_SIZE;
        unsafe { KBD_IRQ_RING[idx] = KbdIrqEntry { seq, cpu, apic, tid, scancode }; }
        // FIX v2: single-path direct — process synchronously in IRQ.
        // Previous dual-path (direct + queued) caused double-char.
        // Queued-only (push_event) left keyboard dead because dispatch_pending
        // is not guaranteed to run before the blocked READB waiter is checked
        // (idle dispatch delayed by IDLE_TIME_SLICE / scheduler state).
        // Direct path does push_byte+wake_blocked_readers immediately, matching
        // pre-P0 working behavior.
        let _seq = crate::kbd::event::kbd_event_handler_direct(scancode);
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
    // Publish the shared IDT base so APs can `lidt` a valid table. The previous
    // AP path loaded a freshly zeroed page, so the first interrupt on an AP
    // would triple fault.
    KERNEL_IDT_BASE.store(&*IDT as *const _ as u64, core::sync::atomic::Ordering::Release);
}

/// Base address of the shared kernel IDT, published after `IDT.load()`.
static KERNEL_IDT_BASE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// IDTR descriptor pointing at the shared kernel IDT. Before `init()` runs the
/// base is 0 (unusable); every AP calls this only after the BSP loaded the IDT.
pub fn kernel_idt_descriptor() -> crate::hal::raw::IdtDescriptor {
    let base = KERNEL_IDT_BASE.load(core::sync::atomic::Ordering::Acquire);
    crate::hal::raw::IdtDescriptor::from_raw((256 * 16 - 1) as u16, base)
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
