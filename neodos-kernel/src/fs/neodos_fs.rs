// src/fs/neodos_fs.rs

#![allow(dead_code)]

use crate::buffer::block_cache::BlockCache;
use crate::buffer::page_cache::PageCache;
use crate::drivers::block::BlockDevice;
use crate::serial_println;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Superblock {
    pub magic: u32,              // 0x4F444F4E = "NEOD" little-endian
    pub block_size: u32,         // Typ. 4096
    pub num_blocks: u32,         // Total blocks
    pub num_inodes: u32,         // Max 256
    pub created: u64,            // Timestamp
    pub label_len: u8,           // Volume label length (0-11)
    pub label: [u8; 11],         // Volume label (11 bytes, DOS standard)
    pub reserved: [u8; 472],    // Padding to 512 bytes
}

pub const SUPERBLOCK_MAGIC: u32 = 0x4F444F4E;  // "NEOD"
pub const BLOCK_SIZE: usize = 4096;
pub const ROOT_INODE: u32 = 0;
pub const BLOCK_BITMAP_BYTES: usize = 320;

pub struct BlockBitmap {
    bits: [u8; BLOCK_BITMAP_BYTES],
}

impl BlockBitmap {
    pub fn new() -> Self {
        BlockBitmap { bits: [0; BLOCK_BITMAP_BYTES] }
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

    pub fn free(&mut self, block: u32) {
        let idx = block as usize / 8;
        let bit = block as usize % 8;
        if idx < BLOCK_BITMAP_BYTES {
            self.bits[idx] &= !(1u8 << bit);
        }
    }

    pub fn mark_used(&mut self, block: u32) {
        let idx = block as usize / 8;
        let bit = block as usize % 8;
        if idx < BLOCK_BITMAP_BYTES {
            self.bits[idx] |= 1u8 << bit;
        }
    }
}

#[repr(packed)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Inode {
    pub inode_num: u32,          // 0-255
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
    pub padding: [u8; 160],      // 96 + 160 = 256 bytes exactly
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
#[allow(dead_code)]
pub const ATTR_VOLUME: u8   = 0x08;
pub const ATTR_DIR: u8      = 0x10;
pub const ATTR_ARCHIVE: u8  = 0x20;

#[repr(packed)]
#[derive(Debug, Clone, Copy)]
pub struct DirectoryEntry {
    pub inode_num: u32,
    pub name_len: u8,
    pub entry_type: u8,          // 1=file, 2=dir
    #[allow(dead_code)]
    pub attributes: u8,          // DOS attributes (R=1, H=2, S=4, A=16)
    pub name: [u8; 249],         // Adjusted to fit 256 bytes total
}

pub const DIR_ENTRY_SIZE: usize = 256;

#[derive(Debug)]
pub enum FsError {
    InvalidSuperblock,
    InvalidBlockSize,
    NotADirectory,
    NotAFile,
    FileNotFound,
    #[allow(dead_code)]
    BlockReadError,
    NoInodeAvailable,
    NoBlockAvailable,
    BlockDeviceError,
    DirectoryNotEmpty,
}

impl From<()> for FsError {
    fn from(_: ()) -> Self {
        FsError::BlockDeviceError
    }
}

pub struct InodeCache {
    pub(crate) inodes: [Option<Inode>; 256],
}

impl InodeCache {
    pub fn new() -> Self {
        InodeCache {
            inodes: [None; 256],
        }
    }
    
