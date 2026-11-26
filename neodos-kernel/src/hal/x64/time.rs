use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use crate::hal::raw;

pub static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

/// System tick rate in Hz. Defaults to the PIT rate until the timer subsystem
/// registers the real source (see `set_timer_hooks`).
static TICK_RATE_HZ: AtomicU64 = AtomicU64::new(18);
static TIMER_SOURCE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Timer source identifier pushed by the timer subsystem (0=PIT,1=HPET,2=APIC).
pub fn set_timer_source(source: u8) {
    TIMER_SOURCE.store(source, Ordering::Relaxed);
}

pub fn timer_source() -> u8 {
    TIMER_SOURCE.load(Ordering::Relaxed)
}

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

/// Monotonic system uptime in milliseconds, plus the tick rate in Hz (#467).
///
/// Backed by the single BSP-only `TIMER_TICKS` counter, so it never decreases
/// and is SMP-safe (the value is global, not per-CPU).
pub fn uptime_ms() -> (u64, u32) {
    let hz = get_tick_rate().max(1);
    let ms = TIMER_TICKS
        .load(Ordering::Relaxed)
        .saturating_mul(1000)
        / hz;
    (ms, hz.min(u32::MAX as u64) as u32)
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

// ── TSC helpers (shared by net/benchmarks; HAL is a legal dependency) ──

static TSC_KHZ: AtomicU64 = AtomicU64::new(0);

/// Read the CPU Time-Stamp Counter (RDTSC).
#[inline(always)]
pub fn rdtsc() -> u64 {
    unsafe { raw::raw_read_tsc() }
}

/// Calibrate the TSC against PIT channel 2 (~10 ms). Returns ticks/ms.
fn calibrate_tsc_khz() -> u64 {
    const DELAY_TICKS: u16 = 11932; // ~10 ms at 1.193182 MHz

    let gate = crate::hal::inb(0x61);
    crate::hal::outb(0x61, (gate & 0xFD) | 0x01);
    crate::hal::outb(0x43, 0xB0);
    crate::hal::outb(0x42, (DELAY_TICKS & 0xFF) as u8);
    crate::hal::outb(0x42, (DELAY_TICKS >> 8) as u8);
    let gate2 = crate::hal::inb(0x61);
    crate::hal::outb(0x61, gate2 & 0xFE);
    crate::hal::outb(0x61, gate2 | 0x01);

    let start = rdtsc();
    while (crate::hal::inb(0x61) & 0x20) == 0 {}
    let end = rdtsc();
    let khz = end.saturating_sub(start) / 10;
    if khz == 0 { 1000 } else { khz }
}

/// Initialise TSC calibration once, early in boot. Returns ticks/ms.
pub fn init_tsc() -> u64 {
    let khz = calibrate_tsc_khz();
    TSC_KHZ.store(khz, Ordering::Relaxed);
    khz
}

/// Calibrated TSC frequency in ticks per millisecond (0 before init).
pub fn tsc_khz() -> u64 {
    TSC_KHZ.load(Ordering::Relaxed)
}

/// Convert a TSC delta to milliseconds using the calibrated frequency.
pub fn elapsed_ms(start: u64, end: u64) -> u64 {
    let khz = tsc_khz();
    if khz == 0 { return 0; }
    end.saturating_sub(start) / khz
}

/// Wall-clock date/time fields used by RTC and timezone code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct DateTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
}
