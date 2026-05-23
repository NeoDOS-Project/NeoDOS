use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::serial_println;
use crate::scheduler::{current_scheduler, ProcessState};
use crate::panic_classification::PanicClass;
use crate::trace::TraceEvent;

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
    ".extern NEED_RESCHED",
    ".extern clear_need_resched",
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
    "test r15, r15",
    "jnz 1f",
    ".extern EXIT_NOW",
    "cmp byte ptr [rip + EXIT_NOW], 0",
    "jz 1f",
    "mov byte ptr [rip + EXIT_NOW], 0",
    ".extern exit_to_kernel",
    "jmp exit_to_kernel",
    "1:",
    "call clear_need_resched",
    "test al, al",
    "jz 3f",
    "mov rdi, rsp",
    "call syscall_try_resched",
    "mov rsp, rax",
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

        unsafe {
            idt[0x80]
                .set_handler_addr(x86_64::VirtAddr::new(syscall_handler_asm as *const () as u64))
                .set_privilege_level(x86_64::PrivilegeLevel::Ring3)
                .disable_interrupts(true);
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

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    crate::trace_event!(TraceEvent::Panic, 0, 0, 0, 0);
    panic_classified!(PanicClass::UnknownCpuException,
        "Divide error: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn debug_handler(_: InterruptStackFrame) {
    serial_println!("[IRQ] Debug exception");
}

extern "x86-interrupt" fn nmi_handler(_: InterruptStackFrame) {
    crate::trace_event!(TraceEvent::Panic, 1, 0, 0, 0);
    panic_classified!(PanicClass::UnknownCpuException, "Non-maskable interrupt");
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    serial_println!("[IRQ] Breakpoint: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "Overflow: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn bounds_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "Bound range: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "Invalid opcode: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    panic_classified!(PanicClass::UnknownCpuException,
        "Device not available: rip={:#x}", stack_frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, error_code: u64) -> ! {
    crate::trace_event!(TraceEvent::Panic, 2, error_code, 0, 0);
    panic_classified!(PanicClass::DoubleFault,
        "Double fault: rip={:#x} rsp={:#x} error={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        error_code);
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
    serial_println!(
        "[IRQ] GPF: error={:#x} rip={:#x} cs={:#x} rflags={:#x} rsp={:#x} tick={}",
        error_code, rip,
        stack_frame.code_segment,
        stack_frame.cpu_flags, rsp,
crate::hal::get_ticks(),
        );
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
    }

    let rip = stack_frame.instruction_pointer.as_u64();
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

#[no_mangle]
pub extern "C" fn timer_handler_inner(current_rsp: u64) -> u64 {
    crate::invariants::timer_irq_enter();
    crate::invariants::irq_enter_check(32);

    crate::hal::increment_ticks();
    let current_tick = crate::hal::get_ticks();

    crate::trace_event!(TraceEvent::IrqTimerTick, current_tick, current_rsp, 0, 0);

    {
        use core::sync::atomic::Ordering;
        let last_flush = crate::globals::LAST_FLUSH_TICK.load(Ordering::Relaxed);
        if current_tick.saturating_sub(last_flush) >= crate::globals::FLUSH_INTERVAL_TICKS {
            crate::globals::NEED_CACHE_FLUSH.store(true, Ordering::Relaxed);
        }
    }

    crate::tsr::dispatch_interrupt(0x1C);

    let scheduler_mutex = current_scheduler();
    let mut scheduler = scheduler_mutex.lock();

    scheduler.on_timer_tick();

    let pid = scheduler.current_pid;

    // ── User process (PID > 0) ───────────────────────────────────
    // NEVER save current.rsp here — the timer may have fired during
    // Ring 0 execution (e.g., while the kernel was processing a
    // syscall), producing a 3-item iretq frame.  Only
    // syscall_try_resched saves RSP because INT 0x80 always comes
    // from Ring 3 with a full 5-item iretq frame.
    if pid > 0 {
        let alive = scheduler.current_process_mut()
            .is_some_and(|p| p.state != ProcessState::Terminated);
        if alive {
            crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
            crate::hal::ack_irq(32);
            crate::invariants::timer_irq_exit();
            crate::invariants::irq_exit_clear();
            return current_rsp;
        }
        // Process is dead or missing — fall through to idle
        scheduler.current_pid = 0;
    }

    // ── Idle process (PID 0) ─────────────────────────────────────
    if scheduler.has_non_idle_processes() {
        crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
    }
    crate::hal::ack_irq(32);
    crate::invariants::timer_irq_exit();
    crate::invariants::irq_exit_clear();

    // Push TimerTick event (lock‑free, IRQ‑safe)
    let _ = crate::eventbus::EVENT_BUS.push_event(
        crate::eventbus::EVENT_TIMER_TICK,
        crate::eventbus::SOURCE_HAL,
        1,   // pit device_id
        current_tick,
        0,
        0,
    );

    current_rsp
}

extern "x86-interrupt" fn keyboard_handler(_: InterruptStackFrame) {
    use crate::drivers::keyboard::KeyboardDriver;

    if let Some(scancode) = KeyboardDriver::read_scancode() {
        if let Some(ascii) = KeyboardDriver::scancode_to_ascii(scancode) {
            crate::input::push_byte(ascii);
            crate::syscall::wake_blocked_readers();
        }
        if KeyboardDriver::ctrl_alt_del_pressed(scancode) {
            crate::serial_println!("[IRQ] [Ctrl+Alt+Del] Powering off...");
            crate::hal::ack_irq(33);
            crate::hal::poweroff();
        }

        // Push KeyboardInput event (lock‑free, IRQ‑safe)
        let _ = crate::eventbus::EVENT_BUS.push_event(
            crate::eventbus::EVENT_KEYBOARD_INPUT,
            crate::eventbus::SOURCE_HAL,
            3,   // ps2kbd device_id
            scancode as u64,
            0,
            0,
        );
    }
    crate::hal::ack_irq(33);
}

pub fn init() {
    IDT.load();
}
