#![allow(dead_code)]

use crate::vfs::partition::PartitionInfo;
use crate::drivers::block::BlockDevice;
use crate::test_case;
use crate::test_eq;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PageCacheLevel {
    None,
    L1,
    L2,
}

/// Future: AES-XTS crypto context stub.
pub struct CryptoContext;

/// Unified I/O stack for block devices with partition awareness and caching.
///
/// An `IoStack` bundles a device reference with optional partition info
/// so all I/O through the stack automatically applies partition offset
/// translation and (optionally) caching.
pub struct IoStack {
    pub device_id: usize,
    pub partition: Option<PartitionInfo>,
    pub cache_level: PageCacheLevel,
}

impl IoStack {
    pub fn new(device_id: usize) -> Self {
        IoStack {
            device_id,
            partition: None,
            cache_level: PageCacheLevel::L1,
        }
    }

    pub fn with_partition(device_id: usize, partition: PartitionInfo, cache: PageCacheLevel) -> Self {
        IoStack {
            device_id,
            partition: Some(partition),
            cache_level: cache,
        }
    }

    /// Translate a partition-relative LBA to an absolute device LBA.
    /// If no partition is configured, the LBA passes through unchanged.
    pub fn translate_lba(&self, lba: u64) -> u64 {
        match &self.partition {
            Some(p) => p.base_lba + lba,
            None => lba,
        }
    }

    /// Read sectors through the unified I/O path.
    ///
    /// 1. Translate LBA: lba = partition.base_lba + lba_offset
    /// 2. Check cache (if L1/L2): hit → copy
    /// 3. Miss → read from device via submit_irp / read_blocks
    /// 4. Decrypt (future)
    /// 5. Return
    ///
    /// `lba` is partition-relative; `count` is the number of 512-byte sectors.
    pub fn read_sectors(&self, lba: u64, count: u64, buf: &mut [u8]) -> Result<(), ()> {
        let abs_lba = self.translate_lba(lba);

        if self.cache_level != PageCacheLevel::None && count == 1 && buf.len() >= 512 {
            let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
            if let Some(cache) = cache_lock.as_mut() {
                let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
                if let Some(dev) = bdevs_lock.get(self.device_id) {
                    if let Ok(sector) = cache.get_sector(abs_lba as u32, dev) {
                        buf[..512].copy_from_slice(sector);
                        return Ok(());
                    }
                }
            }
        }

        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.device_id).ok_or(())?;
        dev.read_blocks(abs_lba, count as u8, buf)
    }

    /// Write sectors through the unified I/O path.
    ///
    /// Translates LBA and writes directly to the device.
    pub fn write_sectors(&self, lba: u64, count: u64, buf: &[u8]) -> Result<(), ()> {
        let abs_lba = self.translate_lba(lba);
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.device_id).ok_or(())?;
        dev.write_blocks(abs_lba, count as u8, buf)
    }

    /// Convenience: read a single sector.
    pub fn read_sector(&self, lba: u64) -> Result<[u8; 512], ()> {
        let mut buf = [0u8; 512];
        self.read_sectors(lba, 1, &mut buf)?;
        Ok(buf)
    }

    /// Convenience: write a single sector.
    pub fn write_sector(&self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        self.write_sectors(lba, 1, data)
    }

    /// Obtain a mutable reference to the underlying block device.
    /// Used by filesystems that need direct device access (e.g. for BlockCache).
    pub fn with_device<F, R>(&self, f: F) -> Result<R, ()>
    where
        F: FnOnce(&mut dyn BlockDevice) -> R,
    {
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.device_id).ok_or(())?;
        Ok(f(dev))
    }
}

// ── A5.1 Tests ────────────────────────────────────────────────────────

fn test_iostack_partition_offset() -> Result<(), &'static str> {
    let part = PartitionInfo::new(2048, 204800, [0; 16]);
    let stack = IoStack::with_partition(0, part, PageCacheLevel::None);
    test_eq!(stack.translate_lba(0), 2048);
    test_eq!(stack.translate_lba(100), 2148);
    test_eq!(stack.translate_lba(204800), 206848);
    Ok(())
}

fn test_iostack_no_partition() -> Result<(), &'static str> {
    let stack = IoStack::new(0);
    test_eq!(stack.translate_lba(0), 0);
    test_eq!(stack.translate_lba(100), 100);
    test_eq!(stack.translate_lba(999999), 999999);
    Ok(())
}

fn test_iostack_cache_levels() -> Result<(), &'static str> {
    let part = PartitionInfo::new(0, 1000, [0; 16]);
    let stack_none = IoStack::with_partition(0, part, PageCacheLevel::None);
    let stack_l1 = IoStack::with_partition(0, part, PageCacheLevel::L1);
    let stack_l2 = IoStack::with_partition(0, part, PageCacheLevel::L2);
    test_eq!(stack_none.cache_level, PageCacheLevel::None);
    test_eq!(stack_l1.cache_level, PageCacheLevel::L1);
    test_eq!(stack_l2.cache_level, PageCacheLevel::L2);
    Ok(())
}

fn test_iostack_partition_read_device() -> Result<(), &'static str> {
    // Test that reading through IoStack with partition offset
    // correctly reads the device. Device 0 must be available.
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
    if bdevs.count() == 0 {
        // Skip if no block device
        return Ok(());
    }
    let dev = bdevs.get(0).ok_or("No device 0")?;
    let saved = dev.base_lba();
    dev.set_base_lba(0);
    let sector0 = dev.read_sector(0).map_err(|_| "Failed to read sector 0")?;
    dev.set_base_lba(saved);
    drop(bdevs);

    // Now read through IoStack without partition (should get same data)
    let stack = IoStack::new(0);
    let io_sector = stack.read_sector(0).map_err(|_| "IoStack read failed")?;
    test_eq!(io_sector, sector0);
    Ok(())
}

fn test_iostack_partition_offset_correct() -> Result<(), &'static str> {
    let part = PartitionInfo::new(500, 1000, [0; 16]);
    let stack = IoStack::with_partition(0, part, PageCacheLevel::None);
    test_eq!(stack.translate_lba(0), 500);
    test_eq!(stack.translate_lba(1), 501);
    test_eq!(stack.translate_lba(999), 1499);
    Ok(())
}

pub fn register_tests() {
    test_case!("iostack_partition_offset", { test_iostack_partition_offset()?; });
    test_case!("iostack_no_partition", { test_iostack_no_partition()?; });
    test_case!("iostack_cache_levels", { test_iostack_cache_levels()?; });
    test_case!("iostack_partition_read_device", { test_iostack_partition_read_device()?; });
    test_case!("iostack_partition_offset_correct", { test_iostack_partition_offset_correct()?; });
}
