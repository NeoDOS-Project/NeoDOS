// src/fs/neodos_fs.rs

#![allow(dead_code)]

use crate::buffer::block_cache::BlockCache;
use crate::buffer::page_cache::PageCache;
use crate::drivers::block::BlockDevice;
use crate::vfs::io::IoStack;
use crate::serial_println;

// ── CRC32 (standard Ethernet/802.3 polynomial) ──

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ── Superblock ──

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Superblock {
    pub magic: u32,              // 0x4F444F4E = "NEOD" little-endian
    pub block_size: u32,         // Typ. 4096
    pub num_blocks: u32,         // Total blocks
    pub num_inodes: u32,         // Max inodes
    pub created: u64,            // Timestamp
    pub label_len: u8,           // Volume label length (0-11)
    pub label: [u8; 11],         // Volume label (11 bytes, DOS standard)
    pub checksum_interval: u32,  // How often to verify checksums (0 = disabled)
    pub reserved: [u8; 464],    // Padding to 512 bytes (reserved[0..4] holds CRC32)
}

impl Superblock {
    /// CRC32 covers bytes 0..36 (magic through label_len+label), skipping the
    /// checksum/reserved area. Stored in reserved[0..4].
    pub fn compute_checksum(&self) -> u32 {
        let self_bytes = unsafe {
            core::slice::from_raw_parts(self as *const _ as *const u8, 36)
        };
        crc32(self_bytes)
    }

    pub fn update_checksum(&mut self) {
        let cksum = self.compute_checksum();
        self.reserved[0..4].copy_from_slice(&cksum.to_le_bytes());
    }

    pub fn verify_checksum(&self) -> bool {
        let stored = u32::from_le_bytes([
            self.reserved[0], self.reserved[1],
            self.reserved[2], self.reserved[3],
        ]);
        stored == 0 || stored == self.compute_checksum()
    }
}

pub const SUPERBLOCK_MAGIC: u32 = 0x4F444F4E;  // "NEOD"
pub const BLOCK_SIZE: usize = 4096;
pub const ROOT_INODE: u32 = 0;
pub const MAX_DIRECT_BLOCKS: usize = 12;
pub const INDIRECT_ENTRIES: usize = 1024;       // 4096 / 4
pub const MAX_FILE_BLOCKS: usize = MAX_DIRECT_BLOCKS + INDIRECT_ENTRIES; // 1036

// ── DOS reserved names ──

const DOS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4",
    "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn is_reserved_dos_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    // Check exact matches
    if DOS_RESERVED_NAMES.contains(&upper.as_str()) {
        return true;
    }
    // Check for names with extensions (CON.TXT etc.)
    if let Some(base) = upper.split('.').next() {
        if DOS_RESERVED_NAMES.contains(&base) {
            return true;
        }
    }
    false
}

// ── Block bitmap ──

pub struct BlockBitmap {
    bits: alloc::vec::Vec<u8>,
}

impl BlockBitmap {
    pub fn new(num_blocks: u32) -> Self {
        let size = (num_blocks as usize + 7) / 8;
        BlockBitmap { bits: alloc::vec![0u8; size] }
    }

    pub fn alloc(&mut self) -> Option<u32> {
        for (i, byte) in self.bits.iter_mut().enumerate() {
            if *byte != 0xFF {
                for bit in 0..8 {
                    let mask = 1u8 << bit;
                    if *byte & mask == 0 {
                        *byte |= mask;
                        return Some((i * 8 + bit) as u32);
                    }
                }
            }
        }
        None
    }

    pub fn alloc_specific(&mut self, block: u32) -> bool {
        let idx = block as usize / 8;
        let bit = block as usize % 8;
        if idx < self.bits.len() {
            let mask = 1u8 << bit;
            if self.bits[idx] & mask == 0 {
                self.bits[idx] |= mask;
                return true;
            }
        }
        false
    }

    pub fn free(&mut self, block: u32) {
        let idx = block as usize / 8;
        let bit = block as usize % 8;
        if idx < self.bits.len() {
            self.bits[idx] &= !(1u8 << bit);
        }
    }

    pub fn mark_used(&mut self, block: u32) {
        let idx = block as usize / 8;
        let bit = block as usize % 8;
        if idx < self.bits.len() {
            self.bits[idx] |= 1u8 << bit;
        }
    }

    pub fn is_used(&self, block: u32) -> bool {
        let idx = block as usize / 8;
        let bit = block as usize % 8;
        if idx < self.bits.len() {
            (self.bits[idx] & (1u8 << bit)) != 0
        } else {
            false
        }
    }
}

// ── Inode ──

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Inode {
    pub inode_num: u32,          // 0-65535
    pub mode: u16,               // File type + permissions (0x40 = dir, 0x80 = file)
    pub size: u32,               // Bytes
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub link_count: u16,
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub direct_blocks: [u32; 12],
    pub indirect_block: u32,
    pub checksum: u32,           // CRC32 of inode_num..indirect_block (first 96 bytes)
    pub padding: [u8; 156],      // 100 + 156 = 256 bytes exactly
}

impl Inode {
    /// CRC32 covers bytes 0..96 (inode_num through indirect_block).
    /// Checksum field and padding are excluded.
    pub fn compute_checksum(&self) -> u32 {
        let self_bytes = unsafe {
            core::slice::from_raw_parts(self as *const _ as *const u8, 96)
        };
        crc32(self_bytes)
    }

    pub fn set_checksum(&mut self) {
        self.checksum = self.compute_checksum();
    }

    pub fn verify_checksum(&self) -> bool {
        if self.checksum == 0 {
            return true; // No checksum set
        }
        self.checksum == self.compute_checksum()
    }
}

pub const MODE_DIR: u16 = 0x40;
pub const MODE_FILE: u16 = 0x80;

// Permission flags (stored in mode bits 0-4, coexist with MODE_DIR/MODE_FILE)
pub const PERM_R: u16 = 0x0001;
pub const PERM_W: u16 = 0x0002;
pub const PERM_X: u16 = 0x0004;
pub const PERM_S: u16 = 0x0008;
pub const PERM_D: u16 = 0x0010;

// DOS file attributes
pub const ATTR_READONLY: u8 = 0x01;
pub const ATTR_HIDDEN: u8   = 0x02;
pub const ATTR_SYSTEM: u8   = 0x04;
pub const ATTR_VOLUME: u8   = 0x08;
pub const ATTR_DIR: u8      = 0x10;
pub const ATTR_ARCHIVE: u8  = 0x20;

// ── DirectoryEntry ──

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DirectoryEntry {
    pub inode_num: u32,
    pub name_len: u8,
    pub entry_type: u8,          // 1=file, 2=dir
    pub attributes: u8,          // DOS attributes (R=1, H=2, S=4, A=16)
    pub name: [u8; 249],         // Fits 256 bytes total
}

