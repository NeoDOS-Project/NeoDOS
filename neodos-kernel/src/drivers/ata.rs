// src/drivers/ata.rs

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

/// LBA drive-select base (bits 7–4). Must be identical for read and write:
/// QEMU maps `-drive index=0` to IDE master (`0xE0`) and `index=1` to slave (`0xF0`).
/// The NeoDOS FS is on index=1 (the data disk), so we target the slave drive.
const ATA_DRIVE_SELECT_LBA_BASE: u8 = 0xF0;

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
        }
    }

    pub fn write_sector(&mut self, lba: u32, data: &[u8; 512]) -> Result<(), ()> {
        if lba > 0x0FFFFFFF { return Err(()); }
        self.write_sector_inner(lba, data)
    }

    fn write_sector_inner(&mut self, lba: u32, data: &[u8; 512]) -> Result<(), ()> {
        unsafe {
            self.wait_not_busy_simple()?;
            
            self.sector_count_port.write(1);
            self.lba_low_port.write((lba & 0xFF) as u8);
            self.lba_mid_port.write(((lba >> 8) & 0xFF) as u8);
            self.lba_high_port.write(((lba >> 16) & 0xFF) as u8);
            
            let drive_byte = ATA_DRIVE_SELECT_LBA_BASE | ((lba >> 24) & 0x0F) as u8;
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

    fn read_sector_inner(&mut self, lba: u32) -> Result<[u8; 512], AtaError> {
        self.wait_not_busy()?;

        unsafe {
            // Select drive (Slave) and LBA high bits
            self.drive_sel_port.write(ATA_DRIVE_SELECT_LBA_BASE | ((lba >> 24) & 0x0F) as u8);
            
            // Set sector count to 1
            self.sector_count_port.write(1);
            
            // Set LBA bits
            self.lba_low_port.write(lba as u8);
            self.lba_mid_port.write((lba >> 8) as u8);
            self.lba_high_port.write((lba >> 16) as u8);
            
            // Send read command
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
}
