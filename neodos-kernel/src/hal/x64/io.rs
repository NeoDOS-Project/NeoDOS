use crate::hal::raw;

#[no_mangle]
#[inline(never)]
pub extern "C" fn inb(port: u16) -> u8 {
    unsafe { raw::raw_inb(port) }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn outb(port: u16, val: u8) {
    unsafe { raw::raw_outb(port, val); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn inw(port: u16) -> u16 {
    unsafe { raw::raw_inw(port) }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn outw(port: u16, val: u16) {
    unsafe { raw::raw_outw(port, val); }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn inl(port: u16) -> u32 {
    unsafe { raw::raw_inl(port) }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn outl(port: u16, val: u32) {
    unsafe { raw::raw_outl(port, val); }
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
