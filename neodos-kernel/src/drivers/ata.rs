// src/drivers/ata.rs

#![allow(dead_code)]

use x86_64::instructions::port::Port;

const ATA_PRIMARY_DATA: u16 = 0x1F0;
const ATA_PRIMARY_ERROR: u16 = 0x1F1;
const ATA_PRIMARY_FEATURES: u16 = 0x1F1;
const ATA_PRIMARY_SECTOR_COUNT: u16 = 0x1F2;
const ATA_PRIMARY_LBA_LOW: u16 = 0x1F3;
const ATA_PRIMARY_LBA_MID: u16 = 0x1F4;
const ATA_PRIMARY_LBA_HIGH: u16 = 0x1F5;
const ATA_PRIMARY_DRIVE_SEL: u16 = 0x1F6;
const ATA_PRIMARY_COMMAND: u16 = 0x1F7;
const ATA_PRIMARY_STATUS: u16 = 0x1F7;

const ATA_CMD_READ_PIO: u8 = 0x20;
const ATA_CMD_READ_MULTIPLE: u8 = 0xC4;
const ATA_CMD_WRITE_MULTIPLE: u8 = 0xC5;
const ATA_CMD_READ_DMA: u8 = 0xC8;
const ATA_CMD_WRITE_DMA: u8 = 0xCA;

// Bus-master DMA registers (offsets from BMBA)
const BM_COMMAND: u16 = 0x0;
const BM_STATUS: u16 = 0x2;
const BM_PRDT_ADDRESS: u16 = 0x4;

// BM_COMMAND bits
const BM_CMD_START: u8 = 0x01;
const BM_CMD_WRITE: u8 = 0x08;

// BM_STATUS bits
const BM_STAT_ACTIVE: u8 = 0x01;
const BM_STAT_ERROR: u8 = 0x02;
const BM_STAT_INTERRUPT: u8 = 0x04;

#[repr(C, packed)]
struct PrdtEntry {
    data_buffer_phys: u32,
    count: u16,
    eot: u16,
}

#[repr(align(4096))]
struct DmaAligned([u8; 4096]);

static mut PRDT: DmaAligned = DmaAligned([0u8; 4096]);
static mut DMA_DATA: DmaAligned = DmaAligned([0u8; 4096]);

/// LBA drive-select base (bits 7–4). With the unified GPT disk, both NeoDOS FS
/// and FAT32 ESP are on the same physical drive (master, index=0).
const ATA_DRIVE_SELECT_LBA_BASE: u8 = 0xE0;

/// Master drive — used by the FAT32 driver for absolute-LBA reads.
const ATA_DRIVE_SELECT_MASTER: u8 = 0xE0;

pub struct AtaDriver {
    data_port: Port<u16>,
    _error_port: Port<u8>,
    sector_count_port: Port<u8>,
    lba_low_port: Port<u8>,
    lba_mid_port: Port<u8>,
    lba_high_port: Port<u8>,
    drive_sel_port: Port<u8>,
    command_port: Port<u8>,
    status_port: Port<u8>,
    bmba: Option<u16>,
    base_lba: u32,
}

#[derive(Debug)]
pub enum AtaError {
    Busy,
    Error,
    Timeout,
}

impl AtaDriver {
    pub fn new() -> Self {
        AtaDriver {
            data_port: Port::new(ATA_PRIMARY_DATA),
            _error_port: Port::new(ATA_PRIMARY_ERROR),
            sector_count_port: Port::new(ATA_PRIMARY_SECTOR_COUNT),
            lba_low_port: Port::new(ATA_PRIMARY_LBA_LOW),
            lba_mid_port: Port::new(ATA_PRIMARY_LBA_MID),
            lba_high_port: Port::new(ATA_PRIMARY_LBA_HIGH),
            drive_sel_port: Port::new(ATA_PRIMARY_DRIVE_SEL),
            command_port: Port::new(ATA_PRIMARY_COMMAND),
            status_port: Port::new(ATA_PRIMARY_STATUS),
            bmba: None,
            base_lba: 0,
        }
    }

