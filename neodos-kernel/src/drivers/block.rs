#![allow(dead_code)]

use alloc::boxed::Box;
use crate::drivers::ahci::AhciDriver;
use crate::drivers::ata::AtaDriver;
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

// ── BlockDevice routing: tries AHCI, falls back to ATA ──────────────

/// A `BlockDevice` that tries an AHCI driver first, falling back to ATA.
/// Used when the primary disk has both legacy IDE and AHCI support.
pub struct AtaWithAhciFallback {
    pub ahci: Option<AhciDriver>,
    pub ata: AtaDriver,
}

impl BlockDevice for AtaWithAhciFallback {
    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        if let Some(ref mut ahci) = self.ahci {
            if ahci.read_sectors(lba as u32, count, buf).is_ok() {
                return Ok(());
            }
        }
        self.ata.read_sectors(lba as u32, count, buf).map_err(|_| ())
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        if let Some(ref mut ahci) = self.ahci {
            if ahci.write_sectors(lba as u32, count, buf).is_ok() {
                return Ok(());
            }
        }
        self.ata.write_sectors(lba as u32, count, buf).map_err(|_| ())
    }

    fn set_base_lba(&mut self, lba: u64) {
        self.ata.set_base_lba(lba as u32);
        if let Some(ref mut ahci) = self.ahci {
            ahci.set_base_lba(lba as u32);
        }
    }

    fn base_lba(&self) -> u64 {
        self.ata.base_lba() as u64
    }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        if let Some(ref mut ahci) = self.ahci {
            if let Ok(data) = ahci.read_sector(lba as u32) {
                return Ok(data);
            }
        }
        self.ata.read_sector(lba as u32).map_err(|_| ())
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        if let Some(ref mut ahci) = self.ahci {
            if ahci.write_sector(lba as u32, data).is_ok() {
                return Ok(());
            }
        }
        AtaDriver::write_sector(&mut self.ata, lba as u32, data)
    }
}

// ── Direct BlockDevice implementations ──────────────────────────────

impl BlockDevice for AtaDriver {
    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        self.read_sectors(lba as u32, count, buf).map_err(|_| ())
    }

    fn write_blocks(&mut self, lba: u64, count: u8, buf: &[u8]) -> Result<(), ()> {
        self.write_sectors(lba as u32, count, buf).map_err(|_| ())
    }

    fn set_base_lba(&mut self, lba: u64) {
        AtaDriver::set_base_lba(self, lba as u32);
    }

    fn base_lba(&self) -> u64 {
        AtaDriver::base_lba(self) as u64
    }

    fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        AtaDriver::read_sector(self, lba as u32).map_err(|_| ())
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        AtaDriver::write_sector(self, lba as u32, data)
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
