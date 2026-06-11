use core::sync::atomic::{AtomicU64, Ordering};
use crate::hal::raw;

pub static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

#[no_mangle]
#[inline(never)]
pub extern "C" fn get_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn sleep_hint(us: u32) {
    if crate::timers::hpet::hpet_mmio_base() != 0 {
        crate::timers::hpet::sleep_us(us as u64);
        return;
    }
    for _ in 0..us {
        unsafe { raw::raw_outb(0x80u16, 0u8); }
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn increment_ticks() {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn get_tick_rate() -> u64 {
    match crate::timers::active() {
        crate::timers::TimerSource::Hpet | crate::timers::TimerSource::ApicTimer => {
            1_000_000_000 / crate::timers::TICK_INTERVAL_US
        }
        crate::timers::TimerSource::Pit => 18,
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn init_system_timer() {
    crate::timers::init();
}

// ── Force ABI symbol retention ──
#[used]
static KEEP_TIME_GET_TICKS: unsafe extern "C" fn() -> u64 = get_ticks;
#[used]
static KEEP_TIME_SLEEP_HINT: unsafe extern "C" fn(u32) = sleep_hint;
#[used]
static KEEP_TIME_INCREMENT_TICKS: unsafe extern "C" fn() = increment_ticks;
#[used]
static KEEP_TIME_GET_TICK_RATE: unsafe extern "C" fn() -> u64 = get_tick_rate;
#[used]
static KEEP_TIME_INIT_TIMER: unsafe extern "C" fn() = init_system_timer;
