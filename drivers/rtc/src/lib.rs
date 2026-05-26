#![no_std]
#![no_main]

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
    fn hst_push_event(et: u32, src: u32, dev: u32, d0: u64, d1: u64, fl: u32) -> i64;
}

const CMOS_ADDR: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
const REG_DAY: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
const REG_STATUS_B: u8 = 0x0B;

const EVENT_RTC_READ: u32 = 10;
const EVENT_RTC_DATA: u32 = 11;
const SOURCE_DRIVER: u32 = 1;

const STATUS_B_BCD: u8 = 0x04;

static INITIALIZED: AtomicU8 = AtomicU8::new(0);
static ACTIVE: AtomicU8 = AtomicU8::new(0);

fn bcd_to_bin(bcd: u8) -> u8 {
    ((bcd & 0xF0) >> 4) * 10 + (bcd & 0x0F)
}

fn read_cmos(reg: u8) -> u8 {
    unsafe {
        hst_outb(CMOS_ADDR, reg);
        hst_inb(CMOS_DATA)
    }
}

fn read_datetime() -> u64 {
    let second = read_cmos(REG_SECONDS);
    let minute = read_cmos(REG_MINUTES);
    let hour = read_cmos(REG_HOURS);
    let day = read_cmos(REG_DAY);
    let month = read_cmos(REG_MONTH);
    let year = read_cmos(REG_YEAR);
    let reg_b = read_cmos(REG_STATUS_B);

    if (reg_b & STATUS_B_BCD) == 0 {
        let second = bcd_to_bin(second);
        let minute = bcd_to_bin(minute);
        let hour = bcd_to_bin(hour);
        let day = bcd_to_bin(day);
        let month = bcd_to_bin(month);
        let year = bcd_to_bin(year);
        (second as u64)
            | ((minute as u64) << 8)
            | ((hour as u64) << 16)
            | ((day as u64) << 24)
            | ((month as u64) << 32)
            | ((year as u64) << 40)
    } else {
        (second as u64)
            | ((minute as u64) << 8)
            | ((hour as u64) << 16)
            | ((day as u64) << 24)
            | ((month as u64) << 32)
            | ((year as u64) << 40)
    }
}

#[no_mangle]
pub extern "C" fn driver_init() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) != 0 {
        return -1;
    }
    INITIALIZED.store(1, Ordering::Release);
    let msg = b"rtc.nem: init OK\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_activate() -> i32 {
    if INITIALIZED.load(Ordering::Relaxed) == 0 {
        return -1;
    }
    ACTIVE.store(1, Ordering::Release);
    let msg = b"rtc.nem: activated\r\n";
    unsafe { hst_log(2, msg.as_ptr(), msg.len()) };
    0
}

#[no_mangle]
pub extern "C" fn driver_on_event(event: *const NeoEvent) -> i32 {
    if ACTIVE.load(Ordering::Relaxed) == 0 || event.is_null() {
        return -1;
    }
    let ev = unsafe { &*event };
    if ev.event_type != EVENT_RTC_READ {
        return 1;
    }
    let packed = read_datetime();
    let _ = unsafe {
        hst_push_event(EVENT_RTC_DATA, SOURCE_DRIVER, 0, packed, 0, 0)
    };
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
