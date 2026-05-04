// src/fs/neodos_fs.rs

use crate::drivers::ata::{AtaDriver, AtaError};
use crate::buffer::block_cache::BlockCache;
use crate::serial_println;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Superblock {
    pub magic: u32,              // 0x4F444F4E = "NEOD" little-endian
    pub block_size: u32,         // Typ. 4096
    pub num_blocks: u32,         // Total blocks
    pub num_inodes: u32,         // Max 256
    pub created: u64,            // Timestamp
    pub reserved: [u8; 484],     // Padding to 512 bytes
}

pub const SUPERBLOCK_MAGIC: u32 = 0x4F444F4E;  // "NEOD"
pub const BLOCK_SIZE: usize = 4096;

#[repr(packed)]
#[derive(Debug, Clone, Copy)]
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

#[repr(packed)]
#[derive(Debug, Clone, Copy)]
pub struct DirectoryEntry {
    pub inode_num: u32,
    pub name_len: u8,
    pub entry_type: u8,          // 1=file, 2=dir
    pub name: [u8; 250],         // Slightly adjusted to fit 256 bytes total
}

pub const DIR_ENTRY_SIZE: usize = 256;

#[derive(Debug)]
pub enum FsError {
    InvalidSuperblock,
    InvalidBlockSize,
    NotADirectory,
    NotAFile,
    FileNotFound,
    BlockReadError,
    NoInodeAvailable,
    NoBlockAvailable,
    Ata(AtaError),
}

impl From<AtaError> for FsError {
    fn from(err: AtaError) -> Self {
        FsError::Ata(err)
    }
}

pub struct InodeCache {
    inodes: [Option<Inode>; 256],
}

impl InodeCache {
    pub fn new() -> Self {
        InodeCache {
            inodes: [None; 256],
        }
    }
    
