use crate::drivers::block::BlockDevice;

const CACHE_SIZE: usize = 128;
const MAX_CACHE_SIZE: usize = 2048;
const MIN_CACHE_SIZE: usize = 64;

mod hash;
mod slot;
mod tests;

use hash::{HashTable, ReadaheadState, EMPTY_READAHEAD, make_inode_key};
use slot::{CacheSlot, EMPTY_SLOT};
pub use tests::register_tests;

// ── Public cache statistics ─────────────────────────────────────────

#[derive(Copy, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub dirty_flushes: u64,
    pub readahead_hits: u64,
    pub current_entries: usize,
    pub dirty_count: usize,
    pub pending_writes: usize,
    pub hash_table_len: usize,
    pub hash_table_capacity: usize,
}


// ── Unified page cache ─────────────────────────────────────────────

pub struct PageCache {
    slots: [CacheSlot; CACHE_SIZE],
    hash: HashTable<CACHE_SIZE>,
    lru_head: Option<u16>,
    lru_tail: Option<u16>,
    readahead: [ReadaheadState; 64],
    readahead_count: usize,
    counter: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
    dirty_flushes: u64,
    readahead_hits: u64,
    dirty_count: u16,
}

impl PageCache {
    pub const fn new() -> Self {
        PageCache {
            slots: [EMPTY_SLOT; CACHE_SIZE],
            hash: HashTable::new(),
            lru_head: None,
            lru_tail: None,
            readahead: [EMPTY_READAHEAD; 64],
            readahead_count: 0,
            counter: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            dirty_flushes: 0,
            readahead_hits: 0,
            dirty_count: 0,
        }
    }

    // ── Sector-level API (replaces BlockCache) ─────────────────────

    pub fn get_sector(&mut self, lba: u32, dev_tag: u64, dev: &mut dyn BlockDevice) -> Result<&[u8], ()> {
        let page_lba = (lba as u64) & !7;
        let offset = ((lba as u64) & 7) as usize * 512;
        let data = self.load_page(page_lba, dev_tag, dev)?;
        Ok(&data[offset..offset + 512])
    }

    pub fn get_sector_mut(&mut self, lba: u32, dev_tag: u64, dev: &mut dyn BlockDevice) -> Result<&mut [u8], ()> {
        let page_lba = (lba as u64) & !7;
        let sector_bit = 1u8 << ((lba as u64) & 7);
        let offset = ((lba as u64) & 7) as usize * 512;

        let slot_id = self.get_or_load_slot(page_lba, dev_tag, 0, dev)?;
        let slot = &mut self.slots[slot_id as usize];
        slot.dirty_sectors |= sector_bit;
        if !slot.dirty {
            slot.dirty = true;
            self.dirty_count += 1;
            slot.dirty_since_tick = self.counter;
        }
        Ok(&mut slot.data[offset..offset + 512])
    }

    pub fn mark_dirty_sector(&mut self, lba: u32) {
        let page_lba = (lba as u64) & !7;
        let sector_bit = 1u8 << ((lba as u64) & 7);
        if let Some(slot_id) = self.hash.get(page_lba) {
            let slot = &mut self.slots[slot_id as usize];
            if slot.valid {
                slot.dirty_sectors |= sector_bit;
                if !slot.dirty {
                    slot.dirty = true;
                    self.dirty_count += 1;
                    slot.dirty_since_tick = self.counter;
                }
            }
        }
    }

    // ── Page-level API (file data) ─────────────────────────────────

    pub fn read_page(
        &mut self,
        drive_id: u8,
        inode: u32,
        block_num: u32,
        data_lba: u64,
        dev_tag: u64,
        dev: &mut dyn BlockDevice,
    ) -> Result<&[u8; 4096], ()> {
        self.counter = self.counter.wrapping_add(1);
        let key = data_lba;
        let slot_id = if let Some(id) = self.hash.get(key) {
            if self.slots[id as usize].valid && self.slots[id as usize].dev_tag == dev_tag {
                self.hits += 1;
                self.move_to_head(id);
                self.update_readahead(inode, block_num);
                id
            } else {
                self.misses += 1;
                self.evict_slot(id, dev);
                let id = self.evict_lru(dev)?;
                self.populate_slot(id, key, dev_tag, make_inode_key(drive_id, inode, block_num), dev)?;
                id
            }
        } else {
            self.misses += 1;
            let id = self.evict_lru(dev)?;
            self.populate_slot(id, key, dev_tag, make_inode_key(drive_id, inode, block_num), dev)?;
            id
        };
        self.update_readahead(inode, block_num);
        Ok(&self.slots[slot_id as usize].data)
    }

