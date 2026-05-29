#![allow(dead_code)]

use alloc::boxed::Box;
use crate::drivers::ahci::AhciDriver;
use crate::drivers::nvme::NvmeDriver;
use core::sync::atomic::{AtomicU64, Ordering};

pub const MAX_BLOCK_DEVICES: usize = 8;

pub struct BlockDeviceManager {
    devices: [Option<Box<dyn BlockDevice>>; MAX_BLOCK_DEVICES],
    count: usize,
}

impl BlockDeviceManager {
    pub fn new() -> Self {
        let devices: [Option<Box<dyn BlockDevice>>; MAX_BLOCK_DEVICES] = Default::default();
        BlockDeviceManager { devices, count: 0 }
    }

    pub fn register(&mut self, dev: Box<dyn BlockDevice>) -> Option<usize> {
        if self.count >= MAX_BLOCK_DEVICES {
            return None;
        }
        let idx = self.count;
        self.devices[idx] = Some(dev);
        self.count += 1;
        Some(idx)
    }

    pub fn get(&mut self, idx: usize) -> Option<&mut (dyn BlockDevice + '_)> {
        if let Some(slot) = self.devices.get_mut(idx) {
            if let Some(ref mut dev) = slot {
                return Some(&mut **dev);
            }
        }
        None
    }

    pub fn swap(&mut self, idx: usize, dev: Box<dyn BlockDevice>) -> Option<Box<dyn BlockDevice>> {
        self.devices.get_mut(idx)?.replace(dev)
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

pub trait BlockDevice: Send {
    fn num_sectors(&self) -> Option<u64> { None }

    fn sector_size(&self) -> u32 { 512 }

    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()>;

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()>;

    fn flush(&mut self) -> Result<(), ()> { Ok(()) }

    fn set_base_lba(&mut self, lba: u64);

    fn base_lba(&self) -> u64;

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        let mut buf = [0u8; 512];
        self.read_blocks(lba, 1, &mut buf)?;
        Ok(buf)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        self.write_blocks(lba, 1, data)
    }
}

// ── RamDisk: backed by a memory buffer loaded by the bootloader ──────

static RAM_DISK_BASE: AtomicU64 = AtomicU64::new(0);
static RAM_DISK_SIZE: AtomicU64 = AtomicU64::new(0);

pub fn set_ram_disk(base: u64, size: u64) {
    RAM_DISK_BASE.store(base, Ordering::Relaxed);
    RAM_DISK_SIZE.store(size, Ordering::Relaxed);
}

fn ram_disk_buf() -> Option<&'static [u8]> {
    let base = RAM_DISK_BASE.load(Ordering::Relaxed);
    let size = RAM_DISK_SIZE.load(Ordering::Relaxed) as usize;
    if base != 0 && size >= 512 {
        unsafe { Some(core::slice::from_raw_parts(base as *const u8, size)) }
    } else {
        None
    }
}

pub struct RamDisk;

impl RamDisk {
    pub fn available() -> bool {
        ram_disk_buf().is_some()
    }
}

impl BlockDevice for RamDisk {
    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let ram = ram_disk_buf().ok_or(())?;
        let offset = (lba as usize) * 512;
        let len = (count as usize) * 512;
        if offset + len <= ram.len() && buf.len() >= len {
            buf[..len].copy_from_slice(&ram[offset..offset + len]);
            Ok(())
        } else {
            Err(())
        }
    }

    fn write_blocks(&mut self, _lba: u64, _count: u8, _buf: &[u8]) -> Result<(), ()> {
        Err(())
    }

    fn set_base_lba(&mut self, _lba: u64) {}
    fn base_lba(&self) -> u64 { 0 }
}

// ── Direct BlockDevice implementations ──────────────────────────────

impl BlockDevice for crate::drivers::ata::BootAta {
    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        self.read_blocks(lba, count, buf)
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        self.write_blocks(lba, count, buf)
    }

    fn set_base_lba(&mut self, lba: u64) {
        self.set_base_lba(lba);
    }

    fn base_lba(&self) -> u64 {
        self.base_lba()
    }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        self.read_sector(lba)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        self.write_sector(lba, data)
    }
}

