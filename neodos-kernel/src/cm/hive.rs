use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const NULL_CELL: u32 = 0xFFFFFFFF;
pub const MAX_CELLS: usize = 2048;

pub const REG_NONE: u32 = 0;
pub const REG_SZ: u32 = 1;
pub const REG_DWORD: u32 = 2;
pub const REG_BINARY: u32 = 3;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellType {
    Free = 0,
    Key = 1,
    Value = 2,
    Security = 3,
}

#[derive(Clone, Debug)]
pub struct KeyCell {
    pub name: String,
    pub parent_cell: u32,
    pub subkeys_head: u32,
    pub subkeys_sibling: u32,
    pub values_head: u32,
    pub sec_desc_cell: u32,
    pub last_write: u64,
}

impl KeyCell {
    pub fn new(name: &str, parent: u32) -> Self {
        KeyCell {
            name: name.to_string(),
            parent_cell: parent,
            subkeys_head: NULL_CELL,
            subkeys_sibling: NULL_CELL,
            values_head: NULL_CELL,
            sec_desc_cell: NULL_CELL,
            last_write: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ValueCell {
    pub name: String,
    pub value_type: u32,
    pub data: Vec<u8>,
    pub next: u32,
}

impl ValueCell {
    pub fn new(name: &str, value_type: u32, data: &[u8]) -> Self {
        ValueCell {
            name: name.to_string(),
            value_type,
            data: data.to_vec(),
            next: NULL_CELL,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        if self.value_type == REG_SZ {
            core::str::from_utf8(&self.data).ok()
        } else {
            None
        }
    }

    pub fn as_dword(&self) -> Option<u32> {
        if self.value_type == REG_DWORD && self.data.len() >= 4 {
            Some(u32::from_le_bytes([self.data[0], self.data[1], self.data[2], self.data[3]]))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct SecurityCell {
    pub sd_data: Vec<u8>,
    pub next: u32,
}

#[derive(Clone, Debug)]
pub enum Cell {
    Free,
    Key(KeyCell),
    Value(ValueCell),
    Security(SecurityCell),
}

impl Cell {
    pub fn cell_type(&self) -> CellType {
        match self {
            Cell::Free => CellType::Free,
            Cell::Key(_) => CellType::Key,
            Cell::Value(_) => CellType::Value,
            Cell::Security(_) => CellType::Security,
        }
    }
}

pub struct Hive {
    pub name: String,
    cells: Vec<Option<Cell>>,
    free_head: u32,
    count: usize,
    dirty: bool,
}

impl Hive {
    pub fn new(name: &str) -> Self {
        let mut cells = Vec::with_capacity(MAX_CELLS);
        cells.resize(MAX_CELLS, None);
        let mut hive = Hive {
            name: name.to_string(),
            cells,
            free_head: NULL_CELL,
            count: 0,
            dirty: false,
        };
        let root = KeyCell::new(name, NULL_CELL);
        hive.cells[0] = Some(Cell::Key(root));
        hive.count = 1;
        hive
    }

    pub fn cell_count(&self) -> usize { self.count }
    pub fn is_dirty(&self) -> bool { self.dirty }
    pub fn mark_clean(&mut self) { self.dirty = false; }

    pub fn alloc_cell(&mut self, cell: Cell) -> Option<u32> {
        if self.free_head != NULL_CELL {
            let idx = self.free_head;
            let next_free = match &self.cells[idx as usize] {
                Some(Cell::Free) => {
                    // Read next pointer stored in the free cell data
                    // For simplicity, just scan for the next free
                    self.scan_next_free(idx as usize)
                }
                _ => NULL_CELL,
            };
            if self.cells[idx as usize].is_some() {
                self.free_head = next_free;
                self.cells[idx as usize] = Some(cell);
                self.count += 1;
                self.dirty = true;
                return Some(idx);
            }
        }
        for (i, slot) in self.cells.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(cell);
                self.count += 1;
                self.dirty = true;
                return Some(i as u32);
            }
        }
        None
    }

    fn scan_next_free(&self, start: usize) -> u32 {
        for i in (start + 1)..MAX_CELLS {
            if self.cells[i].is_none() || matches!(self.cells[i], Some(Cell::Free)) {
                return i as u32;
            }
        }
        NULL_CELL
    }

    pub fn free_cell(&mut self, idx: u32) {
        if idx as usize >= MAX_CELLS { return; }
        match &self.cells[idx as usize] {
            Some(Cell::Free) | None => return,
            _ => {}
        }
        self.cells[idx as usize] = Some(Cell::Free);
        self.count -= 1;
        self.dirty = true;
    }

    pub(crate) fn slot(&self, idx: u32) -> Option<&Cell> {
        if (idx as usize) >= MAX_CELLS { return None; }
        self.cells[idx as usize].as_ref()
    }

    pub(crate) fn slot_mut(&mut self, idx: u32) -> Option<&mut Cell> {
        if (idx as usize) >= MAX_CELLS { return None; }
        self.dirty = true;
        self.cells[idx as usize].as_mut()
    }

    pub fn root_cell(&self) -> u32 { 0 }

    // ── Key operations ──

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
        let (subkeys_head, values_head, parent_cell, self_sibling) = match self.slot(idx) {
            Some(Cell::Key(k)) => (k.subkeys_head, k.values_head, k.parent_cell, k.subkeys_sibling),
            _ => return,
        };
        // Free subkeys recursively
        let mut curr = subkeys_head;
        while curr != NULL_CELL {
            let next = match self.slot(curr) {
                Some(Cell::Key(k)) => k.subkeys_sibling,
                _ => NULL_CELL,
            };
            self.delete_key(curr);
            curr = next;
        }
        // Free values
        let mut curr_val = values_head;
        while curr_val != NULL_CELL {
            let next = match self.slot(curr_val) {
                Some(Cell::Value(v)) => v.next,
                _ => NULL_CELL,
            };
            self.free_cell(curr_val);
            curr_val = next;
        }
        // Unlink from parent (use pre-extracted sibling to avoid double borrow)
        if parent_cell != NULL_CELL {
            if let Some(Cell::Key(ref mut pk)) = self.slot_mut(parent_cell) {
                if pk.subkeys_head == idx {
                    pk.subkeys_head = self_sibling;
                } else {
                    let mut prev = pk.subkeys_head;
                    while prev != NULL_CELL {
                        let nxt = match self.slot(prev) {
                            Some(Cell::Key(p)) => p.subkeys_sibling,
                            _ => NULL_CELL,
                        };
                        if nxt == idx {
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
        self.free_cell(idx);
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

    // ── Value operations ──

    pub fn set_value(&mut self, key_idx: u32, name: &str, value_type: u32, data: &[u8]) -> Option<()> {
        // Update existing value if found
        let existing = self.find_value(key_idx, name);
        if let Some(val_idx) = existing {
            if let Some(Cell::Value(ref mut v)) = self.slot_mut(val_idx) {
                v.value_type = value_type;
                v.data = data.to_vec();
                return Some(());
            }
        }
        // Create new value and link at head of key's value list
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

    // ── Persistence ──

    pub fn flush_to_io(&self, _io: &crate::vfs::io::IoStack) -> Result<(), ()> {
        Ok(())
    }

    pub fn load_from_io(_io: &crate::vfs::io::IoStack, name: &str) -> Result<Self, ()> {
        Ok(Hive::new(name))
    }
}