pub const DIR_ENTRY_SIZE: usize = 256;

#[derive(Debug)]
pub enum FsError {
    InvalidSuperblock,
    InvalidBlockSize,
    ChecksumMismatch,
    NotADirectory,
    NotAFile,
    FileNotFound,
    NoInodeAvailable,
    NoBlockAvailable,
    BlockDeviceError,
    DirectoryNotEmpty,
    ReservedName,
    IndirectBlockFull,
}

impl From<()> for FsError {
    fn from(_: ()) -> Self {
        FsError::BlockDeviceError
    }
}

// ── Inode Cache ──

pub struct InodeCache {
    pub(crate) inodes: alloc::vec::Vec<Option<Inode>>,
    num_inodes: u32,
}

impl InodeCache {
    pub fn new() -> Self {
        InodeCache {
            inodes: alloc::vec::Vec::new(),
            num_inodes: 0,
        }
    }

    pub fn ensure_inode_capacity(&mut self, max_idx: usize) {
        if max_idx >= self.inodes.len() {
            self.inodes.resize(max_idx + 1, None);
        }
    }

    pub fn load_inode(&mut self, inode_num: usize, cache: &mut BlockCache, dev: &mut dyn BlockDevice, partition_base: u32)
        -> Result<&Inode, FsError>
    {
        if inode_num >= self.num_inodes as usize {
            return Err(FsError::FileNotFound);
        }

        self.ensure_inode_capacity(inode_num);

        if self.inodes[inode_num].is_none() {
            let inode_sector = 1 + (inode_num as u32 / 2);
            let offset_in_sector = (inode_num % 2) * 256;

            let sector_data = cache.get_sector(inode_sector + partition_base, dev)?;
            let inode: Inode = unsafe {
                core::ptr::read_unaligned(
                    sector_data.as_ptr().add(offset_in_sector) as *const _
                )
            };

            if !inode.verify_checksum() {
                serial_println!("[!] FS: Inode {} checksum FAILED", inode_num);
                return Err(FsError::ChecksumMismatch);
            }

            self.inodes[inode_num] = Some(inode);
        }

        unsafe {
            Ok(self.inodes.as_ptr().add(inode_num).as_ref().unwrap().as_ref().unwrap())
        }
    }

    /// Load without checksum verification (for recovery/journal replay).
    pub fn load_inode_skip_crc(&mut self, inode_num: usize, cache: &mut BlockCache, dev: &mut dyn BlockDevice, partition_base: u32)
        -> Result<&Inode, FsError>
    {
        if inode_num >= self.num_inodes as usize {
            return Err(FsError::FileNotFound);
        }
        self.ensure_inode_capacity(inode_num);
        if self.inodes[inode_num].is_none() {
            let inode_sector = 1 + (inode_num as u32 / 2);
            let offset_in_sector = (inode_num % 2) * 256;
            let sector_data = cache.get_sector(inode_sector + partition_base, dev)?;
            let inode: Inode = unsafe {
                core::ptr::read_unaligned(
                    sector_data.as_ptr().add(offset_in_sector) as *const _
                )
            };
            self.inodes[inode_num] = Some(inode);
        }
        unsafe {
            Ok(self.inodes.as_ptr().add(inode_num).as_ref().unwrap().as_ref().unwrap())
        }
    }
}

// ── Indirect block helpers ──

fn read_indirect_pointers(cache: &mut BlockCache, dev: &mut dyn BlockDevice, abs_lba: u32, data_start: u32, indirect_block: u32) -> Result<[u32; INDIRECT_ENTRIES], FsError> {
    let mut pointers = [0u32; INDIRECT_ENTRIES];
    let block_base = data_start + (indirect_block * 8);
    for i in 0..INDIRECT_ENTRIES {
        let sector_idx = block_base + (i / 128) as u32;
        let offset = (i % 128) * 4;
        let data = cache.get_sector(abs_lba + sector_idx, dev)?;
        pointers[i] = u32::from_le_bytes([
            data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
        ]);
    }
    Ok(pointers)
}

fn write_indirect_pointer(cache: &mut BlockCache, dev: &mut dyn BlockDevice, abs_lba: u32, data_start: u32, indirect_block: u32, idx: usize, value: u32) -> Result<(), FsError> {
    let sector_idx = data_start + (indirect_block * 8) + (idx / 128) as u32;
    let offset = (idx % 128) * 4;
    let data = cache.get_sector_mut(abs_lba + sector_idx, dev)?;
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    cache.mark_dirty(abs_lba + sector_idx);
    Ok(())
}

fn write_indirect_block_all(cache: &mut BlockCache, dev: &mut dyn BlockDevice, abs_lba: u32, data_start: u32, indirect_block: u32, pointers: &[u32; INDIRECT_ENTRIES]) -> Result<(), FsError> {
    let block_base = data_start + (indirect_block * 8);
    for i in 0..INDIRECT_ENTRIES {
        let sector_idx = block_base + (i / 128) as u32;
        let offset = (i % 128) * 4;
        let data = cache.get_sector_mut(abs_lba + sector_idx, dev)?;
        data[offset..offset + 4].copy_from_slice(&pointers[i].to_le_bytes());
        cache.mark_dirty(abs_lba + sector_idx);
    }
    Ok(())
}

// ── NeoDosFs ──

pub struct NeoDosFs {
    pub superblock: Superblock,
    pub inode_cache: InodeCache,
    pub block_bitmap: BlockBitmap,
    pub io_stack: IoStack,
    pub drive_id: u8,
}

impl NeoDosFs {
    fn data_start_sector(&self) -> u32 {
        let inode_table_sectors = (self.superblock.num_inodes * 256 + 511) / 512;
        1 + inode_table_sectors
    }

    fn abs_lba(&self, rel_lba: u32) -> u32 {
        self.io_stack.translate_lba(rel_lba as u64) as u32
    }

    pub(crate) fn directory_byte_span(inode: &Inode) -> usize {
        let mut span = inode.size as usize;
        for i in 0..MAX_DIRECT_BLOCKS {
            let b = inode.direct_blocks[i];
            let has_extent = b != 0
                || ((inode.mode & MODE_DIR) != 0 && i == 0 && inode.size > 0);
            if has_extent {
                span = span.max((i + 1) * BLOCK_SIZE);
            }
        }
        // If indirect_block is allocated, account for full span
        if inode.indirect_block != 0 && span > MAX_DIRECT_BLOCKS * BLOCK_SIZE {
            span = span.max(inode.size as usize);
        }
        span
    }

