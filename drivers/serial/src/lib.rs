#![no_std]
#![no_main]
#![allow(dead_code)]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU8, Ordering};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
pub struct NeoEvent {
    pub event_id: u64,
    pub event_type: u32,
    pub source: u32,
    pub timestamp: u64,
    pub device_id: u32,
    pub driver_target: u32,
    pub data0: u64,
    pub data1: u64,
    pub flags: u32,
}

extern "C" {
    fn hst_inb(port: u16) -> u8;
    fn hst_outb(port: u16, val: u8);
    fn hst_log(level: u32, msg: *const u8, len: usize);
    fn hst_push_input_byte(byte: u8);
}

const COM1: u16 = 0x3F8;

const EVENT_SERIAL_DATA: u32 = 2;

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);

fn serial_init() {
    unsafe {
        hst_outb(COM1 + 1, 0x00);
        // Drain any stale byte from the receiver
        if hst_inb(COM1 + 5) & 1 != 0 {
            hst_inb(COM1);
        }
        hst_outb(COM1 + 3, 0x80);
        hst_outb(COM1, 0x03);
        hst_outb(COM1 + 1, 0x00);
        hst_outb(COM1 + 3, 0x03);
        hst_outb(COM1 + 2, 0xC7);
        hst_outb(COM1 + 4, 0x0B);
        hst_outb(COM1 + 1, 0x01);
    }
}

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }
    serial_init();
    INITIALIZED.store(1, Ordering::Release);
    let msg = b"serial.nem: init OK\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    ACTIVE.store(1, Ordering::Release);
    let msg = b"serial.nem: activated\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 || event.is_null() {
        return -1;
    }
    let ev = unsafe { &*event };
    if ev.event_type != EVENT_SERIAL_DATA {
        return 1;
    }
    // Echo received byte to serial output (loopback)
    unsafe { hst_outb(COM1, ev.data0 as u8) };
    0
}

#[no_mangle]
pub extern "C" fn driver_fini() {
    ACTIVE.store(0, Ordering::Release);
    INITIALIZED.store(0, Ordering::Release);
}

#[no_mangle]
pub extern "C" fn driver_is_active() -> i32 {
    if ACTIVE.load(Ordering::Relaxed) != 0 { 1 } else { 0 }
}
