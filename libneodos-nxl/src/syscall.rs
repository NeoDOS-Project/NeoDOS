// ============================================================
// Raw syscall wrappers (inline asm, not pub — internal only)
// ============================================================
use core::arch::asm;

pub(crate) unsafe fn syscall_0(n: u64) -> u64 {
    let r: u64;
    asm!("mov rax, {}", "int 0x80", in(reg) n, out("rax") r);
    r
}

pub(crate) unsafe fn syscall_1(n: u64, a0: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx",
        "mov rax, {n}", "mov rbx, {a0}", "int 0x80",
        "pop rbx",
        n = in(reg) n, a0 = in(reg) a0,
        out("rax") r,
    );
    r
}

pub(crate) unsafe fn syscall_2(n: u64, a0: u64, a1: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "int 0x80",
        "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1,
        out("rax") r,
    );
    r
}

pub(crate) unsafe fn syscall_3(n: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx", "push rdx",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "mov rdx, {a2}", "int 0x80",
        "pop rdx", "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1, a2 = in(reg) a2,
        out("rax") r,
    );
    r
}

pub(crate) unsafe fn syscall_4(n: u64, a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx", "push rdx", "push r8",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "mov rdx, {a2}",
        "mov r8, {a3}", "int 0x80",
        "pop r8", "pop rdx", "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1,
        a2 = in(reg) a2, a3 = in(reg) a3,
        out("rax") r,
    );
    r
}

pub(crate) unsafe fn syscall_5(n: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx", "push rdx", "push r8", "push r9",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "mov rdx, {a2}",
        "mov r8, {a3}", "mov r9, {a4}", "int 0x80",
        "pop r9", "pop r8", "pop rdx", "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1,
        a2 = in(reg) a2, a3 = in(reg) a3, a4 = in(reg) a4,
        out("rax") r,
    );
    r
}
