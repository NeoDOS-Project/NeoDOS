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

#[derive(Clone)]
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

    // ── NEOH serialization ──

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Collect all non-Free, non-None cells
        let mut entries: Vec<(u32, &Cell)> = Vec::new();
        for (i, slot) in self.cells.iter().enumerate() {
            if let Some(cell) = slot {
                match cell {
                    Cell::Free => {}
                    _ => entries.push((i as u32, cell)),
                }
            }
        }

        // Header placeholder: magic + version + entry_count + checksum
        let header_off = buf.len();
        buf.extend_from_slice(b"NEOH");
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());

        let mut checksum: u32 = 0;
        for (cell_idx, cell) in &entries {
            // cell_index
            checksum = checksum.wrapping_add(*cell_idx);
            buf.extend_from_slice(&cell_idx.to_le_bytes());

            match cell {
                Cell::Key(k) => {
                    let cell_type: u8 = 1;
                    checksum = checksum.wrapping_add(cell_type as u32);
                    buf.push(cell_type);

                    let name_bytes = k.name.as_bytes();
                    let name_len = name_bytes.len() as u16;
                    checksum = checksum.wrapping_add(name_len as u32);
                    buf.extend_from_slice(&name_len.to_le_bytes());
                    for &b in name_bytes {
                        checksum = checksum.wrapping_add(b as u32);
                    }
                    buf.extend_from_slice(name_bytes);

                    checksum = checksum.wrapping_add(k.parent_cell);
                    buf.extend_from_slice(&k.parent_cell.to_le_bytes());
                    checksum = checksum.wrapping_add(k.subkeys_head);
                    buf.extend_from_slice(&k.subkeys_head.to_le_bytes());
                    checksum = checksum.wrapping_add(k.subkeys_sibling);
                    buf.extend_from_slice(&k.subkeys_sibling.to_le_bytes());
                    checksum = checksum.wrapping_add(k.values_head);
                    buf.extend_from_slice(&k.values_head.to_le_bytes());
                    checksum = checksum.wrapping_add(k.sec_desc_cell);
                    buf.extend_from_slice(&k.sec_desc_cell.to_le_bytes());
                    let lw_low = k.last_write as u32;
                    let lw_high = (k.last_write >> 32) as u32;
                    checksum = checksum.wrapping_add(lw_low);
                    checksum = checksum.wrapping_add(lw_high);
                    buf.extend_from_slice(&k.last_write.to_le_bytes());
                }
                Cell::Value(v) => {
                    let cell_type: u8 = 2;
                    checksum = checksum.wrapping_add(cell_type as u32);
                    buf.push(cell_type);

                    let name_bytes = v.name.as_bytes();
                    let name_len = name_bytes.len() as u16;
                    checksum = checksum.wrapping_add(name_len as u32);
                    buf.extend_from_slice(&name_len.to_le_bytes());
                    for &b in name_bytes {
                        checksum = checksum.wrapping_add(b as u32);
                    }
                    buf.extend_from_slice(name_bytes);

                    checksum = checksum.wrapping_add(v.value_type);
                    buf.extend_from_slice(&v.value_type.to_le_bytes());

                    let data_len = v.data.len() as u32;
                    checksum = checksum.wrapping_add(data_len);
                    buf.extend_from_slice(&data_len.to_le_bytes());
                    for &b in &v.data {
                        checksum = checksum.wrapping_add(b as u32);
                    }
                    buf.extend_from_slice(&v.data);

                    let nxt = v.next;
                    checksum = checksum.wrapping_add(nxt);
                    buf.extend_from_slice(&nxt.to_le_bytes());
                }
                Cell::Security(s) => {
                    let cell_type: u8 = 3;
                    checksum = checksum.wrapping_add(cell_type as u32);
                    buf.push(cell_type);

                    let sd_len = s.sd_data.len() as u32;
                    checksum = checksum.wrapping_add(sd_len);
                    buf.extend_from_slice(&sd_len.to_le_bytes());
                    for &b in &s.sd_data {
                        checksum = checksum.wrapping_add(b as u32);
                    }
                    buf.extend_from_slice(&s.sd_data);

                    let nxt = s.next;
                    checksum = checksum.wrapping_add(nxt);
                    buf.extend_from_slice(&nxt.to_le_bytes());
                }
                Cell::Free => { /* should not happen, filtered above */ }
            }
        }

        // Write checksum into header
        let checksum_off = header_off + 4 + 4 + 4;
        buf[checksum_off..checksum_off + 4].copy_from_slice(&checksum.to_le_bytes());

        buf
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, ()> {
        if data.len() < 16 {
            return Err(());
        }
        if &data[0..4] != b"NEOH" {
            return Err(());
        }
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != 1 {
            return Err(());
        }
        let entry_count = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let stored_checksum = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

        let mut cells: Vec<Option<Cell>> = Vec::with_capacity(MAX_CELLS);
        cells.resize(MAX_CELLS, None);

        let mut count: usize = 0;
        let mut pos = 16usize;
        let mut computed_checksum: u32 = 0;

        for _ in 0..entry_count {
            if pos + 5 > data.len() {
                return Err(());
            }
            let cell_idx = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
            computed_checksum = computed_checksum.wrapping_add(cell_idx);
            let cell_type = data[pos + 4];
            computed_checksum = computed_checksum.wrapping_add(cell_type as u32);
            pos += 5;

            match cell_type {
                1 => {
                    // Key
                    if pos + 2 > data.len() { return Err(()); }
                    let name_len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
                    computed_checksum = computed_checksum.wrapping_add(name_len as u32);
                    pos += 2;
                    if pos + name_len > data.len() { return Err(()); }
                    let name = core::str::from_utf8(&data[pos..pos + name_len]).map_err(|_| ())?;
                    for &b in &data[pos..pos + name_len] {
                        computed_checksum = computed_checksum.wrapping_add(b as u32);
                    }
                    pos += name_len;
                    if pos + 4 > data.len() { return Err(()); }
                    let parent_cell = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(parent_cell);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let subkeys_head = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(subkeys_head);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let subkeys_sibling = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(subkeys_sibling);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let values_head = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(values_head);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let sec_desc_cell = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(sec_desc_cell);
                    pos += 4;
                    if pos + 8 > data.len() { return Err(()); }
                    let last_write = u64::from_le_bytes([
                        data[pos], data[pos+1], data[pos+2], data[pos+3],
                        data[pos+4], data[pos+5], data[pos+6], data[pos+7],
                    ]);
                    let lw_low = last_write as u32;
                    let lw_high = (last_write >> 32) as u32;
                    computed_checksum = computed_checksum.wrapping_add(lw_low);
                    computed_checksum = computed_checksum.wrapping_add(lw_high);
                    pos += 8;

                    if (cell_idx as usize) >= MAX_CELLS {
                        return Err(());
                    }
                    cells[cell_idx as usize] = Some(Cell::Key(KeyCell {
                        name: name.to_string(),
                        parent_cell,
                        subkeys_head,
                        subkeys_sibling,
                        values_head,
                        sec_desc_cell,
                        last_write,
                    }));
                    count += 1;
                }
                2 => {
                    // Value
                    if pos + 2 > data.len() { return Err(()); }
                    let name_len = u16::from_le_bytes([data[pos], data[pos+1]]) as usize;
                    computed_checksum = computed_checksum.wrapping_add(name_len as u32);
                    pos += 2;
                    if pos + name_len > data.len() { return Err(()); }
                    let name_str = core::str::from_utf8(&data[pos..pos + name_len]).map_err(|_| ())?;
                    for &b in &data[pos..pos + name_len] {
                        computed_checksum = computed_checksum.wrapping_add(b as u32);
                    }
                    pos += name_len;
                    if pos + 4 > data.len() { return Err(()); }
                    let value_type = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(value_type);
                    pos += 4;
                    if pos + 4 > data.len() { return Err(()); }
                    let data_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(data_len);
                    pos += 4;
                    if pos + data_len as usize > data.len() { return Err(()); }
                    let val_data = data[pos..pos + data_len as usize].to_vec();
                    for &b in &val_data {
                        computed_checksum = computed_checksum.wrapping_add(b as u32);
                    }
                    pos += data_len as usize;
                    if pos + 4 > data.len() { return Err(()); }
                    let nxt = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(nxt);
                    pos += 4;

                    if (cell_idx as usize) >= MAX_CELLS {
                        return Err(());
                    }
                    cells[cell_idx as usize] = Some(Cell::Value(ValueCell {
                        name: name_str.to_string(),
                        value_type,
                        data: val_data,
                        next: nxt,
                    }));
                    count += 1;
                }
                3 => {
                    // Security
                    if pos + 4 > data.len() { return Err(()); }
                    let sd_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(sd_len);
                    pos += 4;
                    if pos + sd_len as usize > data.len() { return Err(()); }
                    let sd_data = data[pos..pos + sd_len as usize].to_vec();
                    for &b in &sd_data {
                        computed_checksum = computed_checksum.wrapping_add(b as u32);
                    }
                    pos += sd_len as usize;
                    if pos + 4 > data.len() { return Err(()); }
                    let nxt = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                    computed_checksum = computed_checksum.wrapping_add(nxt);
                    pos += 4;

                    if (cell_idx as usize) >= MAX_CELLS {
                        return Err(());
                    }
                    cells[cell_idx as usize] = Some(Cell::Security(SecurityCell {
                        sd_data,
                        next: nxt,
                    }));
                    count += 1;
                }
                _ => return Err(()),
            }
        }

        if computed_checksum != stored_checksum {
            return Err(());
        }

        let mut hive = Hive {
            name: String::new(),
            cells,
            free_head: NULL_CELL,
            count,
            dirty: false,
        };
        // Ensure cell 0 exists as root (at minimum a Free placeholder)
        if hive.cells[0].is_none() {
            hive.cells[0] = Some(Cell::Key(KeyCell::new("", NULL_CELL)));
            hive.count += 1;
        }
        Ok(hive)
    }

    // ── Raw I/O persistence (block-level, for direct-disk access) ──

    pub fn flush_to_io(&self, _io: &crate::vfs::io::IoStack) -> Result<(), ()> {
        // VFS-based persistence is preferred; this is a block-level fallback
        // that serializes to the first 4KB-aligned sectors on the device.
        let data = self.serialize();
        if data.len() > 4096 {
            return Err(());
        }
        let mut sector = [0u8; 512];
        let chunk_len = core::cmp::min(data.len(), 512);
        sector[..chunk_len].copy_from_slice(&data[..chunk_len]);
        _io.write_sector(0, &sector)
    }

    pub fn load_from_io(_io: &crate::vfs::io::IoStack, name: &str) -> Result<Self, ()> {
        // For block-level loading, read sector 0 and try to deserialize.
        // VFS-based loading is preferred.
        let sector = _io.read_sector(0)?;
        // Check if there's a NEOH header at the beginning
        if &sector[0..4] == b"NEOH" {
            // Try to load from sector data; may be truncated
            let size = {
                let entry_count = u32::from_le_bytes([sector[8], sector[9], sector[10], sector[11]]);
                let est = 16u32 + entry_count * 48;
                core::cmp::min(512, est as usize)
            };
            if let Ok(hive) = Self::deserialize(&sector[..size]) {
                let mut h = hive;
                h.name = name.to_string();
                return Ok(h);
            }
        }
        Err(())
    }
}
