//! Ob ABI types — extracted from ob.rs (mechanical split)

#[repr(C)]
pub struct ObPipeFds {
    pub reader_fd: u64,
    pub writer_fd: u64,
}


#[repr(C)]
pub struct ObBasicInfo {
    pub obj_type: u32,
    pub refcount: u32,
    pub name: [u8; 32],
}


#[repr(C)]
pub struct ObFileInfo {
    pub size: u64,
    pub drive: u8,
    pub inode: u32,
    pub padding: [u8; 3],
}


#[repr(C)]
pub struct ObProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub priority: u8,
    pub thread_count: u32,
    pub state: u8,
    pub padding: [u8; 2],
}


#[repr(C)]
pub struct ObPipeInfo {
    pub capacity: u32,
    pub read_refs: u32,
    pub write_refs: u32,
}


#[repr(C)]
pub struct ObThreadInfo {
    pub tid: u32,
    pub pid: u32,
    pub state: u8,
    pub priority: u8,
    pub padding: [u8; 2],
}


#[repr(C)]
pub struct ObDeviceInfo {
    pub device_id: u32,
    pub reserved: u32,
}


#[repr(C)]
pub struct SysDateTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    pub valid: u8,
}


#[repr(C)]
pub struct DriveInfoRaw {
    pub letter: u8,
    pub present: u8,
    pub fs_type: [u8; 16],
    pub label: [u8; 32],
    pub total_sectors: u64,
}


#[repr(C)]
pub struct DriverInfoRaw {
    pub id: u32,
    pub state: u8,
    pub category: u8,
    pub driver_type: u8,
    pub api_version: u16,
    pub abi_min: u16,
    pub abi_target: u16,
    pub abi_max: u16,
    pub last_error: u32,
    pub caps: u64,
    pub isolation_mode: u8,
    pub events_received: u64,
    pub tick_count: u64,
    pub registered_at_tick: u64,
    pub name: [u8; 8],
}