impl BlockDevice for AhciDriver {
    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        self.read_sectors(lba as u32, count, buf)
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        self.write_sectors(lba as u32, count, buf)
    }

    fn set_base_lba(&mut self, lba: u64) {
        AhciDriver::set_base_lba(self, lba as u32);
    }

    fn base_lba(&self) -> u64 {
        AhciDriver::base_lba(self) as u64
    }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        AhciDriver::read_sector(self, lba as u32)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        AhciDriver::write_sector(self, lba as u32, data)
    }
}

impl BlockDevice for NvmeDriver {
    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        self.read_sectors(lba, count, buf)
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        self.write_sectors(lba, count, buf)
    }

    fn set_base_lba(&mut self, lba: u64) {
        NvmeDriver::set_base_lba(self, lba as u32);
    }

    fn base_lba(&self) -> u64 {
        NvmeDriver::base_lba(self) as u64
    }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        NvmeDriver::read_sector(self, lba as u32)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        NvmeDriver::write_sector(self, lba as u32, data)
    }
}

// ── NEM block device registry ──
// Allows standalone NEM drivers to register block devices with the kernel.
// These are stored separately from the built-in BlockDeviceManager.

const MAX_NEM_BLOCK_DEVICES: usize = 4;

type NemBlockReadFn = unsafe extern "C" fn(u32, u64, u8, *mut u8) -> i32;
type NemBlockWriteFn = unsafe extern "C" fn(u32, u64, u8, *const u8) -> i32;

pub struct NemBlockDevice {
    pub device_id: u32,
    pub num_sectors: u64,
    pub sector_size: u32,
    pub read_fn: NemBlockReadFn,
    pub write_fn: NemBlockWriteFn,
    pub base_lba: u64,
}

// Safety: NemBlockDevice only contains function pointers and plain data; Send is safe.
unsafe impl Send for NemBlockDevice {}

impl NemBlockDevice {
    pub fn new(
        device_id: u32,
        num_sectors: u64,
        sector_size: u32,
        read_fn: NemBlockReadFn,
        write_fn: NemBlockWriteFn,
    ) -> Self {
        NemBlockDevice {
            device_id,
            num_sectors,
            sector_size,
            read_fn,
            write_fn,
            base_lba: 0,
        }
    }
}

impl BlockDevice for NemBlockDevice {
    fn num_sectors(&self) -> Option<u64> {
        Some(self.num_sectors)
    }

    fn sector_size(&self) -> u32 {
        self.sector_size
    }

    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        let rc = unsafe { (self.read_fn)(self.device_id, abs_lba, count, buf.as_mut_ptr()) };
        if rc == 0 { Ok(()) } else { Err(()) }
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        let rc = unsafe { (self.write_fn)(self.device_id, abs_lba, count, buf.as_ptr()) };
        if rc == 0 { Ok(()) } else { Err(()) }
    }

    fn set_base_lba(&mut self, lba: u64) {
        self.base_lba = lba;
    }

    fn base_lba(&self) -> u64 {
        self.base_lba
    }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        let mut buf = [0u8; 512];
        self.read_blocks(lba, 1, &mut buf)?;
        Ok(buf)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        self.write_blocks(lba, 1, data)
    }
}

/// Register a NemBlockDevice by adding it to the global BlockDeviceManager.
pub fn register_nem_block_device(dev: NemBlockDevice) -> i32 {
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
    match bdevs.register(alloc::boxed::Box::new(dev)) {
        Some(idx) => {
            crate::serial_println!("[BLK] NEM block device registered at idx={}", idx);
            idx as i32
        }
        None => -1,
    }
}

/// Unregister a NemBlockDevice by its index (no-op for now, as BlockDeviceManager
/// does not support direct removal).
pub fn unregister_nem_block_device(_idx: usize) {
    // BlockDeviceManager does not support removal.
    // Future: implement BlockDeviceManager::remove().
}