    pub fn load_inode(&mut self, inode_num: usize, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<&Inode, FsError> 
    {
        if inode_num >= 256 {
            return Err(FsError::FileNotFound);
        }

        if self.inodes[inode_num].is_some() {
            return Ok(self.inodes[inode_num].as_ref().unwrap());
        }
        
        // Inode table @ sector 1, 256 bytes per inode = 2 inodes per sector
        let inode_sector = 1 + (inode_num as u32 / 2);
        let offset_in_sector = (inode_num % 2) * 256;
        
        let sector_data = cache.get_sector(inode_sector, ata)?;
        let inode: Inode = unsafe {
            core::ptr::read_unaligned(
                sector_data.as_ptr().add(offset_in_sector) as *const _
            )
        };
        
        self.inodes[inode_num] = Some(inode);
        Ok(self.inodes[inode_num].as_ref().unwrap())
    }
}

pub struct NeoDosFs {
    pub superblock: Superblock,
    pub inode_cache: InodeCache,
}

impl NeoDosFs {
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
        })
    }
    
    pub fn list_root(&mut self, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<(), FsError> 
    {
        let root_inode = self.inode_cache.load_inode(0, cache, ata)?;
        let mode = root_inode.mode;
        let size = root_inode.size;
        
        if (mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }
        
        let mut bytes_to_read = size as usize;
        let direct_blocks = root_inode.direct_blocks;
        for &block_ptr in &direct_blocks {
            if bytes_to_read == 0 { break; }
            
            let current_block = block_ptr;
            let block_sector = 200 + (current_block * 8);
            let to_read_in_block = if bytes_to_read > BLOCK_SIZE { BLOCK_SIZE } else { bytes_to_read };
            
            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(block_sector + sector_offset, ata)?;
                
                for entry_offset in (0..512).step_by(256) {
                    let entry: DirectoryEntry = unsafe {
                        core::ptr::read_unaligned(
                            sector_data.as_ptr().add(entry_offset) as *const _
                        )
                    };
                    
                    if entry.inode_num != 0 {
                        let inode_num = entry.inode_num;
                        let name_len = entry.name_len;
                        let name_slice = &entry.name[..name_len as usize];
                        if let Ok(name) = core::str::from_utf8(name_slice) {
                            crate::vga::print_str("  ");
                            crate::vga::print_str(name);
                            crate::vga::print_str("\r\n");
                            crate::serial_println!("  {}", name);
                        }
                    }
                }
            }
            bytes_to_read -= to_read_in_block;
        }
        
        Ok(())
    }

    pub fn list_directory(&mut self, inode_num: u32, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<(), FsError> 
    {
        let inode = self.inode_cache.load_inode(inode_num as usize, cache, ata)?;
        let mode = inode.mode;
        let size = inode.size;
        
        if (mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }
        
        let mut bytes_to_read = size as usize;
        let direct_blocks = inode.direct_blocks;
        for &block_ptr in &direct_blocks {
            if bytes_to_read == 0 { break; }
            
            let current_block = block_ptr;
            let block_sector = 200 + (current_block * 8);
            let to_read_in_block = if bytes_to_read > BLOCK_SIZE { BLOCK_SIZE } else { bytes_to_read };
            
            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(block_sector + sector_offset, ata)?;
                
                for entry_offset in (0..512).step_by(256) {
                    let entry: DirectoryEntry = unsafe {
                        core::ptr::read_unaligned(
                            sector_data.as_ptr().add(entry_offset) as *const _
                        )
                    };
                    
                    if entry.inode_num != 0 {
                        let inode_num = entry.inode_num;
                        let name_len = entry.name_len;
                        let name_slice = &entry.name[..name_len as usize];
                        if let Ok(name) = core::str::from_utf8(name_slice) {
                            crate::vga::print_str("  ");
                            crate::vga::print_str(name);
                            crate::vga::print_str("\r\n");
                            crate::serial_println!("  {}", name);
                        }
                    }
                }
            }
            bytes_to_read -= to_read_in_block;
        }
        
        Ok(())
    }

    pub fn find_file(&mut self, filename: &str, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<u32, FsError> 
    {
        let root_inode = self.inode_cache.load_inode(0, cache, ata)?;
        let size = root_inode.size;
        
        let mut bytes_to_read = size as usize;
        let direct_blocks = root_inode.direct_blocks;
        for &block_ptr in &direct_blocks {
            if bytes_to_read == 0 { break; }
            
            let current_block = block_ptr;
            let block_sector = 200 + (current_block * 8);
            let to_read_in_block = if bytes_to_read > BLOCK_SIZE { BLOCK_SIZE } else { bytes_to_read };

            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(block_sector + sector_offset, ata)?;
                
                for entry_offset in (0..512).step_by(256) {
                    let entry: DirectoryEntry = unsafe {
                        core::ptr::read_unaligned(
                            sector_data.as_ptr().add(entry_offset) as *const _
                        )
                    };
                    
                    if entry.inode_num != 0 {
                        let inode_num = entry.inode_num;
                        let name_len = entry.name_len;
                        let name_slice = &entry.name[..name_len as usize];
                        if let Ok(name) = core::str::from_utf8(name_slice) {
                            if name == filename {
                                return Ok(inode_num);
                            }
                        }
                    }
                }
            }
            bytes_to_read -= to_read_in_block;
        }
        
        Err(FsError::FileNotFound)
    }

    pub fn read_file_to_buf(&mut self, inode_num: u32, buf: &mut [u8], cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<usize, FsError> 
    {
        let inode = self.inode_cache.load_inode(inode_num as usize, cache, ata)?;
        let mode = inode.mode;
        let size = inode.size;
        if (mode & MODE_FILE) == 0 {
            return Err(FsError::NotAFile);
        }
        
        let mut bytes_left = size as usize;
        let mut total_read = 0;
        let direct_blocks = inode.direct_blocks;
        
        for &block_ptr in &direct_blocks {
            if bytes_left == 0 || total_read >= buf.len() { break; }
            
            let current_block = block_ptr;
            let block_sector = 200 + (current_block * 8);
            for sector_offset in 0..8 {
                if bytes_left == 0 || total_read >= buf.len() { break; }
                
                let sector_data = cache.get_sector(block_sector + sector_offset, ata)?;
                let to_copy = if bytes_left > 512 { 512 } else { bytes_left };
                let to_copy = if total_read + to_copy > buf.len() { buf.len() - total_read } else { to_copy };
                
                buf[total_read..total_read + to_copy].copy_from_slice(&sector_data[..to_copy]);
                
                total_read += to_copy;
                bytes_left -= to_copy;
            }
        }
        
        Ok(total_read)
    }

    pub fn read_file(&mut self, inode_num: u32, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<(), FsError> 
    {
        let inode = self.inode_cache.load_inode(inode_num as usize, cache, ata)?;
        
        let mode = inode.mode;
        let size = inode.size;
        if (mode & MODE_FILE) == 0 {
            return Err(FsError::NotAFile);
        }
        
        let mut bytes_left = size as usize;
        let direct_blocks = inode.direct_blocks;
        
        for &block_ptr in &direct_blocks {
            if bytes_left == 0 { break; }
            
            let current_block = block_ptr;
            let block_sector = 200 + (current_block * 8);
            for sector_offset in 0..8 {
                if bytes_left == 0 { break; }
                
                let sector_data = cache.get_sector(block_sector + sector_offset, ata)?;
                let to_copy = if bytes_left > 512 { 512 } else { bytes_left };
                
                // Print directly to VGA and Serial
                if let Ok(text) = core::str::from_utf8(&sector_data[..to_copy]) {
                    crate::vga::print_str(text);
                    crate::serial_print!("{}", text);
                }
                
                bytes_left -= to_copy;
            }
        }
        
        Ok(())
    }

    pub fn sync(&mut self, cache: &mut BlockCache, ata: &mut AtaDriver) -> Result<(), FsError> {
        cache.flush(ata).map_err(FsError::Ata)
    }

    pub fn write_inode(&mut self, inode_num: usize, inode: &Inode, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<(), FsError> 
    {
        let inode_sector = 1 + (inode_num as u32 / 2);
        let offset_in_sector = (inode_num % 2) * 256;
        
        let sector_data = cache.get_sector_mut(inode_sector, ata)?;
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

    pub fn find_free_inode(&mut self, cache: &mut BlockCache, ata: &mut AtaDriver) -> Result<u32, FsError> {
        for i in 1..256 { // Start from 1, 0 is root
            let inode = self.inode_cache.load_inode(i, cache, ata)?;
            if inode.inode_num == 0 {
                return Ok(i as u32);
            }
        }
        Err(FsError::NoInodeAvailable)
    }

    pub fn allocate_block(&mut self, cache: &mut BlockCache, ata: &mut AtaDriver) -> Result<u32, FsError> {
        // Simple scanner: scan all inodes to see which blocks are used
        let mut max_block = 0;
        for i in 0..256 {
            let inode = self.inode_cache.load_inode(i, cache, ata)?;
            if inode.inode_num != 0 || i == 0 {
                let num_valid_blocks = (inode.size as usize + BLOCK_SIZE - 1) / BLOCK_SIZE;
                let blocks = inode.direct_blocks; 
                for j in 0..num_valid_blocks.min(12) {
                    let b = blocks[j];
                    if b > max_block { 
                        max_block = b; 
                    }
                }
            }
        }
        
        // If max_block is 0, it means only root is used (block 0), or nothing.
        // The first data block is typically block 1 (if root uses block 0).
        let next_block = if max_block == 0 { 1 } else { max_block + 1 };

        if next_block >= self.superblock.num_blocks {
            serial_println!("[!] FS: No block available. next: {}, total: {}", next_block, self.superblock.num_blocks);
            return Err(FsError::NoBlockAvailable);
        }
        
        Ok(next_block)
    }

    pub fn create_file(&mut self, filename: &str, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<u32, FsError> 
    {
        // 1. Find free inode
        let new_inode_num = self.find_free_inode(cache, ata)?;
        
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
        self.write_inode(new_inode_num as usize, &new_inode, cache, ata)?;
        
        // 4. Add to root directory (simplified: only root for now)
        self.add_directory_entry(0, filename, new_inode_num, 1, cache, ata)?;
        
        Ok(new_inode_num)
    }

    pub fn add_directory_entry(&mut self, dir_inode_num: u32, filename: &str, inode_num: u32, entry_type: u8,
                               cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<(), FsError> 
    {
        let mut dir_inode = *self.inode_cache.load_inode(dir_inode_num as usize, cache, ata)?;
        
        // Search for a free slot in existing blocks
        for block_idx in 0..12 {
            let mut block_ptr = dir_inode.direct_blocks[block_idx];
            if block_idx * BLOCK_SIZE >= dir_inode.size as usize {
                // Need to allocate a new block for the directory
                let new_block = self.allocate_block(cache, ata)?;
                dir_inode.direct_blocks[block_idx] = new_block;
                // Update directory size
                dir_inode.size += BLOCK_SIZE as u32;
                self.write_inode(dir_inode_num as usize, &dir_inode, cache, ata)?;
                
                // Clear the new block
                for s in 0..8 {
                    let sector_data = cache.get_sector_mut(200 + new_block * 8 + s, ata)?;
                    sector_data.fill(0);
                }
                block_ptr = new_block;
            }
            
            let block_sector = 200 + (block_ptr * 8);
            for sector_offset in 0..8 {
                let sector_lba = block_sector + sector_offset;
                let sector_data = cache.get_sector_mut(sector_lba, ata)?;
                
                for entry_offset in (0..512).step_by(256) {
                    let entry_ptr = unsafe { sector_data.as_mut_ptr().add(entry_offset) as *mut DirectoryEntry };
                    let entry = unsafe { &*entry_ptr };
                    
                    if entry.inode_num == 0 {
                        // Found a slot!
                        let mut name_buf = [0u8; 250];
                        let name_bytes = filename.as_bytes();
                        let len = name_bytes.len().min(250);
                        name_buf[..len].copy_from_slice(&name_bytes[..len]);
                        
                        let new_entry = DirectoryEntry {
                            inode_num,
                            name_len: len as u8,
                            entry_type,
                            name: name_buf,
                        };
                        
                        unsafe { core::ptr::write_unaligned(entry_ptr, new_entry); }
                        cache.mark_dirty(sector_lba);
                        return Ok(());
                    }
                }
            }
        }
        
        Err(FsError::NoBlockAvailable) // Directory full
    }

    pub fn write_file(&mut self, inode_num: u32, data: &[u8], cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<usize, FsError> 
    {
        let mut inode = *self.inode_cache.load_inode(inode_num as usize, cache, ata)?;
        
        let mut written = 0;
        let mut block_idx = 0;
        
        while written < data.len() && block_idx < 12 {
            if block_idx * BLOCK_SIZE >= inode.size as usize {
                inode.direct_blocks[block_idx] = self.allocate_block(cache, ata)?;
                inode.size = (block_idx * BLOCK_SIZE) as u32 + BLOCK_SIZE as u32; // Temporary size to ensure allocation works
            }
            
            let block_ptr = inode.direct_blocks[block_idx];
            let block_sector = 200 + (block_ptr * 8);
            
            for sector_offset in 0..8 {
                if written >= data.len() { break; }
                
                let sector_lba = block_sector + sector_offset;
                let sector_data = cache.get_sector_mut(sector_lba, ata)?;
                
                let to_copy = (data.len() - written).min(512);
                sector_data[..to_copy].copy_from_slice(&data[written..written + to_copy]);
                
                written += to_copy;
                cache.mark_dirty(sector_lba);
            }
            block_idx += 1;
        }
        
        inode.size = written as u32;
        self.write_inode(inode_num as usize, &inode, cache, ata)?;
        
        Ok(written)
    }
}
