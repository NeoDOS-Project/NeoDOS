use core::sync::atomic::{AtomicU8, AtomicBool, Ordering};
use crate::eventbus::{EVENT_RTC_READ, EVENT_RTC_DATA, SOURCE_KERNEL};

static RTC_SECOND: AtomicU8 = AtomicU8::new(0);
static RTC_MINUTE: AtomicU8 = AtomicU8::new(0);
static RTC_HOUR: AtomicU8 = AtomicU8::new(0);
static RTC_DAY: AtomicU8 = AtomicU8::new(0);
static RTC_MONTH: AtomicU8 = AtomicU8::new(0);
static RTC_YEAR: AtomicU8 = AtomicU8::new(0);
static RTC_VALID: AtomicBool = AtomicBool::new(false);

pub struct DateTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
}

fn rtc_data_handler(event: &crate::eventbus::Event) {
    let packed = event.data0;
    RTC_SECOND.store(packed as u8, Ordering::Relaxed);
    RTC_MINUTE.store((packed >> 8) as u8, Ordering::Relaxed);
    RTC_HOUR.store((packed >> 16) as u8, Ordering::Relaxed);
    RTC_DAY.store((packed >> 24) as u8, Ordering::Relaxed);
    RTC_MONTH.store((packed >> 32) as u8, Ordering::Relaxed);
    RTC_YEAR.store((packed >> 40) as u8, Ordering::Relaxed);
    RTC_VALID.store(true, Ordering::Release);
}

pub fn init() {
    let _ = crate::eventbus::EVENT_BUS.register_handler(
        EVENT_RTC_DATA,
        rtc_data_handler,
        "rtc_bridge",
    );
}

pub fn request_datetime() -> Option<DateTime> {
    RTC_VALID.store(false, Ordering::Relaxed);
    let _ = crate::eventbus::EVENT_BUS.push_event(
        EVENT_RTC_READ, SOURCE_KERNEL, 0, 0, 0, 0,
    );
    crate::eventbus::EVENT_BUS.dispatch_pending();
    crate::eventbus::EVENT_BUS.dispatch_pending();
    if RTC_VALID.load(Ordering::Acquire) {
        Some(DateTime {
            second: RTC_SECOND.load(Ordering::Relaxed),
            minute: RTC_MINUTE.load(Ordering::Relaxed),
            hour: RTC_HOUR.load(Ordering::Relaxed),
            day: RTC_DAY.load(Ordering::Relaxed),
            month: RTC_MONTH.load(Ordering::Relaxed),
            year: RTC_YEAR.load(Ordering::Relaxed),
        })
    } else {
        None
    }
}
