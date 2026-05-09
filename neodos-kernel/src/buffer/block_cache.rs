// src/buffer/block_cache.rs

use crate::drivers::ata::{AtaDriver, AtaError};

const CACHE_SIZE: usize = 256; // 256 entries = 128 KB (was 64 = 32 KB)

#[derive(Copy, Clone)]
struct CacheEntry {
    lba: u32,
    data: [u8; 512],
    valid: bool,
    dirty: bool,
    last_access: u64,
}

pub struct BlockCache {
    entries: [CacheEntry; CACHE_SIZE],
    counter: u64,
}

impl BlockCache {
    pub fn new() -> Self {
        // Use const initialization to avoid unsafe code
        const EMPTY_ENTRY: CacheEntry = CacheEntry {
            lba: 0,
            data: [0u8; 512],
            valid: false,
            dirty: false,
            last_access: 0,
        };
        
        BlockCache {
            entries: [EMPTY_ENTRY; CACHE_SIZE],
            counter: 0,
        }
    }

    pub fn get_sector(&mut self, lba: u32, ata: &mut AtaDriver) -> Result<&[u8; 512], AtaError> {
        self.counter += 1;

        // 1. Check if in cache
        for i in 0..CACHE_SIZE {
            if self.entries[i].valid && self.entries[i].lba == lba {
                self.entries[i].last_access = self.counter;
                return Ok(&self.entries[i].data);
            }
        }

        // 2. Not in cache, find replacement (LRU)
        let mut lru_idx = 0;
        let mut min_access = u64::MAX;

        for i in 0..CACHE_SIZE {
            if !self.entries[i].valid {
                lru_idx = i;
                break;
            }
            if self.entries[i].last_access < min_access {
                min_access = self.entries[i].last_access;
                lru_idx = i;
            }
        }

        // 3. If LRU entry is dirty, write it back first (must succeed or we lose metadata)
        if self.entries[lru_idx].valid && self.entries[lru_idx].dirty {
            ata
                .write_sector(self.entries[lru_idx].lba, &self.entries[lru_idx].data)
                .map_err(|_| AtaError::Error)?;
        }

        // 4. Read from disk
        let data = ata.read_sector(lba)?;
        
        // 5. Update cache
        self.entries[lru_idx] = CacheEntry {
            lba,
            data,
            valid: true,
            dirty: false,
            last_access: self.counter,
        };

        Ok(&self.entries[lru_idx].data)
    }

    pub fn mark_dirty(&mut self, lba: u32) {
        for entry in &mut self.entries {
            if entry.valid && entry.lba == lba {
                entry.dirty = true;
                return;
            }
        }
    }

    pub fn flush(&mut self, ata: &mut AtaDriver) -> Result<(), AtaError> {
        for entry in &mut self.entries {
            if entry.valid && entry.dirty {
                ata.write_sector(entry.lba, &entry.data).map_err(|_| AtaError::Error)?;
                entry.dirty = false;
            }
        }
        Ok(())
    }

    pub fn get_sector_mut(&mut self, lba: u32, ata: &mut AtaDriver) -> Result<&mut [u8; 512], AtaError> {
        // Ensure it's in cache
        let _ = self.get_sector(lba, ata)?;
        
        // Find it and return mut ref
        for entry in &mut self.entries {
            if entry.valid && entry.lba == lba {
                entry.dirty = true; // Implicitly mark dirty when getting mut
                return Ok(&mut entry.data);
            }
        }
        Err(AtaError::Error)
    }
}
