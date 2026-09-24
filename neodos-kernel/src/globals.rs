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
    if diag_enabled() {
        let tid = crate::scheduler::current_tid();
        if let Some(mut lock) = VFS.try_lock() {
            let res = f(&mut lock);
            res
        } else {
            lock_wait("VFS");
            let mut lock = VFS.lock();
            lock_acquire("VFS");
            let res = f(&mut lock);
            lock_release("VFS");
            res
        }
    } else {
        let mut lock = VFS.lock();
        f(&mut lock)
    }
}

pub fn with_page_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut PageCache) -> R
{
    if diag_enabled() {
        let tid = crate::scheduler::current_tid();
        if let Some(mut lock) = PAGE_CACHE.try_lock() {
            let res = f(&mut lock);
            res
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
    }
}

pub fn with_block_devices<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::drivers::block::BlockDeviceManager) -> R
{
    if diag_enabled() {
        let tid = crate::scheduler::current_tid();
        if let Some(mut lock) = BLOCK_DEVICES.try_lock() {
            let res = f(&mut lock);
            res
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
    }
}

pub fn flush_cache_if_needed() {
    if NEED_CACHE_FLUSH.swap(false, Ordering::Relaxed) {
        if let Some(mut pc_lock) = PAGE_CACHE.try_lock() {
            let mut bdev_lock = BLOCK_DEVICES.lock();
            if let Some(dev) = bdev_lock.get(0) {
                let batch_size = core::cmp::min(pc_lock.dirty_count(), 8);
                if batch_size > 0 {
                    let _ = pc_lock.flush_batch(dev, batch_size);
                }
            }
        }
        let current = crate::hal::get_ticks();
        LAST_FLUSH_TICK.store(current, Ordering::Relaxed);
    }
}
