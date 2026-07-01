use alloc::vec::Vec;
use core::ops::{Index, IndexMut};
use crate::object::{ObId, ObType, ob_create_object, ob_close_object, ob_lookup};

/// Sentinel values for special file descriptors (stdin/stdout/stderr).
/// These are not real ObObject IDs; they sit in object_id to identify
/// the stream type without needing a separate `kind` field.
pub const HANDLE_CLOSED: ObId = 0;
pub const HANDLE_STDIN: ObId  = ObId::MAX;       // 0xFFFF_FFFF_FFFF_FFFF
pub const HANDLE_STDOUT: ObId = ObId::MAX - 1;   // 0xFFFF_FFFF_FFFF_FFFE
pub const HANDLE_STDERR: ObId = ObId::MAX - 2;   // 0xFFFF_FFFF_FFFF_FFFD

#[derive(Debug, Clone, Copy)]
pub struct HandleEntry {
    /// Object Manager ID. 0 = closed.
    /// HANDLE_STDIN/STDOUT/STDERR sentinels for standard streams.
    /// All other values reference an ObObject in the Object Manager.
    pub object_id: ObId,
    /// Per-handle offset for file-like objects (read/write position).
    pub offset: u64,
}

impl HandleEntry {
    pub const fn closed() -> Self {
        HandleEntry { object_id: HANDLE_CLOSED, offset: 0 }
    }

    pub const fn stdin() -> Self {
        HandleEntry { object_id: HANDLE_STDIN, offset: 0 }
    }

    pub const fn stdout() -> Self {
        HandleEntry { object_id: HANDLE_STDOUT, offset: 0 }
    }

    pub const fn stderr() -> Self {
        HandleEntry { object_id: HANDLE_STDERR, offset: 0 }
    }

    /// Create a pipe read handle, registered in the Object Manager.
    /// offset=0 marks this as a read end.
    pub fn pipe_read(pipe_id: u8) -> Self {
        let ob_id = ob_create_object(
            ObType::Pipe,
            core::str::from_utf8(&[b'P', b'I', b'P', b'E', b'_', b'R', pipe_id + b'0']).unwrap_or("PIPE_R"),
            pipe_id as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, offset: 0 }
    }

    /// Create a pipe write handle, registered in the Object Manager.
    /// offset=1 marks this as a write end.
    pub fn pipe_write(pipe_id: u8) -> Self {
        let ob_id = ob_create_object(
            ObType::Pipe,
            core::str::from_utf8(&[b'P', b'I', b'P', b'E', b'_', b'W', pipe_id + b'0']).unwrap_or("PIPE_W"),
            pipe_id as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, offset: 1 }
    }

    /// Create a file handle, registered in the Object Manager.
    /// The drive index is stored in the ObObject's flags field.
    pub fn file(drive: u8, inode: u32) -> Self {
        let ob_id = ob_create_object(
            ObType::Filesystem, "FILE", inode as u64, drive as u32, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, offset: 0 }
    }

    /// Create a device handle.
    pub fn device(device_id: u32) -> Self {
        let ob_id = ob_create_object(
            ObType::Device, "DEVICE", device_id as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, offset: 0 }
    }

    /// Create an event handle.
    pub fn event(event_type: u32) -> Self {
        let ob_id = ob_create_object(
            ObType::Event, "EVENT", event_type as u64, 0, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, offset: 0 }
    }

    /// Create a directory handle.
    /// The drive index is stored in the ObObject's flags field.
    pub fn dir(drive: u8, inode: u32) -> Self {
        let ob_id = ob_create_object(
            ObType::Directory, "DIR", inode as u64, drive as u32, None,
        ).unwrap_or(0);
        HandleEntry { object_id: ob_id, offset: 0 }
    }

