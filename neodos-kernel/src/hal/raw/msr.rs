use core::arch::asm;

#[inline]
pub unsafe fn raw_read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    asm!(
        "rdmsr",
        out("eax") low,
        out("edx") high,
        in("ecx") msr,
        options(nomem, nostack)
    );
    ((high as u64) << 32) | (low as u64)
}

#[inline]
pub unsafe fn raw_write_msr(msr: u32, val: u64) {
    let low = val as u32;
    let high = (val >> 32) as u32;
    asm!(
        "wrmsr",
        in("eax") low,
        in("edx") high,
        in("ecx") msr,
        options(nomem, nostack)
    );
}