    pub fn inode_block_count(inode: &Inode) -> usize {
        let span = if (inode.mode & MODE_DIR) != 0 {
            let mut s = inode.size as usize;
            for i in 0..MAX_DIRECT_BLOCKS {
                let b = inode.direct_blocks[i];
                let has = b != 0 || ((inode.mode & MODE_DIR) != 0 && i == 0 && inode.size > 0);
                if has { s = s.max((i + 1) * BLOCK_SIZE); }
            }
            if inode.indirect_block != 0 && s > MAX_DIRECT_BLOCKS * BLOCK_SIZE {
                s = s.max(inode.size as usize);
            }
            s
        } else {
            inode.size as usize
        };
        if span == 0 { 0 } else { span.div_ceil(BLOCK_SIZE).min(MAX_FILE_BLOCKS) }
    }

    pub(crate) fn rebuild_bitmap_with_io(&mut self) -> Result<(), FsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(FsError::BlockDeviceError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(FsError::BlockDeviceError)?;
        self.rebuild_bitmap(cache, dev)
    }

    pub(crate) fn rebuild_bitmap(&mut self, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<(), FsError> {
        self.block_bitmap = BlockBitmap::new(self.superblock.num_blocks);
        for i in 0..self.superblock.num_inodes as usize {
            let inode = *self.inode_cache.load_inode_skip_crc(i, cache, dev, self.abs_lba(0))?;
            if inode.inode_num != 0 || i == 0 {
                let num_blocks = Self::inode_block_count(&inode);
                let max_blocks = num_blocks.min(MAX_DIRECT_BLOCKS);
                for j in 0..max_blocks {
                    let b = inode.direct_blocks[j];
                    if b != 0 && self.is_valid_data_block(b) {
                        self.block_bitmap.mark_used(b);
                    }
                    if b == 0 && (inode.mode & MODE_DIR) != 0 && inode.size > 0 && j == 0 {
                        self.block_bitmap.mark_used(0);
                    }
                }
                // Handle indirect blocks
                if inode.indirect_block != 0 && num_blocks > MAX_DIRECT_BLOCKS {
                    self.block_bitmap.mark_used(inode.indirect_block);
                    if let Ok(pointers) = read_indirect_pointers(cache, dev, self.abs_lba(0), self.data_start_sector(), inode.indirect_block) {
                        for &ptr in pointers.iter() {
                            if ptr != 0 && self.is_valid_data_block(ptr) {
                                self.block_bitmap.mark_used(ptr);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn is_valid_data_block(&self, block_ptr: u32) -> bool {
        block_ptr < self.superblock.num_blocks
    }

    pub(crate) fn inode_data_block_count(&self, inode: &Inode) -> usize {
        let span = if (inode.mode & MODE_DIR) != 0 {
            Self::directory_byte_span(inode)
        } else if inode.size == 0 {
            0
        } else {
            inode.size as usize
        };
        if span == 0 {
            return 0;
        }
        span.div_ceil(BLOCK_SIZE).min(MAX_FILE_BLOCKS)
    }

    pub(crate) fn get_inode_block_ptr(&self, inode: &Inode, block_idx: usize, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Option<u32> {
        let num_blocks = self.inode_data_block_count(inode);
        if block_idx >= num_blocks {
            return None;
        }

        // Direct blocks
        if block_idx < MAX_DIRECT_BLOCKS {
            let block_ptr = inode.direct_blocks[block_idx];
            let is_dir = (inode.mode & MODE_DIR) != 0;
            let inode_size = inode.size;
            let has_special_first_block = is_dir && block_idx == 0 && inode_size > 0;
            if block_ptr == 0 && !has_special_first_block {
                return None;
            }
            if block_ptr != 0 && !self.is_valid_data_block(block_ptr) {
                return None;
            }
            return Some(if block_ptr == 0 { 0 } else { block_ptr });
        }

        // Indirect block
        let indirect_idx = block_idx - MAX_DIRECT_BLOCKS;
        if indirect_idx >= INDIRECT_ENTRIES {
            return None;
        }
        if inode.indirect_block == 0 {
            return None;
        }
        if !self.is_valid_data_block(inode.indirect_block) {
            return None;
        }

        let pointers = match read_indirect_pointers(cache, dev, self.abs_lba(0), self.data_start_sector(), inode.indirect_block) {
            Ok(p) => p,
            Err(_) => return None,
        };

        let block_ptr = pointers[indirect_idx];
        if block_ptr == 0 {
            return None;
        }
        if !self.is_valid_data_block(block_ptr) {
            return None;
        }
        Some(block_ptr)
    }

    pub fn new(superblock_data: &[u8; 512], io: IoStack) -> Result<Self, FsError> {
        let superblock: Superblock = unsafe {
            core::ptr::read_unaligned(superblock_data.as_ptr() as *const _)
        };

        if superblock.magic != SUPERBLOCK_MAGIC {
            return Err(FsError::InvalidSuperblock);
        }

        if superblock.block_size != BLOCK_SIZE as u32 {
            return Err(FsError::InvalidBlockSize);
        }

        if !superblock.verify_checksum() {
            serial_println!("[!] FS: Superblock checksum FAILED");
            return Err(FsError::ChecksumMismatch);
        }

        serial_println!("[✓] FS: Superblock loaded. Blocks: {}, Inodes: {}", superblock.num_blocks, superblock.num_inodes);

        let mut inode_cache = InodeCache::new();
        inode_cache.num_inodes = superblock.num_inodes;

        Ok(NeoDosFs {
            superblock,
            inode_cache,
            block_bitmap: BlockBitmap::new(superblock.num_blocks),
            io_stack: io,
            drive_id: 0,
        })
    }

    pub fn list_directory(&mut self, inode_num: u32, cache: &mut BlockCache, dev: &mut dyn BlockDevice)
        -> Result<(), FsError>
    {
        let inode = *self.inode_cache.load_inode(inode_num as usize, cache, dev, self.abs_lba(0))?;
        let mode = inode.mode;

        if (mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }

        let mut bytes_to_read = Self::directory_byte_span(&inode);
        for block_idx in 0..self.inode_data_block_count(&inode) {
            if bytes_to_read == 0 {
                break;
            }

            let Some(current_block) = self.get_inode_block_ptr(&inode, block_idx, cache, dev) else {
                continue;
            };
            let block_sector = self.data_start_sector() + (current_block * 8);
            let to_read_in_block = if bytes_to_read > BLOCK_SIZE { BLOCK_SIZE } else { bytes_to_read };

            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(self.abs_lba(block_sector + sector_offset), dev)?;

                for entry_offset in (0..512).step_by(256) {
                    let first_byte = sector_data[entry_offset];
                    if first_byte == 0xE5 {
                        continue;
                    }

                    let entry: DirectoryEntry = unsafe {
                        core::ptr::read_unaligned(
                            sector_data.as_ptr().add(entry_offset) as *const _
                        )
                    };

                    if entry.inode_num != 0 {
                        let name_len = entry.name_len;
                        if name_len == 0 || name_len as usize > entry.name.len() {
                            continue;
                        }
                        let name_slice = &entry.name[..name_len as usize];
                        if let Ok(name) = core::str::from_utf8(name_slice) {
                            crate::console::print_str("  ");
                            crate::console::print_str(name);
                            crate::console::print_str("\r\n");
                            crate::serial_println!("  {}", name);
                        }
                    }
                }
            }
            bytes_to_read -= to_read_in_block;
        }

        Ok(())
    }

    pub(crate) fn find_entry_in_directory(&mut self, dir_inode_num: u32, name: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<(u32, u8), FsError> 
    {
        let dir_inode = *self.inode_cache.load_inode(dir_inode_num as usize, cache, dev, self.abs_lba(0))?;
        if (dir_inode.mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }

        let num_blocks = self.inode_data_block_count(&dir_inode);
        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx, cache, dev) {
                Some(b) => b,
                None => continue,
            };

            let block_sector = self.data_start_sector() + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(self.abs_lba(block_sector + sector_offset), dev)?;
                for entry_off in (0..512).step_by(256) {
                    let first_byte = sector_data[entry_off];
                    if first_byte == 0xE5 || first_byte == 0 {
                        continue;
                    }

                    let entry: DirectoryEntry = unsafe {
                        core::ptr::read_unaligned(sector_data.as_ptr().add(entry_off) as *const _)
                    };

                    let entry_name = core::str::from_utf8(&entry.name[..entry.name_len as usize]).unwrap_or("");
                    if entry_name.eq_ignore_ascii_case(name) {
                        return Ok((entry.inode_num, entry.entry_type));
                    }
                }
            }
        }
        Err(FsError::FileNotFound)
    }

    pub fn find_file_in_directory(&mut self, dir_inode_num: u32, name: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice)
        -> Result<u32, FsError>
    {
        let (inode, entry_type) = self.find_entry_in_directory(dir_inode_num, name, cache, dev)?;
        if entry_type == 1 {
            Ok(inode)
        } else {
            Err(FsError::NotAFile)
        }
    }

    pub fn find_file(&mut self, filename: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice)
        -> Result<u32, FsError>
    {
        self.find_file_in_directory(ROOT_INODE, filename, cache, dev)
    }

    pub fn find_dir_in_dir(&mut self, parent_inode: u32, dirname: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice)
        -> Result<u32, FsError>
    {
        let dir_inode = *self.inode_cache.load_inode(parent_inode as usize, cache, dev, self.abs_lba(0))?;

        if (dir_inode.mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }

        let num_blocks = self.inode_data_block_count(&dir_inode);

        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx, cache, dev) {
                Some(b) => b,
                None => continue,
            };

            let block_sector = self.data_start_sector() + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_lba = self.abs_lba(block_sector + sector_offset);
                let sector_data = cache.get_sector(sector_lba, dev)?;

                for entry_off in (0..512).step_by(DIR_ENTRY_SIZE) {
                    let first_byte = sector_data[entry_off];
                    if first_byte == 0xE5 || first_byte == 0x00 {
                        continue;
                    }

                    let entry_type = sector_data[entry_off + 5];
                    if entry_type != 2 {
                        continue;
                    }

                    let name_len = sector_data[entry_off + 4] as usize;
                    if name_len == 0 || name_len > 250 {
                        continue;
                    }

                    let mut entry_name = [0u8; 256];
                    let copy_len = name_len.min(DIR_ENTRY_SIZE - 7);
                    entry_name[..copy_len].copy_from_slice(&sector_data[entry_off + 7..entry_off + 7 + copy_len]);

                    if core::str::from_utf8(&entry_name[..copy_len])
                        .map(|s| s.eq_ignore_ascii_case(dirname))
                        .unwrap_or(false)
                    {
                        let inode_num = u32::from_le_bytes([
                            sector_data[entry_off],
                            sector_data[entry_off + 1],
                            sector_data[entry_off + 2],
                            sector_data[entry_off + 3]
                        ]);
                        return Ok(inode_num);
                    }
                }
            }
        }

        Err(FsError::FileNotFound)
    }

    pub fn read_file_to_buf(&mut self, inode_num: u32, buf: &mut [u8], cache: &mut BlockCache, page_cache: &mut PageCache, dev: &mut dyn BlockDevice)
        -> Result<usize, FsError>
    {
        let inode = *self.inode_cache.load_inode(inode_num as usize, cache, dev, self.abs_lba(0))?;
        let mode = inode.mode;
        let size = inode.size;
        if (mode & MODE_FILE) == 0 {
            return Err(FsError::NotAFile);
        }

        let mut bytes_left = size as usize;
        let mut total_read = 0;

        for block_idx in 0..self.inode_data_block_count(&inode) {
            if bytes_left == 0 || total_read >= buf.len() { break; }

            let Some(current_block) = self.get_inode_block_ptr(&inode, block_idx, cache, dev) else {
                continue;
            };
            let block_lba = self.abs_lba(self.data_start_sector() + (current_block * 8)) as u64;
            let page = page_cache.read_page(self.drive_id, inode_num, block_idx as u32, block_lba, dev)?;
            let to_copy = bytes_left.min(4096).min(buf.len() - total_read);
            buf[total_read..total_read + to_copy].copy_from_slice(&page[..to_copy]);
            total_read += to_copy;
            bytes_left -= to_copy;
        }

        Ok(total_read)
    }

    pub fn read_file(&mut self, inode_num: u32, cache: &mut BlockCache, page_cache: &mut PageCache, dev: &mut dyn BlockDevice)
        -> Result<(), FsError>
    {
        let inode = *self.inode_cache.load_inode(inode_num as usize, cache, dev, self.abs_lba(0))?;

        let mode = inode.mode;
        let size = inode.size;
        if (mode & MODE_FILE) == 0 {
            return Err(FsError::NotAFile);
        }

        let mut bytes_left = size as usize;

        for block_idx in 0..self.inode_data_block_count(&inode) {
            if bytes_left == 0 { break; }

            let Some(current_block) = self.get_inode_block_ptr(&inode, block_idx, cache, dev) else {
                continue;
            };
            let block_lba = self.abs_lba(self.data_start_sector() + (current_block * 8)) as u64;
            let page = page_cache.read_page(self.drive_id, inode_num, block_idx as u32, block_lba, dev)?;
            let to_copy = bytes_left.min(4096);

            if let Ok(text) = core::str::from_utf8(&page[..to_copy]) {
                crate::console::print_str(text);
                crate::serial_print!("{}", text);
            }

            bytes_left -= to_copy;
        }

        Ok(())
    }

    pub fn sync(&mut self, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<(), FsError> {
        cache.flush(dev).map_err(|_| FsError::BlockDeviceError)
    }

    pub fn get_volume_label(&self) -> &str {
        let label_len = self.superblock.label_len as usize;
        if label_len == 0 || label_len > 11 {
            return "";
        }
        core::str::from_utf8(&self.superblock.label[..label_len]).unwrap_or("")
    }

    pub fn set_volume_label(&mut self, label: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<(), FsError> {
        let label_bytes = label.as_bytes();
        let len = label_bytes.len().min(11);

        self.superblock.label_len = len as u8;
        self.superblock.label[..len].copy_from_slice(&label_bytes[..len]);
        for i in len..11 {
            self.superblock.label[i] = b' ';
        }

        self.superblock.update_checksum();

        let sb_data = cache.get_sector_mut(self.abs_lba(0), dev)?;
        unsafe {
            core::ptr::write_unaligned(sb_data.as_mut_ptr() as *mut Superblock, self.superblock);
        }
        cache.mark_dirty(self.abs_lba(0));

        Ok(())
    }

    pub fn write_inode(&mut self, inode_num: usize, inode: &Inode, cache: &mut BlockCache, dev: &mut dyn BlockDevice)
        -> Result<(), FsError>
    {
        let mut inode_copy = *inode;
        inode_copy.set_checksum();

        let inode_sector = self.abs_lba(1 + (inode_num as u32 / 2));
        let offset_in_sector = (inode_num % 2) * 256;

        let sector_data = cache.get_sector_mut(inode_sector, dev)?;
        unsafe {
            core::ptr::write_unaligned(
                sector_data.as_mut_ptr().add(offset_in_sector) as *mut Inode,
                inode_copy
            );
        }
        cache.mark_dirty(inode_sector);

        self.inode_cache.ensure_inode_capacity(inode_num);
        self.inode_cache.inodes[inode_num] = Some(inode_copy);

        Ok(())
    }

    pub fn find_free_inode(&mut self, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<u32, FsError> {
        for i in 1..self.superblock.num_inodes as usize {
            let inode = self.inode_cache.load_inode_skip_crc(i, cache, dev, self.abs_lba(0))?;
            if inode.inode_num == 0 && inode.mode == 0 {
                return Ok(i as u32);
            }
        }
        Err(FsError::NoInodeAvailable)
    }

    pub fn allocate_block(&mut self, _cache: &mut BlockCache, _dev: &mut dyn BlockDevice) -> Result<u32, FsError> {
        match self.block_bitmap.alloc() {
            Some(block) => {
                if block >= self.superblock.num_blocks {
                    serial_println!("[!] FS: No block available. bitmap gave: {}, total: {}", block, self.superblock.num_blocks);
                    return Err(FsError::NoBlockAvailable);
                }
                Ok(block)
            }
            None => {
                serial_println!("[!] FS: Block bitmap exhausted ({} blocks)", self.superblock.num_blocks);
                Err(FsError::NoBlockAvailable)
            }
        }
    }

    pub fn allocate_indirect_block(&mut self, inode: &mut Inode, indirect_idx: usize, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<u32, FsError> {
        if indirect_idx >= INDIRECT_ENTRIES {
            return Err(FsError::IndirectBlockFull);
        }

        // Allocate indirect block if needed
        if inode.indirect_block == 0 {
            let ib = self.allocate_block(cache, dev)?;
            inode.indirect_block = ib;
            // Zero out the indirect block
            let block_base = self.data_start_sector() + (ib * 8);
            for s in 0..8 {
                let sector_data = cache.get_sector_mut(self.abs_lba(block_base + s), dev)?;
                sector_data.fill(0);
            }
        }

        // Read existing pointers to find a free slot
        let pointers = read_indirect_pointers(cache, dev, self.abs_lba(0), self.data_start_sector(), inode.indirect_block)?;

        if pointers[indirect_idx] != 0 {
            return Ok(pointers[indirect_idx]);
        }

        let new_block = self.allocate_block(cache, dev)?;
        write_indirect_pointer(cache, dev, self.abs_lba(0), self.data_start_sector(), inode.indirect_block, indirect_idx, new_block)?;

        Ok(new_block)
    }

    pub fn free_block(&mut self, block: u32) {
        self.block_bitmap.free(block);
    }

    fn ext_eq(name: &str, ext: &str) -> bool {
        let name = name.as_bytes();
        let ext = ext.as_bytes();
        name.len() >= ext.len()
            && name[name.len() - ext.len()..]
                .iter()
                .zip(ext.iter())
                .all(|(a, b)| a.to_ascii_uppercase() == *b)
    }

    fn default_perms_for_filename(name: &str) -> u16 {
        if Self::ext_eq(name, ".NXE")
            || Self::ext_eq(name, ".NXL")
            || Self::ext_eq(name, ".BAT")
            || Self::ext_eq(name, ".CMD")
        {
            PERM_R | PERM_X
        } else if Self::ext_eq(name, ".NEM") || Self::ext_eq(name, ".SYS") {
            PERM_R
        } else {
            PERM_R | PERM_W
        }
    }

    pub fn create_file_at(&mut self, parent_inode_num: u32, filename: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice)
        -> Result<u32, FsError>
    {
        if is_reserved_dos_name(filename) {
            serial_println!("[!] FS: Cannot create file with reserved DOS name: {}", filename);
            return Err(FsError::ReservedName);
        }

        let new_inode_num = self.find_free_inode(cache, dev)?;

        let new_inode = Inode {
            inode_num: new_inode_num,
            mode: MODE_FILE | Self::default_perms_for_filename(filename),
            size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1,
            owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12],
            indirect_block: 0,
            checksum: 0,
            padding: [0; 156],
        };

        self.write_inode(new_inode_num as usize, &new_inode, cache, dev)?;
        self.add_directory_entry(parent_inode_num, filename, new_inode_num, 1, cache, dev)?;

        Ok(new_inode_num)
    }

    pub fn create_directory_at(&mut self, parent_inode_num: u32, dirname: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice)
        -> Result<u32, FsError>
    {
        if is_reserved_dos_name(dirname) {
            serial_println!("[!] FS: Cannot create directory with reserved DOS name: {}", dirname);
            return Err(FsError::ReservedName);
        }

        let new_inode_num = self.find_free_inode(cache, dev)?;
        let new_inode = Inode {
            inode_num: new_inode_num,
            mode: MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D,
            size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1,
            owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12],
            indirect_block: 0,
            checksum: 0,
            padding: [0; 156],
        };

        self.write_inode(new_inode_num as usize, &new_inode, cache, dev)?;
        self.add_directory_entry(parent_inode_num, dirname, new_inode_num, 2, cache, dev)?;
        Ok(new_inode_num)
    }

    pub fn add_directory_entry(&mut self, dir_inode_num: u32, filename: &str, inode_num: u32, entry_type: u8,
                               cache: &mut BlockCache, dev: &mut dyn BlockDevice)
        -> Result<(), FsError>
    {
        let mut dir_inode = *self.inode_cache.load_inode(dir_inode_num as usize, cache, dev, self.abs_lba(0))?;

        for block_idx in 0..MAX_FILE_BLOCKS {
            // Determine if this block is allocated
            let block_ptr_opt = if block_idx < MAX_DIRECT_BLOCKS {
                if block_idx * BLOCK_SIZE >= dir_inode.size as usize {
                    let new_block = self.allocate_block(cache, dev)?;
                    dir_inode.direct_blocks[block_idx] = new_block;
                    dir_inode.size += BLOCK_SIZE as u32;
                    self.write_inode(dir_inode_num as usize, &dir_inode, cache, dev)?;
                    for s in 0..8 {
                        let sector_data = cache.get_sector_mut(self.abs_lba(self.data_start_sector() + new_block * 8 + s), dev)?;
                        sector_data.fill(0);
                    }
                    Some(new_block)
                } else {
                    let b = dir_inode.direct_blocks[block_idx];
                    if b != 0 { Some(b) } else { None }
                }
            } else {
                // Indirect block
                let indirect_idx = block_idx - MAX_DIRECT_BLOCKS;
                if block_idx * BLOCK_SIZE >= dir_inode.size as usize {
                    let new_block = self.allocate_indirect_block(&mut dir_inode, indirect_idx, cache, dev)?;
                    dir_inode.size = (dir_inode.size as usize + BLOCK_SIZE).min(u32::MAX as usize) as u32;
                    self.write_inode(dir_inode_num as usize, &dir_inode, cache, dev)?;
                    for s in 0..8 {
                        let sector_data = cache.get_sector_mut(self.abs_lba(self.data_start_sector() + new_block * 8 + s), dev)?;
                        sector_data.fill(0);
                    }
                    Some(new_block)
                } else {
                    match self.get_inode_block_ptr(&dir_inode, block_idx, cache, dev) {
                        Some(b) => Some(b),
                        None => continue,
                    }
                }
            };

            let Some(block_ptr) = block_ptr_opt else { continue; };

            let block_sector = self.data_start_sector() + (block_ptr * 8);
            for sector_offset in 0..8 {
                let sector_lba = self.abs_lba(block_sector + sector_offset);
                let sector_data = cache.get_sector_mut(sector_lba, dev)?;

                for entry_offset in (0..512).step_by(256) {
                    let entry_ptr = unsafe { sector_data.as_mut_ptr().add(entry_offset) as *mut DirectoryEntry };
                    let entry = unsafe { &*entry_ptr };

                    if entry.inode_num == 0 {
                        let mut name_buf = [0u8; 249];
                        let name_bytes = filename.as_bytes();
                        let len = name_bytes.len().min(249);
                        name_buf[..len].copy_from_slice(&name_bytes[..len]);

                        let attrs = if entry_type == 2 { ATTR_DIR } else { ATTR_ARCHIVE };

                        let new_entry = DirectoryEntry {
                            inode_num,
                            name_len: len as u8,
                            entry_type,
                            attributes: attrs,
                            name: name_buf,
                        };

                        unsafe { core::ptr::write_unaligned(entry_ptr, new_entry); }
                        cache.mark_dirty(sector_lba);

                        let entry_end = block_idx * BLOCK_SIZE
                            + (sector_offset as usize) * 512
                            + entry_offset
                            + DIR_ENTRY_SIZE;
                        if entry_end > dir_inode.size as usize {
                            dir_inode.size = entry_end as u32;
                            self.write_inode(dir_inode_num as usize, &dir_inode, cache, dev)?;
                        }
                        return Ok(());
                    }
                }
            }
        }

        Err(FsError::NoBlockAvailable)
    }

    pub fn write_file(&mut self, inode_num: u32, data: &[u8], cache: &mut BlockCache, page_cache: &mut PageCache, dev: &mut dyn BlockDevice)
        -> Result<usize, FsError>
    {
        let mut inode = *self.inode_cache.load_inode(inode_num as usize, cache, dev, self.abs_lba(0))?;

        let mut written = 0;
        let mut block_idx = 0;

        while written < data.len() && block_idx < MAX_FILE_BLOCKS {
            if block_idx * BLOCK_SIZE >= inode.size as usize {
                if block_idx < MAX_DIRECT_BLOCKS {
                    inode.direct_blocks[block_idx] = self.allocate_block(cache, dev)?;
                } else {
                    let indirect_idx = block_idx - MAX_DIRECT_BLOCKS;
                    self.allocate_indirect_block(&mut inode, indirect_idx, cache, dev)?;
                }
            }

            let block_ptr = if block_idx < MAX_DIRECT_BLOCKS {
                inode.direct_blocks[block_idx]
            } else {
                let indirect_idx = block_idx - MAX_DIRECT_BLOCKS;
                let pointers = read_indirect_pointers(cache, dev, self.abs_lba(0), self.data_start_sector(), inode.indirect_block)?;
                pointers[indirect_idx]
            };

            let block_lba = self.abs_lba(self.data_start_sector() + (block_ptr * 8)) as u64;

            let page = page_cache.get_page_mut(self.drive_id, inode_num, block_idx as u32, block_lba, dev)?;
            let to_copy = (data.len() - written).min(4096);
            page[..to_copy].copy_from_slice(&data[written..written + to_copy]);
            written += to_copy;

            block_idx += 1;
        }

        if written > inode.size as usize {
            inode.size = written as u32;
        }
        self.write_inode(inode_num as usize, &inode, cache, dev)?;

        Ok(written)
    }

    pub fn delete_file_by_inode(&mut self, parent_inode: u32, _filename: &str, file_inode_num: u32, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<(), FsError> {
        let mut direct_blocks_to_free = [0u32; MAX_DIRECT_BLOCKS];
        let mut indirect_block_to_free = 0u32;
        let mut indirect_ptrs_to_free = alloc::vec::Vec::new();
        {
            let file_inode = *self.inode_cache.load_inode(file_inode_num as usize, cache, dev, self.abs_lba(0))?;
            let num_blocks = Self::inode_block_count(&file_inode);
            for (j, b_ptr) in direct_blocks_to_free.iter_mut().enumerate().take(num_blocks.min(MAX_DIRECT_BLOCKS)) {
                let b = file_inode.direct_blocks[j];
                if b != 0 && b < self.superblock.num_blocks {
                    *b_ptr = b;
                }
            }
            // Collect indirect blocks
            if file_inode.indirect_block != 0 && num_blocks > MAX_DIRECT_BLOCKS {
                indirect_block_to_free = file_inode.indirect_block;
                if let Ok(pointers) = read_indirect_pointers(cache, dev, self.abs_lba(0), self.data_start_sector(), file_inode.indirect_block) {
                    for (idx, &ptr) in pointers.iter().enumerate() {
                        let block_idx = MAX_DIRECT_BLOCKS + idx;
                        if block_idx < num_blocks && ptr != 0 && ptr < self.superblock.num_blocks {
                            indirect_ptrs_to_free.push(ptr);
                        }
                    }
                }
            }
        }

        // Free direct blocks
        for &b in direct_blocks_to_free.iter() {
            if b != 0 {
                self.free_block(b);
            }
        }
        // Free indirect data blocks
        for &b in indirect_ptrs_to_free.iter() {
            if b != 0 {
                self.free_block(b);
            }
        }
        // Free indirect block itself
        if indirect_block_to_free != 0 {
            self.free_block(indirect_block_to_free);
        }

        self.inode_cache.ensure_inode_capacity(file_inode_num as usize);
        self.inode_cache.inodes[file_inode_num as usize] = None;

        let dir_inode = *self.inode_cache.load_inode(parent_inode as usize, cache, dev, self.abs_lba(0))?;

        let num_blocks = self.inode_data_block_count(&dir_inode);

        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx, cache, dev) {
                Some(b) => b,
                None => continue,
            };

            let block_sector = self.data_start_sector() + (actual_block * 8);

            for sector_offset in 0..8 {
                let sector_lba = self.abs_lba(block_sector + sector_offset);
                let sector_data = cache.get_sector_mut(sector_lba, dev)?;

                for entry_offset in (0..512).step_by(DIR_ENTRY_SIZE) {
                    let entry_inode = u32::from_le_bytes([
                        sector_data[entry_offset],
                        sector_data[entry_offset + 1],
                        sector_data[entry_offset + 2],
                        sector_data[entry_offset + 3]
                    ]);

                    let first_byte = sector_data[entry_offset];

                    if entry_inode == 0 {
                        continue;
                    }

                    if first_byte == 0xE5 {
                        if entry_inode == file_inode_num {
                            return Ok(());
                        }
                        continue;
                    }

                    if entry_inode == file_inode_num {
                        sector_data[entry_offset] = 0xE5;
                        cache.mark_dirty(sector_lba);
                        return Ok(());
                    }
                }
            }
        }

        Err(FsError::FileNotFound)
    }

    pub fn rename_file(&mut self, parent_inode: u32, old_name: &str, new_name: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<(), FsError> {
        if is_reserved_dos_name(new_name) {
            return Err(FsError::ReservedName);
        }

        let _file_inode = self.find_file_in_directory(parent_inode, old_name, cache, dev)?;

        let dir_inode = *self.inode_cache.load_inode(parent_inode as usize, cache, dev, self.abs_lba(0))?;

        let num_blocks = self.inode_data_block_count(&dir_inode);

        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx, cache, dev) {
                Some(b) => b,
                None => continue,
            };

            let block_sector = self.data_start_sector() + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_lba = self.abs_lba(block_sector + sector_offset);
                let sector_data = cache.get_sector_mut(sector_lba, dev)?;

                for entry_off in (0..512).step_by(DIR_ENTRY_SIZE) {
                    let first_byte = sector_data[entry_off];
                    if first_byte == 0xE5 || first_byte == 0x00 {
                        continue;
                    }

                    let name_len = sector_data[entry_off + 4] as usize;

                    let mut entry_name = [0u8; 256];
                    let copy_len = name_len.min(DIR_ENTRY_SIZE - 7);
                    entry_name[..copy_len].copy_from_slice(&sector_data[entry_off + 7..entry_off + 7 + copy_len]);

                    if core::str::from_utf8(&entry_name[..copy_len]).map(|s| s.eq_ignore_ascii_case(old_name)).unwrap_or(false) {
                        let new_len = new_name.len().min(DIR_ENTRY_SIZE - 7);
                        sector_data[entry_off + 4] = new_len as u8;
                        sector_data[entry_off + 7..entry_off + 7 + new_len].copy_from_slice(&new_name.as_bytes()[..new_len]);
                        sector_data[entry_off + 7 + new_len..entry_off + 256].fill(0x20);
                        cache.mark_dirty(sector_lba);
                        return Ok(());
                    }
                }
            }
        }

        Err(FsError::FileNotFound)
    }

    pub fn is_directory_empty(&mut self, dir_inode_num: u32, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<bool, FsError> {
        let dir_inode = *self.inode_cache.load_inode(dir_inode_num as usize, cache, dev, self.abs_lba(0))?;

        let num_blocks = self.inode_data_block_count(&dir_inode);

        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx, cache, dev) {
                Some(b) => b,
                None => continue,
            };

            let block_sector = self.data_start_sector() + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_lba = self.abs_lba(block_sector + sector_offset);
                let sector_data = cache.get_sector(sector_lba, dev)?;

                for entry_off in (0..512).step_by(DIR_ENTRY_SIZE) {
                    let first_byte = sector_data[entry_off];
                    if first_byte == 0x00 {
                        return Ok(true);
                    }
                    if first_byte == 0xE5 {
                        continue;
                    }

                    let name_len = sector_data[entry_off + 4] as usize;
                    if name_len == 0 || name_len > 250 {
                        continue;
                    }

                    if name_len == 1 && sector_data[entry_off + 6] == b'.' {
                        continue;
                    }
                    if name_len == 2 && sector_data[entry_off + 6] == b'.' && sector_data[entry_off + 7] == b'.' {
                        continue;
                    }

                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    pub fn delete_directory(&mut self, parent_inode: u32, dirname: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<(), FsError> {
        let dir_inode_num = self.find_dir_in_dir(parent_inode, dirname, cache, dev)?;

        if !self.is_directory_empty(dir_inode_num, cache, dev)? {
            return Err(FsError::DirectoryNotEmpty);
        }

        self.delete_file_by_inode(parent_inode, dirname, dir_inode_num, cache, dev)
    }

    /// Verify checksums on all inodes in the cache/table.
    pub fn verify_all_inode_checksums(&mut self, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<u32, FsError> {
        let mut failures = 0u32;
        for i in 0..self.superblock.num_inodes as usize {
            let inode = *self.inode_cache.load_inode_skip_crc(i, cache, dev, self.abs_lba(0))?;
            if inode.inode_num != 0 || i == 0 {
                if !inode.verify_checksum() {
                    failures += 1;
                }
            }
        }
        Ok(failures)
    }
}

use crate::fs::vfs::{FileSystem, VfsError, VfsNode, DirEntry as VfsDirEntry};

impl From<FsError> for VfsError {
    fn from(err: FsError) -> Self {
        match err {
            FsError::FileNotFound => VfsError::NotFound,
            FsError::NotADirectory => VfsError::NotADirectory,
            FsError::NotAFile => VfsError::NotAFile,
            FsError::NoInodeAvailable => VfsError::IOError,
            FsError::NoBlockAvailable => VfsError::IOError,
            FsError::DirectoryNotEmpty => VfsError::DirectoryNotEmpty,
            FsError::ChecksumMismatch => VfsError::IOError,
            FsError::ReservedName => VfsError::IOError,
            FsError::IndirectBlockFull => VfsError::IOError,
            _ => VfsError::IOError,
        }
    }
}

impl FileSystem for NeoDosFs {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut pc_lock = crate::globals::PAGE_CACHE.lock();
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        let mut temp_buf = alloc::vec![0u8; buf.len() + offset as usize];

        let read = self.read_file_to_buf(inode, &mut temp_buf, cache, &mut pc_lock, dev)?;

        if offset as usize >= read {
            return Ok(0);
        }

        let available = read - offset as usize;
        let to_copy = available.min(buf.len());
        buf[..to_copy].copy_from_slice(&temp_buf[offset as usize..offset as usize + to_copy]);

        Ok(to_copy)
    }

    fn write(&mut self, inode: u32, _offset: u64, buf: &[u8]) -> Result<usize, VfsError> {
        let mut pc_lock = crate::globals::PAGE_CACHE.lock();
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        Ok(self.write_file(inode, buf, cache, &mut pc_lock, dev)?)
    }

    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        let (inode, _entry_type) = self.find_entry_in_directory(dir_inode, name, cache, dev)?;
        let inode_data = self.inode_cache.load_inode(inode as usize, cache, dev, self.abs_lba(0))?;

        Ok(VfsNode {
            inode,
            mode: inode_data.mode,
            size: inode_data.size,
        })
    }

    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<VfsDirEntry>, VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        let inode = *self.inode_cache.load_inode(dir_inode as usize, cache, dev, self.abs_lba(0))?;
        if (inode.mode & MODE_DIR) == 0 {
            return Err(VfsError::NotADirectory);
        }

        let mut current_idx = 0;
        let mut bytes_to_read = Self::directory_byte_span(&inode);
        for block_idx in 0..self.inode_data_block_count(&inode) {
            if bytes_to_read == 0 { break; }
            let Some(current_block) = self.get_inode_block_ptr(&inode, block_idx, cache, dev) else { continue; };
            let block_sector = self.data_start_sector() + (current_block * 8);

            for sector_offset in 0..8 {
                for entry_offset in (0..512).step_by(256) {
                    let entry = {
                        let sector_data = cache.get_sector(self.abs_lba(block_sector + sector_offset), dev)?;
                        let first_byte = sector_data[entry_offset];
                        if first_byte == 0xE5 || first_byte == 0 {
                            continue;
                        }

                        let entry: DirectoryEntry = unsafe {
                            core::ptr::read_unaligned(sector_data.as_ptr().add(entry_offset) as *const _)
                        };

                        if current_idx != index {
                            current_idx += 1;
                            continue;
                        }

                        entry
                    };

                    let name = core::str::from_utf8(&entry.name[..entry.name_len as usize]).unwrap_or("?").into();
                    let inode_data = match self.inode_cache.load_inode(entry.inode_num as usize, cache, dev, self.abs_lba(0)) {
                        Ok(inode_data) => inode_data,
                        Err(_) => {
                            continue;
                        }
                    };

                    return Ok(Some(VfsDirEntry {
                        name,
                        node: VfsNode {
                            inode: entry.inode_num,
                            mode: inode_data.mode,
                            size: inode_data.size,
                        }
                    }));
                }
            }
            bytes_to_read -= BLOCK_SIZE.min(bytes_to_read);
        }

        Ok(None)
    }

    fn mkdir(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        let inode = self.create_directory_at(dir_inode, name, cache, dev)?;
        let inode_data = self.inode_cache.load_inode(inode as usize, cache, dev, self.abs_lba(0))?;
        Ok(VfsNode {
            inode,
            mode: inode_data.mode,
            size: inode_data.size,
        })
    }

    fn remove_file(&mut self, dir_inode: u32, name: &str) -> Result<(), VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        let (file_inode, _entry_type) = self.find_entry_in_directory(dir_inode, name, cache, dev)?;
        let inode_data = self.inode_cache.load_inode(file_inode as usize, cache, dev, self.abs_lba(0))?;
        if (inode_data.mode & MODE_DIR) != 0 {
            return Err(VfsError::NotAFile);
        }
        self.delete_file_by_inode(dir_inode, name, file_inode, cache, dev)?;
        Ok(())
    }

    fn remove_dir(&mut self, dir_inode: u32, name: &str) -> Result<(), VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        self.delete_directory(dir_inode, name, cache, dev)?;
        Ok(())
    }

    fn rename(&mut self, dir_inode: u32, old_name: &str, new_name: &str) -> Result<(), VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        self.rename_file(dir_inode, old_name, new_name, cache, dev)?;
        Ok(())
    }

    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        let inode = self.create_file_at(dir_inode, name, cache, dev)?;
        let inode_data = self.inode_cache.load_inode(inode as usize, cache, dev, self.abs_lba(0))?;
        Ok(VfsNode {
            inode,
            mode: inode_data.mode,
            size: inode_data.size,
        })
    }

    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        let inode_data = self.inode_cache.load_inode(inode as usize, cache, dev, self.abs_lba(0))?;
        Ok(VfsNode {
            inode,
            mode: inode_data.mode,
            size: inode_data.size,
        })
    }

    fn volume_label(&self) -> Result<alloc::string::String, VfsError> {
        Ok(alloc::string::String::from(self.get_volume_label()))
    }

    fn set_volume_label(&mut self, label: &str) -> Result<(), VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;

        self.set_volume_label(label, cache, dev)?;
        Ok(())
    }

    fn fs_type(&self) -> &'static str {
        "NeoDOS"
    }

    fn total_sectors(&self) -> u64 {
        self.superblock.num_blocks as u64 * (BLOCK_SIZE as u64 / 512)
    }
}
