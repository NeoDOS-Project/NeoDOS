use core::sync::atomic::{AtomicU8, Ordering};
use crate::log::LogSubsys;

// ── Interrupt nesting counter (per-CPU) ─────────────────────────────
// Tracks IRQ entry depth for the current CPU. On a single core this should
// never exceed 1 (hardware clears IF on interrupt gate entry). A value > 1
// indicates illegal nested IRQ reentrancy on that CPU. This MUST be per-CPU:
// a global counter made two CPUs handling their own timer IRQ look like a
// nested reentrancy (Phase 13 SMP).

const MAX_CPUS: usize = crate::arch::x64::cpu_local::MAX_CPUS;
static IRQ_NESTING: [AtomicU8; MAX_CPUS] = [const { AtomicU8::new(0) }; MAX_CPUS];

const IRQ_NESTING_MAX: u8 = 1;

/// Index of the CPU executing this code (0 before GS is programmed).
#[inline]
fn invariants_cpu() -> usize {
    if crate::hal::safe::GsBase::read() == 0 {
        return 0;
    }
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() as usize };
    if cpu < MAX_CPUS { cpu } else { 0 }
}

#[inline]
pub fn irq_enter_check(vector: u8) -> bool {
    let cpu = invariants_cpu();
    let prev = IRQ_NESTING[cpu].fetch_add(1, Ordering::SeqCst);
    if prev >= IRQ_NESTING_MAX {
        // Nested IRQ detected!  Log and return false.
        kwarn!(LogSubsys::Kernel, "IRQ_REENTRANCY: cpu={} vector={}, nesting={}", cpu, vector, prev);
        false
    } else {
        true
    }
}

#[inline]
pub fn irq_exit_clear() {
    let cpu = invariants_cpu();
    let prev = IRQ_NESTING[cpu].fetch_sub(1, Ordering::SeqCst);
    if prev == 0 {
        kerror!(LogSubsys::Kernel, "IRQ nesting underflow! cpu={}", cpu);
    }
}

// ── Context switch guard (per-CPU) ──────────────────────────────────
// Prevents illegal context switches (e.g. from inside timer IRQ handler).

static IN_TIMER_IRQ: [AtomicU8; MAX_CPUS] = [const { AtomicU8::new(0) }; MAX_CPUS];

#[inline]
pub fn timer_irq_enter() {
    IN_TIMER_IRQ[invariants_cpu()].store(1, Ordering::SeqCst);
}

#[inline]
pub fn timer_irq_exit() {
    IN_TIMER_IRQ[invariants_cpu()].store(0, Ordering::SeqCst);
}

/// Returns true if the current CPU is inside the timer IRQ handler.
/// schedule() / resched should NOT be called in this context.
#[inline]
pub fn is_in_timer_irq() -> bool {
    IN_TIMER_IRQ[invariants_cpu()].load(Ordering::Relaxed) != 0
}

// ── Stack alignment check ───────────────────────────────────────────
// x86_64 ABI requires 16-byte stack alignment before call.
// RSP must be 8 mod 16 at function entry (after call pushes return addr).

#[inline]
pub fn check_stack_alignment(rsp: u64) -> bool {
    if rsp & 0xF != 0x8 {
        kwarn!(LogSubsys::Kernel, "STACK_MISALIGNED: RSP={:#x} (expected mod 16 == 8)", rsp);
        false
    } else {
        true
    }
}

#[inline]
pub fn check_kernel_stack(rsp: u64, stack_top: u64, stack_bottom: u64) -> bool {
    if rsp >= stack_top || rsp < stack_bottom {
        kerror!(LogSubsys::Kernel, "STACK_OUT_OF_BOUNDS: RSP={:#x} not in [{:#x}, {:#x})",
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
                $crate::serial_println!(
                    "[INV] {}:{}: {} FAILED",
                    core::file!(), core::line!(), core::stringify!($cond)
                );
                $crate::panic_classification::panic_with_class!(
                    $crate::panic_classification::PanicClass::AssertionFailed,
                    "assertion failed: {}", core::stringify!($cond)
                );
            }
        }
    };
    ($cond:expr, $($arg:tt)*) => {
        if cfg!(feature = "validation") {
            if !($cond) {
                $crate::serial_println!(
                    "[INV] {}:{}: {} FAILED — {}",
                    core::file!(), core::line!(), core::stringify!($cond),
                    format_args!($($arg)*)
                );
                $crate::panic_classification::panic_with_class!(
                    $crate::panic_classification::PanicClass::AssertionFailed,
                    $($arg)*
                );
            }
        }
    };
}
