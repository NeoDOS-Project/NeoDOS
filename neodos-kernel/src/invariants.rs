use core::sync::atomic::{AtomicU8, Ordering};

// ── Interrupt nesting counter ───────────────────────────────────────
// Tracks IRQ entry depth.  On a single-core system this should never
// exceed 1 (hardware clears IF on interrupt gate entry).
// A value > 1 indicates illegal nested IRQ reentrancy.

static IRQ_NESTING: AtomicU8 = AtomicU8::new(0);

const IRQ_NESTING_MAX: u8 = 1;

#[inline]
pub fn irq_enter_check(vector: u8) -> bool {
    let prev = IRQ_NESTING.fetch_add(1, Ordering::SeqCst);
    if prev >= IRQ_NESTING_MAX {
        // Nested IRQ detected!  Log and return false.
        crate::serial_println!("[INVARIANT] IRQ_REENTRANCY: vector={}, nesting={}", vector, prev);
        false
    } else {
        true
    }
}

#[inline]
pub fn irq_exit_clear() {
    let prev = IRQ_NESTING.fetch_sub(1, Ordering::SeqCst);
    if prev == 0 {
        crate::serial_println!("[INVARIANT] IRQ nesting underflow!");
    }
}

// ── Context switch guard ────────────────────────────────────────────
// Prevents illegal context switches (e.g. from inside timer IRQ handler).

static IN_TIMER_IRQ: AtomicU8 = AtomicU8::new(0);

#[inline]
pub fn timer_irq_enter() {
    IN_TIMER_IRQ.store(1, Ordering::SeqCst);
}

#[inline]
pub fn timer_irq_exit() {
    IN_TIMER_IRQ.store(0, Ordering::SeqCst);
}

/// Returns true if currently inside timer IRQ handler.
/// schedule() / resched should NOT be called in this context.
#[inline]
pub fn is_in_timer_irq() -> bool {
    IN_TIMER_IRQ.load(Ordering::Relaxed) != 0
}

// ── Stack alignment check ───────────────────────────────────────────
// x86_64 ABI requires 16-byte stack alignment before call.
// RSP must be 8 mod 16 at function entry (after call pushes return addr).

#[inline]
pub fn check_stack_alignment(rsp: u64) -> bool {
    if rsp & 0xF != 0x8 {
        crate::serial_println!("[INVARIANT] STACK_MISALIGNED: RSP={:#x} (expected mod 16 == 8)", rsp);
        false
    } else {
        true
    }
}

#[inline]
pub fn check_kernel_stack(rsp: u64, stack_top: u64, stack_bottom: u64) -> bool {
    if rsp >= stack_top || rsp < stack_bottom {
        crate::serial_println!("[INVARIANT] STACK_OUT_OF_BOUNDS: RSP={:#x} not in [{:#x}, {:#x})",
            rsp, stack_bottom, stack_top);
        false
    } else {
        true
    }
}

// ── Assertion macros (cfg-enabled) ──────────────────────────────────

#[macro_export]
macro_rules! kern_assert {
    ($cond:expr) => {
        if cfg!(feature = "validation") {
            if !($cond) {
                crate::serial_println!(
                    "[ASSERT] {}:{}: {} FAILED",
                    core::file!(), core::line!(), core::stringify!($cond)
                );
                crate::panic_classification::panic_with_class!(
                    crate::panic_classification::PanicClass::AssertionFailed,
                    "assertion failed: {}", core::stringify!($cond)
                );
            }
        }
    };
    ($cond:expr, $($arg:tt)*) => {
        if cfg!(feature = "validation") {
            if !($cond) {
                crate::serial_println!(
                    "[ASSERT] {}:{}: {} FAILED — {}",
                    core::file!(), core::line!(), core::stringify!($cond),
                    format_args!($($arg)*)
                );
                crate::panic_classification::panic_with_class!(
                    crate::panic_classification::PanicClass::AssertionFailed,
                    $($arg)*
                );
            }
        }
    };
}
