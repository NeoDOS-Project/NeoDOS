// src/drivers/ata.rs — Boot stub (PIO only, primary channel)
// Full ATA driver lives in drivers/ata/ (NEM v3 standalone).

const ATA_DATA: u16 = 0x1F0;
const ATA_SECTOR_COUNT: u16 = 0x1F2;
const ATA_LBA_LOW: u16 = 0x1F3;
const ATA_LBA_MID: u16 = 0x1F4;
const ATA_LBA_HIGH: u16 = 0x1F5;
const ATA_DRIVE_SEL: u16 = 0x1F6;
const ATA_COMMAND: u16 = 0x1F7;
const ATA_STATUS: u16 = 0x1F7;

const ATA_CMD_READ_PIO: u8 = 0x20;
const ATA_CMD_WRITE_PIO: u8 = 0x30;
const ATA_DRIVE_SELECT: u8 = 0xE0;

pub struct BootAta {
    base_lba: u64,
}

unsafe impl Send for BootAta {}

impl BootAta {
    pub fn new() -> Self {
        BootAta { base_lba: 0 }
    }

    pub fn set_base_lba(&mut self, lba: u64) { self.base_lba = lba; }
    pub fn base_lba(&self) -> u64 { self.base_lba }

    fn wait_not_busy(&self) -> Result<(), ()> {
        for _ in 0..1000000 {
            if (crate::hal::inb(ATA_STATUS) & 0x80) == 0 {
                return Ok(());
            }
        }
        Err(())
    }

    fn wait_data_ready(&self) -> Result<(), ()> {
        for _ in 0..1000000 {
            let st = crate::hal::inb(ATA_STATUS);
            if (st & 0x08) != 0 { return Ok(()); }
            if (st & 0x01) != 0 { return Err(()); }
        }
        Err(())
    }

    pub fn read_sector(&mut self, lba: u64) -> Result<[u8; 512], ()> {
        let abs = self.base_lba.wrapping_add(lba) as u32;
        self.wait_not_busy()?;
        crate::hal::outb(ATA_DRIVE_SEL, ATA_DRIVE_SELECT | ((abs >> 24) & 0x0F) as u8);
        crate::hal::outb(ATA_SECTOR_COUNT, 1);
        crate::hal::outb(ATA_LBA_LOW, abs as u8);
        crate::hal::outb(ATA_LBA_MID, (abs >> 8) as u8);
        crate::hal::outb(ATA_LBA_HIGH, (abs >> 16) as u8);
        crate::hal::outb(ATA_COMMAND, ATA_CMD_READ_PIO);
        self.wait_data_ready()?;
        let mut buf = [0u8; 512];
        for i in 0..256 {
            let w = crate::hal::inw(ATA_DATA);
            buf[i * 2] = w as u8;
            buf[i * 2 + 1] = (w >> 8) as u8;
        }
        Ok(buf)
    }

    pub fn write_sector(&mut self, lba: u64, data: &[u8; 512]) -> Result<(), ()> {
        let abs = self.base_lba.wrapping_add(lba) as u32;
        self.wait_not_busy()?;
        crate::hal::outb(ATA_SECTOR_COUNT, 1);
        crate::hal::outb(ATA_LBA_LOW, (abs & 0xFF) as u8);
        crate::hal::outb(ATA_LBA_MID, ((abs >> 8) & 0xFF) as u8);
        crate::hal::outb(ATA_LBA_HIGH, ((abs >> 16) & 0xFF) as u8);
        crate::hal::outb(ATA_DRIVE_SEL, ATA_DRIVE_SELECT | ((abs >> 24) & 0x0F) as u8);
        crate::hal::outb(ATA_COMMAND, ATA_CMD_WRITE_PIO);
        self.wait_not_busy()?;
        for i in (0..512).step_by(2) {
            crate::hal::outw(ATA_DATA, u16::from_le_bytes([data[i], data[i + 1]]));
        }
        self.wait_not_busy()?;
        Ok(())
    }

    pub fn read_blocks(&mut self, lba: u64, count: u8, buf: &mut [u8]) -> Result<(), ()> {
        let abs = self.base_lba.wrapping_add(lba) as u32;
        let cnt = count.max(1);
        if buf.len() < (cnt as usize) * 512 { return Err(()); }
        self.wait_not_busy()?;
        crate::hal::outb(ATA_DRIVE_SEL, ATA_DRIVE_SELECT | ((abs >> 24) & 0x0F) as u8);
        crate::hal::outb(ATA_SECTOR_COUNT, cnt);
        crate::hal::outb(ATA_LBA_LOW, abs as u8);
        crate::hal::outb(ATA_LBA_MID, (abs >> 8) as u8);
        crate::hal::outb(ATA_LBA_HIGH, (abs >> 16) as u8);
        crate::hal::outb(ATA_COMMAND, ATA_CMD_READ_PIO);
        for s in 0..cnt as usize {
            self.wait_data_ready()?;
            let off = s * 256;
            for i in 0..256 {
                let w = crate::hal::inw(ATA_DATA);
                buf[off * 2 + i * 2] = w as u8;
                buf[off * 2 + i * 2 + 1] = (w >> 8) as u8;
            }
        }
        Ok(())
    }

    pub fn write_blocks(&mut self, lba: u64, count: u8, data: &[u8]) -> Result<(), ()> {
        let abs = self.base_lba.wrapping_add(lba) as u32;
        let cnt = count.max(1);
        if data.len() < (cnt as usize) * 512 { return Err(()); }
        self.wait_not_busy()?;
        crate::hal::outb(ATA_SECTOR_COUNT, cnt);
        crate::hal::outb(ATA_LBA_LOW, (abs & 0xFF) as u8);
        crate::hal::outb(ATA_LBA_MID, ((abs >> 8) & 0xFF) as u8);
        crate::hal::outb(ATA_LBA_HIGH, ((abs >> 16) & 0xFF) as u8);
        crate::hal::outb(ATA_DRIVE_SEL, ATA_DRIVE_SELECT | ((abs >> 24) & 0x0F) as u8);
        crate::hal::outb(ATA_COMMAND, ATA_CMD_WRITE_PIO);
        self.wait_not_busy()?;
        for s in 0..cnt as usize {
            let off = s * 256;
            for i in 0..256 {
                let w = u16::from_le_bytes([
                    data[off * 2 + i * 2],
                    data[off * 2 + i * 2 + 1],
                ]);
                crate::hal::outw(ATA_DATA, w);
            }
        }
        self.wait_not_busy()?;
        Ok(())
    }
}
