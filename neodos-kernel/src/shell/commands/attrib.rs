use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_attrib(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: ATTRIB [+/-R] [+/-H] [+/-S] filename");
            println!("       ATTRIB filename");
            return;
        }

        // Check if this is just displaying attributes or modifying
        let mut show_only = true;
        let mut new_attrs: Option<u8> = None;
        let mut filename = "";
        
        for arg in args {
            if arg.starts_with("+") || arg.starts_with("-") {
                show_only = false;
                let attr_char = arg.chars().nth(1).unwrap_or(' ').to_ascii_uppercase();
                let is_set = arg.as_bytes()[0] == b'+';
                
                if new_attrs.is_none() {
                    new_attrs = Some(0);
                }
                
                let mask = match attr_char {
                    'R' => crate::fs::neodos_fs::ATTR_READONLY,
                    'H' => crate::fs::neodos_fs::ATTR_HIDDEN,
                    'S' => crate::fs::neodos_fs::ATTR_SYSTEM,
                    'A' => crate::fs::neodos_fs::ATTR_ARCHIVE,
                    _ => 0,
                };
                
                if let Some(attrs) = new_attrs {
                    if is_set {
                        new_attrs = Some(attrs | mask);
                    } else {
                        new_attrs = Some(attrs & !mask);
                    }
                }
            } else {
                filename = arg;
            }
        }
        
        if filename.is_empty() {
            println!("Usage: ATTRIB [+/-R] [+/-H] [+/-S] filename");
            return;
        }
        
        // Find the file
        match self.resolve_file_inode(filename) {
            Ok(_inode_num) => {
                // Get current attributes by reading directory entry
                let (parent_path, leaf) = self.split_parent_and_leaf(filename);
                let parent_inode = if parent_path.is_empty() {
                    self.current_dir_inode
                } else {
                    match self.resolve_directory_arg(parent_path) {
                        Ok((inode, _, _)) => inode,
                        Err(_) => {
                            println!("Path not found");
                            return;
                        }
                    }
                };
                
                // Find entry and get current attributes
                if let Some(attrs) = self.get_file_attributes(parent_inode, leaf) {
                    if show_only {
                        // Just display attributes
                        print_attrib_attrs(attrs);
                    } else {
                        // Set new attributes
                        if let Some(new_attrs) = new_attrs {
                            self.set_file_attributes(parent_inode, leaf, new_attrs);
                            let _ = self.cache.flush(self.ata);
                            print_attrib_attrs(new_attrs);
                        }
                    }
                } else {
                    println!("File not found");
                }
            }
            Err(_) => {
                println!("File not found: {}", filename);
            }
        }
    }
    
    fn get_file_attributes(&mut self, parent_inode: u32, filename: &str) -> Option<u8> {
        let dir_inode = match self.fs.inode_cache.load_inode(parent_inode as usize, self.cache, self.ata) {
            Ok(i) => *i,
            Err(_) => return None,
        };
        
        let num_blocks = self.fs.inode_data_block_count(&dir_inode);
        
        for block_idx in 0..num_blocks {
            let actual_block = match self.fs.get_inode_block_ptr(&dir_inode, block_idx) {
                Some(b) => b,
                None => continue,
            };
            
            let block_sector = 200 + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_lba = block_sector + sector_offset;
                let sector_data = match self.cache.get_sector(sector_lba, self.ata) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                
                for entry_off in (0..512).step_by(256) {
                    let first_byte = sector_data[entry_off];
                    if first_byte == 0xE5 || first_byte == 0x00 {
                        continue;
                    }
                    
                    let name_len = sector_data[entry_off + 4] as usize;
                    if name_len == 0 || name_len > 249 {
                        continue;
                    }
                    
                    let mut entry_name = [0u8; 64];
                    let copy_len = name_len.min(63);
                    entry_name[..copy_len].copy_from_slice(&sector_data[entry_off + 7..entry_off + 7 + copy_len]);
                    
                    if core::str::from_utf8(&entry_name[..copy_len])
                        .map(|s| s.eq_ignore_ascii_case(filename))
                        .unwrap_or(false) 
                    {
                        return Some(sector_data[entry_off + 6]); // attributes byte
                    }
                }
            }
        }
        
        None
    }
    
    fn set_file_attributes(&mut self, parent_inode: u32, filename: &str, attrs: u8) {
        let dir_inode = match self.fs.inode_cache.load_inode(parent_inode as usize, self.cache, self.ata) {
            Ok(i) => *i,
            Err(_) => return,
        };
        
        let num_blocks = self.fs.inode_data_block_count(&dir_inode);
        
        for block_idx in 0..num_blocks {
            let actual_block = match self.fs.get_inode_block_ptr(&dir_inode, block_idx) {
                Some(b) => b,
                None => continue,
            };
            
            let block_sector = 200 + (actual_block * 8);
            for sector_offset in 0..8 {
                let sector_lba = block_sector + sector_offset;
                let sector_data = match self.cache.get_sector_mut(sector_lba, self.ata) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                
                for entry_off in (0..512).step_by(256) {
                    let first_byte = sector_data[entry_off];
                    if first_byte == 0xE5 || first_byte == 0x00 {
                        continue;
                    }
                    
                    let name_len = sector_data[entry_off + 4] as usize;
                    if name_len == 0 || name_len > 249 {
                        continue;
                    }
                    
                    let mut entry_name = [0u8; 64];
                    let copy_len = name_len.min(63);
                    entry_name[..copy_len].copy_from_slice(&sector_data[entry_off + 7..entry_off + 7 + copy_len]);
                    
                    if core::str::from_utf8(&entry_name[..copy_len])
                        .map(|s| s.eq_ignore_ascii_case(filename))
                        .unwrap_or(false) 
                    {
                        sector_data[entry_off + 6] = attrs;
                        self.cache.mark_dirty(sector_lba);
                        return;
                    }
                }
            }
        }
    }
}

fn print_attrib_attrs(attrs: u8) {
    let r = if attrs & 0x01 != 0 { 'R' } else { '-' };
    let h = if attrs & 0x02 != 0 { 'H' } else { '-' };
    let s = if attrs & 0x04 != 0 { 'S' } else { '-' };
    let a = if attrs & 0x20 != 0 { 'A' } else { '-' };
    crate::println!("{}{}{}{}", r, h, s, a);
}