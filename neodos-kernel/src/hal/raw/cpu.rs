use core::arch::asm;

#[inline]
pub unsafe fn raw_sti() {
    asm!("sti", options(nostack, nomem));
}

#[inline]
pub unsafe fn raw_cli() {
    asm!("cli", options(nostack, nomem));
}

#[inline]
pub unsafe fn raw_halt() -> ! {
    loop {
        asm!("hlt", options(nostack, nomem));
    }
}

#[inline]
pub unsafe fn raw_hlt_once() {
    asm!("hlt", options(nomem, nostack));
}

#[inline]
pub unsafe fn raw_pause() {
    asm!("pause", options(nomem, nostack));
}

#[inline]
pub unsafe fn raw_read_tsc() -> u64 {
    let low: u32;
    let high: u32;
    asm!("rdtsc", out("eax") low, out("edx") high, options(nomem, nostack));
    ((high as u64) << 32) | (low as u64)
}

#[inline]
pub unsafe fn raw_read_tscp() -> (u64, u32) {
    let low: u32;
    let high: u32;
    let aux: u32;
    asm!("rdtscp", out("eax") low, out("edx") high, out("ecx") aux, options(nomem, nostack));
    (((high as u64) << 32) | (low as u64), aux)
}

#[inline]
pub unsafe fn raw_cpuid(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let mut eax = leaf;
    let mut ecx = subleaf;
    let ebx: u32;
    let edx: u32;
    asm!(
        "push rbx",
        "cpuid",
        "mov {ebx_out:e}, ebx",
        "pop rbx",
        inout("eax") eax,
        inout("ecx") ecx,
        ebx_out = lateout(reg) ebx,
        lateout("edx") edx,
        options(nomem),
    );
    (eax, ebx, ecx, edx)
}

