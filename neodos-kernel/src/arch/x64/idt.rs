use lazy_static::lazy_static;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::serial_println;
use crate::scheduler::{current_scheduler, ProcessState};
use crate::arch::x64::pic::PICS;

core::arch::global_asm!(
    ".extern timer_handler_inner",
    ".global timer_handler_asm",
    "timer_handler_asm:",
    // Save all registers
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
    
    // Call the Rust handler
    "mov rdi, rsp", // Pass current RSP as first argument
    "call timer_handler_inner",
    
    // The Rust handler returns the NEW RSP in RAX
    "mov rsp, rax",
    
    // Restore all registers from the new stack
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
    
    // IRETQ
    "iretq"
);

// INT 0x80 syscall trampolín.
//
// Mirrors timer_handler_asm: saves all GP registers, calls syscall_dispatch,
// then checks NEED_RESCHED flag. If set, calls syscall_try_resched(current_rsp)
// to switch stacks, similar to the timer preemption path.
core::arch::global_asm!(
    ".extern syscall_dispatch",
    ".extern syscall_try_resched",
    ".extern NEED_RESCHED",
    ".extern clear_need_resched",
    ".global syscall_handler_asm",
    "syscall_handler_asm:",
    // Save all GP registers (same order as timer trampolín)
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
    // Save syscall number in r15 before dispatch clobbers RAX
    "mov r15, [rsp]",
    // syscall_dispatch(rax, rbx, rcx, rdx)
    "mov rdi, [rsp + 0]",
    "mov rsi, [rsp + 8]",
    "mov rdx, [rsp + 16]",
    "mov rcx, [rsp + 24]",
    "call syscall_dispatch",
    "mov [rsp + 0], rax",
    // Check if original syscall was sys_exit (0) for shell return
    "test r15, r15",
    "jnz 1f",
    ".extern EXIT_NOW",
    "cmp byte ptr [rip + EXIT_NOW], 0",
    "jz 1f",
    "mov byte ptr [rip + EXIT_NOW], 0",
    ".extern exit_to_kernel",
    "jmp exit_to_kernel",
    // Check NEED_RESCHED for voluntary context switch (sys_yield, sys_waitpid, sys_read)
    "1:",
    "call clear_need_resched",
    "test al, al",
    "jz 3f",
    // ---- Reschedule requested ----
    "mov rdi, rsp",
    "call syscall_try_resched",
    "mov rsp, rax",
    "3:",
    // ---- Normal restore ----
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
        
        // Exceptions
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
        
        // IRQs
        unsafe {
            idt[32].set_handler_addr(x86_64::VirtAddr::new(timer_handler_asm as *const () as u64));
        }
        idt[33].set_handler_fn(keyboard_handler);   // IRQ1
        
        // Syscall (INT 0x80) — interrupt gate from Ring 3
        // Interrupt gate (IF=0) prevents timer preemption during syscall handling,
        // which can corrupt the shared TSS.RSP0 kernel stack frames.
        unsafe {
            idt[0x80]
                .set_handler_addr(x86_64::VirtAddr::new(syscall_handler_asm as *const () as u64))
                .set_privilege_level(x86_64::PrivilegeLevel::Ring3)
                .disable_interrupts(true);
        }
        
        idt
    };
}

// Exception handlers
extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    panic!("Divide by zero: {:#?}", stack_frame);
}

extern "x86-interrupt" fn debug_handler(_: InterruptStackFrame) {
    serial_println!("Debug exception");
}

extern "x86-interrupt" fn nmi_handler(_: InterruptStackFrame) {
    panic!("Non-maskable interrupt");
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    serial_println!("Breakpoint: {:#?}", stack_frame);
}

extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    panic!("Overflow: {:#?}", stack_frame);
}

extern "x86-interrupt" fn bounds_handler(stack_frame: InterruptStackFrame) {
    panic!("Bound range exceeded: {:#?}", stack_frame);
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    panic!("Invalid opcode: {:#?}", stack_frame);
}

extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    panic!("Device not available: {:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) -> ! {
    panic!("Double fault: {:#?}, error: {}", stack_frame, error_code);
}

extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic!("Invalid TSS: {:#?}, error: {}", stack_frame, error_code);
}

extern "x86-interrupt" fn segment_not_present_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic!("Segment not present: {:#?}, error: {}", stack_frame, error_code);
}

extern "x86-interrupt" fn stack_segment_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic!("Stack segment fault: {:#?}, error: {}", stack_frame, error_code);
}

extern "x86-interrupt" fn gpf_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    serial_println!(
        "GPF: error={:#x} rip={:#x} cs={:#x} rflags={:#x} rsp={:#x} tick={}",
        error_code,
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.code_segment,
        stack_frame.cpu_flags,
        stack_frame.stack_pointer.as_u64(),
        crate::scheduler::TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed),
    );
    panic!("General protection fault: {:?}, error: {}", stack_frame, error_code);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let virt = Cr2::read().as_u64();
    let is_user = error_code.contains(PageFaultErrorCode::USER_MODE);
    let is_write = error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE);
    let is_not_present = !error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION);

    // On-demand heap page allocation for user-mode accesses
    if is_user && is_not_present {
        if crate::arch::x64::paging::handle_heap_page_fault(virt, true, is_write) {
            return; // Instruction re-executed
        }
    }

    panic!(
        "Page fault @ 0x{:x} (user={}, write={}, np={}) — rip={:#x}",
        virt,
        is_user,
        is_write,
        is_not_present,
        stack_frame.instruction_pointer.as_u64(),
    );
}

