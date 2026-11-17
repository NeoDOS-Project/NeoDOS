//! Bounded lock-order invariant checker for the filesystem I/O locks (#343).
//!
//! The kernel takes a small set of global filesystem locks in one canonical
//! order:
//!
//! ```text
//! VFS  ->  MOUNT_MANAGER  ->  PAGE_CACHE  ->  BLOCK_DEVICES
//! ```
//!
//! `MOUNT_MANAGER` protects the `MountManager` (drive mount points + their Ob
//! namespace entries) and is only ever acquired while `VFS` is already held
//! (`vfs_mount_filesystem` / `vfs_unmount_filesystem`) or on its own, so it
//! ranks directly below `VFS`. Before this rank existed, the unified mount and
//! unmount paths took `VFS` and `MOUNT_MANAGER` without any lock-order
//! bookkeeping (issue #519), so an inversion involving them would have been
//! invisible to this checker.
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
///
/// Process-lifecycle locks (NEODOS-02/03, #632/#633; checker coverage added by
/// #667). Canonical order:
///
/// ```text
/// SCHEDULER -> USER_MEMORY_LOCK
///           -> ZOMBIE_QUEUE / KSTACK_QUARANTINE            (siblings)
///           -> OB_TABLE -> OB_NAMESPACE
///           -> VFS -> MOUNT_MANAGER -> PAGE_CACHE -> BLOCK_DEVICES
/// ```
///
/// `ZOMBIE_QUEUE`, `KSTACK_QUARANTINE` and `OB_TABLE` are never nested with
/// each other: the reaper drops the zombie lock before `recycle_terminated`
/// takes the Ob locks, and drains the kstack quarantine before locking the
/// queue. Their relative rank is therefore only a tie-breaker.
pub const SCHEDULER: u8 = 10;
pub const USER_MEMORY_LOCK: u8 = 9;
pub const ZOMBIE_QUEUE: u8 = 8;
pub const KSTACK_QUARANTINE: u8 = 7;
pub const OB_TABLE: u8 = 6;
pub const OB_NAMESPACE: u8 = 5;
pub const VFS: u8 = 4;
/// The `MountManager` (drive mount points). Acquired under `VFS`.
pub const MOUNT_MANAGER: u8 = 3;
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
            let _m = Guard::new(MOUNT_MANAGER);
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

    test_case!("lock_order_detects_mount_manager_then_vfs", {
        // #519 regression guard: the unified mount/unmount paths take VFS and
        // then MOUNT_MANAGER. Taking MOUNT_MANAGER before VFS is an inversion.
        VIOLATIONS.store(0, Ordering::Relaxed);
        {
            let _m = Guard::new(MOUNT_MANAGER);
            let _v = Guard::new(VFS);
        }
        test_true!(violations() >= 1);
        // VFS -> MOUNT_MANAGER is the canonical order and must stay clean.
        VIOLATIONS.store(0, Ordering::Relaxed);
        {
            let _v = Guard::new(VFS);
            let _m = Guard::new(MOUNT_MANAGER);
        }
        test_eq!(violations(), 0);
    });

    test_case!("lock_order_lifecycle_canonical_is_clean", {
        // #667: the documented lifecycle order must not be flagged.
        VIOLATIONS.store(0, Ordering::Relaxed);
        {
            let _s = Guard::new(SCHEDULER);
            let _u = Guard::new(USER_MEMORY_LOCK);
            let _z = Guard::new(ZOMBIE_QUEUE);
            let _k = Guard::new(KSTACK_QUARANTINE);
            let _o = Guard::new(OB_TABLE);
            let _n = Guard::new(OB_NAMESPACE);
        }
        test_eq!(violations(), 0);
    });

    test_case!("lock_order_detects_lifecycle_inversion", {
        // The Object Manager never acquires SCHEDULER (verified by inspection);
        // taking OB_TABLE then SCHEDULER is an inversion and must be detected.
        VIOLATIONS.store(0, Ordering::Relaxed);
        {
            let _o = Guard::new(OB_TABLE);
            let _s = Guard::new(SCHEDULER);
        }
        test_true!(violations() >= 1);
        // ZOMBIE_QUEUE is acquired under SCHEDULER, not the reverse.
        VIOLATIONS.store(0, Ordering::Relaxed);
        {
            let _z = Guard::new(ZOMBIE_QUEUE);
            let _u = Guard::new(USER_MEMORY_LOCK);
        }
        test_true!(violations() >= 1);
        // Canonical descent stays clean afterwards.
        VIOLATIONS.store(0, Ordering::Relaxed);
        {
            let _s = Guard::new(SCHEDULER);
            let _o = Guard::new(OB_TABLE);
        }
        test_eq!(violations(), 0);
    });
}
