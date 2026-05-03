use core::arch::asm;
use crate::arch::x64::gdt::get_selectors;

pub fn execute_usermode(entry_point: u64, stack_pointer: u64) {
    let selectors = get_selectors();
    
    // The trick to entering Ring 3 is to "return" from an interrupt.
    // The IRETQ instruction expects 5 values on the stack:
    // 1. SS (User Data Segment)
    // 2. RSP (User Stack Pointer)
    // 3. RFLAGS
    // 4. CS (User Code Segment)
    // 5. RIP (User Entry Point)
    
    unsafe {
        asm!(
            // Push SS (User Data Segment)
            "push {ss}",
            // Push RSP (User Stack Pointer)
            "push {rsp}",
            // Push RFLAGS (Interrupts enabled: 0x200)
            "push 0x200",
            // Push CS (User Code Segment)
            "push {cs}",
            // Push RIP (User Entry Point)
            "push {rip}",
            // Execute IRETQ to drop privileges and jump!
            "iretq",
            
            ss = in(reg) selectors.user_data.0,
            rsp = in(reg) stack_pointer,
            cs = in(reg) selectors.user_code.0,
            rip = in(reg) entry_point,
            options(noreturn)
        );
    }
}
