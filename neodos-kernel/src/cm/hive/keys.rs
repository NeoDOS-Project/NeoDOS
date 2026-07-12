use alloc::string::String;
use alloc::vec::Vec;

use super::types::{Cell, KeyCell, NULL_CELL};
use super::core::Hive;

impl Hive {
    pub fn create_key(&mut self, parent: u32, name: &str) -> Option<u32> {
        match self.slot(parent) {
            Some(Cell::Key(_)) => {}
            _ => return None,
        }
        if self.find_key(parent, name).is_some() {
            return None;
        }
        let mut new_key = KeyCell::new(name, parent);
        if let Some(Cell::Key(ref pk)) = self.slot(parent) {
            new_key.subkeys_sibling = pk.subkeys_head;
        }
        let new_idx = self.alloc_cell(Cell::Key(new_key))?;
        if let Some(Cell::Key(ref mut pk)) = self.slot_mut(parent) {
            pk.subkeys_head = new_idx;
        }
        Some(new_idx)
    }

    pub fn find_key(&self, parent: u32, name: &str) -> Option<u32> {
        let head = match self.slot(parent) {
            Some(Cell::Key(k)) => k.subkeys_head,
            _ => return None,
        };
        let mut curr = head;
        while curr != NULL_CELL {
            if let Some(Cell::Key(k)) = self.slot(curr) {
                if k.name.eq_ignore_ascii_case(name) {
                    return Some(curr);
                }
                curr = k.subkeys_sibling;
            } else { break; }
        }
        None
    }

    pub fn open_key_by_path(&self, start: u32, path: &str) -> Option<u32> {
        let parts: Vec<&str> = path.split('\\').filter(|p| !p.is_empty()).collect();
        let mut curr = start;
        for part in &parts {
            curr = self.find_key(curr, part)?;
        }
        Some(curr)
    }

    pub fn delete_key(&mut self, idx: u32) {
        if idx == 0 { return; }
        if self.slot(idx).is_none() { return; }
        let mut stack: Vec<(u32, bool)> = alloc::vec![(idx, false)];
        while let Some((curr, visited)) = stack.pop() {
            if !visited {
                let subkeys_head = match self.slot(curr) {
                    Some(Cell::Key(k)) => k.subkeys_head,
                    _ => { self.free_cell(curr); continue; }
                };
                stack.push((curr, true));
                let mut sk = subkeys_head;
                while sk != NULL_CELL {
                    let next = match self.slot(sk) {
                        Some(Cell::Key(k)) => k.subkeys_sibling,
                        _ => NULL_CELL,
                    };
                    stack.push((sk, false));
                    sk = next;
                }
            } else {
                let (values_head, parent_cell, self_sibling) = match self.slot(curr) {
                    Some(Cell::Key(k)) => (k.values_head, k.parent_cell, k.subkeys_sibling),
                    _ => { self.free_cell(curr); continue; }
                };
                let mut cv = values_head;
                while cv != NULL_CELL {
                    let next = match self.slot(cv) {
                        Some(Cell::Value(v)) => v.next,
                        _ => NULL_CELL,
                    };
                    self.free_cell(cv);
                    cv = next;
                }
                if parent_cell != NULL_CELL {
                    if let Some(Cell::Key(ref mut pk)) = self.slot_mut(parent_cell) {
                        if pk.subkeys_head == curr {
                            pk.subkeys_head = self_sibling;
                        } else {
                            let mut prev = pk.subkeys_head;
                            while prev != NULL_CELL {
                                let nxt = match self.slot(prev) {
                                    Some(Cell::Key(p)) => p.subkeys_sibling,
                                    _ => NULL_CELL,
                                };
                                if nxt == curr {
                                    if let Some(Cell::Key(ref mut pp)) = self.slot_mut(prev) {
                                        pp.subkeys_sibling = self_sibling;
                                    }
                                    break;
                                }
                                prev = nxt;
                            }
                        }
                    }
                }
                self.free_cell(curr);
            }
        }
    }

    pub fn enum_key(&self, parent: u32, index: u32) -> Option<String> {
        let head = match self.slot(parent) {
            Some(Cell::Key(k)) => k.subkeys_head,
            _ => return None,
        };
        let mut curr = head;
        let mut i = 0u32;
        while curr != NULL_CELL {
            if i == index {
                if let Some(Cell::Key(k)) = self.slot(curr) {
                    return Some(k.name.clone());
                }
                return None;
            }
            if let Some(Cell::Key(k)) = self.slot(curr) {
                curr = k.subkeys_sibling;
            } else { break; }
            i += 1;
        }
        None
    }

    pub fn key_count(&self, parent: u32) -> u32 {
        let head = match self.slot(parent) {
            Some(Cell::Key(k)) => k.subkeys_head,
            _ => return 0,
        };
        let mut n = 0;
        let mut curr = head;
        while curr != NULL_CELL {
            n += 1;
            if let Some(Cell::Key(k)) = self.slot(curr) {
                curr = k.subkeys_sibling;
            } else { break; }
        }
        n
    }
}
