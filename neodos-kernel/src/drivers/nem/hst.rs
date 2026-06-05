use crate::eventbus;
use crate::input;
use crate::hal;
use crate::drivers::caps::{CAP_IRQ, CAP_PORTIO, CAP_EVENT_BUS, CAP_INPUT, CAP_TIMING, CAP_LOG};
use crate::drivers::driver_runtime;
use crate::drivers::nem::driver::current_driver_id;
use crate::drivers::isolation;

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

/// Check that the current driver has the required capability.
/// Returns true if the capability is held or no driver context is set (kernel code).
fn check_cap(required: u64) -> bool {
    let id = current_driver_id();
    if id == 0 {
        return true; // kernel context — always allowed
    }
    driver_runtime::check_driver_cap(id, required).is_ok()
}

unsafe extern "C" fn hst_inb(port: u16) -> u8 {
    if !check_cap(CAP_PORTIO) { return 0; }
    hal::inb(port)
}
unsafe extern "C" fn hst_outb(port: u16, val: u8) {
    if !check_cap(CAP_PORTIO) { return; }
    hal::outb(port, val)
}
unsafe extern "C" fn hst_inw(port: u16) -> u16 {
    if !check_cap(CAP_PORTIO) { return 0; }
    hal::inw(port)
}
unsafe extern "C" fn hst_outw(port: u16, val: u16) {
    if !check_cap(CAP_PORTIO) { return; }
    hal::outw(port, val)
}
unsafe extern "C" fn hst_inl(port: u16) -> u32 {
    if !check_cap(CAP_PORTIO) { return 0; }
    hal::inl(port)
}
unsafe extern "C" fn hst_outl(port: u16, val: u32) {
    if !check_cap(CAP_PORTIO) { return; }
    hal::outl(port, val)
}
unsafe extern "C" fn hst_push_event(et: u32, src: u32, dev: u32, d0: u64, d1: u64, fl: u32) -> i64 {
    if !check_cap(CAP_EVENT_BUS) { return -1; }
    match eventbus::push_event(et, src, dev, d0, d1, fl) {
        Ok(id) => id as i64,
        Err(_) => -1,
    }
}
unsafe extern "C" fn hst_push_input(byte: u8) {
    if !check_cap(CAP_INPUT) { return; }
    input::push_byte(byte);
    crate::syscall::wake_blocked_readers();
}
unsafe extern "C" fn hst_get_ticks() -> u64 {
    if !check_cap(CAP_TIMING) { return 0; }
    hal::get_ticks()
}
unsafe extern "C" fn hst_ack_irq(vec: u8) {
    if !check_cap(CAP_IRQ) { return; }
    hal::ack_irq(vec);
}
unsafe extern "C" fn hst_log(_level: u32, msg: *const u8, len: usize) {
    if !check_cap(CAP_LOG) { return; }
    // X4: Validate driver pointer before dereferencing
    let driver_id = current_driver_id();
    if driver_id != 0 {
        if isolation::validate_export_ptr(msg, len, false).is_err() {
            crate::serial_println!("[ISO] DENIED: hst_log with invalid pointer from driver {}", driver_id);
            return;
        }
    }
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
