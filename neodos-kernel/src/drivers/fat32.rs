#![allow(dead_code)]

use crate::drivers::block::BlockDevice;

#[derive(Debug)]
pub enum Fat32Error {
    NotFat32,
    InvalidBootSector,
    NotFound,
    IsDirectory,
    NotDirectory,
}

#[derive(Clone, Copy)]
pub struct BootSector {
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u32,
    pub num_fats: u8,
    pub sectors_per_fat: u32,
    pub root_cluster: u32,
    pub volume_label: [u8; 11],
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
            volume_label: data[71..82].try_into().ok()?,
        })
    }

    pub fn data_start(&self) -> u32 {
        self.reserved_sectors + (self.num_fats as u32 * self.sectors_per_fat)
    }
}

pub struct DirEntry {
    pub name: [u8; 11],
    pub name_len: usize,
    pub is_directory: bool,
    pub cluster: u32,
    pub size: u32,
}

pub struct Fat32Driver {
    pub boot_sector: BootSector,
    base_lba: u32,
}

impl Fat32Driver {
    pub fn new() -> Result<Self, Fat32Error> {
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(0).ok_or(Fat32Error::NotFound)?;
        let saved_base = dev.base_lba();
        dev.set_base_lba(0);
        let boot_sector_bytes = match dev.read_sector(0) {
            Ok(b) => b,
            Err(_) => {
                dev.set_base_lba(saved_base);
                return Err(Fat32Error::NotFound);
            }
        };

        let (boot_sector, base_lba) = match BootSector::from_bytes(&boot_sector_bytes) {
            Some(bs) => (bs, 0),
            None => {
                let bs = match dev.read_sector(2048) {
                    Ok(b) => b,
                    Err(_) => {
                        dev.set_base_lba(saved_base);
                        return Err(Fat32Error::NotFound);
                    }
                };
                match BootSector::from_bytes(&bs) {
                    Some(bs) => (bs, 2048),
                    None => {
                        dev.set_base_lba(saved_base);
                        return Err(Fat32Error::NotFat32);
                    }
                }
            }
        };
        dev.set_base_lba(saved_base);

        crate::serial_println!("[FAT32] Boot partition ready");
        crate::serial_println!("  Clusters: {} sectors, FAT: {} sectors",
            boot_sector.sectors_per_cluster, boot_sector.sectors_per_fat);
        crate::serial_println!("  Root cluster: {}", boot_sector.root_cluster);

        Ok(Fat32Driver { boot_sector, base_lba })
    }

    fn read_sector(&self, dev: &mut dyn BlockDevice, lba: u32) -> Result<[u8; 512], Fat32Error> {
        let saved_base = dev.base_lba();
        dev.set_base_lba(0);
        let result = dev.read_sector((self.base_lba + lba) as u64).map_err(|_| Fat32Error::NotFound);
        dev.set_base_lba(saved_base);
        result
    }

    fn read_fat_entry(&self, dev: &mut dyn BlockDevice, cluster: u32) -> Result<u32, Fat32Error> {
        let fat_start = self.boot_sector.reserved_sectors;
        let entry_offset = cluster * 4;
        let sector_idx = entry_offset / 512;
        let offset_in_sector = (entry_offset % 512) as usize;

        let sector = self.read_sector(dev, fat_start + sector_idx)?;

        let val = u32::from_le_bytes([
            sector[offset_in_sector],
            sector[offset_in_sector + 1],
            sector[offset_in_sector + 2],
            sector[offset_in_sector + 3],
        ]);

        Ok(val & 0x0FFFFFFF)
    }

    fn name_to_11byte(name: &[u8]) -> [u8; 11] {
        let mut raw = [0x20u8; 11];
        if let Some(dot) = name.iter().position(|&b| b == b'.') {
            let base = &name[..dot];
            let ext = &name[dot + 1..];
            let n = base.len().min(8);
            raw[..n].copy_from_slice(&base[..n]);
            let e = ext.len().min(3);
            raw[8..8 + e].copy_from_slice(&ext[..e]);
        } else {
            let n = name.len().min(8);
            raw[..n].copy_from_slice(&name[..n]);
        }
        for b in &mut raw {
            *b = b.to_ascii_uppercase();
        }
        raw
    }

