#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;
use lazy_static::lazy_static;
use crate::buffer::block_cache::BlockCache;
use crate::buffer::page_cache::PageCache;

lazy_static! {
    pub static ref BLOCK_DEVICES: Mutex<crate::drivers::block::BlockDeviceManager> = Mutex::new(crate::drivers::block::BlockDeviceManager::new());
}
pub static BLOCK_CACHE: Mutex<Option<BlockCache>> = Mutex::new(None);
pub static PAGE_CACHE: Mutex<PageCache> = Mutex::new(PageCache::new());
pub static VFS: Mutex<crate::fs::vfs::Vfs> = Mutex::new(crate::fs::vfs::Vfs::new());

pub static NEED_CACHE_FLUSH: AtomicBool = AtomicBool::new(false);
pub static LAST_FLUSH_TICK: AtomicU64 = AtomicU64::new(0);
pub const FLUSH_INTERVAL_TICKS: u64 = 180;

pub static NEED_PAGE_CACHE_FLUSH: AtomicBool = AtomicBool::new(false);

pub fn with_vfs<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::fs::vfs::Vfs) -> R
{
    let mut lock = VFS.lock();
    f(&mut lock)
}

pub fn with_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut BlockCache) -> R
{
    let mut lock = BLOCK_CACHE.lock();
    let cache = lock.as_mut().expect("BLOCK_CACHE not initialized");
    f(cache)
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
        if let Some(mut cache_lock) = BLOCK_CACHE.try_lock() {
            if let Some(cache) = cache_lock.as_mut() {
                let mut bdev_lock = BLOCK_DEVICES.lock();
                if let Some(dev) = bdev_lock.get(0) {
                    let _ = cache.flush(dev);
                }
            }
        }
        let current = crate::hal::get_ticks();
        LAST_FLUSH_TICK.store(current, Ordering::Relaxed);
    }
    if NEED_PAGE_CACHE_FLUSH.swap(false, Ordering::Relaxed) {
        if let Some(mut pc_lock) = PAGE_CACHE.try_lock() {
            let mut bdev_lock = BLOCK_DEVICES.lock();
            if let Some(dev) = bdev_lock.get(0) {
                let _ = pc_lock.flush(dev);
            }
        }
    }
}
