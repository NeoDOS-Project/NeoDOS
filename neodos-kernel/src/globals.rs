use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;
use lazy_static::lazy_static;
use crate::buffer::page_cache::PageCache;

/// Enable lock contention diagnostic output. Set to true at boot to trace
/// VFS/PAGE_CACHE/BLOCK_DEVICES lock wait/acquisition/release.
pub static LOCK_DIAG: AtomicBool = AtomicBool::new(false);

lazy_static! {
    pub static ref BLOCK_DEVICES: Mutex<crate::drivers::block::BlockDeviceManager> = Mutex::new(crate::drivers::block::BlockDeviceManager::new());
}
pub static PAGE_CACHE: Mutex<PageCache> = Mutex::new(PageCache::new());
pub static VFS: Mutex<crate::fs::vfs::Vfs> = Mutex::new(crate::fs::vfs::Vfs::new());

pub static NEED_CACHE_FLUSH: AtomicBool = AtomicBool::new(false);
pub static LAST_FLUSH_TICK: AtomicU64 = AtomicU64::new(0);
pub const FLUSH_INTERVAL_TICKS: u64 = 180;

/// Partition base LBA for primary block device (set during boot from GPT scan).
pub static PRIMARY_PARTITION_BASE: AtomicU64 = AtomicU64::new(0);

fn diag_enabled() -> bool {
    LOCK_DIAG.load(Ordering::Relaxed)
}

fn lock_wait(name: &str) {
    if diag_enabled() {
        let tid = crate::scheduler::current_tid();
        crate::serial_println!("[LOCK_WAIT] lock={} tid={}", name, tid);
    }
}

fn lock_acquire(name: &str) {
    if diag_enabled() {
        let tid = crate::scheduler::current_tid();
        crate::serial_println!("[LOCK_ACQUIRE] lock={} tid={}", name, tid);
    }
}

fn lock_release(name: &str) {
    if diag_enabled() {
        let tid = crate::scheduler::current_tid();
        crate::serial_println!("[LOCK_RELEASE] lock={} tid={}", name, tid);
    }
}

pub fn with_vfs<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::fs::vfs::Vfs) -> R
{
    with_vfs_site(crate::scheduler::diag::VFS_SITE_OTHER, f)
}

/// Like [`with_vfs`] but tags the caller site for #345 Phase 2B diagnostics.
pub fn with_vfs_site<F, R>(site: u64, f: F) -> R
where
    F: FnOnce(&mut crate::fs::vfs::Vfs) -> R
{
    // #376: a thread holding VFS must not be descheduled by the timer; a
    // Ready Ring-0 frame is rejected by the Ring-3 selection paths and the
    // lock would be held forever.
    crate::scheduler::preempt_disable();
    // Canonical order VFS -> PAGE_CACHE -> BLOCK_DEVICES (#343).
    let _order = crate::lock_order::Guard::new(crate::lock_order::VFS);
    let res = if diag_enabled() {
        let tid = crate::scheduler::current_tid();
        let _ = tid;
        if let Some(mut lock) = VFS.try_lock() {
            crate::scheduler::diag::vfs_owner_acquired(site);
            let res = f(&mut lock);
            crate::scheduler::diag::vfs_owner_released();
            res
        } else {
            lock_wait("VFS");
            // #345 Phase 2B: record the waiter + a bounded snapshot of the owner.
            let start = crate::scheduler::diag::vfs_waiter_begin(site);
            let mut lock = VFS.lock();
            crate::scheduler::diag::vfs_wait_end(start, site);
            crate::scheduler::diag::vfs_owner_acquired(site);
            lock_acquire("VFS");
            let res = f(&mut lock);
            lock_release("VFS");
            crate::scheduler::diag::vfs_owner_released();
            res
        }
    } else {
        let mut lock = VFS.lock();
        crate::scheduler::diag::vfs_owner_acquired(site);
        let res = f(&mut lock);
        crate::scheduler::diag::vfs_owner_released();
        res
    };
    crate::scheduler::preempt_enable();
    res
}

pub fn with_page_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut PageCache) -> R
{
    crate::scheduler::preempt_disable();
    let _order = crate::lock_order::Guard::new(crate::lock_order::PAGE_CACHE);
    let res = if diag_enabled() {
        let tid = crate::scheduler::current_tid();
        let _ = tid;
        if let Some(mut lock) = PAGE_CACHE.try_lock() {
            f(&mut lock)
        } else {
            lock_wait("PAGE_CACHE");
            let mut lock = PAGE_CACHE.lock();
            lock_acquire("PAGE_CACHE");
            let res = f(&mut lock);
            lock_release("PAGE_CACHE");
            res
        }
    } else {
        let mut lock = PAGE_CACHE.lock();
        f(&mut lock)
    };
    crate::scheduler::preempt_enable();
    res
}

pub fn with_block_devices<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::drivers::block::BlockDeviceManager) -> R
{
    crate::scheduler::preempt_disable();
    let _order = crate::lock_order::Guard::new(crate::lock_order::BLOCK_DEVICES);
    let res = if diag_enabled() {
        let tid = crate::scheduler::current_tid();
        let _ = tid;
        if let Some(mut lock) = BLOCK_DEVICES.try_lock() {
            f(&mut lock)
        } else {
            lock_wait("BLOCK_DEVICES");
            let mut lock = BLOCK_DEVICES.lock();
            lock_acquire("BLOCK_DEVICES");
            let res = f(&mut lock);
            lock_release("BLOCK_DEVICES");
            res
        }
    } else {
        let mut lock = BLOCK_DEVICES.lock();
        f(&mut lock)
    };
    crate::scheduler::preempt_enable();
    res
}

pub fn flush_cache_if_needed() {
    if NEED_CACHE_FLUSH.swap(false, Ordering::Relaxed) {
        crate::scheduler::preempt_disable();
        if let Some(mut pc_lock) = PAGE_CACHE.try_lock() {
            let _ord_pc = crate::lock_order::Guard::new(crate::lock_order::PAGE_CACHE);
            let _ord_bd = crate::lock_order::Guard::new(crate::lock_order::BLOCK_DEVICES);
            let mut bdev_lock = BLOCK_DEVICES.lock();
            if let Some(dev) = bdev_lock.get(0) {
                let batch_size = core::cmp::min(pc_lock.dirty_count(), 8);
                if batch_size > 0 {
                    let _ = pc_lock.flush_batch(dev, batch_size);
                }
            }
        }
        crate::scheduler::preempt_enable();
        let current = crate::hal::get_ticks();
        LAST_FLUSH_TICK.store(current, Ordering::Relaxed);
    }
}
