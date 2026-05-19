use core::arch::asm;

#[no_mangle]
#[inline(never)]
pub extern "C" fn inb(port: u16) -> u8 {
    let val: u8;
    unsafe { asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack, preserves_flags)); }
    val
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn outb(port: u16, val: u8) {
    unsafe { asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags)); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn inw(port: u16) -> u16 {
    let val: u16;
    unsafe { asm!("in ax, dx", out("ax") val, in("dx") port, options(nomem, nostack, preserves_flags)); }
    val
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn outw(port: u16, val: u16) {
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags)); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn inl(port: u16) -> u32 {
    let val: u32;
    unsafe { asm!("in eax, dx", out("eax") val, in("dx") port, options(nomem, nostack, preserves_flags)); }
    val
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn outl(port: u16, val: u32) {
    unsafe { asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags)); }
}

// ── Force ABI symbol retention ──
#[used]
static KEEP_IO_INB: unsafe extern "C" fn(u16) -> u8 = inb;
#[used]
static KEEP_IO_OUTB: unsafe extern "C" fn(u16, u8) = outb;
#[used]
static KEEP_IO_INW: unsafe extern "C" fn(u16) -> u16 = inw;
#[used]
static KEEP_IO_OUTW: unsafe extern "C" fn(u16, u16) = outw;
#[used]
static KEEP_IO_INL: unsafe extern "C" fn(u16) -> u32 = inl;
#[used]
static KEEP_IO_OUTL: unsafe extern "C" fn(u16, u32) = outl;
