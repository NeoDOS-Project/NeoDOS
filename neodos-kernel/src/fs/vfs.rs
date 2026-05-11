// src/fs/vfs.rs - Virtual File System layer
// Provides directory-aware file and directory lookups

use crate::drivers::ata::AtaDriver;
use crate::buffer::block_cache::BlockCache;
use crate::fs::neodos_fs::{NeoDosFs, FsError, MODE_DIR, ROOT_INODE};

impl NeoDosFs {
    pub fn resolve_directory_path(
        &mut self,
        base_inode: u32,
        base_path: &[u8],
        base_path_len: usize,
        raw_path: &str,
        cache: &mut BlockCache,
        ata: &mut AtaDriver,
    ) -> Result<(u32, [u8; 128], usize), FsError> {
        let mut resolved_path = [0u8; 128];
        let mut resolved_len = 1usize;
        resolved_path[0] = b'\\';

        let bytes = raw_path.as_bytes();
        let mut path_start = 0usize;

        // Drive letters are validated only in the shell via DriveManager; paths here
        // must be logical paths without `X:` (e.g. `/SYSTEM` or `\FOO` or relative).
        let mut current_inode = if raw_path.starts_with('\\') || raw_path.starts_with('/') {
            ROOT_INODE
        } else {
            let safe_len = base_path_len.min(base_path.len()).min(resolved_path.len());
            if safe_len > 0 {
                resolved_path[..safe_len].copy_from_slice(&base_path[..safe_len]);
                resolved_len = safe_len;
            }
            base_inode
        };

        while path_start < bytes.len() && (bytes[path_start] == b'\\' || bytes[path_start] == b'/') {
            path_start += 1;
        }

        let mut component_start = path_start;
        while component_start <= bytes.len() {
            let mut component_end = component_start;
            while component_end < bytes.len() && bytes[component_end] != b'\\' && bytes[component_end] != b'/' {
                component_end += 1;
            }

            if component_end > component_start {
                let component = core::str::from_utf8(&bytes[component_start..component_end])
                    .map_err(|_| FsError::FileNotFound)?;

                if component == "." {
                    // Keep current inode/path.
                } else if component == ".." {
                    self.pop_path_component(&mut resolved_path, &mut resolved_len);
                    current_inode = self.resolve_inode_from_absolute_path(
                        &resolved_path[..resolved_len],
                        cache,
                        ata,
                    )?;
                } else {
                    current_inode = self.find_directory_in_directory(current_inode, component, cache, ata)?;
                    self.push_path_component(&mut resolved_path, &mut resolved_len, component);
                }
            }

            component_start = component_end + 1;
        }

        Ok((current_inode, resolved_path, resolved_len))
    }

    fn resolve_inode_from_absolute_path(
        &mut self,
        abs_path: &[u8],
        cache: &mut BlockCache,
        ata: &mut AtaDriver,
    ) -> Result<u32, FsError> {
        if abs_path.is_empty() || (abs_path.len() == 1 && abs_path[0] == b'\\') {
            return Ok(ROOT_INODE);
        }

        let mut current_inode = ROOT_INODE;
        let mut start = if abs_path[0] == b'\\' { 1 } else { 0 };
        while start < abs_path.len() {
            let mut end = start;
            while end < abs_path.len() && abs_path[end] != b'\\' {
                end += 1;
            }

            if end > start {
                let component = core::str::from_utf8(&abs_path[start..end])
                    .map_err(|_| FsError::FileNotFound)?;
                current_inode = self.find_directory_in_directory(current_inode, component, cache, ata)?;
            }

            start = end + 1;
        }

        Ok(current_inode)
    }

    fn pop_path_component(&self, path: &mut [u8; 128], len: &mut usize) {
        if *len <= 1 {
            path[0] = b'\\';
            *len = 1;
            return;
        }

        let mut new_len = *len;
        while new_len > 1 {
            new_len -= 1;
            if path[new_len] == b'\\' {
                break;
            }
        }

        *len = if new_len == 0 { 1 } else { new_len };
        path[*len..].fill(0);
    }

