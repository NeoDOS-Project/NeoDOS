use crate::buffer::block_cache::BlockCache;
use crate::drivers::ahci::AhciDriver;
use crate::drivers::ata::AtaDriver;
use crate::drivers::fat32::Fat32Driver;
use crate::fs::neodos_fs::NeoDosFs;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

pub static ATA_DRIVER: Mutex<Option<AtaDriver>> = Mutex::new(None);
pub static ATA_DRIVER_SECONDARY: Mutex<Option<AtaDriver>> = Mutex::new(None);
pub static AHCI_DRIVER: Mutex<Option<AhciDriver>> = Mutex::new(None);
pub static BLOCK_CACHE: Mutex<Option<BlockCache>> = Mutex::new(None);
pub static NEODOS_FS: Mutex<Option<NeoDosFs>> = Mutex::new(None);
pub static FAT32_DRIVER: Mutex<Option<Fat32Driver>> = Mutex::new(None);

pub static RAM_DISK_BASE: AtomicU64 = AtomicU64::new(0);
pub static RAM_DISK_SIZE: AtomicU64 = AtomicU64::new(0);

pub fn ram_disk_buf() -> Option<&'static [u8]> {
    let base = RAM_DISK_BASE.load(Ordering::Relaxed);
    let size = RAM_DISK_SIZE.load(Ordering::Relaxed) as usize;
    if base != 0 && size >= 512 {
        unsafe { Some(core::slice::from_raw_parts(base as *const u8, size)) }
    } else {
        None
    }
}

pub static NEED_CACHE_FLUSH: AtomicBool = AtomicBool::new(false);
pub static LAST_FLUSH_TICK: AtomicU64 = AtomicU64::new(0);
pub const FLUSH_INTERVAL_TICKS: u64 = 180;

pub fn with_ata<F, R>(f: F) -> R 
where
    F: FnOnce(&mut AtaDriver) -> R
{
    let mut lock = ATA_DRIVER.lock();
    let ata = lock.as_mut().expect("ATA_DRIVER not initialized");
    f(ata)
}

pub fn with_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut BlockCache) -> R
{
    let mut lock = BLOCK_CACHE.lock();
    let cache = lock.as_mut().expect("BLOCK_CACHE not initialized");
    f(cache)
}

pub fn with_fs<F, R>(f: F) -> R
where
    F: FnOnce(&mut NeoDosFs) -> R
{
    let mut lock = NEODOS_FS.lock();
    let fs = lock.as_mut().expect("NEODOS_FS not initialized");
    f(fs)
}

pub fn with_fs_and_cache<F, R>(f: F) -> R
where
    F: FnOnce(&mut NeoDosFs, &mut BlockCache) -> R
{
    let mut fs_lock = NEODOS_FS.lock();
    let mut cache_lock = BLOCK_CACHE.lock();
    let fs = fs_lock.as_mut().expect("NEODOS_FS not initialized");
    let cache = cache_lock.as_mut().expect("BLOCK_CACHE not initialized");
    f(fs, cache)
}

pub fn with_all<F, R>(f: F) -> R
where
    F: FnOnce(&mut NeoDosFs, &mut BlockCache, &mut AtaDriver) -> R
{
    let mut fs_lock = NEODOS_FS.lock();
    let mut cache_lock = BLOCK_CACHE.lock();
    let mut ata_lock = ATA_DRIVER.lock();
    let fs = fs_lock.as_mut().expect("NEODOS_FS not initialized");
    let cache = cache_lock.as_mut().expect("BLOCK_CACHE not initialized");
    let ata = ata_lock.as_mut().expect("ATA_DRIVER not initialized");
    f(fs, cache, ata)
}

pub fn flush_cache_if_needed() {
    if NEED_CACHE_FLUSH.swap(false, Ordering::Relaxed) {
        if let (Some(mut cache_lock), Some(mut ata_lock)) = (BLOCK_CACHE.try_lock(), ATA_DRIVER.try_lock()) {
            if let (Some(cache), Some(ata)) = (cache_lock.as_mut(), ata_lock.as_mut()) {
                let _ = cache.flush(ata);
            }
        }
        let current = crate::scheduler::TIMER_TICKS.load(Ordering::Relaxed);
        LAST_FLUSH_TICK.store(current, Ordering::Relaxed);
    }
}
