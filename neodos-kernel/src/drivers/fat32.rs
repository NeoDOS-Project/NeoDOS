// src/drivers/fat32.rs - Minimal FAT32 driver for boot partition

use crate::drivers::ata::AtaDriver;

#[derive(Debug)]
pub enum Fat32Error {
    NotFat32,
    InvalidBootSector,
    NotFound,
}

#[derive(Clone, Copy)]
pub struct BootSector {
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u32,
    pub num_fats: u8,
    pub sectors_per_fat: u32,
    pub root_cluster: u32,
}

impl BootSector {
    pub fn from_bytes(data: &[u8; 512]) -> Option<Self> {
        if data[510] != 0x55 || data[511] != 0xAA {
            return None;
        }

        let fs_type = &data[82..90];
        if fs_type != b"FAT32   " {
            return None;
        }

        let bytes_per_sector = u16::from_le_bytes([data[11], data[12]]) as u32;
        if bytes_per_sector != 512 {
            return None;
        }

        Some(BootSector {
            bytes_per_sector,
            sectors_per_cluster: data[13],
            reserved_sectors: u16::from_le_bytes([data[14], data[15]]) as u32,
            num_fats: data[16],
            sectors_per_fat: u32::from_le_bytes([data[36], data[37], data[38], data[39]]),
            root_cluster: u32::from_le_bytes([data[44], data[45], data[46], data[47]]),
        })
    }

    pub fn data_start(&self) -> u32 {
        self.reserved_sectors + (self.num_fats as u32 * self.sectors_per_fat)
    }
}

pub struct Fat32Driver {
    pub boot_sector: BootSector,
}

impl Fat32Driver {
    pub fn new(ata: &mut AtaDriver) -> Result<Self, Fat32Error> {
        // Try sector 0 first (MBR partition)
        let boot_sector_bytes = match ata.read_sector_master(0) {
            Ok(b) => b,
            Err(_) => return Err(Fat32Error::NotFound),
        };
        
        let boot_sector = match BootSector::from_bytes(&boot_sector_bytes) {
            Some(bs) => bs,
            None => {
                // Try sector 2048 (GPT ESP offset)
                let bs = match ata.read_sector_master(2048) {
                    Ok(b) => b,
                    Err(_) => return Err(Fat32Error::NotFound),
                };
                match BootSector::from_bytes(&bs) {
                    Some(bs) => bs,
                    None => return Err(Fat32Error::NotFat32),
                }
            }
        };

        crate::serial_println!("[FAT32] Boot partition ready");
        crate::serial_println!("  Clusters: {} sectors, FAT: {} sectors", 
            boot_sector.sectors_per_cluster, boot_sector.sectors_per_fat);
        crate::serial_println!("  Root cluster: {}", boot_sector.root_cluster);

        Ok(Fat32Driver { boot_sector })
    }

    fn read_sector(&self, ata: &mut AtaDriver, lba: u32) -> Result<[u8; 512], Fat32Error> {
        ata.read_sector_master(lba).map_err(|_| Fat32Error::NotFound)
    }

    fn read_fat_entry(&self, ata: &mut AtaDriver, cluster: u32) -> Result<u32, Fat32Error> {
        let fat_start = self.boot_sector.reserved_sectors;
        let entry_offset = cluster * 4;
        let sector_idx = entry_offset / 512;
        let offset_in_sector = (entry_offset % 512) as usize;

        let sector = self.read_sector(ata, fat_start + sector_idx)?;

        let val = u32::from_le_bytes([
            sector[offset_in_sector],
            sector[offset_in_sector + 1],
            sector[offset_in_sector + 2],
            sector[offset_in_sector + 3],
        ]);

        Ok(val & 0x0FFFFFFF)
    }

    pub fn find_file(&self, ata: &mut AtaDriver, filename: &[u8]) -> Result<(u32, u32), Fat32Error> {
        self.find_in_directory(ata, self.boot_sector.root_cluster, filename)
    }

    fn find_in_directory(&self, ata: &mut AtaDriver, dir_cluster: u32, filename: &[u8]) -> Result<(u32, u32), Fat32Error> {
        let data_start = self.boot_sector.data_start();
        let sectors_per_cluster = self.boot_sector.sectors_per_cluster as u32;
        let mut cluster = dir_cluster;

        loop {
            let lba = data_start + (cluster - 2) * sectors_per_cluster;

            for _ in 0..sectors_per_cluster {
                let sector = self.read_sector(ata, lba)?;

                for entry_off in (0..512).step_by(32) {
                    let entry = &sector[entry_off..entry_off + 32];
                    if entry[0] == 0x00 {
                        return Err(Fat32Error::NotFound);
                    }
                    if entry[0] == 0xE5 {
                        continue;
                    }

                    let attrs = entry[11];
                    if attrs == 0x0F { continue; } // Skip LFN
                    if attrs & 0x10 != 0 { continue; } // Skip directories

                    let name = &entry[0..11];
                    let mut clean_name = [0u8; 11];
                    let mut clean_len = 0;
                    for i in 0..11 {
                        if name[i] == 0x20 { break; }
                        clean_name[clean_len] = name[i];
                        clean_len += 1;
                    }

                    if filename.len() == clean_len && 
                       filename.iter().zip(clean_name.iter()).all(|(a, b)| a == b) {
                        let cluster_high = u16::from_le_bytes([entry[20], entry[21]]) as u32;
                        let cluster_low = u16::from_le_bytes([entry[26], entry[27]]) as u32;
                        let start_cluster = (cluster_high << 16) | cluster_low;
                        let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]);
                        return Ok((start_cluster, size));
                    }
                }
            }

            let next = self.read_fat_entry(ata, cluster)?;
            if next >= 0x0FFFFFF8 { break; }
            cluster = next;
        }

        Err(Fat32Error::NotFound)
    }

    pub fn read_file(&self, ata: &mut AtaDriver, filename: &[u8], buf: &mut [u8]) -> Result<usize, Fat32Error> {
        let (start_cluster, _) = self.find_file(ata, filename)?;
        
        let data_start = self.boot_sector.data_start();
        let sectors_per_cluster = self.boot_sector.sectors_per_cluster as u32;
        let mut cluster = start_cluster;
        let mut offset = 0;

        loop {
            let lba = data_start + (cluster - 2) * sectors_per_cluster;

            for _ in 0..sectors_per_cluster {
                if offset >= buf.len() { return Ok(offset); }
                let sector = self.read_sector(ata, lba)?;
                let copy_len = 512.min(buf.len() - offset);
                buf[offset..offset + copy_len].copy_from_slice(&sector[..copy_len]);
                offset += copy_len;
            }

            let next = self.read_fat_entry(ata, cluster)?;
            if next >= 0x0FFFFFF8 { break; }
            cluster = next;
        }

        Ok(offset)
    }
}