use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;
use lazy_static::lazy_static;
use crate::buffer::page_cache::PageCache;

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

pub fn with_vfs<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::fs::vfs::Vfs) -> R
{
    let mut lock = VFS.lock();
    f(&mut lock)
}

pub fn with_page_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut PageCache) -> R
{
    let mut lock = PAGE_CACHE.lock();
    f(&mut lock)
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