    pub fn set_base_lba(&mut self, lba: u32) {
        self.base_lba = lba;
    }

    pub fn write_sector(&mut self, lba: u32, data: &[u8; 512]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        if abs_lba > 0x0FFFFFFF { return Err(()); }
        self.write_sector_inner(lba, data)
    }

    fn write_sector_inner(&mut self, lba: u32, data: &[u8; 512]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        unsafe {
            self.wait_not_busy_simple()?;
            
            self.sector_count_port.write(1);
            self.lba_low_port.write((abs_lba & 0xFF) as u8);
            self.lba_mid_port.write(((abs_lba >> 8) & 0xFF) as u8);
            self.lba_high_port.write(((abs_lba >> 16) & 0xFF) as u8);
            
            let drive_byte = ATA_DRIVE_SELECT_LBA_BASE | ((abs_lba >> 24) & 0x0F) as u8;
            self.drive_sel_port.write(drive_byte);
            
            self.command_port.write(0x30); // WRITE SECTORS

            self.wait_not_busy_simple()?;

            for i in (0..512).step_by(2) {
                let word = u16::from_le_bytes([data[i], data[i+1]]);
                self.data_port.write(word);
            }
            
            // Wait for disk to finish writing
            self.wait_not_busy_simple()?;
        }
        Ok(())
    }

    fn wait_not_busy_simple(&mut self) -> Result<(), ()> {
        let mut timeout = 0;
        unsafe {
            while (self.status_port.read() & 0x80) != 0 {
                timeout += 1;
                if timeout > 100000 { return Err(()); }
            }
        }
        Ok(())
    }

    fn wait_not_busy(&mut self) -> Result<(), AtaError> {
        for _ in 0..1000000 {
            let status = unsafe { self.status_port.read() };
            if (status & 0x80) == 0 {
                return Ok(());
            }
        }
        Err(AtaError::Timeout)
    }

    fn wait_data_ready(&mut self) -> Result<(), AtaError> {
        for _ in 0..1000000 {
            let status = unsafe { self.status_port.read() };
            if (status & 0x08) != 0 {
                return Ok(());
            }
            if (status & 0x01) != 0 {
                return Err(AtaError::Error);
            }
        }
        Err(AtaError::Timeout)
    }

    pub fn read_sector(&mut self, lba: u32) -> Result<[u8; 512], AtaError> {
        self.read_sector_inner(lba)
    }

    pub fn read_sector_master(&mut self, lba: u32) -> Result<[u8; 512], AtaError> {
        self.read_sector_master_inner(lba)
    }

    fn read_sector_master_inner(&mut self, lba: u32) -> Result<[u8; 512], AtaError> {
        self.wait_not_busy()?;

        unsafe {
            self.drive_sel_port.write(ATA_DRIVE_SELECT_MASTER | ((lba >> 24) & 0x0F) as u8);
            self.sector_count_port.write(1);
            self.lba_low_port.write(lba as u8);
            self.lba_mid_port.write((lba >> 8) as u8);
            self.lba_high_port.write((lba >> 16) as u8);
            self.command_port.write(ATA_CMD_READ_PIO);
        }

        self.wait_data_ready()?;

        let mut buffer = [0u8; 512];
        for i in 0..256 {
            let word = unsafe { self.data_port.read() };
            buffer[i * 2] = word as u8;
            buffer[i * 2 + 1] = (word >> 8) as u8;
        }

        Ok(buffer)
    }

    fn read_sector_inner(&mut self, lba: u32) -> Result<[u8; 512], AtaError> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        self.wait_not_busy()?;

        unsafe {
            self.drive_sel_port.write(ATA_DRIVE_SELECT_LBA_BASE | ((abs_lba >> 24) & 0x0F) as u8);
            
            self.sector_count_port.write(1);
            
            self.lba_low_port.write(abs_lba as u8);
            self.lba_mid_port.write((abs_lba >> 8) as u8);
            self.lba_high_port.write((abs_lba >> 16) as u8);
            
            self.command_port.write(ATA_CMD_READ_PIO);
        }

