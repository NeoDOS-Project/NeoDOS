use core::sync::atomic::{AtomicU64, Ordering};

pub static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

#[no_mangle]
#[inline(never)]
pub extern "C" fn get_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn sleep_hint(us: u32) {
    for _ in 0..us {
        unsafe { core::arch::asm!("out dx, al", in("dx") 0x80u16, in("al") 0u8,
            options(nomem, nostack, preserves_flags)); }
    }
}

/// Increment the tick counter by one.  Called by the timer IRQ handler.
#[no_mangle]
#[inline(never)]
pub extern "C" fn increment_ticks() {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
}

// ── Force ABI symbol retention ──
#[used]
static KEEP_TIME_GET_TICKS: unsafe extern "C" fn() -> u64 = get_ticks;
#[used]
static KEEP_TIME_SLEEP_HINT: unsafe extern "C" fn(u32) = sleep_hint;
#[used]
static KEEP_TIME_INCREMENT_TICKS: unsafe extern "C" fn() = increment_ticks;