    pub fn load_inode(&mut self, inode_num: usize, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<&Inode, FsError> 
    {
        if inode_num >= 256 {
            return Err(FsError::FileNotFound);
        }

        if let Some(ref cached) = self.inodes[inode_num] {
            return Ok(cached);
        }
        
        // Inode table @ sector 1, 256 bytes per inode = 2 inodes per sector
        let inode_sector = 1 + (inode_num as u32 / 2);
        let offset_in_sector = (inode_num % 2) * 256;
        
        let sector_data = cache.get_sector(inode_sector, dev)?;
        let inode: Inode = unsafe {
            core::ptr::read_unaligned(
                sector_data.as_ptr().add(offset_in_sector) as *const _
            )
        };
        
        self.inodes[inode_num] = Some(inode);
        Ok(self.inodes[inode_num].as_ref().ok_or(FsError::FileNotFound)?)
    }
}

pub struct NeoDosFs {
    pub superblock: Superblock,
    pub inode_cache: InodeCache,
    pub block_bitmap: BlockBitmap,
}

impl NeoDosFs {
    /// Span of directory data that actually exists on disk (`inode.size` may under-report).
    ///
    /// Root inode stores its first directory block as pointer **0** (valid block index); treating
    /// `!= 0` as the only allocated block would ignore that and under-size scans.
    pub(crate) fn directory_byte_span(inode: &Inode) -> usize {
        let mut span = inode.size as usize;
        for i in 0..12 {
            let b = inode.direct_blocks[i];
            let has_extent = b != 0
                || ((inode.mode & MODE_DIR) != 0 && i == 0 && inode.size > 0);
            if has_extent {
                span = span.max((i + 1) * BLOCK_SIZE);
            }
        }
        span
    }

    /// Static version of inode_data_block_count — usable without &self.
    pub fn inode_block_count(inode: &Inode) -> usize {
        let span = if (inode.mode & MODE_DIR) != 0 {
            let mut s = inode.size as usize;
            for i in 0..12 {
                let b = inode.direct_blocks[i];
                let has = b != 0 || ((inode.mode & MODE_DIR) != 0 && i == 0 && inode.size > 0);
                if has { s = s.max((i + 1) * BLOCK_SIZE); }
            }
            s
        } else {
            inode.size as usize
        };
        if span == 0 { 0 } else { ((span + BLOCK_SIZE - 1) / BLOCK_SIZE).min(12) }
    }