extern "x86-interrupt" fn x87_handler(stack_frame: InterruptStackFrame) {
    panic!("x87 floating point: {:#?}", stack_frame);
}

extern "x86-interrupt" fn alignment_check_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    panic!("Alignment check: {:#?}, error: {}", stack_frame, error_code);
}

extern "x86-interrupt" fn machine_check_handler(stack_frame: InterruptStackFrame) -> ! {
    panic!("Machine check: {:#?}", stack_frame);
}

extern "x86-interrupt" fn simd_handler(stack_frame: InterruptStackFrame) {
    panic!("SIMD floating point: {:#?}", stack_frame);
}

extern "x86-interrupt" fn virtualization_handler(stack_frame: InterruptStackFrame) {
    panic!("Virtualization: {:#?}", stack_frame);
}

#[no_mangle]
pub extern "C" fn timer_handler_inner(current_rsp: u64) -> u64 {
    let current_tick = crate::scheduler::TIMER_TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1;

    // Check if periodic cache flush is needed (set flag, actual flush happens in safe context)
    {
        use core::sync::atomic::Ordering;
        let last_flush = crate::globals::LAST_FLUSH_TICK.load(Ordering::Relaxed);
        if current_tick.saturating_sub(last_flush) >= crate::globals::FLUSH_INTERVAL_TICKS {
            crate::globals::NEED_CACHE_FLUSH.store(true, Ordering::Relaxed);
        }
    }

    // Call TSRs for INT 0x1C (Timer Hook)
    crate::tsr::dispatch_interrupt(0x1C);

    let scheduler_mutex = current_scheduler();
    let mut scheduler = scheduler_mutex.lock();

    scheduler.on_timer_tick();

    let pid = scheduler.current_pid;

    // ── Idle process (PID 0) ───────────────────────────────────────
    // Idle uses its private IDLE_STACK, so the timer handler's pushes
    // go to that private stack, NOT to the global TSS.RSP0.  It is
    // therefore safe to context-switch away from idle in the timer
    // handler — the next process's saved frame on TSS.RSP0 will not
    // be overwritten.
    if pid == 0 {
        if !scheduler.has_non_idle_processes() {
            unsafe { PICS.lock().notify_end_of_interrupt(32); }
            return current_rsp;
        }
        // Save idle's own RSP (on idle stack) and find next ready
        // process.  The returned RSP points to the user process's
        // saved frame (on TSS.RSP0, set during that process's timer
        // tick or by init_ring3_interrupt_stack_frame).
        let next = scheduler.schedule();
        unsafe { PICS.lock().notify_end_of_interrupt(32); }
        return unsafe { (*next).rsp };
    }

    // ── Ring-3 process (PID > 0) ──────────────────────────────────
    // The timer handler's register pushes are on the GLOBAL TSS.RSP0
    // stack.  If we context-switched here, the returned RSP would
    // point to another process's saved frame — also on TSS.RSP0 — and
    // the next Ring-3→Ring-0 timer interrupt would reload TSS.RSP0
    // fresh, overwriting that saved frame with the new process's data.
    //
    // Instead: save the stack-pointer only, set NEED_RESCHED, and
    // return the SAME RSP so the interrupt returns to the current
    // process.  The actual context switch happens in the syscall-
    // return path (syscall_handler_asm → syscall_try_resched), which
    // runs with IF=0 (interrupt gate on INT 0x80), so no timer can
    // fire mid-switch.
    {
        let mut current_terminated = false;
        if let Some(current) = scheduler.current_process_mut() {
            if current.state != ProcessState::Terminated {
                current.rsp = current_rsp;
                current.state = ProcessState::Ready;
            } else {
                current_terminated = true;
            }
        } else {
            scheduler.current_pid = 0;
        }
        if current_terminated {
            unsafe { PICS.lock().notify_end_of_interrupt(32); }
            return current_rsp;
        }
        crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
    }

    unsafe { PICS.lock().notify_end_of_interrupt(32); }
    current_rsp
}

extern "x86-interrupt" fn keyboard_handler(_: InterruptStackFrame) {
    use crate::arch::x64::pic::PICS;
    use crate::drivers::keyboard::KeyboardDriver;
    
    unsafe {
        if let Some(scancode) = KeyboardDriver::read_scancode() {
            if let Some(ascii) = KeyboardDriver::scancode_to_ascii(scancode) {
                crate::input::push_byte(ascii);
                crate::syscall::wake_blocked_readers();
            }
            // Check Ctrl+Alt+Del after modifiers are updated by scancode_to_ascii
            if KeyboardDriver::ctrl_alt_del_pressed(scancode) {
                crate::serial_println!("[Ctrl+Alt+Del] Powering off...");
                PICS.lock().notify_end_of_interrupt(33);
                crate::arch::x64::poweroff();
            }
        }
        PICS.lock().notify_end_of_interrupt(33);
    }
}

pub fn init() {
    IDT.load();
}

// syscall_handler_asm is defined in the global_asm! block above.
// syscall_dispatch lives in src/syscall.rs and is linked by name.
