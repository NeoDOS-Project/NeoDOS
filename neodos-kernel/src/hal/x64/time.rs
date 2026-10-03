use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use crate::hal::raw;

pub static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

/// System tick rate in Hz. Defaults to the PIT rate until the timer subsystem
/// registers the real source (see `set_timer_hooks`).
static TICK_RATE_HZ: AtomicU64 = AtomicU64::new(18);

/// Optional busy-wait hook (`fn(u32)`, microseconds); 0 = not registered.
static SLEEP_HOOK: AtomicUsize = AtomicUsize::new(0);

/// Optional timer-init hook (`fn()`); 0 = not registered.
static INIT_HOOK: AtomicUsize = AtomicUsize::new(0);

/// Register the timer subsystem's tick rate and optional hooks.
///
/// The HAL never reaches into `timers`; the timer subsystem pushes its state
/// down through this entry point at init time.
pub fn set_timer_hooks(tick_rate_hz: u64, sleep: Option<fn(u32)>, init: Option<fn()>) {
    TICK_RATE_HZ.store(tick_rate_hz, Ordering::Relaxed);
    SLEEP_HOOK.store(sleep.map_or(0, |f| f as usize), Ordering::Release);
    INIT_HOOK.store(init.map_or(0, |f| f as usize), Ordering::Release);
}

/// Busy-wait `us` microseconds using port 0x80.
///
/// Does not invoke the sleep hook, so the timer subsystem can use it from its
/// own fallback paths without recursion.
#[inline]
pub fn io_delay(us: u32) {
    for _ in 0..us {
        unsafe { raw::raw_outb(0x80u16, 0u8); }
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn get_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn sleep_hint(us: u32) {
    let hook = SLEEP_HOOK.load(Ordering::Acquire);
    if hook != 0 {
        let f: fn(u32) = unsafe { core::mem::transmute(hook) };
        f(us);
        return;
    }
    io_delay(us);
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn increment_ticks() {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn get_tick_rate() -> u64 {
    TICK_RATE_HZ.load(Ordering::Relaxed)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn init_system_timer() {
    let hook = INIT_HOOK.load(Ordering::Acquire);
    if hook != 0 {
        let f: fn() = unsafe { core::mem::transmute(hook) };
        f();
    }
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
