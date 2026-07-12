use alloc::string::String;

use super::types::{Cell, ValueCell, NULL_CELL};
use super::core::Hive;

impl Hive {
    pub fn set_value(&mut self, key_idx: u32, name: &str, value_type: u32, data: &[u8]) -> Option<()> {
        let existing = self.find_value(key_idx, name);
        if let Some(val_idx) = existing {
            if let Some(Cell::Value(ref mut v)) = self.slot_mut(val_idx) {
                v.value_type = value_type;
                v.data = data.to_vec();
                return Some(());
            }
        }
        let mut value = ValueCell::new(name, value_type, data);
        if let Some(Cell::Key(k)) = self.slot(key_idx) {
            value.next = k.values_head;
        }
        let val_idx = self.alloc_cell(Cell::Value(value))?;
        if let Some(Cell::Key(ref mut k)) = self.slot_mut(key_idx) {
            k.values_head = val_idx;
        }
        Some(())
    }

    pub fn find_value(&self, key_idx: u32, name: &str) -> Option<u32> {
        let head = match self.slot(key_idx) {
            Some(Cell::Key(k)) => k.values_head,
            _ => return None,
        };
        let mut curr = head;
        while curr != NULL_CELL {
            if let Some(Cell::Value(v)) = self.slot(curr) {
                if v.name.eq_ignore_ascii_case(name) {
                    return Some(curr);
                }
                curr = v.next;
            } else { break; }
        }
        None
    }

    pub fn query_value(&self, key_idx: u32, name: &str) -> Option<ValueCell> {
        let val_idx = self.find_value(key_idx, name)?;
        match self.slot(val_idx) {
            Some(Cell::Value(v)) => Some(v.clone()),
            _ => None,
        }
    }

    pub fn delete_value(&mut self, key_idx: u32, name: &str) -> bool {
        let head = match self.slot(key_idx) {
            Some(Cell::Key(k)) => k.values_head,
            _ => return false,
        };
        let mut curr = head;
        let mut prev = NULL_CELL;
        while curr != NULL_CELL {
            let next = match self.slot(curr) {
                Some(Cell::Value(v)) => {
                    if v.name.eq_ignore_ascii_case(name) {
                        Some(v.next)
                    } else {
                        None
                    }
                }
                _ => break,
            };
            if let Some(nxt) = next {
                if prev == NULL_CELL {
                    if let Some(Cell::Key(ref mut k)) = self.slot_mut(key_idx) {
                        k.values_head = nxt;
                    }
                } else {
                    if let Some(Cell::Value(ref mut pv)) = self.slot_mut(prev) {
                        pv.next = nxt;
                    }
                }
                self.free_cell(curr);
                return true;
            }
            prev = curr;
            curr = match self.slot(curr) {
                Some(Cell::Value(v)) => v.next,
                _ => NULL_CELL,
            };
        }
        false
    }

    pub fn enum_value(&self, key_idx: u32, index: u32) -> Option<String> {
        let head = match self.slot(key_idx) {
            Some(Cell::Key(k)) => k.values_head,
            _ => return None,
        };
        let mut curr = head;
        let mut i = 0u32;
        while curr != NULL_CELL {
            if i == index {
                if let Some(Cell::Value(v)) = self.slot(curr) {
                    return Some(v.name.clone());
                }
                return None;
            }
            if let Some(Cell::Value(v)) = self.slot(curr) {
                curr = v.next;
            } else { break; }
            i += 1;
        }
        None
    }

    pub fn value_count(&self, key_idx: u32) -> u32 {
        let head = match self.slot(key_idx) {
            Some(Cell::Key(k)) => k.values_head,
            _ => return 0,
        };
        let mut n = 0;
        let mut curr = head;
        while curr != NULL_CELL {
            n += 1;
            if let Some(Cell::Value(v)) = self.slot(curr) {
                curr = v.next;
            } else { break; }
        }
        n
    }
}