    pub fn get_page_mut(
        &mut self,
        drive_id: u8,
        inode: u32,
        block_num: u32,
        data_lba: u64,
        dev_tag: u64,
        dev: &mut dyn BlockDevice,
    ) -> Result<&mut [u8; 4096], ()> {
        self.counter = self.counter.wrapping_add(1);
        let key = data_lba;
        let slot_id = if let Some(id) = self.hash.get(key) {
            if self.slots[id as usize].valid && self.slots[id as usize].dev_tag == dev_tag {
                self.hits += 1;
                self.move_to_head(id);
                id
            } else {
                self.misses += 1;
                self.evict_slot(id, dev);
                let id = self.evict_lru(dev)?;
                self.populate_slot(id, key, dev_tag, make_inode_key(drive_id, inode, block_num), dev)?;
                id
            }
        } else {
            self.misses += 1;
            let id = self.evict_lru(dev)?;
            self.populate_slot(id, key, dev_tag, make_inode_key(drive_id, inode, block_num), dev)?;
            id
        };
        self.update_readahead(inode, block_num);
        let slot = &mut self.slots[slot_id as usize];
        slot.dirty_sectors = 0xFF;
        if !slot.dirty {
            slot.dirty = true;
            self.dirty_count += 1;
            slot.dirty_since_tick = self.counter;
        }
        Ok(&mut slot.data)
    }

    // ── Cache-only operations (no disk I/O) ────────────────────────

    pub fn peek(&self, lba: u64) -> Option<&[u8; 4096]> {
        if let Some(slot_id) = self.hash.get(lba) {
            let slot = &self.slots[slot_id as usize];
            if slot.valid {
                return Some(&slot.data);
            }
        }
        None
    }

    /// Legacy peek by inode key — scans linearly if needed.
    pub fn peek_inode(&self, drive_id: u8, inode: u32, block_num: u32) -> Option<&[u8; 4096]> {
        let target_key = make_inode_key(drive_id, inode, block_num);
        for slot in &self.slots {
            if slot.valid && slot.inode_key == target_key {
                return Some(&slot.data);
            }
        }
        None
    }

    /// Mark a page dirty (by inode key) — only works if page is in cache.
    pub fn mark_dirty(&mut self, drive_id: u8, inode: u32, block_num: u32) {
        let target_key = make_inode_key(drive_id, inode, block_num);
        for slot in &mut self.slots {
            if slot.valid && slot.inode_key == target_key {
                slot.dirty_sectors = 0xFF;
                if !slot.dirty {
                    slot.dirty = true;
                    slot.dirty_since_tick = self.counter;
                    self.dirty_count += 1;
                }
                return;
            }
        }
    }

    // ── Flush ──────────────────────────────────────────────────────

    pub fn flush(&mut self, dev: &mut dyn BlockDevice) -> Result<(), ()> {
        for i in 0..CACHE_SIZE {
            if self.slots[i].valid && self.slots[i].dirty && !self.slots[i].write_pending {
                let lba = self.slots[i].lba;
                let dirty = self.slots[i].dirty_sectors;
                let data = &self.slots[i].data;
                for s in 0..8u8 {
                    if (dirty >> s) & 1 != 0 {
                        let offset = (s as usize) * 512;
                        let mut sector = [0u8; 512];
                        sector.copy_from_slice(&data[offset..offset + 512]);
                        dev.write_sector((lba + s as u64) as u64, &sector)?;
                    }
                }
                self.slots[i].dirty = false;
                self.slots[i].dirty_sectors = 0;
                self.dirty_count = self.dirty_count.saturating_sub(1);
                self.dirty_flushes += 1;
            }
        }
        Ok(())
    }

