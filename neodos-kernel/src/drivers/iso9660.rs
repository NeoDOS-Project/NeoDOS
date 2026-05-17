#![allow(dead_code)]

use crate::drivers::block::BlockDevice;

const ISO_BLOCK_SIZE: usize = 2048;
const PVD_SECTOR: u32 = 16;
const MAX_CACHED_FILES: usize = 256;

#[derive(Debug)]
pub enum Iso9660Error {
    NotIso9660,
    NotFound,
    IsDirectory,
    NotDirectory,
    IOError,
}

struct ExtentCache {
    entries: [(u32, u32, bool); MAX_CACHED_FILES], // (extent_lba, extent_len, is_dir)
    count: usize,
}

impl ExtentCache {
    const fn new() -> Self {
        ExtentCache {
            entries: [(0, 0, false); MAX_CACHED_FILES],
            count: 0,
        }
    }

    fn add(&mut self, lba: u32, len: u32, is_dir: bool) {
        if self.count >= MAX_CACHED_FILES { return; }
        for i in 0..self.count {
            if self.entries[i].0 == lba { return; }
        }
        self.entries[self.count] = (lba, len, is_dir);
        self.count += 1;
    }

    fn get(&self, lba: u32) -> Option<(u32, bool)> {
        for i in 0..self.count {
            if self.entries[i].0 == lba {
                return Some((self.entries[i].1, self.entries[i].2));
            }
        }
        None
    }
}

pub struct Iso9660Driver {
    root_extent_lba: u32,
    root_extent_len: u32,
    volume_label: [u8; 32],
    extent_cache: ExtentCache,
}

impl Iso9660Driver {
    pub fn new(dev: &mut dyn BlockDevice) -> Result<Self, Iso9660Error> {
        let saved = dev.base_lba();
        dev.set_base_lba(0);

        let mut pvd = [0u8; ISO_BLOCK_SIZE];
        Self::read_iso_block(dev, PVD_SECTOR, &mut pvd).map_err(|_| Iso9660Error::IOError)?;

        dev.set_base_lba(saved);

        if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
            return Err(Iso9660Error::NotIso9660);
        }

        let block_size = u16::from_le_bytes([pvd[128], pvd[129]]) as u32;
        if block_size != 2048 {
            return Err(Iso9660Error::NotIso9660);
        }

        let root_extent_lba = u32::from_le_bytes([pvd[158], pvd[159], pvd[160], pvd[161]]);
        let root_extent_len = u32::from_le_bytes([pvd[166], pvd[167], pvd[168], pvd[169]]);

        let mut volume_label = [b' '; 32];
        volume_label.copy_from_slice(&pvd[40..72]);

        let mut driver = Iso9660Driver {
            root_extent_lba,
            root_extent_len,
            volume_label,
            extent_cache: ExtentCache::new(),
        };
        driver.extent_cache.add(root_extent_lba, root_extent_len, true);

        crate::serial_println!("[ISO9660] PVD OK: root LBA={} len={} blocksize={}",
            root_extent_lba, root_extent_len, block_size);

