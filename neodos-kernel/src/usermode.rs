use core::arch::asm;
use crate::arch::x64::gdt::get_selectors;

#[no_mangle]
static mut EXIT_RSP: u64 = 0;
#[no_mangle]
static mut EXIT_RIP: u64 = 0;

core::arch::global_asm!(
    ".global execute_usermode_asm",
    "execute_usermode_asm:",
    // RDI = entry_point, RSI = stack_pointer, RDX = user_code, RCX = user_data
    // Save return context so sys_exit can longjmp back here
    "lea rax, [rip + 1f]",
    "mov [rip + EXIT_RIP], rax",
    "mov [rip + EXIT_RSP], rsp",
    // Push Ring-3 frame
    "push rcx",
    "push rsi",
    "push 0x200",
    "push rdx",
    "push rdi",
    "iretq",
    "1:",
    // Re-enable interrupts (INT 0x80 entry disables them)
    "sti",
    "ret",

    ".global exit_to_kernel",
    "exit_to_kernel:",
    "mov rsp, [rip + EXIT_RSP]",
    "push [rip + EXIT_RIP]",
    "ret",
);

extern "C" {
    fn execute_usermode_asm(entry: u64, stack: u64, cs: u64, ss: u64);
    fn exit_to_kernel();
}

pub fn execute_usermode(entry_point: u64, stack_pointer: u64) {
    let selectors = get_selectors();
    unsafe {
        execute_usermode_asm(
            entry_point,
            stack_pointer,
            selectors.user_code.0 as u64,
            selectors.user_data.0 as u64,
        );
    }
}