    pub fn flush_inode(&mut self, drive_id: u8, inode: u32, dev: &mut dyn BlockDevice) -> Result<(), ()> {
        for i in 0..CACHE_SIZE {
            if self.slots[i].valid && self.slots[i].dirty && !self.slots[i].write_pending {
                let k = self.slots[i].inode_key;
                if k == 0 { continue; }
                let slot_drive = (k >> 56) as u8;
                let slot_inode = ((k >> 32) as u32) & 0x00FF_FFFF;
                if slot_drive == drive_id && slot_inode == inode {
                    let lba = self.slots[i].lba;
                    let dirty = self.slots[i].dirty_sectors;
                    let data = &self.slots[i].data;
                    for s in 0..8u8 {
                        if (dirty >> s) & 1 != 0 {
                            let offset = (s as usize) * 512;
                            let mut sector = [0u8; 512];
                            sector.copy_from_slice(&data[offset..offset + 512]);
                            dev.write_sector((lba + s as u64) as u64, &sector)?;
                        }
                    }
                    self.slots[i].dirty = false;
                    self.slots[i].dirty_sectors = 0;
                    self.dirty_count = self.dirty_count.saturating_sub(1);
                    self.dirty_flushes += 1;
                }
            }
        }
        Ok(())
    }

    // ── Invalidation ───────────────────────────────────────────────

    pub fn invalidate_inode(&mut self, drive_id: u8, inode: u32) {
        for i in 0..CACHE_SIZE {
            if self.slots[i].valid {
                let k = self.slots[i].inode_key;
                if k == 0 { continue; }
                let slot_drive = (k >> 56) as u8;
                let slot_inode = ((k >> 32) as u32) & 0x00FF_FFFF;
                if slot_drive == drive_id && slot_inode == inode {
                    if self.slots[i].dirty {
                        self.dirty_count = self.dirty_count.saturating_sub(1);
                    }
                    let lba = self.slots[i].lba;
                    self.unlink_lru(i as u16);
                    self.hash.remove(lba);
                    self.slots[i] = EMPTY_SLOT;
                }
            }
        }
    }

    /// Invalidate cached pages that overlap with [start_lba, end_lba).
    /// Direct writes (B-tree nodes) go through raw device; we must evict
    /// stale page cache entries so future reads get the fresh data.
    pub fn invalidate_range(&mut self, start_lba: u64, end_lba: u64) {
        let page_start = start_lba & !7;
        let page_end = end_lba.saturating_sub(1) & !7;
        let mut i = 0usize;
        while i < CACHE_SIZE {
            if self.slots[i].valid && self.slots[i].lba >= page_start && self.slots[i].lba <= page_end {
                if self.slots[i].dirty {
                    self.dirty_count = self.dirty_count.saturating_sub(1);
                }
                let lba = self.slots[i].lba;
                self.unlink_lru(i as u16);
                self.hash.remove(lba);
                self.slots[i] = EMPTY_SLOT;
            } else {
                i += 1;
            }
        }
    }

    /// Invalidate every cached page belonging to `dev_tag`. Used when a block
    /// device is (re-)registered at an index whose previous cache must not be
    /// handed to the new device.
    pub fn invalidate_device(&mut self, dev_tag: u64) {
        let mut i = 0usize;
        while i < CACHE_SIZE {
            if self.slots[i].valid && self.slots[i].dev_tag == dev_tag {
                if self.slots[i].dirty {
                    self.dirty_count = self.dirty_count.saturating_sub(1);
                }
                let lba = self.slots[i].lba;
                self.unlink_lru(i as u16);
                self.hash.remove(lba);
                self.slots[i] = EMPTY_SLOT;
            } else {
                i += 1;
            }
        }
    }

    // ── Stats ──────────────────────────────────────────────────────

    pub fn entry_count(&self) -> usize {
        self.hash.len
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty_count as usize
    }