#[inline]
pub unsafe fn raw_read_cr0() -> u64 {
    let val: u64;
    asm!("mov {}, cr0", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_cr2() -> u64 {
    let val: u64;
    asm!("mov {}, cr2", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_cr3() -> u64 {
    let val: u64;
    asm!("mov {}, cr3", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_cr4() -> u64 {
    let val: u64;
    asm!("mov {}, cr4", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_write_cr3(val: u64) {
    asm!("mov cr3, {}", in(reg) val, options(nomem, nostack));
}

#[inline]
pub unsafe fn raw_invlpg(virt: u64) {
    asm!("invlpg [{}]", in(reg) virt, options(nostack, nomem, preserves_flags));
}

#[repr(C, packed)]
pub struct InvpcidDescriptor {
    pub pcid: u64,
    pub addr: u64,
}

#[inline]
pub unsafe fn raw_invpcid(descriptor: &InvpcidDescriptor, ty: u64) {
    asm!(
        "invpcid {0:r}, [{1}]",
        in(reg) ty,
        in(reg) descriptor,
        options(nostack),
    );
}

#[inline]
pub unsafe fn raw_rdrand() -> Option<u64> {
    let mut val: u64;
    let success: u8;
    asm!(
        "rdrand {0:r}",
        "setc {1}",
        out(reg) val,
        out(reg_byte) success,
        options(nomem, nostack),
    );
    if success != 0 { Some(val) } else { None }
}

#[inline]
pub unsafe fn raw_has_rdrand() -> bool {
    let (_, _, ecx, _) = raw_cpuid(1, 0);
    (ecx & (1 << 30)) != 0
}

#[inline]
pub unsafe fn raw_read_rflags() -> u64 {
    let flags: u64;
    asm!("pushfq; pop {}", out(reg) flags, options(nomem, nostack));
    flags
}

#[inline]
pub unsafe fn raw_lgdt(gdt_ptr: &super::GdtDescriptor) {
    asm!("lgdt [{}]", in(reg) gdt_ptr, options(nostack));
}

#[inline]
pub unsafe fn raw_lidt(idt_ptr: &super::IdtDescriptor) {
    asm!("lidt [{}]", in(reg) idt_ptr, options(nostack));
}

#[inline]
pub unsafe fn raw_ltr(sel: u16) {
    asm!("ltr {0:x}", in(reg) sel, options(nostack, nomem));
}

#[inline]
pub unsafe fn raw_set_segment_regs(ds: u16, _es: u16, _ss: u16) {
    asm!(
        "mov ds, {0:x}",
        "mov es, {0:x}",
        "mov ss, {0:x}",
        in(reg) ds,
    );
}

#[inline]
pub unsafe fn raw_set_gs(sel: u16) {
    asm!("mov gs, {0:x}", in(reg) sel, options(nostack, nomem));
}

#[inline]
pub unsafe fn raw_set_fs(sel: u16) {
    asm!("mov fs, {0:x}", in(reg) sel, options(nostack, nomem));
}

#[inline]
pub unsafe fn raw_read_rsp() -> u64 {
    let val: u64;
    asm!("mov {}, rsp", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_rbp() -> u64 {
    let val: u64;
    asm!("mov {}, rbp", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_rax() -> u64 {
    let val: u64;
    asm!("mov {}, rax", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_rbx() -> u64 {
    let val: u64;
    asm!("mov {}, rbx", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_rcx() -> u64 {
    let val: u64;
    asm!("mov {}, rcx", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_rdx() -> u64 {
    let val: u64;
    asm!("mov {}, rdx", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_rsi() -> u64 {
    let val: u64;
    asm!("mov {}, rsi", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_rdi() -> u64 {
    let val: u64;
    asm!("mov {}, rdi", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_r8() -> u64 {
    let val: u64;
    asm!("mov {}, r8", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_r9() -> u64 {
    let val: u64;
    asm!("mov {}, r9", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_r10() -> u64 {
    let val: u64;
    asm!("mov {}, r10", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_r11() -> u64 {
    let val: u64;
    asm!("mov {}, r11", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_r12() -> u64 {
    let val: u64;
    asm!("mov {}, r12", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_r13() -> u64 {
    let val: u64;
    asm!("mov {}, r13", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_r14() -> u64 {
    let val: u64;
    asm!("mov {}, r14", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_read_r15() -> u64 {
    let val: u64;
    asm!("mov {}, r15", out(reg) val, options(nomem, nostack));
    val
}

#[inline]
pub unsafe fn raw_rep_stosd(base: *mut u32, count: usize, color: u32) {
    asm!(
        "rep stosd",
        inout("rcx") count => _,
        inout("rdi") base => _,
        in("eax") color,
        options(nostack, preserves_flags)
    );
}

#[inline]
pub unsafe fn raw_debug_port_write(val: u8) {
    asm!("out dx, al", in("dx") 0x80u16, in("al") val, options(preserves_flags, nostack));
}

#[inline]
pub unsafe fn raw_gs_read_u64(offset: u32) -> u64 {
    let val: u64;
    asm!(
        "mov {0:r}, gs:[{1}]",
        out(reg) val,
        in(reg) offset as u64,
        options(nostack, nomem)
    );
    val
}

#[inline]
pub unsafe fn raw_gs_read_u32(offset: u32) -> u32 {
    let val: u32;
    asm!(
        "mov {0:e}, gs:[{1}]",
        out(reg) val,
        in(reg) offset as u64,
        options(nostack, nomem)
    );
    val
}

#[inline]
pub unsafe fn raw_gs_read_u16(offset: u32) -> u16 {
    let val: u32;
    asm!(
        "mov {0:e}, gs:[{1}]",
        out(reg) val,
        in(reg) offset as u64,
        options(nostack, nomem)
    );
    val as u16
}

#[inline]
pub unsafe fn raw_gs_read_u8(offset: u32) -> u8 {
    let val: u32;
    asm!(
        "mov {0:e}, gs:[{1}]",
        out(reg) val,
        in(reg) offset as u64,
        options(nostack, nomem)
    );
    val as u8
}

#[inline]
pub unsafe fn raw_gs_write_u64(offset: u32, val: u64) {
    asm!(
        "mov gs:[{0}], {1:r}",
        in(reg) offset as u64,
        in(reg) val,
        options(nostack, nomem)
    );
}

#[inline]
pub unsafe fn raw_gs_write_u16(offset: u32, val: u16) {
    asm!(
        "mov gs:[{0}], {1:e}",
        in(reg) offset as u64,
        in(reg) val as u32,
        options(nostack, nomem)
    );
}

#[inline]
pub unsafe fn raw_gs_write_u8(offset: u32, val: u8) {
    asm!(
        "mov gs:[{0}], {1:e}",
        in(reg) offset as u64,
        in(reg) val as u32,
        options(nostack, nomem)
    );
}
