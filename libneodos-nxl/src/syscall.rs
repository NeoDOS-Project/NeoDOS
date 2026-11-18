// ============================================================
// Raw syscall wrappers (inline asm, not pub — internal only)
//
// The arguments are staged in registers that the block itself writes and that
// are declared as clobbered (`out(...) _`), so the compiler cannot place an
// input there. Copying straight into `rbx`/`rcx`/`rdx`/`r8`/`r9` let it assign
// two arguments to the same register and clobber one with the other: the kernel
// then received an address in the wrong register and returned EFAULT.
// ============================================================
use core::arch::asm;

pub(crate) unsafe fn syscall_0(n: u64) -> u64 {
    let r: u64;
    unsafe {
        asm!("mov rax, {n}", "int 0x80", n = in(reg) n, out("rax") r);
    }
    r
}

pub(crate) unsafe fn syscall_1(n: u64, a0: u64) -> u64 {
    let r: u64;
    unsafe {
        asm!(
            "mov r8, {a0}",
            "mov rax, {n}",
            "push rbx",
            "mov rbx, r8",
            "int 0x80",
            "pop rbx",
            n = in(reg) n,
            a0 = in(reg) a0,
            out("rax") r,
            out("r8") _,
        );
    }
    r
}

pub(crate) unsafe fn syscall_2(n: u64, a0: u64, a1: u64) -> u64 {
    let r: u64;
    unsafe {
        asm!(
            "mov r8, {a0}",
            "mov r9, {a1}",
            "mov rax, {n}",
            "push rbx", "push rcx",
            "mov rbx, r8",
            "mov rcx, r9",
            "int 0x80",
            "pop rcx", "pop rbx",
            n = in(reg) n,
            a0 = in(reg) a0,
            a1 = in(reg) a1,
            out("rax") r,
            out("r8") _, out("r9") _,
        );
    }
    r
}

pub(crate) unsafe fn syscall_3(n: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let r: u64;
    unsafe {
        asm!(
            "mov r8, {a0}",
            "mov r9, {a1}",
            "mov r10, {a2}",
            "mov rax, {n}",
            "push rbx", "push rcx", "push rdx",
            "mov rbx, r8",
            "mov rcx, r9",
            "mov rdx, r10",
            "int 0x80",
            "pop rdx", "pop rcx", "pop rbx",
            n = in(reg) n,
            a0 = in(reg) a0,
            a1 = in(reg) a1,
            a2 = in(reg) a2,
            out("rax") r,
            out("r8") _, out("r9") _, out("r10") _,
        );
    }
    r
}

pub(crate) unsafe fn syscall_4(n: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let r: u64;
    unsafe {
        asm!(
            "mov r10, {a0}",
            "mov r11, {a1}",
            "mov rdi, {a2}",
            "mov rsi, {a3}",
            "mov rax, {n}",
            "push rbx", "push rcx", "push rdx", "push r8",
            "mov rbx, r10",
            "mov rcx, r11",
            "mov rdx, rdi",
            "mov r8, rsi",
            "int 0x80",
            "pop r8", "pop rdx", "pop rcx", "pop rbx",
            n = in(reg) n,
            a0 = in(reg) a0,
            a1 = in(reg) a1,
            a2 = in(reg) a2,
            a3 = in(reg) a3,
            out("rax") r,
            out("r10") _, out("r11") _, out("rdi") _, out("rsi") _, out("r8") _,
        );
    }
    r
}

pub(crate) unsafe fn syscall_5(n: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    let r: u64;
    unsafe {
        asm!(
            "mov r10, {a0}",
            "mov r11, {a1}",
            "mov rdi, {a2}",
            "mov rsi, {a3}",
            "mov r12, {a4}",
            "mov rax, {n}",
            "push rbx", "push rcx", "push rdx", "push r8", "push r9",
            "mov rbx, r10",
            "mov rcx, r11",
            "mov rdx, rdi",
            "mov r8, rsi",
            "mov r9, r12",
            "int 0x80",
            "pop r9", "pop r8", "pop rdx", "pop rcx", "pop rbx",
            n = in(reg) n,
            a0 = in(reg) a0,
            a1 = in(reg) a1,
            a2 = in(reg) a2,
            a3 = in(reg) a3,
            a4 = in(reg) a4,
            out("rax") r,
            out("r10") _, out("r11") _, out("rdi") _, out("rsi") _, out("r12") _,
            out("r8") _, out("r9") _,
        );
    }
    r
}
