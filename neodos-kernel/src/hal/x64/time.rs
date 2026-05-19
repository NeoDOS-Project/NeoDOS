use core::sync::atomic::Ordering;

pub extern "C" fn get_ticks() -> u64 {
    crate::scheduler::TIMER_TICKS.load(Ordering::Relaxed)
}

pub extern "C" fn sleep_hint(us: u32) {
    for _ in 0..us {
        unsafe { core::arch::asm!("out dx, al", in("dx") 0x80u16, in("al") 0u8,
            options(nomem, nostack, preserves_flags)); }
    }
}
