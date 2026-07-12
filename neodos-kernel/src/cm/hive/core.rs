use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::types::{Cell, KeyCell, NULL_CELL, MAX_CELLS};

#[derive(Clone)]
pub struct Hive {
    pub name: String,
    pub(crate) cells: Vec<Option<Cell>>,
    pub(crate) next_alloc_hint: u32,
    pub(crate) count: usize,
    pub(crate) dirty: bool,
}

impl Hive {
    pub fn new(name: &str) -> Self {
        let mut cells = Vec::with_capacity(MAX_CELLS);
        cells.resize(MAX_CELLS, None);
        let mut hive = Hive {
            name: name.to_string(),
            cells,
            next_alloc_hint: 1,
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
        let start = self.next_alloc_hint as usize;
        let len = self.cells.len();
        for offset in 0..len {
            let i = (start + offset) % len;
            if self.cells[i].is_none() {
                self.cells[i] = Some(cell);
                self.count += 1;
                self.dirty = true;
                self.next_alloc_hint = ((i as u32) + 1) % len as u32;
                return Some(i as u32);
            }
        }
        let idx = len;
        self.cells.push(Some(cell));
        self.count += 1;
        self.dirty = true;
        self.next_alloc_hint = (idx + 1) as u32;
        Some(idx as u32)
    }

    pub fn free_cell(&mut self, idx: u32) {
        if (idx as usize) >= self.cells.len() { return; }
        if self.cells[idx as usize].is_none() { return; }
        self.cells[idx as usize] = None;
        self.count -= 1;
        self.dirty = true;
    }

    pub(crate) fn slot(&self, idx: u32) -> Option<&Cell> {
        if (idx as usize) >= self.cells.len() { return None; }
        self.cells[idx as usize].as_ref()
    }

    pub(crate) fn slot_mut(&mut self, idx: u32) -> Option<&mut Cell> {
        if (idx as usize) >= self.cells.len() { return None; }
        self.dirty = true;
        self.cells[idx as usize].as_mut()
    }

    pub fn root_cell(&self) -> u32 { 0 }
}
