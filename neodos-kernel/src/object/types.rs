use alloc::string::String;

pub const OB_NAME_LEN: usize = 32;

pub type ObId = u64;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObType {
    Unknown = 0,
    Process = 1,
    Driver = 2,
    Device = 3,
    Pipe = 4,
    EventBus = 5,
    BlockDevice = 6,
    Filesystem = 7,
    MemoryRegion = 8,
    Symlink = 9,
    MountPoint = 10,
    Directory = 11,
    Key = 12,
    Event = 13,
    Semaphore = 14,
    Timer = 15,
}

impl ObType {
    pub fn to_str(self) -> &'static str {
        match self {
            ObType::Unknown => "UNKNOWN",
            ObType::Process => "PROCESS",
            ObType::Driver => "DRIVER",
            ObType::Device => "DEVICE",
            ObType::Pipe => "PIPE",
            ObType::EventBus => "EVENTBUS",
            ObType::BlockDevice => "BLOCKDEV",
            ObType::Filesystem => "FILESYSTEM",
            ObType::MemoryRegion => "MEMREGION",
            ObType::Symlink => "SYMLINK",
            ObType::MountPoint => "MOUNTPOINT",
            ObType::Directory => "DIRECTORY",
            ObType::Key => "REGKEY",
            ObType::Event => "EVENT",
            ObType::Semaphore => "SEMAPHORE",
            ObType::Timer => "TIMER",
        }
    }
}

/// Error codes for Object Manager operations.
#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObError {
    Success = 0,
    NotFound = -1,
    AlreadyExists = -2,
    InvalidParam = -3,
    RefCountHeld = -4,
    OutOfMemory = -5,
    AccessDenied = -6,
    NotSupported = -7,
    InvalidType = -8,
    TableFull = -9,
}

impl ObError {
    pub fn to_str(self) -> &'static str {
        match self {
            ObError::Success => "SUCCESS",
            ObError::NotFound => "NOT_FOUND",
            ObError::AlreadyExists => "ALREADY_EXISTS",
            ObError::InvalidParam => "INVALID_PARAM",
            ObError::RefCountHeld => "REFCOUNT_HELD",
            ObError::OutOfMemory => "OUT_OF_MEMORY",
            ObError::AccessDenied => "ACCESS_DENIED",
            ObError::NotSupported => "NOT_SUPPORTED",
            ObError::InvalidType => "INVALID_TYPE",
            ObError::TableFull => "TABLE_FULL",
        }
    }

    pub fn as_err_code(self) -> i64 {
        self as i64
    }
}

/// Snapshot of an object for enumeration (no borrow on the table).
#[derive(Debug, Clone)]
pub struct ObObjectSnapshot {
    pub id: ObId,
    pub obj_type: ObType,
    pub name: String,
    pub refcount: u32,
    pub flags: u32,
    pub native_id: u64,
}