    fn parse_entry(entry: &[u8; 32]) -> Option<DirEntry> {
        if entry[0] == 0x00 || entry[0] == 0xE5 {
            return None;
        }
        let attrs = entry[11];
        if attrs == 0x0F {
            return None;
        }

        let mut name = [0x20u8; 11];
        name.copy_from_slice(&entry[..11]);

        if name[0] == 0x20 {
            return None;
        }

        let cluster_high = u16::from_le_bytes([entry[20], entry[21]]) as u32;
        let cluster_low = u16::from_le_bytes([entry[26], entry[27]]) as u32;
        let cluster = (cluster_high << 16) | cluster_low;
        let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]);

        Some(DirEntry {
            name,
            name_len: 11,
            is_directory: (attrs & 0x10) != 0,
            cluster,
            size,
        })
    }

    fn find_entry_in_directory(
        &self,
        dev: &mut dyn BlockDevice,
        dir_cluster: u32,
        name_11: &[u8; 11],
    ) -> Result<DirEntry, Fat32Error> {
        let data_start = self.boot_sector.data_start();
        let sectors_per_cluster = self.boot_sector.sectors_per_cluster as u32;
        let mut cluster = dir_cluster;

        loop {
            let lba = data_start + (cluster - 2) * sectors_per_cluster;

            for i in 0..sectors_per_cluster {
                let sector = self.read_sector(dev, lba + i)?;

                for entry_off in (0..512).step_by(32) {
                    let array: &[u8; 32] = match sector[entry_off..entry_off + 32].try_into() {
                        Ok(a) => a,
                        Err(_) => continue,
                    };
                    let Some(parsed) = Self::parse_entry(array) else {
                        if sector[entry_off] == 0x00 {
                            return Err(Fat32Error::NotFound);
                        }
                        continue;
                    };

                    if &parsed.name[..] == name_11 {
                        return Ok(parsed);
                    }
                }
            }

            let next = self.read_fat_entry(dev, cluster)?;
            if next >= 0x0FFFFFF8 {
                break;
            }
            cluster = next;
        }

        Err(Fat32Error::NotFound)
    }

    pub fn resolve_path(&self, dev: &mut dyn BlockDevice, path: &str) -> Result<DirEntry, Fat32Error> {
        let bytes = path.as_bytes();
        if bytes.is_empty() || bytes == b"/" || bytes == b"\\" {
            return Ok(DirEntry {
                name: [0x20u8; 11],
                name_len: 1,
                is_directory: true,
                cluster: self.boot_sector.root_cluster,
                size: 0,
            });
        }

        let mut part_buf = [""; 32];
        let mut part_count = 0;
        for segment in path.split(|c| c == '/' || c == '\\') {
            if !segment.is_empty() && part_count < 32 {
                part_buf[part_count] = segment;
                part_count += 1;
            }
        }

        let mut current_cluster = self.boot_sector.root_cluster;
        for i in 0..part_count {
            let segment = part_buf[i];
            let name_11 = Self::name_to_11byte(segment.as_bytes());
            let entry = self.find_entry_in_directory(dev, current_cluster, &name_11)?;
            if !entry.is_directory || i == part_count - 1 {
                return Ok(entry);
            }
            current_cluster = entry.cluster;
        }

        Ok(DirEntry {
            name: [0x20u8; 11],
            name_len: 1,
            is_directory: true,
            cluster: current_cluster,
            size: 0,
        })
    }

    pub fn list_directory(&self, dev: &mut dyn BlockDevice, path: &str) -> Result<(), Fat32Error> {
        let dir_entry = self.resolve_path(dev, path)?;
        if !dir_entry.is_directory {
            return Err(Fat32Error::NotDirectory);
        }

        let data_start = self.boot_sector.data_start();
        let sectors_per_cluster = self.boot_sector.sectors_per_cluster as u32;
        let mut cluster = dir_entry.cluster;

        loop {
            let lba = data_start + (cluster - 2) * sectors_per_cluster;

            for i in 0..sectors_per_cluster {
                let sector = self.read_sector(dev, lba + i)?;

                for entry_off in (0..512).step_by(32) {
                    let array: &[u8; 32] = match sector[entry_off..entry_off + 32].try_into() {
                        Ok(a) => a,
                        Err(_) => continue,
                    };
                    let Some(entry) = Self::parse_entry(array) else {
                        if sector[entry_off] == 0x00 {
                            return Ok(());
                        }
                        continue;
                    };

                    let mut name_part = [0u8; 8];
                    let mut ext_part = [0u8; 3];
                    let mut name_len = 0;
                    let mut ext_len = 0;
                    for j in 0..8 {
                        if entry.name[j] == 0x20 {
                            break;
                        }
                        name_part[name_len] = entry.name[j];
                        name_len += 1;
                    }
                    for j in 0..3 {
                        if entry.name[8 + j] == 0x20 {
                            break;
                        }
                        ext_part[ext_len] = entry.name[8 + j];
                        ext_len += 1;
                    }

                    if entry.is_directory {
                        let s = core::str::from_utf8(&name_part[..name_len]).unwrap_or("?");
                        crate::println!("  {:<8}   <DIR>", s);
                    } else {
                        let s = core::str::from_utf8(&name_part[..name_len]).unwrap_or("?");
                        let e = core::str::from_utf8(&ext_part[..ext_len]).unwrap_or("?");
                        crate::println!("  {:<8}.{:<3} {:>8}", s, e, entry.size);
                    }
                }
            }

            let next = self.read_fat_entry(dev, cluster)?;
            if next >= 0x0FFFFFF8 {
                break;
            }
            cluster = next;
        }

        Ok(())
    }

    pub fn read_file_by_cluster(
        &self,
        dev: &mut dyn BlockDevice,
        start_cluster: u32,
        buf: &mut [u8],
    ) -> Result<usize, Fat32Error> {
        let data_start = self.boot_sector.data_start();
        let sectors_per_cluster = self.boot_sector.sectors_per_cluster as u32;
        let mut cluster = start_cluster;
        let mut offset = 0;

        loop {
            let lba = data_start + (cluster - 2) * sectors_per_cluster;

            for i in 0..sectors_per_cluster {
                if offset >= buf.len() {
                    return Ok(offset);
                }
                let sector = self.read_sector(dev, lba + i)?;
                let copy_len = 512.min(buf.len() - offset);
                buf[offset..offset + copy_len].copy_from_slice(&sector[..copy_len]);
                offset += copy_len;
            }

            let next = self.read_fat_entry(dev, cluster)?;
            if next >= 0x0FFFFFF8 {
                break;
            }
            cluster = next;
        }

        Ok(offset)
    }

    pub fn find_file(&self, dev: &mut dyn BlockDevice, filename: &[u8]) -> Result<DirEntry, Fat32Error> {
        let name_11 = Self::name_to_11byte(filename);
        self.find_entry_in_directory(dev, self.boot_sector.root_cluster, &name_11)
    }

    pub fn read_file(&self, dev: &mut dyn BlockDevice, filename: &[u8], buf: &mut [u8]) -> Result<usize, Fat32Error> {
        let entry = self.find_file(dev, filename)?;
        if entry.is_directory {
            return Err(Fat32Error::IsDirectory);
        }
        self.read_file_by_cluster(dev, entry.cluster, buf)
    }

    pub fn read_file_by_path(&self, dev: &mut dyn BlockDevice, path: &str, buf: &mut [u8]) -> Result<usize, Fat32Error> {
        let entry = self.resolve_path(dev, path)?;
        if entry.is_directory {
            return Err(Fat32Error::IsDirectory);
        }
        self.read_file_by_cluster(dev, entry.cluster, buf)
    }
}

