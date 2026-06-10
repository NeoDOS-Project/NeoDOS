//! IRQL (Interrupt Request Level) — per-CPU interrupt priority mechanism.
//!
//! IRQL is a per-CPU priority mechanism that controls which interrupts
//! can fire. Higher IRQL levels mask more interrupts:
//!
//! ```text
//! PASSIVE  (0) — normal kernel/user code, all interrupts enabled
//! APC      (1) — APC delivery, most device interrupts enabled
//! DISPATCH (2) — DPC delivery + scheduler, timer/device IRQs masked
//! DIRQL    (3–11) — device interrupt handlers (mapped to vectors 32–40)
//! HIGH     (15) — NMI, machine check
//! ```
//!
//! On x86-64:
//! - PASSIVE/APC: interrupts stay enabled (STI)
//! - DISPATCH+: interrupts disabled (CLI)
//!
//! Storage: per-CPU `current_irql` field in KPRCB (GS-segment offset 0x016).

use crate::arch::x64::cpu_local;
use crate::hal::x64::cpu::{disable_interrupts, enable_interrupts};

// ── IRQL level constants ──────────────────────────────────────────────

/// Passive level — normal execution, all interrupts enabled.
pub const PASSIVE_LEVEL: u8 = 0;

/// APC level — APC delivery, most device interrupts still enabled.
pub const APC_LEVEL: u8 = 1;

/// DISPATCH level — DPC delivery + scheduler. Timer and device IRQs masked.
/// Page faults at this level are fatal (bugcheck KI_EXCEPTION_ACCESS_VIOLATION).
pub const DISPATCH_LEVEL: u8 = 2;

/// Minimum DIRQL — device interrupt handler level.
pub const DIRQL_BASE: u8 = 3;

/// Maximum IRQL (NMI, machine check).
pub const HIGH_LEVEL: u8 = 15;

// ── Platform IRQL functions ───────────────────────────────────────────

/// Raise the current CPU's IRQL to `new_level`.
///
/// Returns the previous IRQL. If the new level is higher than the current
/// and >= DISPATCH_LEVEL, disables interrupts (CLI).
///
/// # Safety
/// Must be called with interrupts enabled or at PASSIVE_LEVEL.
/// Caller must ensure `lower_irql` is called exactly once with the
/// returned old level.
#[inline(always)]
pub unsafe fn raise_irql(new_level: u8) -> u8 {
    let old_level = cpu_local::this_cpu_irql();
    if new_level > old_level {
        cpu_local::this_cpu_set_irql(new_level);
        if new_level >= DISPATCH_LEVEL {
            disable_interrupts();
        }
    }
    old_level
}

/// Lower the current CPU's IRQL to the previous level.
///
/// If the old level was below DISPATCH and the current level was at or
/// above DISPATCH, re-enables interrupts (STI).
///
/// # Safety
/// `old_level` must be the value returned by a prior `raise_irql` call.
#[inline(always)]
pub unsafe fn lower_irql(old_level: u8) {
    let current = cpu_local::this_cpu_irql();
    cpu_local::this_cpu_set_irql(old_level);
    if current >= DISPATCH_LEVEL && old_level < DISPATCH_LEVEL {
        enable_interrupts();
    }
}

/// Get the current CPU's IRQL level (read-only, no side effects).
#[inline(always)]
pub fn current_irql() -> u8 {
    unsafe { cpu_local::this_cpu_irql() }
}

/// Check if the current CPU is at DISPATCH_LEVEL or higher.
#[inline(always)]
pub fn at_or_above_dispatch() -> bool {
    current_irql() >= DISPATCH_LEVEL
}

// ── IrqMutex: spinlock with automatic IRQL raise/lower ────────────────

/// A wrapper around `spin::Mutex` that automatically raises IRQL to
/// `DISPATCH_LEVEL` on acquire and lowers it on release.
///
/// This is the preferred way to protect shared kernel state that must
/// not be interrupted by timer/device IRQs. Using `IrqMutex` directly
/// satisfies the invariant: holding a spinlock implies IRQL >= DISPATCH.
pub struct IrqMutex<T> {
    inner: spin::Mutex<T>,
}

impl<T> IrqMutex<T> {
    /// Create a new `IrqMutex` wrapping the given value.
    pub const fn new(val: T) -> Self {
        IrqMutex {
            inner: spin::Mutex::new(val),
        }
    }

    /// Lock the mutex, raising IRQL to DISPATCH_LEVEL.
    /// Returns an `IrqMutexGuard` that lowers IRQL on drop.
    #[inline]
    pub fn lock(&self) -> IrqMutexGuard<'_, T> {
        let old_irql = unsafe { raise_irql(DISPATCH_LEVEL) };
        let lock = self.inner.lock();
        IrqMutexGuard { lock, old_irql }
    }

    /// Try to lock the mutex without spinning. Returns `Some(IrqMutexGuard)`
    /// if the lock was acquired, `None` otherwise. On success, IRQL is raised.
    #[inline]
    pub fn try_lock(&self) -> Option<IrqMutexGuard<'_, T>> {
        let old_irql = unsafe { raise_irql(DISPATCH_LEVEL) };
        match self.inner.try_lock() {
            Some(lock) => Some(IrqMutexGuard { lock, old_irql }),
            None => {
                unsafe { lower_irql(old_irql) };
                None
            }
        }
    }
}

/// Guard returned by `IrqMutex::lock()`. Lowers IRQL when dropped.
pub struct IrqMutexGuard<'a, T: 'a + ?Sized> {
    lock: spin::MutexGuard<'a, T, spin::relax::Spin>,
    old_irql: u8,
}

impl<'a, T: ?Sized> core::ops::Deref for IrqMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.lock
    }
}

impl<'a, T: ?Sized> core::ops::DerefMut for IrqMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.lock
    }
}

impl<'a, T: ?Sized> Drop for IrqMutexGuard<'a, T> {
    fn drop(&mut self) {
        unsafe { lower_irql(self.old_irql) };
    }
}

// ── IRQL-aware execute helper ─────────────────────────────────────────

/// Execute a closure at DISPATCH_LEVEL, then restore the previous IRQL.
///
/// This is a drop-in replacement for `without_interrupts` that uses
/// IRQL semantics instead of blanket interrupt disable.
#[inline]
pub fn at_dispatch<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let old_irql = unsafe { raise_irql(DISPATCH_LEVEL) };
    let result = f();
    unsafe { lower_irql(old_irql) };
    result
}
