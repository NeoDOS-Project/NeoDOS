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

// ═══════════════════════════════════════════════════════════════════════
// SMP observability — additive ObInfoClass payloads (ABI v8 compatible)
//
// These structs are NEW. They do not alter any existing struct layout.
// They are returned by the new `ObInfoClass::CpuStats` (24) and
// `ObInfoClass::ThreadStats` (25) classes.
//
// Snapshot protocol (both classes):
//   buffer = [StatsHeader][Entry; returned]
//   - `total`    : objects available in this snapshot
//   - `returned` : objects actually copied (may be < total if buffer small)
//   - `entry_size` : sizeof(Entry) for forward-compatible parsing
//   The syscall returns the number of bytes written. A caller detecting
//   `returned < total` knows the snapshot was truncated (no silent loss).
// ═══════════════════════════════════════════════════════════════════════

/// Version of the stats snapshot layout (bump on incompatible change).
pub const STATS_VERSION: u32 = 1;

/// Common header for `CpuStats` / `ThreadStats` snapshots. 16 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StatsHeader {
    pub version: u32,
    pub total: u32,
    pub returned: u32,
    pub entry_size: u32,
}

/// Per-CPU counters snapshot. `online` is 0/1. Counters are monotonic per
/// CPU (`timer_tick_count` is NOT CPU busy time — it counts timer interrupts).
///
/// NOTE: `interrupt_count` is exposed for ABI completeness but the kernel
/// currently has no increment site for it, so it reads 0. `timer_tick_count`
/// and `context_switch_count` are maintained. 40 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CpuStatsEntry {
    pub interrupt_count: u64,
    pub context_switch_count: u64,
    pub timer_tick_count: u64,
    pub cpu_id: u32,
    pub apic_id: u32,
    pub online: u8,
    pub _pad: [u8; 7],
}

/// Per-thread snapshot. `cpu_id` is `Kthread.cpu` (the CPU the thread is
/// currently enqueued/running on), NOT derived from any other field.
/// 24 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ThreadStatsEntry {
    pub tid: u32,
    pub pid: u32,
    pub cpu_id: u32,
    pub priority: u8,
    pub state: u8,
    pub _pad: [u8; 2],
    pub cpu_ticks: u64,
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