use crate::fs::vfs::{FileSystem, VfsError, VfsNode, DirEntry as VfsDirEntry, MODE_DIR, MODE_FILE};

impl From<Fat32Error> for VfsError {
    fn from(err: Fat32Error) -> Self {
        match err {
            Fat32Error::NotFound => VfsError::NotFound,
            Fat32Error::IsDirectory => VfsError::NotAFile,
            Fat32Error::NotDirectory => VfsError::NotADirectory,
            _ => VfsError::IOError,
        }
    }
}

impl FileSystem for Fat32Driver {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(0).ok_or(VfsError::IOError)?;

        let mut temp_buf = alloc::vec::Vec::with_capacity(buf.len() + offset as usize);
        temp_buf.resize(buf.len() + offset as usize, 0);
        
        let read = self.read_file_by_cluster(dev, inode, &mut temp_buf)?;
        
        if offset as usize >= read {
            return Ok(0);
        }
        
        let available = read - offset as usize;
        let to_copy = available.min(buf.len());
        buf[..to_copy].copy_from_slice(&temp_buf[offset as usize..offset as usize + to_copy]);
        
        Ok(to_copy)
    }

    fn write(&mut self, _inode: u32, _offset: u64, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::PermissionDenied) 
    }

    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(0).ok_or(VfsError::IOError)?;

        let name_11 = Self::name_to_11byte(name.as_bytes());
        let entry = self.find_entry_in_directory(dev, dir_inode, &name_11)?;
        
        Ok(VfsNode {
            inode: entry.cluster,
            mode: if entry.is_directory { MODE_DIR } else { MODE_FILE },
            size: entry.size,
        })
    }

    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<VfsDirEntry>, VfsError> {
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(0).ok_or(VfsError::IOError)?;

        let data_start = self.boot_sector.data_start();
        let sectors_per_cluster = self.boot_sector.sectors_per_cluster as u32;
        let mut cluster = dir_inode;
        let mut current_idx = 0;

        loop {
            let lba = data_start + (cluster - 2) * sectors_per_cluster;

            for i in 0..sectors_per_cluster {
                let sector = self.read_sector(dev, lba + i)?;

                for entry_off in (0..512).step_by(32) {
                    let array: &[u8; 32] = match sector[entry_off..entry_off + 32].try_into() {
                        Ok(a) => a,
                        Err(_) => continue,
                    };
                    let Some(entry) = Self::parse_entry(array) else {
                        if sector[entry_off] == 0x00 {
                            return Ok(None);
                        }
                        continue;
                    };

                    if current_idx == index {
                        let mut name = alloc::string::String::new();
                        for j in 0..8 {
                            if entry.name[j] == 0x20 { break; }
                            name.push(entry.name[j] as char);
                        }
                        let has_ext = entry.name[8..11].iter().any(|b| *b != 0x20);
                        if !entry.is_directory && has_ext {
                            name.push('.');
                            for j in 0..3 {
                                if entry.name[8 + j] == 0x20 { break; }
                                name.push(entry.name[8 + j] as char);
                            }
                        }

                        return Ok(Some(VfsDirEntry {
                            name,
                            node: VfsNode {
                                inode: entry.cluster,
                                mode: if entry.is_directory { MODE_DIR } else { MODE_FILE },
                                size: entry.size,
                            }
                        }));
                    }
                    current_idx += 1;
                }
            }

            let next = self.read_fat_entry(dev, cluster)?;
            if next >= 0x0FFFFFF8 {
                break;
            }
            cluster = next;
        }

        Ok(None)
    }

    fn mkdir(&mut self, _dir_inode: u32, _name: &str) -> Result<VfsNode, VfsError> {
        Err(VfsError::PermissionDenied)
    }

    fn create(&mut self, _dir_inode: u32, _name: &str) -> Result<VfsNode, VfsError> {
        Err(VfsError::PermissionDenied)
    }

    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError> {
        if inode == 0 || inode == self.boot_sector.root_cluster {
            return Ok(VfsNode {
                inode: self.boot_sector.root_cluster,
                mode: MODE_DIR,
                size: 0,
            });
        }
        Ok(VfsNode {
            inode,
            mode: MODE_FILE, 
            size: 0,
        })
    }

    fn volume_label(&self) -> Result<alloc::string::String, VfsError> {
        let len = self
            .boot_sector
            .volume_label
            .iter()
            .rposition(|b| *b != b' ')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let label = core::str::from_utf8(&self.boot_sector.volume_label[..len])
            .unwrap_or("");
        Ok(alloc::string::String::from(label))
    }
}
