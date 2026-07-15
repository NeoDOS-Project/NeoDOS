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
            let data = if self.data.last() == Some(&0) {
                &self.data[..self.data.len()-1]
            } else {
                &self.data[..]
            };
            core::str::from_utf8(data).ok()
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
