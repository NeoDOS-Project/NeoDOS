use alloc::vec::Vec;
use core::ops::{Index, IndexMut};
use crate::object::{ObId, ObType, ob_create_object, ob_close_object};

// ── Handle type constants (LEGACY — kept for backward compat during migration) ──

pub const HANDLE_CLOSED: u8 = 0;
pub const HANDLE_STDIN: u8 = 1;
pub const HANDLE_STDOUT: u8 = 2;
pub const HANDLE_STDERR: u8 = 3;
pub const HANDLE_PIPE_READ: u8 = 4;
pub const HANDLE_PIPE_WRITE: u8 = 5;
pub const HANDLE_FILE: u8 = 6;
pub const HANDLE_DEVICE: u8 = 7;
pub const HANDLE_EVENT: u8 = 8;
pub const HANDLE_DIR: u8 = 9;

#[derive(Debug, Clone, Copy)]
pub struct HandleEntry {
    /// Object Manager ID (primary reference, OB-002).
    /// 0 means this entry is not backed by Ob — used only for stdin/stdout/stderr.
    pub object_id: ObId,
    /// Legacy type discriminator (deprecated, will be removed in v0.43+).
    pub kind: u8,
    /// Legacy type-specific id (deprecated).
    pub id: u32,
    /// Legacy extra field (deprecated).
    pub extra: u32,
    /// Per-handle offset for file-like objects.
    pub offset: u64,
}

impl HandleEntry {
    pub const fn closed() -> Self {
        HandleEntry { object_id: 0, kind: HANDLE_CLOSED, id: 0, extra: 0, offset: 0 }
    }

    pub const fn stdin() -> Self {
        HandleEntry { object_id: 0, kind: HANDLE_STDIN, id: 0, extra: 0, offset: 0 }
    }

    pub const fn stdout() -> Self {
        HandleEntry { object_id: 0, kind: HANDLE_STDOUT, id: 0, extra: 0, offset: 0 }
    }

    pub const fn stderr() -> Self {
        HandleEntry { object_id: 0, kind: HANDLE_STDERR, id: 0, extra: 0, offset: 0 }
    }

    /// Create a pipe read handle, registered in the Object Manager.
    pub fn pipe_read(pipe_id: u8) -> Self {
        let ob_id = ob_create_object(
            ObType::Pipe,
            core::str::from_utf8(&[b'P', b'I', b'P', b'E', b'_', b'R', pipe_id + b'0']).unwrap_or("PIPE_R"),
            pipe_id as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, kind: HANDLE_PIPE_READ, id: pipe_id as u32, extra: 0, offset: 0 }
    }

    /// Create a pipe write handle, registered in the Object Manager.
    pub fn pipe_write(pipe_id: u8) -> Self {
        let ob_id = ob_create_object(
            ObType::Pipe,
            core::str::from_utf8(&[b'P', b'I', b'P', b'E', b'_', b'W', pipe_id + b'0']).unwrap_or("PIPE_W"),
            pipe_id as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, kind: HANDLE_PIPE_WRITE, id: pipe_id as u32, extra: 0, offset: 0 }
    }

    /// Create a file handle, registered in the Object Manager.
    pub fn file(drive: u8, inode: u32) -> Self {
        let ob_id = ob_create_object(
            ObType::Filesystem, "FILE", inode as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, kind: HANDLE_FILE, id: inode, extra: drive as u32, offset: 0 }
    }

    /// Create a device handle.
    pub fn device(device_id: u32) -> Self {
        let ob_id = ob_create_object(
            ObType::Device, "DEVICE", device_id as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, kind: HANDLE_DEVICE, id: device_id, extra: 0, offset: 0 }
    }

    /// Create an event handle.
    pub fn event(event_type: u32) -> Self {
        let ob_id = ob_create_object(
            ObType::Event, "EVENT", event_type as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, kind: HANDLE_EVENT, id: event_type, extra: 0, offset: 0 }
    }

    /// Create a directory handle.
    pub fn dir(drive: u8, inode: u32) -> Self {
        let ob_id = ob_create_object(
            ObType::Directory, "DIR", inode as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, kind: HANDLE_DIR, id: inode, extra: drive as u32, offset: 0 }
    }

    /// Close this handle: release the Ob reference.
    pub fn close(&mut self) {
        if self.object_id != 0 {
            let _ = ob_close_object(self.object_id);
            self.object_id = 0;
        }
        *self = HandleEntry::closed();
    }

    pub fn is_open(&self) -> bool {
        self.kind != HANDLE_CLOSED
    }

    pub fn is_pipe(&self) -> bool {
        self.kind == HANDLE_PIPE_READ || self.kind == HANDLE_PIPE_WRITE
    }

    pub fn is_file(&self) -> bool {
        self.kind == HANDLE_FILE
    }

    pub fn is_dir(&self) -> bool {
        self.kind == HANDLE_DIR
    }

    pub fn pipe_id(&self) -> Option<u8> {
        if self.is_pipe() { Some(self.id as u8) } else { None }
    }

    pub fn file_inode(&self) -> Option<u32> {
        if self.is_file() { Some(self.id) } else { None }
    }

    pub fn file_drive(&self) -> u8 {
        self.extra as u8
    }

    pub fn dir_inode(&self) -> Option<u32> {
        if self.is_dir() { Some(self.id) } else { None }
    }

    pub fn dir_drive(&self) -> u8 {
        self.extra as u8
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
