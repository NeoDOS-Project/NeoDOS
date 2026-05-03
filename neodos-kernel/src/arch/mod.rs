// x86_64 CPU intrinsics and control registers

#[inline]
pub fn read_cr3() -> u64 {
    let mut value: u64 = 0;
    unsafe {
        core::arch::asm!("mov rax, cr3", out("rax") value, options(nomem, nostack));
    }
    value
}

#[inline]
pub fn read_cr4() -> u64 {
    let mut value: u64 = 0;
    unsafe {
        core::arch::asm!("mov rax, cr4", out("rax") value, options(nomem, nostack));
    }
    value
}

#[inline]
pub fn read_rsp() -> u64 {
    let mut value: u64 = 0;
    unsafe {
        core::arch::asm!("mov rax, rsp", out("rax") value, options(nomem, nostack));
    }
    value
}

#[inline]
pub fn halt() -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nostack, nomem));
        }
    }
}

#[inline]
pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti");
    }
}

#[inline]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli");
    }
}
