//! Fixed-capacity slot table with distinguishable exhaustion.
//!
//! Several managers (timers, semaphores, sections, event handlers, ...) keep a
//! bounded array of optional entries and hand-roll the alloc/free loop. This
//! table centralizes that pattern and reports exhaustion explicitly instead of
//! returning `None`/`false`, so callers can map it to their own error type
//! (usually `ObError::TableFull`).
//!
//! The table is not synchronized: owners wrap it in their own lock.

/// Returned by [`SlotTable::alloc`] when no free slot is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotTableFull;

/// Bounded table of `N` optional slots.
pub struct SlotTable<T, const N: usize> {
    slots: [Option<T>; N],
    used: usize,
}

impl<T, const N: usize> SlotTable<T, N> {
    pub const fn new() -> Self {
        SlotTable {
            slots: [const { None }; N],
            used: 0,
        }
    }

    /// Store `value` in the first free slot and return its index.
    pub fn alloc(&mut self, value: T) -> Result<usize, SlotTableFull> {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(value);
                self.used += 1;
                return Ok(index);
            }
        }
        Err(SlotTableFull)
    }

    /// Remove and return the entry at `index`, if present.
    pub fn free(&mut self, index: usize) -> Option<T> {
        let value = self.slots.get_mut(index)?.take();
        if value.is_some() {
            self.used -= 1;
        }
        value
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.slots.get(index)?.as_ref()
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.slots.get_mut(index)?.as_mut()
    }

    pub fn contains(&self, index: usize) -> bool {
        self.get(index).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, &T)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_ref().map(|value| (index, value)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut T)> + '_ {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_mut().map(|value| (index, value)))
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn capacity(&self) -> usize {
        N
    }

    pub fn is_full(&self) -> bool {
        self.used >= N
    }

    pub fn clear(&mut self) {
        for slot in self.slots.iter_mut() {
            *slot = None;
        }
        self.used = 0;
    }
}

pub fn register_tests() {
    use crate::{test_case, test_eq, test_true};

    #[derive(Debug, PartialEq)]
    struct Entry(u32);

    test_case!("slot_table_alloc_free_reuse", {
        let mut table: SlotTable<Entry, 2> = SlotTable::new();
        test_eq!(table.capacity(), 2);
        test_eq!(table.alloc(Entry(1)), Ok(0));
        test_eq!(table.alloc(Entry(2)), Ok(1));
        test_eq!(table.alloc(Entry(3)), Err(SlotTableFull));
        test_true!(table.is_full());

        test_eq!(table.free(0), Some(Entry(1)));
        test_eq!(table.used(), 1);
        test_eq!(table.alloc(Entry(4)), Ok(0));
        test_eq!(table.get(0), Some(&Entry(4)));
    });

    test_case!("slot_table_get_iter_clear", {
        let mut table: SlotTable<Entry, 3> = SlotTable::new();
        table.alloc(Entry(10)).unwrap();
        table.alloc(Entry(20)).unwrap();
        table.free(1);

        test_eq!(table.get(0), Some(&Entry(10)));
        test_true!(table.get(1).is_none());
        test_true!(table.contains(0));
        test_true!(!table.contains(2));

        let values: alloc::vec::Vec<u32> = table.iter().map(|(_, e)| e.0).collect();
        test_eq!(values.len(), 1);
        test_eq!(values[0], 10);

        table.clear();
        test_eq!(table.used(), 0);
        test_true!(table.iter().next().is_none());
    });
}

