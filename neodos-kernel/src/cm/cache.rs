use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Cell cache — simple BTreeMap-based cache for recently accessed cells.
/// In a full implementation this would be an LRU cache with eviction.
pub struct CellCache {
    entries: BTreeMap<u32, CachedCell>,
    max_entries: usize,
    hits: u64,
    misses: u64,
}

pub struct CachedCell {
    pub data: Vec<u8>,
    pub dirty: bool,
}

impl CellCache {
    pub fn new(max: usize) -> Self {
        CellCache {
            entries: BTreeMap::new(),
            max_entries: max,
            hits: 0,
            misses: 0,
        }
    }

    pub fn lookup(&mut self, cell_idx: u32) -> Option<&CachedCell> {
        if self.entries.contains_key(&cell_idx) {
            self.hits += 1;
            self.entries.get(&cell_idx)
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn lookup_mut(&mut self, cell_idx: u32) -> Option<&mut CachedCell> {
        if self.entries.contains_key(&cell_idx) {
            self.hits += 1;
            self.entries.get_mut(&cell_idx)
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn insert(&mut self, cell_idx: u32, data: Vec<u8>) {
        if self.entries.len() >= self.max_entries {
            // Evict the first entry (simple FIFO eviction)
            if let Some(first) = self.entries.keys().next().copied() {
                self.entries.remove(&first);
            }
        }
        self.entries.insert(cell_idx, CachedCell { data, dirty: false });
    }

    pub fn mark_dirty(&mut self, cell_idx: u32) {
        if let Some(entry) = self.entries.get_mut(&cell_idx) {
            entry.dirty = true;
        }
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { self.hits as f64 / total as f64 }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
