//! Unified Handle Table — per-process resource abstraction.
//!
//! Every opened resource (file, pipe, device, event) is represented by a
//! handle entry in a fixed-size per-process table.  System calls operate
//! on handles (small integers / fd numbers) instead of raw kernel objects,
//! providing a uniform access model.

pub const HANDLE_CLOSED: u8 = 0;
pub const HANDLE_STDIN: u8 = 1;
pub const HANDLE_STDOUT: u8 = 2;
pub const HANDLE_STDERR: u8 = 3;
pub const HANDLE_PIPE_READ: u8 = 4;
pub const HANDLE_PIPE_WRITE: u8 = 5;
pub const HANDLE_FILE: u8 = 6;
pub const HANDLE_DEVICE: u8 = 7;
pub const HANDLE_EVENT: u8 = 8;

pub const MAX_HANDLES: usize = 16;

/// A single entry in the per-process handle table.
///
/// Interpretation of fields depends on `kind`:
///
/// | kind              | id              | extra           | offset         |
/// |-------------------|-----------------|-----------------|----------------|
/// | CLOSED            | —               | —               | —              |
/// | STDIN / STDOUT / STDERR | —         | —               | —              |
/// | PIPE_READ / PIPE_WRITE | pipe_id   | —               | —              |
/// | FILE              | inode           | drive_idx       | r/w cursor     |
/// | DEVICE            | device_id       | —               | —              |
/// | EVENT             | event_type      | —               | —              |
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

pub type HandleTable = [HandleEntry; MAX_HANDLES];

/// Create the default handle table for a Ring 3 process:
///  fd 0 = stdin, fd 1 = stdout, fd 2 = stderr, rest closed.
pub fn default_handle_table() -> HandleTable {
    let mut table = [HandleEntry::closed(); MAX_HANDLES];
    table[0] = HandleEntry::stdin();
    table[1] = HandleEntry::stdout();
    table[2] = HandleEntry::stderr();
    table
}

/// Create a fully closed handle table (for Ring 0 kernel threads).
pub fn closed_handle_table() -> HandleTable {
    [HandleEntry::closed(); MAX_HANDLES]
}

/// Find the lowest available (closed) handle slot, starting from `start`.
pub fn alloc_handle(table: &mut HandleTable, entry: HandleEntry) -> Option<u8> {
    for i in 3..MAX_HANDLES {
        if table[i].kind == HANDLE_CLOSED {
            table[i] = entry;
            return Some(i as u8);
        }
    }
    None
}

/// Find two consecutive closed handle slots starting from `start`.
pub fn alloc_two_handles(table: &mut HandleTable, e1: HandleEntry, e2: HandleEntry) -> Option<(u8, u8)> {
    let mut first: Option<u8> = None;
    let mut second: Option<u8> = None;
    for i in 3..MAX_HANDLES {
        if table[i].kind == HANDLE_CLOSED {
            if first.is_none() {
                first = Some(i as u8);
            } else if second.is_none() {
                second = Some(i as u8);
                break;
            }
        }
    }
    match (first, second) {
        (Some(a), Some(b)) => {
            table[a as usize] = e1;
            table[b as usize] = e2;
            Some((a, b))
        }
        _ => None,
    }
}
