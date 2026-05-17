#![allow(dead_code)]

use crate::drivers::ahci::AhciDriver;
use crate::drivers::ata::AtaDriver;

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

impl BlockDevice for AtaDriver {
    fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        if let Some(ram) = crate::globals::ram_disk_buf() {
            let offset = (lba as usize) * 512;
            let len = (count as usize) * 512;
            if offset + len <= ram.len() && buf.len() >= len {
                buf[..len].copy_from_slice(&ram[offset..offset + len]);
                return Ok(());
            }
        }
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