        Ok(driver)
    }

    fn read_iso_block(dev: &mut dyn BlockDevice, block: u32, buf: &mut [u8; ISO_BLOCK_SIZE]) -> Result<(), ()> {
        let dev_sector = block as u64 * 4;
        dev.read_blocks(dev_sector, 4, buf)
    }

    fn read_extent_bytes(dev: &mut dyn BlockDevice, extent_lba: u32, extent_len: u32, offset: u64, buf: &mut [u8]) -> Result<usize, Iso9660Error> {
        if offset as u32 >= extent_len {
            return Ok(0);
        }
        let start = offset as usize;
        let avail = (extent_len as usize).saturating_sub(start);
        let to_read = buf.len().min(avail);
        if to_read == 0 {
            return Ok(0);
        }

        let first_block = extent_lba + (start / ISO_BLOCK_SIZE) as u32;
        let last_byte = start + to_read - 1;
        let last_block = extent_lba + (last_byte / ISO_BLOCK_SIZE) as u32;
        let num_blocks = (last_block - first_block + 1) as usize;

        let mut tmp = [0u8; ISO_BLOCK_SIZE];
        let mut written = 0usize;
        for i in 0..num_blocks {
            Self::read_iso_block(dev, first_block + i as u32, &mut tmp)
                .map_err(|_| Iso9660Error::IOError)?;

            let copy_start = if i == 0 { start % ISO_BLOCK_SIZE } else { 0 };
            let raw_end = start + to_read;
            let copy_end = if i == num_blocks - 1 {
                let end_rel = raw_end % ISO_BLOCK_SIZE;
                if end_rel == 0 { ISO_BLOCK_SIZE } else { end_rel }
            } else {
                ISO_BLOCK_SIZE
            };
            let chunk = copy_end - copy_start;
            buf[written..written + chunk].copy_from_slice(&tmp[copy_start..copy_end]);
            written += chunk;
        }
        Ok(written)
    }

    fn for_each_dir_entry<F>(dev: &mut dyn BlockDevice, extent_lba: u32, extent_len: u32, mut f: F) -> Result<(), Iso9660Error>
    where
        F: FnMut(u32, u32, bool, &str) -> bool
    {
        let mut remaining = extent_len as usize;
        let mut block = extent_lba;

        while remaining > 0 {
            let mut sector = [0u8; ISO_BLOCK_SIZE];
            Self::read_iso_block(dev, block, &mut sector)
                .map_err(|_| Iso9660Error::IOError)?;

            let mut off = 0usize;
            while off + 33 <= ISO_BLOCK_SIZE {
                let rec_len = sector[off] as usize;
                if rec_len == 0 {
                    break;
                }
                if off + rec_len > ISO_BLOCK_SIZE {
                    break;
                }

                let name_len = sector[off + 32] as usize;

                if name_len == 1 && (sector[off + 33] == 0x00 || sector[off + 33] == 0x01) {
                    off += rec_len;
                    continue;
                }

                if name_len > 0 && name_len <= 200 {
                    let entry_lba = u32::from_le_bytes([
                        sector[off + 2], sector[off + 3], sector[off + 4], sector[off + 5],
                    ]);
                    let entry_len = u32::from_le_bytes([
                        sector[off + 10], sector[off + 11], sector[off + 12], sector[off + 13],
                    ]);
                    let is_dir = (sector[off + 25] & 0x02) != 0;

                    let mut actual_len = name_len;
                    let name_start = off + 33;
                    if actual_len > 2
                        && sector[name_start + actual_len - 2] == b';'
                        && sector[name_start + actual_len - 1] >= b'0'
                        && sector[name_start + actual_len - 1] <= b'9'
                    {
                        actual_len -= 2;
                    }

                    if actual_len > 0 {
                        if let Ok(name) = core::str::from_utf8(&sector[name_start..name_start + actual_len]) {
                            if f(entry_lba, entry_len, is_dir, name) {
                                return Ok(());
                            }
                        }
                    }
                }

                off += rec_len;
            }

            remaining = remaining.saturating_sub(ISO_BLOCK_SIZE);
            block += 1;
        }

        Ok(())
    }

    fn find_entry_in_dir(&mut self, dev: &mut dyn BlockDevice, dir_lba: u32, dir_len: u32, name: &str) -> Result<(u32, u32, bool), Iso9660Error> {
        let mut result = Err(Iso9660Error::NotFound);
        let target = name;
        Self::for_each_dir_entry(dev, dir_lba, dir_len, |lba, len, is_dir, entry_name| {
            if entry_name.eq_ignore_ascii_case(target) {
                result = Ok((lba, len, is_dir));
                true
            } else {
                false
            }
        })?;
        if let Ok((lba, len, is_dir)) = result {
            self.extent_cache.add(lba, len, is_dir);
        }
        result
    }
}

use crate::fs::vfs::{FileSystem, VfsError, VfsNode, DirEntry as VfsDirEntry, MODE_DIR, MODE_FILE};

