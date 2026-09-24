pub mod hpet;
pub mod apic;

use crate::log::LogSubsys;

/// Active timer source selected at boot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimerSource {
    Pit = 0,
    Hpet = 1,
    ApicTimer = 2,
}

use core::sync::atomic::{AtomicU8, Ordering};

/// The currently active timer source (0=PIT, 1=HPET, 2=APIC timer).
static ACTIVE_TIMER: AtomicU8 = AtomicU8::new(0);

/// Target tick interval in microseconds (1 ms = 1000 µs at 1 KHz).
pub const TICK_INTERVAL_US: u64 = 1000;

/// ACPI RSDP physical address from bootloader (0 if not provided).
pub static mut BOOT_RSDP_ADDR: u64 = 0;

/// Set the active timer source.
pub fn set_active(source: TimerSource) {
    ACTIVE_TIMER.store(source as u8, Ordering::SeqCst);
}

/// Get the active timer source.
pub fn active() -> TimerSource {
    match ACTIVE_TIMER.load(Ordering::Relaxed) {
        0 => TimerSource::Pit,
        1 => TimerSource::Hpet,
        2 => TimerSource::ApicTimer,
        _ => TimerSource::Pit,
    }
}

/// Initialize timer subsystem.
/// Attempts HPET first, falls back to PIT.
pub fn init() {
    kinfo!(LogSubsys::Timers, "Initializing timer subsystem...");

    if hpet::init_hpet() {
        kinfo!(LogSubsys::Timers, "HPET initialized at 1 KHz");
        set_active(TimerSource::Hpet);
        return;
    }

    kwarn!(LogSubsys::Timers, "HPET not available, using PIT (18.2 Hz)");
    set_active(TimerSource::Pit);
}
