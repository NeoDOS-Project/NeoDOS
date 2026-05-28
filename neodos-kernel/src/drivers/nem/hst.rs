use crate::eventbus;
use crate::input;
use crate::hal;

pub type HstInb = unsafe extern "C" fn(u16) -> u8;
pub type HstOutb = unsafe extern "C" fn(u16, u8);
pub type HstInw = unsafe extern "C" fn(u16) -> u16;
pub type HstOutw = unsafe extern "C" fn(u16, u16);
pub type HstInl = unsafe extern "C" fn(u16) -> u32;
pub type HstOutl = unsafe extern "C" fn(u16, u32);
pub type HstPushEvent = unsafe extern "C" fn(u32, u32, u32, u64, u64, u32) -> i64;
pub type HstPushInput = unsafe extern "C" fn(u8);
pub type HstGetTicks = unsafe extern "C" fn() -> u64;
pub type HstAckIrq = unsafe extern "C" fn(u8);
pub type HstLog = unsafe extern "C" fn(u32, *const u8, usize);

#[repr(C)]
pub struct HalServiceTable {
    pub inb: HstInb,
    pub outb: HstOutb,
    pub inw: HstInw,
    pub outw: HstOutw,
    pub inl: HstInl,
    pub outl: HstOutl,
    pub push_event: HstPushEvent,
    pub push_input_byte: HstPushInput,
    pub get_ticks: HstGetTicks,
    pub ack_irq: HstAckIrq,
    pub log: HstLog,
}

unsafe extern "C" fn hst_inb(port: u16) -> u8 { hal::inb(port) }
unsafe extern "C" fn hst_outb(port: u16, val: u8) { hal::outb(port, val) }
unsafe extern "C" fn hst_inw(port: u16) -> u16 { hal::inw(port) }
unsafe extern "C" fn hst_outw(port: u16, val: u16) { hal::outw(port, val) }
unsafe extern "C" fn hst_inl(port: u16) -> u32 { hal::inl(port) }
unsafe extern "C" fn hst_outl(port: u16, val: u32) { hal::outl(port, val) }
unsafe extern "C" fn hst_push_event(et: u32, src: u32, dev: u32, d0: u64, d1: u64, fl: u32) -> i64 {
    match eventbus::push_event(et, src, dev, d0, d1, fl) {
        Ok(id) => id as i64,
        Err(_) => -1,
    }
}
unsafe extern "C" fn hst_push_input(byte: u8) {
    input::push_byte(byte);
    crate::syscall::wake_blocked_readers();
}
unsafe extern "C" fn hst_get_ticks() -> u64 { hal::get_ticks() }
unsafe extern "C" fn hst_ack_irq(vec: u8) { hal::ack_irq(vec); }
unsafe extern "C" fn hst_log(level: u32, msg: *const u8, len: usize) {
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(msg, len)) };
    crate::serial_println!("[DRV] {}", s);
}

pub fn build_hst() -> HalServiceTable {
    HalServiceTable {
        inb: hst_inb,
        outb: hst_outb,
        inw: hst_inw,
        outw: hst_outw,
        inl: hst_inl,
        outl: hst_outl,
        push_event: hst_push_event,
        push_input_byte: hst_push_input,
        get_ticks: hst_get_ticks,
        ack_irq: hst_ack_irq,
        log: hst_log,
    }
}