        self.wait_data_ready()?;

        let mut buffer = [0u8; 512];
        for i in 0..256 {
            let word = unsafe { self.data_port.read() };
            buffer[i * 2] = word as u8;
            buffer[i * 2 + 1] = (word >> 8) as u8;
        }

        Ok(buffer)
    }

    pub fn read_sectors(&mut self, lba: u32, count: u8, buf: &mut [u8]) -> Result<(), AtaError> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        let count = count.max(1);
        let total_bytes = (count as usize) * 512;
        if buf.len() < total_bytes {
            return Err(AtaError::Error);
        }
        self.wait_not_busy()?;
        unsafe {
            self.drive_sel_port.write(ATA_DRIVE_SELECT_LBA_BASE | ((abs_lba >> 24) & 0x0F) as u8);
            self.sector_count_port.write(count);
            self.lba_low_port.write(abs_lba as u8);
            self.lba_mid_port.write((abs_lba >> 8) as u8);
            self.lba_high_port.write((abs_lba >> 16) as u8);
            self.command_port.write(ATA_CMD_READ_MULTIPLE);
        }
        for s in 0..count as usize {
            self.wait_data_ready()?;
            let off = s * 256;
            for i in 0..256 {
                let word = unsafe { self.data_port.read() };
                buf[off * 2 + i * 2] = word as u8;
                buf[off * 2 + i * 2 + 1] = (word >> 8) as u8;
            }
        }
        Ok(())
    }

    pub fn init_dma(&mut self, base: u16) {
        self.bmba = Some(base);
    }

    pub fn read_dma(&mut self, lba: u32, count: u8, buf: &mut [u8]) -> Result<(), AtaError> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        let bmba = self.bmba.ok_or(AtaError::Error)?;
        let count = count.max(1).min(8);
        let total_bytes = (count as usize) * 512;
        if buf.len() < total_bytes {
            return Err(AtaError::Error);
        }

        unsafe {
            let prdt_phys = &PRDT as *const _ as u32;
            let data_phys = &DMA_DATA as *const _ as u32;

            let prdt_entries =
                core::slice::from_raw_parts_mut(&mut PRDT.0 as *mut u8 as *mut PrdtEntry, 512);
            prdt_entries[0].data_buffer_phys = data_phys;
            prdt_entries[0].count = (total_bytes as u16).min(0xFFFE);
            prdt_entries[0].eot = 0x8000;
            for i in 1..512 {
                prdt_entries[i].data_buffer_phys = 0;
                prdt_entries[i].count = 0;
                prdt_entries[i].eot = 0;
            }

            core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

            let mut bm_cmd: Port<u8> = Port::new(bmba + BM_COMMAND);
            let mut bm_status: Port<u8> = Port::new(bmba + BM_STATUS);
            let mut bm_prdt: Port<u32> = Port::new(bmba + BM_PRDT_ADDRESS);

            bm_status.write(BM_STAT_INTERRUPT);
            bm_cmd.write(0x00);
            bm_prdt.write(prdt_phys);

            self.wait_not_busy()?;
            self.drive_sel_port
                .write(ATA_DRIVE_SELECT_LBA_BASE | ((abs_lba >> 24) & 0x0F) as u8);
            self.sector_count_port.write(count);
            self.lba_low_port.write(abs_lba as u8);
            self.lba_mid_port.write((abs_lba >> 8) as u8);
            self.lba_high_port.write((abs_lba >> 16) as u8);
            self.command_port.write(ATA_CMD_READ_DMA);

            bm_cmd.write(BM_CMD_START);

            for _ in 0..2000000 {
                let st = bm_status.read();
                if (st & BM_STAT_ACTIVE) == 0 {
                    if (st & BM_STAT_ERROR) != 0 || (self.status_port.read() & 0x01) != 0 {
                        return Err(AtaError::Error);
                    }
                    core::ptr::copy_nonoverlapping(
                        DMA_DATA.0.as_ptr(),
                        buf.as_mut_ptr(),
                        total_bytes,
                    );
                    return Ok(());
                }
            }
        }
        Err(AtaError::Timeout)
    }

    pub fn write_dma(&mut self, lba: u32, count: u8, data: &[u8]) -> Result<(), AtaError> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        let bmba = self.bmba.ok_or(AtaError::Error)?;
        let count = count.max(1).min(8);
        let total_bytes = (count as usize) * 512;
        if data.len() < total_bytes {
            return Err(AtaError::Error);
        }

        unsafe {
            let prdt_phys = &PRDT as *const _ as u32;
            let data_phys = &DMA_DATA as *const _ as u32;

            core::ptr::copy_nonoverlapping(data.as_ptr(), DMA_DATA.0.as_mut_ptr(), total_bytes);

            let prdt_entries =
                core::slice::from_raw_parts_mut(&mut PRDT.0 as *mut u8 as *mut PrdtEntry, 512);
            prdt_entries[0].data_buffer_phys = data_phys;
            prdt_entries[0].count = (total_bytes as u16).min(0xFFFE);
            prdt_entries[0].eot = 0x8000;
            for i in 1..512 {
                prdt_entries[i].data_buffer_phys = 0;
                prdt_entries[i].count = 0;
                prdt_entries[i].eot = 0;
            }

            core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

            let mut bm_cmd: Port<u8> = Port::new(bmba + BM_COMMAND);
            let mut bm_status: Port<u8> = Port::new(bmba + BM_STATUS);
            let mut bm_prdt: Port<u32> = Port::new(bmba + BM_PRDT_ADDRESS);

            bm_status.write(BM_STAT_INTERRUPT);
            bm_cmd.write(0x00);
            bm_prdt.write(prdt_phys);

            self.wait_not_busy()?;
            self.drive_sel_port
                .write(ATA_DRIVE_SELECT_LBA_BASE | ((abs_lba >> 24) & 0x0F) as u8);
            self.sector_count_port.write(count);
            self.lba_low_port.write((abs_lba & 0xFF) as u8);
            self.lba_mid_port.write(((abs_lba >> 8) & 0xFF) as u8);
            self.lba_high_port.write(((abs_lba >> 16) & 0xFF) as u8);
            self.command_port.write(ATA_CMD_WRITE_DMA);

            bm_cmd.write(BM_CMD_START | BM_CMD_WRITE);

            for _ in 0..2000000 {
                let st = bm_status.read();
                if (st & BM_STAT_ACTIVE) == 0 {
                    if (st & BM_STAT_ERROR) != 0 {
                        return Err(AtaError::Error);
                    }
                    return Ok(());
                }
            }
        }
        Err(AtaError::Timeout)
    }

    pub fn write_sectors(&mut self, lba: u32, count: u8, data: &[u8]) -> Result<(), ()> {
        let abs_lba = self.base_lba.wrapping_add(lba);
        let count = count.max(1);
        if abs_lba > 0x0FFFFFFF { return Err(()); }
        let total_bytes = (count as usize) * 512;
        if data.len() < total_bytes { return Err(()); }
        unsafe {
            self.wait_not_busy_simple()?;
            self.sector_count_port.write(count);
            self.lba_low_port.write((abs_lba & 0xFF) as u8);
            self.lba_mid_port.write(((abs_lba >> 8) & 0xFF) as u8);
            self.lba_high_port.write(((abs_lba >> 16) & 0xFF) as u8);
            let drive_byte = ATA_DRIVE_SELECT_LBA_BASE | ((abs_lba >> 24) & 0x0F) as u8;
            self.drive_sel_port.write(drive_byte);
            self.command_port.write(ATA_CMD_WRITE_MULTIPLE);
            self.wait_not_busy_simple()?;
            for s in 0..count as usize {
                let off = s * 256;
                for i in 0..256 {
                    let word = u16::from_le_bytes([data[off * 2 + i * 2], data[off * 2 + i * 2 + 1]]);
                    self.data_port.write(word);
                }
            }
            self.wait_not_busy_simple()?;
        }
        Ok(())
    }
}
