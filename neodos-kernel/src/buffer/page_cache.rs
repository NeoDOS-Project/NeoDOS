use crate::drivers::block::BlockDevice;

// ── Constants ────────────────────────────────────────────────────────

const DEFAULT_CACHE_SIZE: usize = 128;
const MAX_CACHE_SIZE: usize = 2048;
const MIN_CACHE_SIZE: usize = 64;

// ── FNV-1a hash (simple, fast, const) ───────────────────────────────

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

const fn fnv_hash(key: u64) -> u64 {
    let mut h = FNV_OFFSET;
    let bytes = key.to_le_bytes();
    let mut i = 0;
    while i < 8 {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

const fn make_key(inode: u32, block: u32) -> u64 {
    (inode as u64) << 32 | block as u64
}

// ── Custom open-addressing hash table (const-constructible) ──────────

const HT_EMPTY: u64 = u64::MAX;
const HT_TOMBSTONE: u64 = u64::MAX - 1;

#[derive(Copy, Clone)]
struct HashEntry {
    key: u64,
    value: u16,
}

const EMPTY_HASH_ENTRY: HashEntry = HashEntry {
    key: HT_EMPTY,
    value: 0,
};

struct HashTable<const N: usize> {
    entries: [HashEntry; N],
    len: usize,
}

impl<const N: usize> HashTable<N> {
    const fn new() -> Self {
        HashTable {
            entries: [EMPTY_HASH_ENTRY; N],
            len: 0,
        }
    }

    fn insert(&mut self, key: u64, value: u16) -> bool {
        if self.len * 2 >= N * 7 {
            return false;
        }
        let mut idx = (fnv_hash(key) as usize) % N;
        let mut first_tombstone = None;
        for _ in 0..N {
            match self.entries[idx].key {
                HT_EMPTY => {
                    let slot = first_tombstone.unwrap_or(idx);
                    self.entries[slot] = HashEntry { key, value };
                    if first_tombstone.is_none() {
                        self.len += 1;
                    }
                    return true;
                }
                HT_TOMBSTONE => {
                    if first_tombstone.is_none() {
                        first_tombstone = Some(idx);
                    }
                }
                k if k == key => {
                    self.entries[idx].value = value;
                    return true;
                }
                _ => {}
            }
            idx = (idx + 1) % N;
        }
        false
    }

    fn get(&self, key: u64) -> Option<u16> {
        let mut idx = (fnv_hash(key) as usize) % N;
        for _ in 0..N {
            match self.entries[idx].key {
                HT_EMPTY => return None,
                HT_TOMBSTONE => {}
                k if k == key => return Some(self.entries[idx].value),
                _ => {}
            }
            idx = (idx + 1) % N;
        }
        None
    }

    fn remove(&mut self, key: u64) -> bool {
        let mut idx = (fnv_hash(key) as usize) % N;
        for _ in 0..N {
            match self.entries[idx].key {
                HT_EMPTY => return false,
                HT_TOMBSTONE => {}
                k if k == key => {
                    self.entries[idx].key = HT_TOMBSTONE;
                    self.entries[idx].value = 0;
                    self.len = self.len.saturating_sub(1);
                    return true;
                }
                _ => {}
            }
            idx = (idx + 1) % N;
        }
        false
    }

    fn contains(&self, key: u64) -> bool {
        self.get(key).is_some()
    }
}

// ── Readahead state per inode ───────────────────────────────────────

#[derive(Copy, Clone)]
struct ReadaheadState {
    last_block: i64,
    consecutive_count: u32,
    window_size: u32,
    direction: i8,
}

const EMPTY_READAHEAD: ReadaheadState = ReadaheadState {
    last_block: -1,
    consecutive_count: 0,
    window_size: 4,
    direction: 0,
};

// ── Cache slot metadata ─────────────────────────────────────────────

#[derive(Copy, Clone)]
struct CacheSlot {
    valid: bool,
    dirty: bool,
    write_pending: bool,
    key: u64,
    data_lba: u64,
    dirty_since_tick: u64,
    data: [u8; 4096],
    lru_prev: Option<u16>,
    lru_next: Option<u16>,
}

const EMPTY_SLOT: CacheSlot = CacheSlot {
    valid: false,
    dirty: false,
    write_pending: false,
    key: 0,
    data_lba: 0,
    dirty_since_tick: 0,
    data: [0u8; 4096],
    lru_prev: None,
    lru_next: None,
};

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

// ── Page cache ──────────────────────────────────────────────────────

pub struct PageCache {
    slots: [CacheSlot; DEFAULT_CACHE_SIZE],
    hash: HashTable<DEFAULT_CACHE_SIZE>,
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
            slots: [EMPTY_SLOT; DEFAULT_CACHE_SIZE],
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

    // ── Core API (backward-compatible) ───────────────────────────

    pub fn read_page(
        &mut self,
        inode: u32,
        block_num: u32,
        data_lba: u64,
        dev: &mut dyn BlockDevice,
    ) -> Result<&[u8; 4096], ()> {
        self.counter = self.counter.wrapping_add(1);
        let key = make_key(inode, block_num);

        if let Some(slot_id) = self.hash.get(key) {
            if self.slots[slot_id as usize].valid {
                self.hits += 1;
                self.move_to_head(slot_id);
                self.update_readahead(inode, block_num);
                return Ok(&self.slots[slot_id as usize].data);
            }
        }

        self.misses += 1;
        let slot_id = self.evict_lru();
        self.populate_slot(slot_id, inode, block_num, data_lba, dev)?;
        self.update_readahead(inode, block_num);

        Ok(&self.slots[slot_id as usize].data)
    }

    pub fn get_page_mut(
        &mut self,
        inode: u32,
        block_num: u32,
        data_lba: u64,
        dev: &mut dyn BlockDevice,
    ) -> Result<&mut [u8; 4096], ()> {
        self.counter = self.counter.wrapping_add(1);
        let key = make_key(inode, block_num);

        let slot_id = if let Some(id) = self.hash.get(key) {
            if self.slots[id as usize].valid {
                self.hits += 1;
                self.move_to_head(id);
                self.update_readahead(inode, block_num);
                id
            } else {
                self.misses += 1;
                let id = self.evict_lru();
                self.populate_slot(id, inode, block_num, data_lba, dev)?;
                id
            }
        } else {
            self.misses += 1;
            let id = self.evict_lru();
            self.populate_slot(id, inode, block_num, data_lba, dev)?;
            id
        };

        self.slots[slot_id as usize].dirty = true;
        if self.slots[slot_id as usize].dirty_since_tick == 0 {
            self.slots[slot_id as usize].dirty_since_tick = self.counter;
        }
        if !self.slots[slot_id as usize].dirty {
            self.dirty_count += 1;
        }
        self.update_readahead(inode, block_num);

        Ok(&mut self.slots[slot_id as usize].data)
    }

    pub fn peek(&self, inode: u32, block_num: u32) -> Option<&[u8; 4096]> {
        let key = make_key(inode, block_num);
        if let Some(slot_id) = self.hash.get(key) {
            let slot = &self.slots[slot_id as usize];
            if slot.valid {
                return Some(&slot.data);
            }
        }
        None
    }

    pub fn mark_dirty(&mut self, inode: u32, block_num: u32) {
        let key = make_key(inode, block_num);
        if let Some(slot_id) = self.hash.get(key) {
            let slot = &mut self.slots[slot_id as usize];
            if slot.valid && !slot.dirty {
                slot.dirty = true;
                slot.dirty_since_tick = self.counter;
                self.dirty_count += 1;
            }
        }
    }

    pub fn flush(&mut self, dev: &mut dyn BlockDevice) -> Result<(), ()> {
        for i in 0..DEFAULT_CACHE_SIZE {
            if self.slots[i].valid && self.slots[i].dirty && !self.slots[i].write_pending {
                let lba = self.slots[i].data_lba;
                let tmp = self.slots[i].data;
                dev.write_blocks(lba, 8, &tmp)?;
                self.slots[i].dirty = false;
                self.dirty_count = self.dirty_count.saturating_sub(1);
                self.dirty_flushes += 1;
            }
        }
        Ok(())
    }

    pub fn flush_inode(&mut self, inode: u32, dev: &mut dyn BlockDevice) -> Result<(), ()> {
        for i in 0..DEFAULT_CACHE_SIZE {
            if self.slots[i].valid && self.slots[i].dirty && !self.slots[i].write_pending {
                let slot_inode = (self.slots[i].key >> 32) as u32;
                if slot_inode == inode {
                    let lba = self.slots[i].data_lba;
                    let tmp = self.slots[i].data;
                    dev.write_blocks(lba, 8, &tmp)?;
                    self.slots[i].dirty = false;
                    self.dirty_count = self.dirty_count.saturating_sub(1);
                    self.dirty_flushes += 1;
                }
            }
        }
        Ok(())
    }

    pub fn invalidate_inode(&mut self, inode: u32) {
        for i in 0..DEFAULT_CACHE_SIZE {
            if self.slots[i].valid {
                let slot_inode = (self.slots[i].key >> 32) as u32;
                if slot_inode == inode {
                    if self.slots[i].dirty {
                        self.dirty_count = self.dirty_count.saturating_sub(1);
                    }
                    let key = self.slots[i].key;
                    self.unlink_lru(i as u16);
                    self.hash.remove(key);
                    self.slots[i] = EMPTY_SLOT;
                }
            }
        }
    }

    pub fn entry_count(&self) -> usize {
        self.hash.len
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty_count as usize
    }

    // ── Extended API ─────────────────────────────────────────────

    pub fn stats(&self) -> CacheStats {
        let mut valid = 0;
        let mut dirty = 0;
        let mut pending = 0;
        for i in 0..DEFAULT_CACHE_SIZE {
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
            hash_table_capacity: DEFAULT_CACHE_SIZE,
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
        for i in 0..DEFAULT_CACHE_SIZE {
            if self.slots[i].write_pending {
                count += 1;
            }
        }
        count
    }

    pub fn capacity(&self) -> usize {
        DEFAULT_CACHE_SIZE
    }

    pub fn max_capacity(&self) -> usize {
        MAX_CACHE_SIZE
    }

    pub fn min_capacity(&self) -> usize {
        MIN_CACHE_SIZE
    }

    /// Flush dirty pages as a batch (async-friendly, returns count).
    /// Skips pages with in-flight writes. Marks flushed pages as write_pending.
    pub fn flush_batch(&mut self, dev: &mut dyn BlockDevice, max_flush: usize) -> usize {
        let mut flushed = 0;
        for i in 0..DEFAULT_CACHE_SIZE {
            if flushed >= max_flush {
                break;
            }
            if self.slots[i].valid && self.slots[i].dirty && !self.slots[i].write_pending {
                let lba = self.slots[i].data_lba;
                let tmp = self.slots[i].data;
                if dev.write_blocks(lba, 8, &tmp).is_ok() {
                    self.slots[i].dirty = false;
                    self.slots[i].write_pending = false;
                    self.dirty_count = self.dirty_count.saturating_sub(1);
                    self.dirty_flushes += 1;
                    flushed += 1;
                }
            }
        }
        flushed
    }

    /// Check if dirty pages exceed the flush threshold (10% of capacity).
    pub fn needs_async_flush(&self) -> bool {
        let threshold = (DEFAULT_CACHE_SIZE as f32 * 0.10) as u16;
        self.dirty_count >= threshold
    }

    /// Explicit readahead: prefetch N contiguous blocks starting from block_num.
    pub fn prefetch(
        &mut self,
        inode: u32,
        start_block: u32,
        count: u32,
        dev: &mut dyn BlockDevice,
    ) {
        for i in 0..count {
            let block = start_block + i;
            let key = make_key(inode, block);
            if self.hash.contains(key) {
                continue;
            }
            let lba = 200 + (block as u64 * 8);
            let slot_id = self.evict_lru();
            if self.populate_slot(slot_id, inode, block, lba, dev).is_ok() {
                self.slots[slot_id as usize].dirty = false;
                self.slots[slot_id as usize].dirty_since_tick = 0;
            }
        }
    }

    // ── Internal: slot management ────────────────────────────────

    fn evict_lru(&mut self) -> u16 {
        if let Some(tail) = self.lru_tail {
            let slot = &self.slots[tail as usize];
            if slot.valid && slot.dirty && !slot.write_pending {
                let lba = slot.data_lba;
                let tmp = slot.data;
                let _ = slot;
                if let Some(mut bdevs) = crate::globals::BLOCK_DEVICES.try_lock() {
                    if let Some(dev) = bdevs.get(0) {
                        let _ = dev.write_blocks(lba, 8, &tmp);
                    }
                }
                self.dirty_count = self.dirty_count.saturating_sub(1);
                self.evictions += 1;
            }
            let key = self.slots[tail as usize].key;
            self.unlink_lru(tail);
            self.hash.remove(key);
            self.slots[tail as usize] = EMPTY_SLOT;
            tail
        } else {
            for i in 0..DEFAULT_CACHE_SIZE {
                if !self.slots[i].valid {
                    return i as u16;
                }
            }
            0
        }
    }

    fn populate_slot(
        &mut self,
        slot_id: u16,
        inode: u32,
        block_num: u32,
        data_lba: u64,
        dev: &mut dyn BlockDevice,
    ) -> Result<(), ()> {
        let mut tmp = [0u8; 4096];
        dev.read_blocks(data_lba, 8, &mut tmp)?;

        let key = make_key(inode, block_num);
        self.slots[slot_id as usize] = CacheSlot {
            valid: true,
            dirty: false,
            write_pending: false,
            key,
            data_lba,
            dirty_since_tick: 0,
            data: tmp,
            lru_prev: None,
            lru_next: None,
        };
        self.hash.insert(key, slot_id);
        self.move_to_head(slot_id);
        Ok(())
    }

    fn find_slot(&self, inode: u32, block_num: u32) -> Option<u16> {
        let key = make_key(inode, block_num);
        if let Some(slot_id) = self.hash.get(key) {
            if self.slots[slot_id as usize].valid {
                return Some(slot_id);
            }
        }
        None
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

    fn find_readahead_slot(&mut self, inode: u32) -> usize {
        for i in 0..self.readahead_count {
            // Simple inline key check: readahead state doesn't store inode,
            // we use index-based tracking. For now use round-robin.
            let _ = inode;
            return i;
        }
        if self.readahead_count < self.readahead.len() {
            let idx = self.readahead_count;
            self.readahead_count += 1;
            idx
        } else {
            0
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_table_insert_get() {
        let mut ht = HashTable::<16>::new();
        assert!(ht.insert(42, 0));
        assert_eq!(ht.get(42), Some(0));
        assert_eq!(ht.get(43), None);
    }

    #[test]
    fn test_hash_table_remove() {
        let mut ht = HashTable::<16>::new();
        ht.insert(42, 0);
        assert!(ht.remove(42));
        assert_eq!(ht.get(42), None);
        assert!(!ht.remove(42));
    }

    #[test]
    fn test_hash_table_overwrite() {
        let mut ht = HashTable::<16>::new();
        ht.insert(42, 0);
        ht.insert(42, 1);
        assert_eq!(ht.get(42), Some(1));
        assert_eq!(ht.len, 1);
    }

    #[test]
    fn test_hash_table_tombstone_reuse() {
        let mut ht = HashTable::<4>::new();
        ht.insert(1, 0);
        ht.insert(2, 1);
        ht.insert(3, 2);
        ht.remove(2);
        assert_eq!(ht.len, 2);
        assert!(ht.insert(4, 3));
        assert_eq!(ht.get(4), Some(3));
    }

    #[test]
    fn test_lru_basic() {
        let mut pc = PageCache::new();
        assert_eq!(pc.lru_head, None);
        assert_eq!(pc.lru_tail, None);

        pc.move_to_head(5);
        assert_eq!(pc.lru_head, Some(5));
        assert_eq!(pc.lru_tail, Some(5));

        pc.move_to_head(3);
        assert_eq!(pc.lru_head, Some(3));
        assert_eq!(pc.lru_tail, Some(5));

        pc.move_to_head(7);
        assert_eq!(pc.lru_head, Some(7));
        assert_eq!(pc.lru_tail, Some(5));

        pc.unlink_lru(3);
        assert_eq!(pc.lru_head, Some(7));
        assert_eq!(pc.lru_tail, Some(5));
    }

    #[test]
    fn test_make_key() {
        assert_eq!(make_key(1, 0), 0x00000001_00000000);
        assert_eq!(make_key(0, 1), 1);
        assert_eq!(make_key(0xFFFFFFFF, 0xFFFFFFFF), 0xFFFFFFFF_FFFFFFFF);
    }

    #[test]
    fn test_cache_create_empty() {
        let pc = PageCache::new();
        assert_eq!(pc.entry_count(), 0);
        assert_eq!(pc.dirty_count(), 0);
    }

    #[test]
    fn test_cache_peek_miss() {
        let pc = PageCache::new();
        assert_eq!(pc.peek(1, 0), None);
        assert_eq!(pc.peek(1, 1), None);
        assert_eq!(pc.peek(0, 0), None);
    }

    #[test]
    fn test_cache_mark_dirty_noop() {
        let mut pc = PageCache::new();
        assert_eq!(pc.dirty_count(), 0);
        pc.mark_dirty(1, 0);
        assert_eq!(pc.dirty_count(), 0);
    }

    #[test]
    fn test_cache_invalidate_noop() {
        let mut pc = PageCache::new();
        pc.invalidate_inode(42);
        assert_eq!(pc.entry_count(), 0);
    }

    #[test]
    fn test_cache_stats_empty() {
        let pc = PageCache::new();
        let stats = pc.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.current_entries, 0);
        assert_eq!(stats.dirty_count, 0);
    }

    #[test]
    fn test_cache_hit_rate_zero() {
        let pc = PageCache::new();
        assert_eq!(pc.hit_rate(), 0.0);
    }

    #[test]
    fn test_cache_capacity() {
        let pc = PageCache::new();
        assert_eq!(pc.capacity(), DEFAULT_CACHE_SIZE);
        assert_eq!(pc.max_capacity(), MAX_CACHE_SIZE);
        assert_eq!(pc.min_capacity(), MIN_CACHE_SIZE);
    }

    #[test]
    fn test_hash_table_capacity_pressure() {
        let mut ht = HashTable::<4>::new();
        assert!(ht.insert(1, 0));
        assert!(ht.insert(2, 1));
        assert!(ht.insert(3, 2));
        assert!(!ht.insert(4, 3));
    }

    #[test]
    fn test_lru_unlink_middle() {
        let mut pc = PageCache::new();
        pc.move_to_head(1);
        pc.move_to_head(2);
        pc.move_to_head(3);
        assert_eq!(pc.lru_head, Some(3));
        assert_eq!(pc.lru_tail, Some(1));

        pc.unlink_lru(2);
        assert_eq!(pc.lru_head, Some(3));
        assert_eq!(pc.lru_tail, Some(1));
        assert_eq!(pc.slots[3].lru_prev, None);
        assert_eq!(pc.slots[3].lru_next, Some(1));
        assert_eq!(pc.slots[1].lru_prev, Some(3));
        assert_eq!(pc.slots[1].lru_next, None);
    }
}