    /// Create an Object Manager handle (backed by an existing ObObject).
    /// Used by ObOpen (RAX=60) to reference kernel objects via the namespace.
    pub fn ob_object(object_id: ObId, _access_mask: u32) -> Self {
        HandleEntry { object_id, offset: 0 }
    }

    /// Close this handle: release the Ob reference.
    /// Only calls ob_close_object if the ObObject is still alive.
    /// Double-close and stale-handle scenarios are safe.
    pub fn close(&mut self) {
        if self.has_ob_object() && self.is_valid() {
            let _ = ob_close_object(self.object_id);
        }
        *self = HandleEntry::closed();
    }

    /// True if this handle references a real ObObject (not a standard stream).
    /// Standard streams (stdin=MAX, stdout=MAX-1, stderr=MAX-2) and
    /// closed (0) all return false.
    pub fn has_ob_object(&self) -> bool {
        self.object_id > 0 && self.object_id < HANDLE_STDERR
    }

    pub fn is_open(&self) -> bool {
        self.object_id != HANDLE_CLOSED
    }

    pub fn is_stdio(&self) -> bool {
        self.object_id >= HANDLE_STDERR
    }

    pub fn is_stdin(&self) -> bool {
        self.object_id == HANDLE_STDIN
    }

    pub fn is_stdout(&self) -> bool {
        self.object_id == HANDLE_STDOUT
    }

    pub fn is_stderr(&self) -> bool {
        self.object_id == HANDLE_STDERR
    }

    /// Check whether the referenced ObObject still exists in the Object Manager.
    /// Standard-stream sentinels (stdin/stdout/stderr) always report valid.
    pub fn is_valid(&self) -> bool {
        if !self.has_ob_object() {
            return true;
        }
        ob_lookup(self.object_id).is_some()
    }

    /// True if the handle is open AND the referenced ObObject is still alive.
    pub fn is_open_and_valid(&self) -> bool {
        self.is_open() && self.is_valid()
    }

    /// True if this handle is a pipe read end (offset=0).
    pub fn is_pipe_read(&self) -> bool {
        self.obj_type() == Some(ObType::Pipe) && self.offset == 0
    }

    /// True if this handle is a pipe write end (offset=1).
    pub fn is_pipe_write(&self) -> bool {
        self.obj_type() == Some(ObType::Pipe) && self.offset == 1
    }

    /// Look up the associated ObObject, if any.
    pub fn obj_type(&self) -> Option<ObType> {
        if !self.has_ob_object() { return None; }
        ob_lookup(self.object_id).map(|o| o.obj_type)
    }

    /// Get the native_id from the ObObject (pipe_id, inode, pid, etc.)
    pub fn native_id(&self) -> Option<u64> {
        if !self.has_ob_object() { return None; }
        ob_lookup(self.object_id).map(|o| o.native_id)
    }

    /// Get the drive index from a file/dir ObObject (stored in flags).
    pub fn drive(&self) -> Option<u8> {
        if !self.has_ob_object() { return None; }
        ob_lookup(self.object_id).map(|o| (o.flags & 0xFF) as u8)
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
            if !self.entries[i].is_open() {
                self.entries[i] = entry;
                return Some(i as u8);
            }
        }
        let len = self.entries.len();
        if len > 255 {
            return None;
        }
        let fd = len as u8;
        self.entries.push(entry);
        Some(fd)
    }

    pub fn alloc_two_handles(&mut self, e1: HandleEntry, e2: HandleEntry) -> Option<(u8, u8)> {
        let mut first: Option<u8> = None;
        let mut second: Option<u8> = None;
        for i in 3..self.entries.len() {
            if !self.entries[i].is_open() {
                if first.is_none() {
                    first = Some(i as u8);
                } else if second.is_none() {
                    second = Some(i as u8);
                    break;
                }
            }
        }
        if let (Some(f), None) = (first, second) {
            self.entries.push(e2);
            self.entries[f as usize] = e1;
            return Some((f, self.entries.len() as u8 - 1));
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