use crate::drivers::block::BlockDevice;

const PAGE_CACHE_SIZE: usize = 512;

#[derive(Copy, Clone)]
struct PageCacheEntry {
    valid: bool,
    dirty: bool,
    key_inode: u32,
    key_block: u32,
    data_lba: u64,
    last_access: u64,
    data: [u8; 4096],
}

pub struct PageCache {
    entries: [PageCacheEntry; PAGE_CACHE_SIZE],
    counter: u64,
}

impl PageCache {
    pub const fn new() -> Self {
        const EMPTY: PageCacheEntry = PageCacheEntry {
            valid: false,
            dirty: false,
            key_inode: 0,
            key_block: 0,
            data_lba: 0,
            last_access: 0,
            data: [0u8; 4096],
        };
        PageCache {
            entries: [EMPTY; PAGE_CACHE_SIZE],
            counter: 0,
        }
    }

    pub fn read_page(
        &mut self,
        inode: u32,
        block_num: u32,
        data_lba: u64,
        dev: &mut dyn BlockDevice,
    ) -> Result<&[u8; 4096], ()> {
        self.counter = self.counter.wrapping_add(1);

        for i in 0..PAGE_CACHE_SIZE {
            let e = &self.entries[i];
            if e.valid && e.key_inode == inode && e.key_block == block_num {
                self.entries[i].last_access = self.counter;
                return Ok(&self.entries[i].data);
            }
        }

        let idx = self.find_lru();

        if self.entries[idx].valid && self.entries[idx].dirty {
            let lba = self.entries[idx].data_lba;
            dev.write_blocks(lba, 8, &self.entries[idx].data)?;
        }

        let mut tmp = [0u8; 4096];
        dev.read_blocks(data_lba, 8, &mut tmp)?;

        self.entries[idx] = PageCacheEntry {
            valid: true,
            dirty: false,
            key_inode: inode,
            key_block: block_num,
            data_lba,
            last_access: self.counter,
            data: tmp,
        };

        Ok(&self.entries[idx].data)
    }

    pub fn get_page_mut(
        &mut self,
        inode: u32,
        block_num: u32,
        data_lba: u64,
        dev: &mut dyn BlockDevice,
    ) -> Result<&mut [u8; 4096], ()> {
        let _ = self.read_page(inode, block_num, data_lba, dev)?;

        for i in 0..PAGE_CACHE_SIZE {
            let e = &self.entries[i];
            if e.valid && e.key_inode == inode && e.key_block == block_num {
                self.entries[i].dirty = true;
                return Ok(&mut self.entries[i].data);
            }
        }
        Err(())
    }

    pub fn peek(&self, inode: u32, block_num: u32) -> Option<&[u8; 4096]> {
        for i in 0..PAGE_CACHE_SIZE {
            let e = &self.entries[i];
            if e.valid && e.key_inode == inode && e.key_block == block_num {
                return Some(&e.data);
            }
        }
        None
    }

    pub fn mark_dirty(&mut self, inode: u32, block_num: u32) {
        for i in 0..PAGE_CACHE_SIZE {
            let e = &self.entries[i];
            if e.valid && e.key_inode == inode && e.key_block == block_num {
                self.entries[i].dirty = true;
                return;
            }
        }
    }

    pub fn flush(&mut self, dev: &mut dyn BlockDevice) -> Result<(), ()> {
        for i in 0..PAGE_CACHE_SIZE {
            if self.entries[i].valid && self.entries[i].dirty {
                let lba = self.entries[i].data_lba;
                dev.write_blocks(lba, 8, &self.entries[i].data)?;
                self.entries[i].dirty = false;
            }
        }
        Ok(())
    }

    pub fn flush_inode(&mut self, inode: u32, dev: &mut dyn BlockDevice) -> Result<(), ()> {
        for i in 0..PAGE_CACHE_SIZE {
            if self.entries[i].valid && self.entries[i].dirty && self.entries[i].key_inode == inode
            {
                let lba = self.entries[i].data_lba;
                dev.write_blocks(lba, 8, &self.entries[i].data)?;
                self.entries[i].dirty = false;
            }
        }
        Ok(())
    }

    pub fn invalidate_inode(&mut self, inode: u32) {
        for i in 0..PAGE_CACHE_SIZE {
            if self.entries[i].valid && self.entries[i].key_inode == inode {
                self.entries[i].valid = false;
                self.entries[i].dirty = false;
            }
        }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.iter().filter(|e| e.valid).count()
    }

    pub fn dirty_count(&self) -> usize {
        self.entries.iter().filter(|e| e.valid && e.dirty).count()
    }

    fn find_lru(&self) -> usize {
        let mut idx = 0;
        let mut oldest = u64::MAX;
        for i in 0..PAGE_CACHE_SIZE {
            if !self.entries[i].valid {
                return i;
            }
            if self.entries[i].last_access < oldest {
                oldest = self.entries[i].last_access;
                idx = i;
            }
        }
        idx
    }
}