    fn push_path_component(&self, path: &mut [u8; 128], len: &mut usize, component: &str) {
        if *len > 1 && *len < path.len() {
            path[*len] = b'\\';
            *len += 1;
        }

        for &b in component.as_bytes() {
            if *len >= path.len() {
                break;
            }
            path[*len] = b;
            *len += 1;
        }
    }

    /// VFS Core: Find entry by name in any directory
    /// Returns (inode_num, entry_type)
    pub fn find_entry_in_directory(&mut self, dir_inode_num: u32, filename: &str, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<(u32, u8), FsError>
    {
        let dir_inode = *self.inode_cache.load_inode(dir_inode_num as usize, cache, ata)?;
        
        if (dir_inode.mode & MODE_DIR) == 0 {
            return Err(FsError::NotADirectory);
        }

        let mut bytes_to_read = NeoDosFs::directory_byte_span(&dir_inode);
        for block_idx in 0..self.inode_data_block_count(&dir_inode) {
            if bytes_to_read == 0 { break; }
            let Some(current_block) = self.get_inode_block_ptr(&dir_inode, block_idx) else {
                continue;
            };
            let block_sector = 200 + (current_block * 8);
            let to_read_in_block = if bytes_to_read > crate::fs::neodos_fs::BLOCK_SIZE { crate::fs::neodos_fs::BLOCK_SIZE } else { bytes_to_read };

            for sector_offset in 0..8 {
                let sector_data = cache.get_sector(block_sector + sector_offset, ata)?;
                
                for entry_offset in (0..512).step_by(256) {
                    // Skip deleted entries (first byte 0xE5)
                    let first_byte = sector_data[entry_offset];
                    if first_byte == 0xE5 {
                        continue;
                    }
                    
                    let entry: crate::fs::neodos_fs::DirectoryEntry = unsafe {
                        core::ptr::read_unaligned(
                            sector_data.as_ptr().add(entry_offset) as *const _
                        )
                    };
                    
                    if entry.inode_num != 0 {
                        let inode_num = entry.inode_num;
                        let name_len = entry.name_len;
                        if name_len == 0 || name_len as usize > entry.name.len() {
                            continue;
                        }
                        let name_slice = &entry.name[..name_len as usize];
                        if let Ok(name) = core::str::from_utf8(name_slice) {
                            // Case-insensitive comparison
                            if aeq_ignore_ascii_case(name, filename) {
                                return Ok((inode_num, entry.entry_type));
                            }
                        }
                    }
                }
            }
            bytes_to_read -= to_read_in_block;
        }
        
        Err(FsError::FileNotFound)
    }

    /// VFS: Find file in specific directory
    pub fn find_file_in_directory(&mut self, dir_inode_num: u32, filename: &str, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<u32, FsError> 
    {
        let (inode_num, entry_type) = self.find_entry_in_directory(dir_inode_num, filename, cache, ata)?;
        
        // Validate it's a file using entry_type from directory (1 = file, 2 = dir)
        if entry_type != 1 {
            crate::serial_println!("[VFS] find_file_in_directory: entry_type={} not a file", entry_type);
            return Err(FsError::NotAFile);
        }
        
        Ok(inode_num)
    }

    /// VFS: Find directory by name in parent directory
    pub fn find_directory_in_directory(&mut self, dir_inode_num: u32, dirname: &str, cache: &mut BlockCache, ata: &mut AtaDriver) 
        -> Result<u32, FsError> 
    {
        let (inode_num, entry_type) = self.find_entry_in_directory(dir_inode_num, dirname, cache, ata)?;
        
        // Validate it's a directory using entry_type from directory (1 = file, 2 = dir)
        if entry_type != 2 {
            return Err(FsError::NotADirectory);
        }
        
        Ok(inode_num)
    }

}

fn aeq_ignore_ascii_case(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.eq_ignore_ascii_case(b)
}