impl From<Iso9660Error> for VfsError {
    fn from(err: Iso9660Error) -> Self {
        match err {
            Iso9660Error::NotFound => VfsError::NotFound,
            Iso9660Error::IsDirectory => VfsError::NotAFile,
            Iso9660Error::NotDirectory => VfsError::NotADirectory,
            _ => VfsError::IOError,
        }
    }
}

impl FileSystem for Iso9660Driver {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut dev_lock = crate::globals::ATA_DRIVER.lock();
        let dev: &mut dyn BlockDevice = dev_lock.as_mut().ok_or(VfsError::IOError)?;
        let saved = dev.base_lba();
        dev.set_base_lba(0);

        let (extent_len, _is_dir) = self.extent_cache.get(inode).ok_or(VfsError::IOError)?;
        let result = Self::read_extent_bytes(dev, inode, extent_len, offset, buf)?;

        dev.set_base_lba(saved);
        Ok(result)
    }

    fn write(&mut self, _inode: u32, _offset: u64, _buf: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::PermissionDenied)
    }

    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let mut dev_lock = crate::globals::ATA_DRIVER.lock();
        let dev: &mut dyn BlockDevice = dev_lock.as_mut().ok_or(VfsError::IOError)?;
        let saved = dev.base_lba();
        dev.set_base_lba(0);

        let (dir_len, _) = self.extent_cache.get(dir_inode).ok_or(VfsError::NotADirectory)?;
        let (entry_lba, entry_len, is_dir) = self.find_entry_in_dir(dev, dir_inode, dir_len, name)?;

        dev.set_base_lba(saved);
        Ok(VfsNode {
            inode: entry_lba,
            mode: if is_dir { MODE_DIR } else { MODE_FILE },
            size: entry_len,
        })
    }

    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<VfsDirEntry>, VfsError> {
        let mut dev_lock = crate::globals::ATA_DRIVER.lock();
        let dev: &mut dyn BlockDevice = dev_lock.as_mut().ok_or(VfsError::IOError)?;
        let saved = dev.base_lba();
        dev.set_base_lba(0);

        let (dir_len, _) = self.extent_cache.get(dir_inode).ok_or(VfsError::NotADirectory)?;

        let mut current_idx = 0usize;
        let mut result: Option<(u32, u32, bool, alloc::string::String)> = None;

        Self::for_each_dir_entry(dev, dir_inode, dir_len, |lba, len, is_dir, name| {
            if current_idx == index {
                result = Some((lba, len, is_dir, alloc::string::String::from(name)));
                true
            } else {
                current_idx += 1;
                false
            }
        })?;

        if let Some((lba, len, is_dir, name)) = result {
            self.extent_cache.add(lba, len, is_dir);
            return Ok(Some(VfsDirEntry {
                name,
                node: VfsNode {
                    inode: lba,
                    mode: if is_dir { MODE_DIR } else { MODE_FILE },
                    size: len,
                },
            }));
        }

        dev.set_base_lba(saved);
        Ok(result)
    }

    fn mkdir(&mut self, _dir_inode: u32, _name: &str) -> Result<VfsNode, VfsError> {
        Err(VfsError::PermissionDenied)
    }

    fn create(&mut self, _dir_inode: u32, _name: &str) -> Result<VfsNode, VfsError> {
        Err(VfsError::PermissionDenied)
    }

    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError> {
        if inode == self.root_extent_lba {
            return Ok(VfsNode {
                inode,
                mode: MODE_DIR,
                size: self.root_extent_len,
            });
        }
        match self.extent_cache.get(inode) {
            Some((len, is_dir)) => Ok(VfsNode {
                inode,
                mode: if is_dir { MODE_DIR } else { MODE_FILE },
                size: len,
            }),
            None => Ok(VfsNode {
                inode,
                mode: MODE_FILE,
                size: 0,
            }),
        }
    }

    fn volume_label(&self) -> Result<alloc::string::String, VfsError> {
        let end = self.volume_label.iter().rposition(|&b| b != b' ').map(|i| i + 1).unwrap_or(0);
        Ok(core::str::from_utf8(&self.volume_label[..end]).unwrap_or("").into())
    }
}