    pub fn stats(&self) -> CacheStats {
        let mut valid = 0;
        let mut dirty = 0;
        let mut pending = 0;
        for i in 0..CACHE_SIZE {
            if self.slots[i].valid {
                valid += 1;
                if self.slots[i].dirty {
                    dirty += 1;
                }
                if self.slots[i].write_pending {
                    pending += 1;
                }
            }
        }
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            dirty_flushes: self.dirty_flushes,
            readahead_hits: self.readahead_hits,
            current_entries: valid,
            dirty_count: dirty,
            pending_writes: pending,
            hash_table_len: self.hash.len,
            hash_table_capacity: CACHE_SIZE,
        }
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64 * 100.0
        }
    }

    pub fn pending_write_count(&self) -> usize {
        let mut count = 0;
        for i in 0..CACHE_SIZE {
            if self.slots[i].write_pending {
                count += 1;
            }
        }
        count
    }

    pub fn capacity(&self) -> usize {
        CACHE_SIZE
    }

    pub fn max_capacity(&self) -> usize {
        MAX_CACHE_SIZE
    }

    pub fn min_capacity(&self) -> usize {
        MIN_CACHE_SIZE
    }

    pub fn flush_batch(&mut self, dev: &mut dyn BlockDevice, max_flush: usize) -> usize {
        let mut flushed = 0;
        for i in 0..CACHE_SIZE {
            if flushed >= max_flush {
                break;
            }
            if self.slots[i].valid && self.slots[i].dirty && !self.slots[i].write_pending {
                let lba = self.slots[i].lba;
                let dirty = self.slots[i].dirty_sectors;
                let data = self.slots[i].data;
                let mut ok = true;
                for s in 0..8u8 {
                    if (dirty >> s) & 1 != 0 {
                        let offset = (s as usize) * 512;
                        let mut sector = [0u8; 512];
                        sector.copy_from_slice(&data[offset..offset + 512]);
                        if dev.write_sector((lba + s as u64) as u64, &sector).is_err() {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    self.slots[i].dirty = false;
                    self.slots[i].dirty_sectors = 0;
                    self.slots[i].write_pending = false;
                    self.dirty_count = self.dirty_count.saturating_sub(1);
                    self.dirty_flushes += 1;
                    flushed += 1;
                }
            }
        }
        flushed
    }

    pub fn needs_async_flush(&self) -> bool {
        let threshold = (CACHE_SIZE as f32 * 0.10) as u16;
        self.dirty_count >= threshold
    }

    pub fn prefetch(
        &mut self,
        drive_id: u8,
        inode: u32,
        start_block: u32,
        count: u32,
        dev: &mut dyn BlockDevice,
    ) {
        let _ = (drive_id, inode, start_block, count, dev);
    }

    // ── Internal: slot management ────────────────────────────────

    fn load_page(&mut self, lba: u64, dev_tag: u64, dev: &mut dyn BlockDevice) -> Result<&[u8; 4096], ()> {
        self.counter = self.counter.wrapping_add(1);
        if let Some(slot_id) = self.hash.get(lba) {
            if self.slots[slot_id as usize].valid && self.slots[slot_id as usize].dev_tag == dev_tag {
                self.hits += 1;
                self.move_to_head(slot_id);
                return Ok(&self.slots[slot_id as usize].data);
            }
            self.evict_slot(slot_id, dev);
        }
        self.misses += 1;
        let slot_id = self.evict_lru(dev)?;
        self.populate_slot(slot_id, lba, dev_tag, 0, dev)?;
        Ok(&self.slots[slot_id as usize].data)
    }

    fn get_or_load_slot(&mut self, lba: u64, dev_tag: u64, inode_key: u64, dev: &mut dyn BlockDevice) -> Result<u16, ()> {
        self.counter = self.counter.wrapping_add(1);
        if let Some(slot_id) = self.hash.get(lba) {
            if self.slots[slot_id as usize].valid && self.slots[slot_id as usize].dev_tag == dev_tag {
                self.hits += 1;
                self.move_to_head(slot_id);
                return Ok(slot_id);
            }
            self.evict_slot(slot_id, dev);
        }
        self.misses += 1;
        let slot_id = self.evict_lru(dev)?;
        self.populate_slot(slot_id, lba, dev_tag, inode_key, dev)?;
        Ok(slot_id)
    }

    /// Evict a specific slot (flushing it first if dirty). Used when the cached
    /// page belongs to a different device than the one being accessed.
    fn evict_slot(&mut self, slot_id: u16, dev: &mut dyn BlockDevice) {
        let i = slot_id as usize;
        if !self.slots[i].valid { return; }
        let (lba, dirty, tmp) = {
            let s = &self.slots[i];
            (s.lba, s.dirty_sectors, s.data)
        };
        if self.slots[i].dirty && !self.slots[i].write_pending {
            for s in 0..8u8 {
                if (dirty >> s) & 1 != 0 {
                    let offset = (s as usize) * 512;
                    let mut sector = [0u8; 512];
                    sector.copy_from_slice(&tmp[offset..offset + 512]);
                    let _ = dev.write_sector(lba + s as u64, &sector);
                }
            }
            self.dirty_count = self.dirty_count.saturating_sub(1);
        }
        self.unlink_lru(slot_id);
        self.hash.remove(lba);
        self.slots[i] = EMPTY_SLOT;
    }

    fn evict_lru(&mut self, dev: &mut dyn BlockDevice) -> Result<u16, ()> {
        for i in 0..CACHE_SIZE {
            if !self.slots[i].valid {
                return Ok(i as u16);
            }
        }
        if let Some(tail) = self.lru_tail {
            let slot = &self.slots[tail as usize];
            if slot.valid && slot.dirty && !slot.write_pending {
                let lba = slot.lba;
                let dirty = slot.dirty_sectors;
                let tmp = slot.data;
                let _ = slot;
                for s in 0..8u8 {
                    if (dirty >> s) & 1 != 0 {
                        let offset = (s as usize) * 512;
                        let mut sector = [0u8; 512];
                        sector.copy_from_slice(&tmp[offset..offset + 512]);
                        let _ = dev.write_sector((lba + s as u64) as u64, &sector);
                    }
                }
                self.dirty_count = self.dirty_count.saturating_sub(1);
                self.evictions += 1;
            }
            let lba = self.slots[tail as usize].lba;
            self.unlink_lru(tail);
            self.hash.remove(lba);
            self.slots[tail as usize] = EMPTY_SLOT;
            Ok(tail)
        } else {
            for i in 0..CACHE_SIZE {
                if !self.slots[i].valid {
                    return Ok(i as u16);
                }
            }
            Ok(0)
        }
    }

    fn populate_slot(
        &mut self,
        slot_id: u16,
        lba: u64,
        dev_tag: u64,
        inode_key: u64,
        dev: &mut dyn BlockDevice,
    ) -> Result<(), ()> {
        let mut tmp = [0u8; 4096];
        for s in 0..8u64 {
            let offset = (s as usize) * 512;
            let sector = dev.read_sector((lba + s) as u64)?;
            tmp[offset..offset + 512].copy_from_slice(&sector);
        }
        self.slots[slot_id as usize] = CacheSlot {
            valid: true,
            dirty: false,
            write_pending: false,
            lba,
            dev_tag,
            dirty_sectors: 0,
            inode_key,
            dirty_since_tick: 0,
            data: tmp,
            lru_prev: None,
            lru_next: None,
        };
        self.hash.insert(lba, slot_id);
        self.move_to_head(slot_id);
        Ok(())
    }

    // ── Internal: LRU doubly-linked list ─────────────────────────

    fn move_to_head(&mut self, id: u16) {
        if self.lru_head == Some(id) {
            return;
        }
        self.unlink_lru(id);
        self.slots[id as usize].lru_prev = None;
        self.slots[id as usize].lru_next = self.lru_head;
        if let Some(old_head) = self.lru_head {
            self.slots[old_head as usize].lru_prev = Some(id);
        }
        self.lru_head = Some(id);
        if self.lru_tail.is_none() {
            self.lru_tail = Some(id);
        }
    }

    fn unlink_lru(&mut self, id: u16) {
        let prev = self.slots[id as usize].lru_prev;
        let next = self.slots[id as usize].lru_next;

        if let Some(p) = prev {
            self.slots[p as usize].lru_next = next;
        } else if self.lru_head == Some(id) {
            self.lru_head = next;
        }

        if let Some(n) = next {
            self.slots[n as usize].lru_prev = prev;
        } else if self.lru_tail == Some(id) {
            self.lru_tail = prev;
        }

        self.slots[id as usize].lru_prev = None;
        self.slots[id as usize].lru_next = None;
    }

    // ── Internal: readahead tracking ─────────────────────────────

    fn update_readahead(&mut self, inode: u32, block_num: u32) {
        let idx = self.find_readahead_slot(inode);
        if idx >= self.readahead.len() { return; }
        let state = &mut self.readahead[idx];

        if state.last_block < 0 {
            state.last_block = block_num as i64;
            state.consecutive_count = 1;
            state.direction = 0;
            return;
        }

        let diff = block_num as i64 - state.last_block;

        if diff == 1 && (state.direction == 1 || state.direction == 0) {
            state.direction = 1;
            state.consecutive_count += 1;
            if state.consecutive_count >= state.window_size {
                state.window_size = core::cmp::min(state.window_size * 2, 32);
            }
        } else if diff == -1 && (state.direction == -1 || state.direction == 0) {
            state.direction = -1;
            state.consecutive_count += 1;
            if state.consecutive_count >= state.window_size {
                state.window_size = core::cmp::min(state.window_size * 2, 32);
            }
        } else if diff != 0 {
            state.direction = diff.signum() as i8;
            state.consecutive_count = 1;
            state.window_size = 4;
        }

        state.last_block = block_num as i64;
    }

    fn find_readahead_slot(&self, _inode: u32) -> usize {
        0
    }
}
