//! A3.4 SEH — User-mode Structured Exception Handling.

use core::arch::asm;

// ── Exception type constants ──

pub const EXCEPTION_DIVIDE_ERROR: u32 = 0;
pub const EXCEPTION_GPF: u32 = 13;
pub const EXCEPTION_PAGE_FAULT: u32 = 14;

pub type ExceptionHandler = extern "C" fn(u32, u64, u64) -> u32;

/// Register an SEH handler for the current thread.
///
/// `handler` is an optional callback (None to clear).
/// Returns 0 on success, -1 on error.
pub fn sys_set_exception_handler(handler: Option<ExceptionHandler>) -> i64 {
    let fn_addr = match handler {
        Some(f) => f as u64,
        None => 0u64,
    };
    let r: u64;
    unsafe {
        asm!(
            "push rbx",
            "mov rax, 29",
            "mov rbx, {a0}",
            "int 0x80",
            "pop rbx",
            a0 = in(reg) fn_addr,
            out("rax") r,
            out("rcx") _, out("rdx") _,
            out("rsi") _, out("rdi") _,
            out("r8") _, out("r9") _,
            out("r10") _, out("r11") _,
            out("r12") _, out("r13") _,
            out("r14") _, out("r15") _,
        );
    }
    let signed = r as i64;
    if signed < 0 { -1 } else { 0 }
}
