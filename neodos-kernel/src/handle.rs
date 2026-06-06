use alloc::vec::Vec;
use core::ops::{Index, IndexMut};

pub const HANDLE_CLOSED: u8 = 0;
pub const HANDLE_STDIN: u8 = 1;
pub const HANDLE_STDOUT: u8 = 2;
pub const HANDLE_STDERR: u8 = 3;
pub const HANDLE_PIPE_READ: u8 = 4;
pub const HANDLE_PIPE_WRITE: u8 = 5;
pub const HANDLE_FILE: u8 = 6;
pub const HANDLE_DEVICE: u8 = 7;
pub const HANDLE_EVENT: u8 = 8;

#[derive(Debug, Clone, Copy)]
pub struct HandleEntry {
    pub kind: u8,
    pub id: u32,
    pub extra: u32,
    pub offset: u64,
}

impl HandleEntry {
    pub const fn closed() -> Self {
        HandleEntry { kind: HANDLE_CLOSED, id: 0, extra: 0, offset: 0 }
    }

    pub const fn stdin() -> Self {
        HandleEntry { kind: HANDLE_STDIN, id: 0, extra: 0, offset: 0 }
    }

    pub const fn stdout() -> Self {
        HandleEntry { kind: HANDLE_STDOUT, id: 0, extra: 0, offset: 0 }
    }

    pub const fn stderr() -> Self {
        HandleEntry { kind: HANDLE_STDERR, id: 0, extra: 0, offset: 0 }
    }

    pub fn pipe_read(pipe_id: u8) -> Self {
        HandleEntry { kind: HANDLE_PIPE_READ, id: pipe_id as u32, extra: 0, offset: 0 }
    }

    pub fn pipe_write(pipe_id: u8) -> Self {
        HandleEntry { kind: HANDLE_PIPE_WRITE, id: pipe_id as u32, extra: 0, offset: 0 }
    }

    pub fn file(drive: u8, inode: u32) -> Self {
        HandleEntry { kind: HANDLE_FILE, id: inode, extra: drive as u32, offset: 0 }
    }

    pub fn device(device_id: u32) -> Self {
        HandleEntry { kind: HANDLE_DEVICE, id: device_id, extra: 0, offset: 0 }
    }

    pub fn event(event_type: u32) -> Self {
        HandleEntry { kind: HANDLE_EVENT, id: event_type, extra: 0, offset: 0 }
    }
}

const CLOSED_SENTINEL: HandleEntry = HandleEntry::closed();

#[derive(Debug, Clone)]
pub struct HandleTable {
    entries: Vec<HandleEntry>,
}

impl HandleTable {
    pub fn new() -> Self {
        HandleTable { entries: Vec::new() }
    }

    pub fn with_defaults() -> Self {
        let mut table = HandleTable { entries: Vec::new() };
        table.entries.push(HandleEntry::stdin());
        table.entries.push(HandleEntry::stdout());
        table.entries.push(HandleEntry::stderr());
        table
    }

    pub fn entries(&self) -> &[HandleEntry] {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut [HandleEntry] {
        &mut self.entries
    }

    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, HandleEntry> {
        self.entries.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, fd: u8) -> HandleEntry {
        let idx = fd as usize;
        self.entries.get(idx).copied().unwrap_or(CLOSED_SENTINEL)
    }

    pub fn set(&mut self, fd: u8, entry: HandleEntry) {
        let idx = fd as usize;
        if idx >= self.entries.len() {
            self.entries.resize(idx + 1, HandleEntry::closed());
        }
        self.entries[idx] = entry;
    }

    pub fn alloc_handle(&mut self, entry: HandleEntry) -> Option<u8> {
        for i in 3..self.entries.len() {
            if self.entries[i].kind == HANDLE_CLOSED {
                self.entries[i] = entry;
                return Some(i as u8);
            }
        }
        let fd = self.entries.len() as u8;
        self.entries.push(entry);
        Some(fd)
    }

    pub fn alloc_two_handles(&mut self, e1: HandleEntry, e2: HandleEntry) -> Option<(u8, u8)> {
        let mut first: Option<u8> = None;
        let mut second: Option<u8> = None;
        for i in 3..self.entries.len() {
            if self.entries[i].kind == HANDLE_CLOSED {
                if first.is_none() {
                    first = Some(i as u8);
                } else if second.is_none() {
                    second = Some(i as u8);
                    break;
                }
            }
        }
        if first.is_some() && second.is_none() {
            second = Some(self.entries.len() as u8);
            self.entries.push(e2);
            self.entries[first.unwrap() as usize] = e1;
            return Some((first.unwrap(), second.unwrap()));
        }
        match (first, second) {
            (Some(a), Some(b)) => {
                self.entries[a as usize] = e1;
                self.entries[b as usize] = e2;
                Some((a, b))
            }
            _ => None,
        }
    }
}

impl Index<usize> for HandleTable {
    type Output = HandleEntry;
    fn index(&self, index: usize) -> &HandleEntry {
        self.entries.get(index).unwrap_or(&CLOSED_SENTINEL)
    }
}

impl IndexMut<usize> for HandleTable {
    fn index_mut(&mut self, index: usize) -> &mut HandleEntry {
        if index >= self.entries.len() {
            self.entries.resize(index + 1, HandleEntry::closed());
        }
        &mut self.entries[index]
    }
}

pub fn default_handle_table() -> HandleTable {
    HandleTable::with_defaults()
}

pub fn closed_handle_table() -> HandleTable {
    HandleTable::new()
}

pub fn alloc_handle(table: &mut HandleTable, entry: HandleEntry) -> Option<u8> {
    table.alloc_handle(entry)
}

pub fn alloc_two_handles(table: &mut HandleTable, e1: HandleEntry, e2: HandleEntry) -> Option<(u8, u8)> {
    table.alloc_two_handles(e1, e2)
}
