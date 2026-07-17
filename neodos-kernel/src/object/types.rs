use alloc::string::String;

pub const OB_NAME_LEN: usize = 128;

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
    Thread = 16,
    Section = 17,
    Socket = 18,
    Service = 20,
    PowerManager = 21,
    KeyboardDevice = 22,
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
            ObType::Thread => "THREAD",
            ObType::Section => "SECTION",
            ObType::Socket => "SOCKET",
            ObType::Service => "SERVICE",
            ObType::PowerManager => "POWERMANAGER",
            ObType::KeyboardDevice => "KEYBOARD",
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

// ═══════════════════════════════════════════════════════════════════════
// OB-012: ObQueryInfo — Info Classes
// ═══════════════════════════════════════════════════════════════════════

/// Info classes for sys_ob_query_info (RAX=62).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObInfoClass {
    Basic = 0,
    Name = 1,
    File = 2,
    Process = 3,
    Thread = 4,
    Pipe = 5,
    Device = 6,
    CpuInfo = 7,
    Version = 8,
    DateTime = 9,
    Memory = 10,
    Drives = 11,
    Drivers = 12,
    Cwd = 13,
    KeyboardLayout = 14,
    ReadContent = 15,
    VolumeLabel = 16,
    SocketInfo = 17,
    SocketAddr = 18,
    TcpStatus = 19,
    NicInfo = 20,
    RegistryKey = 21,
    RegistryValue = 22,
    SocketRecv = 23,
    ServiceState = 29,
    ServiceConfig = 30,
    ServiceStatus = 31,
    PowerState = 32,
    FsckStatus = 33,
    ProcessId = 34,
    KeyboardInfo = 35,
    KeyboardCaps = 36,
    KeyboardLayouts = 37,
    Hostname = 38,
}

/// Info classes for sys_ob_set_info (RAX=63).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObSetInfoClass {
    ProcessPriority = 0,
    ThreadPriority = 1,
    ObjectName = 2,
    Security = 3,
    ProcessTerminate = 4,
    KeyboardLayout = 5,
    VfsRename = 6,
    WriteContent = 7,
    SetCwd = 8,
    SetVolumeLabel = 9,
    TimerStart = 10,
    TimerCancel = 11,
    SemaphoreRelease = 12,
    SectionMapView = 13,
    SectionUnmapView = 14,
    FileCreate = 15,
    FileDelete = 16,
    SetProcessVt = 17,
    SocketConnect = 18,
    SocketBind = 19,
    SocketListen = 20,
    SocketSend = 21,
    SocketClose = 22,
    RegistryCreateKey = 23,
    RegistryDeleteKey = 24,
    RegistrySetValue = 25,
    RegistryDeleteValue = 26,
    SetNicIp = 27,
    ServiceStart = 33,
    ServiceStop = 34,
    ServiceRestart = 35,
    ServiceSetConfig = 36,
    PowerShutdown = 37,
    PowerReboot = 38,
    FsckRepair = 39,
    KeyboardSetLayout = 43,
    KeyboardSetRepeatDelay = 44,
    KeyboardSetRepeatRate = 45,
    KeyboardSetLeds = 46,
    KeyboardSetModifier = 47,
    SetHostname = 49,
}

// ═══════════════════════════════════════════════════════════════════════
// OB-014: ObEnum — Directory Entry
// ═══════════════════════════════════════════════════════════════════════

/// ABI-stable entry written by sys_ob_enum (RAX=64).
/// Compatible extension: old code can read id+obj_type+name (first 44 bytes),
/// new code additionally reads mode+size (52 bytes total).
#[repr(C)]
pub struct ObEnumEntry {
    pub id: ObId,
    pub obj_type: u32,
    pub name: [u8; 32],
    pub mode: u16,
    pub _pad: [u8; 2],
    pub size: u32,
}
