use crate::buffer::block_cache::BlockCache;
use crate::drivers::ahci::AhciDriver;
use crate::drivers::ata::AtaDriver;
use crate::drivers::fat32::Fat32Driver;
use crate::fs::neodos_fs::NeoDosFs;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub static mut ATA_DRIVER: Option<AtaDriver> = None;
pub static mut ATA_DRIVER_SECONDARY: Option<AtaDriver> = None;
pub static mut AHCI_DRIVER: Option<AhciDriver> = None;
pub static mut BLOCK_CACHE: Option<BlockCache> = None;
pub static mut NEODOS_FS: Option<NeoDosFs> = None;
pub static mut FAT32_DRIVER: Option<Fat32Driver> = None;
pub static mut RAM_DISK_BASE: u64 = 0;
pub static mut RAM_DISK_SIZE: u64 = 0;

pub fn ram_disk_buf() -> Option<&'static [u8]> {
    unsafe {
        let base = RAM_DISK_BASE;
        let size = RAM_DISK_SIZE as usize;
        if base != 0 && size >= 512 {
            Some(core::slice::from_raw_parts(base as *const u8, size))
        } else {
            None
        }
    }
}

pub static NEED_CACHE_FLUSH: AtomicBool = AtomicBool::new(false);
pub static LAST_FLUSH_TICK: AtomicU64 = AtomicU64::new(0);
pub const FLUSH_INTERVAL_TICKS: u64 = 180;

pub fn with_ata<F, R>(f: F) -> R 
where
    F: FnOnce(&mut AtaDriver) -> R
{
    unsafe {
        let ata = ATA_DRIVER.as_mut().expect("ATA_DRIVER not initialized");
        f(ata)
    }
}

pub fn with_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut BlockCache) -> R
{
    unsafe {
        let cache = BLOCK_CACHE.as_mut().expect("BLOCK_CACHE not initialized");
        f(cache)
    }
}

pub fn with_fs<F, R>(f: F) -> R
where
    F: FnOnce(&mut NeoDosFs) -> R
{
    unsafe {
        let fs = NEODOS_FS.as_mut().expect("NEODOS_FS not initialized");
        f(fs)
    }
}

pub fn with_fs_and_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut NeoDosFs, &mut BlockCache) -> R
{
    unsafe {
        let fs = NEODOS_FS.as_mut().expect("NEODOS_FS not initialized");
        let cache = BLOCK_CACHE.as_mut().expect("BLOCK_CACHE not initialized");
        f(fs, cache)
    }
}

pub fn with_all<F, R>(f: F) -> R
where
    F: FnOnce(&mut NeoDosFs, &mut BlockCache, &mut AtaDriver) -> R
{
    unsafe {
        let fs = NEODOS_FS.as_mut().expect("NEODOS_FS not initialized");
        let cache = BLOCK_CACHE.as_mut().expect("BLOCK_CACHE not initialized");
        let ata = ATA_DRIVER.as_mut().expect("ATA_DRIVER not initialized");
        f(fs, cache, ata)
    }
}

pub fn flush_cache_if_needed() {
    if NEED_CACHE_FLUSH.swap(false, Ordering::Relaxed) {
        unsafe {
            if let (Some(cache), Some(ata)) = (BLOCK_CACHE.as_mut(), ATA_DRIVER.as_mut()) {
                let _ = cache.flush(ata);
            }
        }
        let current = crate::scheduler::TIMER_TICKS.load(Ordering::Relaxed);
        LAST_FLUSH_TICK.store(current, Ordering::Relaxed);
    }
}
