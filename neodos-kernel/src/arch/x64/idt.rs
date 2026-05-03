use lazy_static::lazy_static;
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

extern "C" {
    fn timer_handler_asm();
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
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(0);  // IST0 for double fault
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
            idt[32].set_handler_addr(x86_64::VirtAddr::new(timer_handler_asm as u64));
        }
        idt[33].set_handler_fn(keyboard_handler);   // IRQ1
        
        // Syscall (INT 0x80)
        idt[0x80]
            .set_handler_fn(syscall_handler)
            .set_privilege_level(x86_64::PrivilegeLevel::Ring3);
        
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
    panic!("General protection fault: {:#?}, error: {}", stack_frame, error_code);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    panic!("Page fault: {:#?}, error: {:?}", stack_frame, error_code);
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
    let scheduler_mutex = current_scheduler();
    let mut scheduler = scheduler_mutex.lock();
    
    // Call TSRs for INT 0x1C (Timer Hook)
    crate::tsr::dispatch_interrupt(0x1C);

    scheduler.on_timer_tick();
    
    // Save current process state
    let pid = scheduler.current_pid;
    if pid > 0 {
        let current = scheduler.current_process();
        current.rsp = current_rsp;
        current.state = ProcessState::Ready;
    }
    
    // Switch to next process
    let next = scheduler.schedule();
    
    // ACK the interrupt
    unsafe {
        PICS.lock().notify_end_of_interrupt(32);
    }
    
    unsafe { (*next).rsp }
}

extern "x86-interrupt" fn keyboard_handler(_: InterruptStackFrame) {
    use crate::arch::x64::pic::PICS;
    
    // Call TSRs for INT 0x21 (DOS Call hook simulation)
    crate::tsr::dispatch_interrupt(0x21);

    // Minimal keyboard handler: just read and ignore for now
    unsafe {
        let mut port = x86_64::instructions::port::Port::<u8>::new(0x60);
        let _scancode = port.read();
        PICS.lock().notify_end_of_interrupt(33);
    }
}

pub fn init() {
    IDT.load();
}

extern "x86-interrupt" fn syscall_handler(stack_frame: InterruptStackFrame) {
    // Basic syscall handler for Phase 6
    serial_println!("Syscall invoked from Ring 3!");
    // Later we will read RAX for the syscall number
}
