use crate::drivers::ahci::AhciDriver;
use crate::drivers::ata::AtaDriver;

pub trait BlockDeviceOps {
    fn set_base_lba(&mut self, lba: u32);
    fn base_lba(&self) -> u32;
    fn read_sector(&mut self, lba: u32) -> Result<[u8; 512], ()>;
    fn read_sector_master(&mut self, lba: u32) -> Result<[u8; 512], ()>;
}

impl BlockDeviceOps for AtaDriver {
    fn set_base_lba(&mut self, lba: u32) { self.set_base_lba(lba); }
    fn base_lba(&self) -> u32 { self.base_lba() }
    fn read_sector(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        self.read_sector(lba).map_err(|_| ())
    }
    fn read_sector_master(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        self.read_sector_master(lba).map_err(|_| ())
    }
}

impl BlockDeviceOps for AhciDriver {
    fn set_base_lba(&mut self, lba: u32) { self.set_base_lba(lba); }
    fn base_lba(&self) -> u32 { self.base_lba() }
    fn read_sector(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        self.read_sector(lba)
    }
    fn read_sector_master(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        self.read_sector_master(lba)
    }
}

pub enum BlockDevice {
    Ata(AtaDriver),
    Ahci(AhciDriver),
}

impl BlockDevice {
    pub fn set_base_lba(&mut self, lba: u32) {
        match self {
            BlockDevice::Ata(d) => BlockDeviceOps::set_base_lba(d, lba),
            BlockDevice::Ahci(d) => BlockDeviceOps::set_base_lba(d, lba),
        }
    }

    pub fn base_lba(&self) -> u32 {
        match self {
            BlockDevice::Ata(d) => BlockDeviceOps::base_lba(d),
            BlockDevice::Ahci(d) => BlockDeviceOps::base_lba(d),
        }
    }

    pub fn read_sector(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        match self {
            BlockDevice::Ata(d) => BlockDeviceOps::read_sector(d, lba),
            BlockDevice::Ahci(d) => BlockDeviceOps::read_sector(d, lba),
        }
    }

    pub fn read_sector_master(&mut self, lba: u32) -> Result<[u8; 512], ()> {
        match self {
            BlockDevice::Ata(d) => BlockDeviceOps::read_sector_master(d, lba),
            BlockDevice::Ahci(d) => BlockDeviceOps::read_sector_master(d, lba),
        }
    }
}
