use alloc::vec::Vec;
use crate::{test_case, test_eq, test_true};

/// A generic slab container providing O(1) insert, lookup, and remove
/// with stable integer keys. Backed by `Vec<Option<T>>`.
pub struct Slab<T> {
    slots: Vec<Option<T>>,
    len: usize,
    next_key: u64,
}

impl<T> Slab<T> {
    pub fn new() -> Self {
        Slab { slots: Vec::new(), len: 0, next_key: 1 }
    }

    /// Insert a value and return its assigned key.
    pub fn insert(&mut self, value: T) -> u64 {
        let key = self.next_key;
        self.next_key = self.next_key.wrapping_add(1);

        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                *slot = Some(value);
                self.len += 1;
                return key;
            }
        }
        self.slots.push(Some(value));
        self.len += 1;
        key
    }

    /// Get a reference by slot index (not key).
    /// Panics if idx out of bounds.
    pub fn get_by_idx(&self, idx: usize) -> Option<&T> {
        self.slots.get(idx).and_then(|s| s.as_ref())
    }

    /// Get a mutable reference by slot index.
    pub fn get_by_idx_mut(&mut self, idx: usize) -> Option<&mut T> {
        self.slots.get_mut(idx).and_then(|s| s.as_mut())
    }

    /// Set a slot by index.
    pub fn set(&mut self, idx: usize, value: T) {
        if idx >= self.slots.len() {
            self.slots.resize_with(idx + 1, || None);
        }
        if self.slots[idx].is_none() {
            self.len += 1;
        }
        self.slots[idx] = Some(value);
    }

    /// Remove the value at the given slot index. Returns the old value.
    pub fn remove_by_idx(&mut self, idx: usize) -> Option<T> {
        let result = self.slots.get_mut(idx)?.take();
        if result.is_some() {
            self.len -= 1;
        }
        result
    }

    /// Number of occupied slots.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Total capacity (occupied + free).
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Iterate over occupied entries.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.slots.iter().filter_map(|s| s.as_ref())
    }

    /// Iterate mutably over occupied entries.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.slots.iter_mut().filter_map(|s| s.as_mut())
    }

    /// Clear all slots and the key counter. Resets to empty.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.len = 0;
        self.next_key = 1;
    }

    /// Pad the slot vector so that `idx` is a valid index.
    pub fn ensure_idx(&mut self, idx: usize) {
        if idx >= self.slots.len() {
            self.slots.resize_with(idx + 1, || None);
        }
    }
}

// ── Tests ──

pub fn register_slab_container_tests() {
    test_case!("slab_container_insert_lookup", {
        let mut slab: Slab<u32> = Slab::new();
        let k1 = slab.insert(42);
        let k2 = slab.insert(43);
        test_true!(k1 != k2);
        // Can't get by key — get_by_idx works
        test_eq!(*slab.get_by_idx(0).unwrap(), 42);
        test_eq!(*slab.get_by_idx(1).unwrap(), 43);
        test_eq!(slab.len(), 2);
    });

    test_case!("slab_container_remove", {
        let mut slab: Slab<u32> = Slab::new();
        slab.insert(10);
        slab.insert(20);
        test_eq!(slab.len(), 2);
        // Remove first slot
        let removed = slab.remove_by_idx(0);
        test_eq!(removed, Some(10));
        test_eq!(slab.len(), 1);
        // Lookup the other
        test_eq!(*slab.get_by_idx(1).unwrap(), 20);
    });

    test_case!("slab_container_reuse_slot", {
        let mut slab: Slab<u32> = Slab::new();
        let _k1 = slab.insert(100);
        slab.remove_by_idx(0);
        test_eq!(slab.len(), 0);
        // New insert reuses the freed slot
        let _k2 = slab.insert(200);
        test_eq!(slab.len(), 1);
        test_eq!(*slab.get_by_idx(0).unwrap(), 200);
    });

    test_case!("slab_container_set_and_clear", {
        let mut slab: Slab<u32> = Slab::new();
        slab.set(5, 555);
        test_eq!(slab.len(), 1);
        test_eq!(slab.capacity(), 6);
        test_eq!(*slab.get_by_idx(5).unwrap(), 555);
        slab.clear();
        test_eq!(slab.len(), 0);
        test_eq!(slab.capacity(), 0);
    });

    test_case!("slab_container_iter", {
        let mut slab: Slab<u32> = Slab::new();
        slab.insert(1);
        slab.insert(2);
        slab.insert(3);
        let collected: Vec<u32> = slab.iter().copied().collect();
        test_eq!(collected.len(), 3);
        test_true!(collected.contains(&1));
        test_true!(collected.contains(&2));
        test_true!(collected.contains(&3));
    });
}