    pub fn rebuild_bitmap(&mut self, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<(), FsError> {
        self.block_bitmap = BlockBitmap::new();
        // Mark system blocks (before data start) as used so they're never allocated
        // Superblock (sector 0) + inode table (sectors 1-128) + gap up to sector 200
        // Data blocks start at sector 200, so system "blocks" are conceptually -25..0
        // We just mark data blocks that correspond to in-use files
        for i in 0..256 {
            let inode = *self.inode_cache.load_inode(i, cache, dev)?;
            if inode.inode_num != 0 || i == 0 {
                let num_blocks = Self::inode_block_count(&inode);
                for j in 0..num_blocks.min(12) {
                    let b = inode.direct_blocks[j];
                    if b != 0 && self.is_valid_data_block(b) {
                        self.block_bitmap.mark_used(b);
                    }
                    // Special case: root dir with content may have ptr=0 for block 0
                    if b == 0 && (inode.mode & MODE_DIR) != 0 && inode.size > 0 && j == 0 {
                        self.block_bitmap.mark_used(0);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn is_valid_data_block(&self, block_ptr: u32) -> bool {
        block_ptr < self.superblock.num_blocks
    }

    pub fn inode_data_block_count(&self, inode: &Inode) -> usize {
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
        ((span + BLOCK_SIZE - 1) / BLOCK_SIZE).min(12)
    }

    pub fn get_inode_block_ptr(&self, inode: &Inode, block_idx: usize) -> Option<u32> {
        let num_blocks = self.inode_data_block_count(inode);
        if block_idx >= num_blocks {
            return None;
        }

        let block_ptr = inode.direct_blocks[block_idx];
        
        // Special case: for directories with content, block 0 can be valid even if ptr is 0
        let is_dir = (inode.mode & MODE_DIR) != 0;
        let inode_size = inode.size;
        let has_special_first_block = is_dir && block_idx == 0 && inode_size > 0;
        
        if block_ptr == 0 && !has_special_first_block {
            return None;
        }
        
        if block_ptr != 0 && !self.is_valid_data_block(block_ptr) {
            return None;
        }

        Some(block_ptr)
    }

    pub fn new(superblock_data: &[u8; 512]) -> Result<Self, FsError> {
        let superblock: Superblock = unsafe {
            core::ptr::read_unaligned(superblock_data.as_ptr() as *const _)
        };
        
        if superblock.magic != SUPERBLOCK_MAGIC {
            return Err(FsError::InvalidSuperblock);
        }
        
        if superblock.block_size != BLOCK_SIZE as u32 {
            return Err(FsError::InvalidBlockSize);
        }
        
        serial_println!("[✓] FS: Superblock loaded. Blocks: {}, Inodes: {}", superblock.num_blocks, superblock.num_inodes);

        Ok(NeoDosFs {
            superblock,
            inode_cache: InodeCache::new(),
            block_bitmap: BlockBitmap::new(),
        })
    }
    
    #[allow(dead_code)]
    pub fn list_root(&mut self, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<(), FsError> 
    {
        let root_inode = *self.inode_cache.load_inode(ROOT_INODE as usize, cache, dev)?;
        let mode = root_inode.mode;
        
        if (mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }
        
        let mut bytes_to_read = Self::directory_byte_span(&root_inode);
        for block_idx in 0..self.inode_data_block_count(&root_inode) {
            if bytes_to_read == 0 {
                break;
            }

            let Some(current_block) = self.get_inode_block_ptr(&root_inode, block_idx) else {
                continue;
            };
            let block_sector = 200 + (current_block * 8);
            let to_read_in_block = if bytes_to_read > BLOCK_SIZE { BLOCK_SIZE } else { bytes_to_read };
            
            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(block_sector + sector_offset, dev)?;
                
                for entry_offset in (0..512).step_by(256) {
                    // Skip deleted entries (first byte 0xE5)
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
                            // Skip volume labels (they appear as directory entries with ATTR_VOLUME)
                            if entry.attributes & ATTR_VOLUME != 0 {
                                continue;
                            }
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

    pub fn list_directory(&mut self, inode_num: u32, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<(), FsError> 
    {
        let inode = *self.inode_cache.load_inode(inode_num as usize, cache, dev)?;
        let mode = inode.mode;
        
        if (mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }
        
        let mut bytes_to_read = Self::directory_byte_span(&inode);
        for block_idx in 0..self.inode_data_block_count(&inode) {
            if bytes_to_read == 0 {
                break;
            }

            let Some(current_block) = self.get_inode_block_ptr(&inode, block_idx) else {
                continue;
            };
            let block_sector = 200 + (current_block * 8);
            let to_read_in_block = if bytes_to_read > BLOCK_SIZE { BLOCK_SIZE } else { bytes_to_read };
            
            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(block_sector + sector_offset, dev)?;
                
                for entry_offset in (0..512).step_by(256) {
                    // Skip deleted entries (first byte 0xE5)
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

    pub fn find_entry_in_directory(&mut self, dir_inode_num: u32, name: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<(u32, u8), FsError> 
    {
        let dir_inode = *self.inode_cache.load_inode(dir_inode_num as usize, cache, dev)?;
        if (dir_inode.mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }

        let num_blocks = self.inode_data_block_count(&dir_inode);
        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx) {
                Some(b) => b,
                None => continue,
            };
            
            let block_sector = 200 + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(block_sector + sector_offset, dev)?;
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

    // Backward compatibility: find in root only
    pub fn find_file(&mut self, filename: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<u32, FsError> 
    {
        self.find_file_in_directory(ROOT_INODE, filename, cache, dev)
    }

    #[allow(dead_code)]
    pub fn find_directory(&mut self, dirname: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<u32, FsError> 
    {
        let (inode, entry_type) = self.find_entry_in_directory(ROOT_INODE, dirname, cache, dev)?;
        if entry_type == 2 {
            Ok(inode)
        } else {
            Err(FsError::NotADirectory)
        }
    }

    pub fn find_dir_in_dir(&mut self, parent_inode: u32, dirname: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<u32, FsError> 
    {
        let dir_inode = *self.inode_cache.load_inode(parent_inode as usize, cache, dev)?;
        
        if (dir_inode.mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }
        
        let num_blocks = self.inode_data_block_count(&dir_inode);
        
        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx) {
                Some(b) => b,
                None => continue,
            };
            
            let block_sector = 200 + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_lba = block_sector + sector_offset;
                let sector_data = cache.get_sector(sector_lba, dev)?;
                
                for entry_off in (0..512).step_by(DIR_ENTRY_SIZE) {
                    let first_byte = sector_data[entry_off];
                    if first_byte == 0xE5 || first_byte == 0x00 {
                        continue;
                    }
                    
                    let entry_type = sector_data[entry_off + 5];
                    // Only look for directories (entry_type = 2)
                    if entry_type != 2 {
                        continue;
                    }
                    
                    let name_len = sector_data[entry_off + 4] as usize;
                    if name_len == 0 || name_len > 250 {
                        continue;
                    }
                    
                    let mut entry_name = [0u8; 256];
                    let copy_len = name_len.min(255);
                    entry_name[..copy_len].copy_from_slice(&sector_data[entry_off + 6..entry_off + 6 + copy_len]);
                    
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
        let inode = *self.inode_cache.load_inode(inode_num as usize, cache, dev)?;
        let mode = inode.mode;
        let size = inode.size;
        if (mode & MODE_FILE) == 0 {
            return Err(FsError::NotAFile);
        }
        
        let mut bytes_left = size as usize;
        let mut total_read = 0;
        
        for block_idx in 0..self.inode_data_block_count(&inode) {
            if bytes_left == 0 || total_read >= buf.len() { break; }

            let Some(current_block) = self.get_inode_block_ptr(&inode, block_idx) else {
                continue;
            };
            let block_lba = 200 + (current_block * 8);
            let page = page_cache.read_page(inode_num, block_idx as u32, block_lba as u64, dev)?;
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
        let inode = *self.inode_cache.load_inode(inode_num as usize, cache, dev)?;
        
        let mode = inode.mode;
        let size = inode.size;
        if (mode & MODE_FILE) == 0 {
            return Err(FsError::NotAFile);
        }
        
        let mut bytes_left = size as usize;
        
        for block_idx in 0..self.inode_data_block_count(&inode) {
            if bytes_left == 0 { break; }

            let Some(current_block) = self.get_inode_block_ptr(&inode, block_idx) else {
                continue;
            };
            let block_lba = 200 + (current_block * 8);
            let page = page_cache.read_page(inode_num, block_idx as u32, block_lba as u64, dev)?;
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
        // Fill rest with spaces
        for i in len..11 {
            self.superblock.label[i] = b' ';
        }
        
        // Write superblock back
        let sb_data = cache.get_sector_mut(0, dev)?;
        unsafe {
            core::ptr::write_unaligned(sb_data.as_mut_ptr() as *mut Superblock, self.superblock);
        }
        cache.mark_dirty(0);
        
        Ok(())
    }

    pub fn write_inode(&mut self, inode_num: usize, inode: &Inode, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<(), FsError> 
    {
        let inode_sector = 1 + (inode_num as u32 / 2);
        let offset_in_sector = (inode_num % 2) * 256;
        
        let sector_data = cache.get_sector_mut(inode_sector, dev)?;
        unsafe {
            core::ptr::write_unaligned(
                sector_data.as_mut_ptr().add(offset_in_sector) as *mut Inode,
                *inode
            );
        }
        cache.mark_dirty(inode_sector);
        
        // Also update cache if it's there
        self.inode_cache.inodes[inode_num] = Some(*inode);
        
        Ok(())
    }

    pub fn find_free_inode(&mut self, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<u32, FsError> {
        for i in 1..256 { // Start from 1, 0 is root
            let inode = self.inode_cache.load_inode(i, cache, dev)?;
            // Unused inode table slots are zeroed; avoid recycling if metadata looks partially set.
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

    pub fn free_block(&mut self, block: u32) {
        self.block_bitmap.free(block);
    }

    #[allow(dead_code)]
    pub fn create_file(&mut self, filename: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<u32, FsError> 
    {
        self.create_file_at(ROOT_INODE, filename, cache, dev)
    }

    #[allow(dead_code)]
    pub fn create_file_at(&mut self, parent_inode_num: u32, filename: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<u32, FsError> 
    {
        // 1. Find free inode
        let new_inode_num = self.find_free_inode(cache, dev)?;
        
        // 2. Create inode
        let new_inode = Inode {
            inode_num: new_inode_num,
            mode: MODE_FILE,
            size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1,
            owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12],
            indirect_block: 0,
            padding: [0; 160],
        };
        
        // 3. Write inode
        self.write_inode(new_inode_num as usize, &new_inode, cache, dev)?;
        
        // 4. Add to requested parent directory
        self.add_directory_entry(parent_inode_num, filename, new_inode_num, 1, cache, dev)?;
        
        Ok(new_inode_num)
    }

    pub fn create_directory_at(&mut self, parent_inode_num: u32, dirname: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice)
        -> Result<u32, FsError>
    {
        let new_inode_num = self.find_free_inode(cache, dev)?;
        let new_inode = Inode {
            inode_num: new_inode_num,
            mode: MODE_DIR,
            size: 0,
            atime: 0, mtime: 0, ctime: 0,
            link_count: 1,
            owner_uid: 0, owner_gid: 0,
            direct_blocks: [0; 12],
            indirect_block: 0,
            padding: [0; 160],
        };

        self.write_inode(new_inode_num as usize, &new_inode, cache, dev)?;
        self.add_directory_entry(parent_inode_num, dirname, new_inode_num, 2, cache, dev)?;
        Ok(new_inode_num)
    }

    pub fn add_directory_entry(&mut self, dir_inode_num: u32, filename: &str, inode_num: u32, entry_type: u8,
                               cache: &mut BlockCache, dev: &mut dyn BlockDevice) 
        -> Result<(), FsError> 
    {
        let mut dir_inode = *self.inode_cache.load_inode(dir_inode_num as usize, cache, dev)?;
        
        // Search for a free slot in existing blocks
        for block_idx in 0..12 {
            let mut block_ptr = dir_inode.direct_blocks[block_idx];
            if block_idx * BLOCK_SIZE >= dir_inode.size as usize {
                // Need to allocate a new block for the directory
                let new_block = self.allocate_block(cache, dev)?;
                dir_inode.direct_blocks[block_idx] = new_block;
                // Update directory size
                dir_inode.size += BLOCK_SIZE as u32;
                self.write_inode(dir_inode_num as usize, &dir_inode, cache, dev)?;
                
                // Clear the new block
                for s in 0..8 {
                    let sector_data = cache.get_sector_mut(200 + new_block * 8 + s, dev)?;
                    sector_data.fill(0);
                }
                block_ptr = new_block;
            }
            
            let block_sector = 200 + (block_ptr * 8);
            for sector_offset in 0..8 {
                let sector_lba = block_sector + sector_offset;
                let sector_data = cache.get_sector_mut(sector_lba, dev)?;
                
                for entry_offset in (0..512).step_by(256) {
                    let entry_ptr = unsafe { sector_data.as_mut_ptr().add(entry_offset) as *mut DirectoryEntry };
                    let entry = unsafe { &*entry_ptr };
                    
                    if entry.inode_num == 0 {
                        // Found a slot!
                        let mut name_buf = [0u8; 249];
                        let name_bytes = filename.as_bytes();
                        let len = name_bytes.len().min(249);
                        name_buf[..len].copy_from_slice(&name_bytes[..len]);
                        
                        // Default attributes: Archive for files, Directory for dirs
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
        
        Err(FsError::NoBlockAvailable) // Directory full
    }

    pub fn write_file(&mut self, inode_num: u32, data: &[u8], cache: &mut BlockCache, page_cache: &mut PageCache, dev: &mut dyn BlockDevice) 
        -> Result<usize, FsError> 
    {
        let mut inode = *self.inode_cache.load_inode(inode_num as usize, cache, dev)?;
        
        let mut written = 0;
        let mut block_idx = 0;
        
        while written < data.len() && block_idx < 12 {
            if block_idx * BLOCK_SIZE >= inode.size as usize {
                inode.direct_blocks[block_idx] = self.allocate_block(cache, dev)?;
                inode.size = (block_idx * BLOCK_SIZE) as u32 + BLOCK_SIZE as u32; 
            }
            
            let block_ptr = inode.direct_blocks[block_idx];
            let block_lba = 200 + (block_ptr * 8);
            
            let page = page_cache.get_page_mut(inode_num, block_idx as u32, block_lba as u64, dev)?;
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

    #[allow(dead_code)]
    pub fn delete_file_by_inode(&mut self, parent_inode: u32, _filename: &str, file_inode_num: u32, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<(), FsError> {
        // Free the file's data blocks – copy pointers first to avoid borrow conflict
        let mut blocks_to_free = [0u32; 12];
        {
            let file_inode = *self.inode_cache.load_inode(file_inode_num as usize, cache, dev)?;
            let num_blocks = Self::inode_block_count(&file_inode);
            for j in 0..num_blocks.min(12) {
                let b = file_inode.direct_blocks[j];
                if b != 0 && b < self.superblock.num_blocks {
                    blocks_to_free[j] = b;
                }
            }
        }
        for &b in blocks_to_free.iter() {
            if b != 0 {
                self.free_block(b);
            }
        }
        // Mark the file's inode as free in the cache
        self.inode_cache.inodes[file_inode_num as usize] = None;

        
        let dir_inode = *self.inode_cache.load_inode(parent_inode as usize, cache, dev)?;
        
        let num_blocks = self.inode_data_block_count(&dir_inode);
        
        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx) {
                Some(b) => b,
                None => continue,
            };
            
            let block_sector = 200 + (actual_block * 8);
            
            for sector_offset in 0..8 {
                let sector_lba = block_sector + sector_offset;
                let sector_data = cache.get_sector_mut(sector_lba, dev)?;
                
                for entry_offset in (0..512).step_by(DIR_ENTRY_SIZE) {
                    let entry_inode = u32::from_le_bytes([
                        sector_data[entry_offset],
                        sector_data[entry_offset + 1],
                        sector_data[entry_offset + 2],
                        sector_data[entry_offset + 3]
                    ]);
                    
                    let first_byte = sector_data[entry_offset];
                    
                    // Skip empty slots
                    if entry_inode == 0 {
                        continue;
                    }
                    
                    // If already deleted (0xE5), treat as success (file is gone)
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
        let _file_inode = self.find_file_in_directory(parent_inode, old_name, cache, dev)?;
        
        let dir_inode = *self.inode_cache.load_inode(parent_inode as usize, cache, dev)?;
        
        let num_blocks = self.inode_data_block_count(&dir_inode);
        
        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx) {
                Some(b) => b,
                None => continue,
            };
            
            let block_sector = 200 + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_lba = block_sector + sector_offset;
                let sector_data = cache.get_sector_mut(sector_lba, dev)?;
                
                for entry_off in (0..512).step_by(DIR_ENTRY_SIZE) {
                    let first_byte = sector_data[entry_off];
                    if first_byte == 0xE5 || first_byte == 0x00 {
                        continue;
                    }
                    
                    let name_len = sector_data[entry_off + 4] as usize;
                    
                    let mut entry_name = [0u8; 256];
                    let copy_len = name_len.min(255);
                    entry_name[..copy_len].copy_from_slice(&sector_data[entry_off + 6..entry_off + 6 + copy_len]);
                    
                    if core::str::from_utf8(&entry_name[..copy_len]).map(|s| s.eq_ignore_ascii_case(old_name)).unwrap_or(false) {
                        let new_len = new_name.len().min(255);
                        sector_data[entry_off + 4] = new_len as u8;
                        sector_data[entry_off + 6..entry_off + 6 + new_len].copy_from_slice(new_name.as_bytes());
                        for i in (entry_off + 6 + new_len)..(entry_off + 256) {
                            sector_data[i] = 0x20;
                        }
                        cache.mark_dirty(sector_lba);
                        return Ok(());
                    }
                }
            }
        }
        
        Err(FsError::FileNotFound)
    }

    pub fn is_directory_empty(&mut self, dir_inode_num: u32, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<bool, FsError> {
        let dir_inode = *self.inode_cache.load_inode(dir_inode_num as usize, cache, dev)?;
        
        let num_blocks = self.inode_data_block_count(&dir_inode);
        
        for block_idx in 0..num_blocks {
            let actual_block = match self.get_inode_block_ptr(&dir_inode, block_idx) {
                Some(b) => b,
                None => continue,
            };
            
            let block_sector = 200 + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_lba = block_sector + sector_offset;
                let sector_data = cache.get_sector(sector_lba, dev)?;
                
                for entry_off in (0..512).step_by(DIR_ENTRY_SIZE) {
                    let first_byte = sector_data[entry_off];
                    // Skip empty or deleted entries, but count . and ..
                    if first_byte == 0x00 {
                        return Ok(true); // End of entries
                    }
                    if first_byte == 0xE5 {
                        continue;
                    }
                    
                    let name_len = sector_data[entry_off + 4] as usize;
                    if name_len == 0 || name_len > 250 {
                        continue;
                    }
                    
                    // Check if it's . or ..
                    if name_len == 1 && sector_data[entry_off + 6] == b'.' {
                        continue;
                    }
                    if name_len == 2 && sector_data[entry_off + 6] == b'.' && sector_data[entry_off + 7] == b'.' {
                        continue;
                    }
                    
                    // Found a real entry, directory is not empty
                    return Ok(false);
                }
            }
        }
        
        Ok(true)
    }

    pub fn delete_directory(&mut self, parent_inode: u32, dirname: &str, cache: &mut BlockCache, dev: &mut dyn BlockDevice) -> Result<(), FsError> {
        // Find the directory inode
        let dir_inode_num = self.find_dir_in_dir(parent_inode, dirname, cache, dev)?;
        
        // Check if directory is empty
        if !self.is_directory_empty(dir_inode_num, cache, dev)? {
            return Err(FsError::DirectoryNotEmpty);
        }
        
        // Use same logic as delete_file_by_inode to remove the entry
        self.delete_file_by_inode(parent_inode, dirname, dir_inode_num, cache, dev)
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
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        let mut temp_buf = alloc::vec::Vec::with_capacity(buf.len() + offset as usize);
        temp_buf.resize(buf.len() + offset as usize, 0);
        
        let read = self.read_file_to_buf(inode, &mut temp_buf, cache, &mut *pc_lock, dev)?;
        
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
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        Ok(self.write_file(inode, buf, cache, &mut *pc_lock, dev)?)
    }

    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        let (inode, _entry_type) = self.find_entry_in_directory(dir_inode, name, cache, dev)?;
        let inode_data = self.inode_cache.load_inode(inode as usize, cache, dev)?;
        
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
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        let inode = *self.inode_cache.load_inode(dir_inode as usize, cache, dev)?;
        if (inode.mode & MODE_DIR) == 0 {
            return Err(VfsError::NotADirectory);
        }

        let mut current_idx = 0;
        let mut bytes_to_read = Self::directory_byte_span(&inode);
        for block_idx in 0..self.inode_data_block_count(&inode) {
            if bytes_to_read == 0 { break; }
            let Some(current_block) = self.get_inode_block_ptr(&inode, block_idx) else { continue; };
            let block_sector = 200 + (current_block * 8);
            
            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(block_sector + sector_offset, dev)?;
                for entry_offset in (0..512).step_by(256) {
                    let first_byte = sector_data[entry_offset];
                    if first_byte == 0xE5 || first_byte == 0 { continue; }
                    
                    if current_idx == index {
                        let entry: DirectoryEntry = unsafe {
                            core::ptr::read_unaligned(sector_data.as_ptr().add(entry_offset) as *const _)
                        };
                        let name = core::str::from_utf8(&entry.name[..entry.name_len as usize]).unwrap_or("?").into();
                        let inode_data = self.inode_cache.load_inode(entry.inode_num as usize, cache, dev)?;
                        return Ok(Some(VfsDirEntry {
                            name,
                            node: VfsNode {
                                inode: entry.inode_num,
                                mode: inode_data.mode,
                                size: inode_data.size,
                            }
                        }));
                    }
                    current_idx += 1;
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
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        let inode = self.create_directory_at(dir_inode, name, cache, dev)?;
        let inode_data = self.inode_cache.load_inode(inode as usize, cache, dev)?;
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
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        let (file_inode, _entry_type) = self.find_entry_in_directory(dir_inode, name, cache, dev)?;
        let inode_data = self.inode_cache.load_inode(file_inode as usize, cache, dev)?;
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
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        self.delete_directory(dir_inode, name, cache, dev)?;
        Ok(())
    }

    fn rename(&mut self, dir_inode: u32, old_name: &str, new_name: &str) -> Result<(), VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        self.rename_file(dir_inode, old_name, new_name, cache, dev)?;
        Ok(())
    }

    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let mut cache_lock = crate::globals::BLOCK_CACHE.lock();
        let cache = cache_lock.as_mut().ok_or(VfsError::IOError)?;
        let mut bdevs_lock = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        let inode = self.create_file_at(dir_inode, name, cache, dev)?;
        let inode_data = self.inode_cache.load_inode(inode as usize, cache, dev)?;
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
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        let inode_data = self.inode_cache.load_inode(inode as usize, cache, dev)?;
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
        let dev = bdevs_lock.get(0).ok_or(VfsError::IOError)?;

        self.set_volume_label(label, cache, dev)?;
        Ok(())
    }
}
