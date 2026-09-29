//! Bounded lock-order invariant checker for the filesystem I/O locks (#343).
//!
//! The kernel takes three global filesystem locks in one canonical order:
//!
//! ```text
//! VFS  ->  PAGE_CACHE  ->  BLOCK_DEVICES
//! ```
//!
//! Acquiring a lock while holding one of *lower* rank is an inversion and, on
//! SMP>1, can deadlock (a real occurrence motivated this check: `NeoDosFsV2`
//! took `BLOCK_DEVICES` before `PAGE_CACHE` while `flush_cache_if_needed`
//! took them the other way round — see
//! `docs/investigation/smp343-service-lock-order-deadlock.md`).
//!
//! This is diagnostic only: it never changes lock semantics, never blocks and
//! never allocates. Violations are counted in a lock-free counter so an
//! inversion becomes observable instead of a silent hang. Coverage is
//! per-CPU and therefore best-effort (a lock held across a context switch can
//! make the held-set stale); it is a regression guard, not the lock itself.

use core::sync::atomic::{AtomicU64, Ordering};

/// Lock ranks, ordered highest (acquired first) to lowest.
pub const VFS: u8 = 3;
pub const PAGE_CACHE: u8 = 2;
pub const BLOCK_DEVICES: u8 = 1;

const SLOTS: usize = 16;

static HELD: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static VIOLATIONS: AtomicU64 = AtomicU64::new(0);

#[inline]
fn slot() -> usize {
    (unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize) % SLOTS
}

/// Record that `rank` is about to be acquired. Returns `true` when this is an
/// inversion (a strictly-lower-ranked lock is already held on this CPU).
#[inline]
pub fn enter(rank: u8) -> bool {
    if rank == 0 || rank > 63 {
        return false;
    }
    let mask = HELD[slot()].load(Ordering::Relaxed);
    let inverted = mask & ((1u64 << rank) - 1) != 0;
    if inverted {
        VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    }
    HELD[slot()].fetch_or(1u64 << rank, Ordering::Relaxed);
    inverted
}

/// Record that `rank` has been released.
#[inline]
pub fn leave(rank: u8) {
    if rank == 0 || rank > 63 {
        return;
    }
    HELD[slot()].fetch_and(!(1u64 << rank), Ordering::Relaxed);
}

/// Total number of acquisition-order violations observed since reset.
pub fn violations() -> u64 {
    VIOLATIONS.load(Ordering::Relaxed)
}

/// RAII marker: declares the upcoming acquisition of `rank`. Declare it
/// *before* the lock guard so it drops *after* the lock is released.
pub struct Guard(u8);

impl Guard {
    #[inline]
    pub fn new(rank: u8) -> Self {
        enter(rank);
        Guard(rank)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        leave(self.0);
    }
}

pub fn register_tests() {
    use crate::{test_case, test_eq, test_true};

    test_case!("lock_order_canonical_is_clean", {
        // Reset and snapshot; other contexts are parked during tests.
        VIOLATIONS.store(0, Ordering::Relaxed);
        {
            let _v = Guard::new(VFS);
            let _p = Guard::new(PAGE_CACHE);
            let _b = Guard::new(BLOCK_DEVICES);
        }
        test_eq!(violations(), 0);
    });

    test_case!("lock_order_detects_block_then_page_cache", {
        VIOLATIONS.store(0, Ordering::Relaxed);
        // The exact #343 inversion: BLOCK_DEVICES held, then PAGE_CACHE.
        {
            let _b = Guard::new(BLOCK_DEVICES);
            let _p = Guard::new(PAGE_CACHE);
        }
        test_true!(violations() >= 1);
        // And the canonical order stays clean afterwards.
        VIOLATIONS.store(0, Ordering::Relaxed);
        {
            let _p = Guard::new(PAGE_CACHE);
            let _b = Guard::new(BLOCK_DEVICES);
        }
        test_eq!(violations(), 0);
    });

    test_case!("lock_order_detects_page_then_vfs", {
        VIOLATIONS.store(0, Ordering::Relaxed);
        {
            let _p = Guard::new(PAGE_CACHE);
            let _v = Guard::new(VFS);
        }
        test_true!(violations() >= 1);
    });
}
