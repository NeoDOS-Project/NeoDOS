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

/// Version of the `SmpStats` layout (bump on incompatible change).
pub const SMP_STATS_VERSION: u32 = 1;

/// Global work-stealing / SMP balancing counters. `steal_attempts` counts every
/// `try_work_steal` call; `steal_success` counts threads actually migrated.
/// 24 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SmpStats {
    pub version: u32,
    pub _pad: u32,
    pub steal_attempts: u64,
    pub steal_success: u64,
}

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

// ── Phase 15-A: coherent process/thread snapshot ABI ────────────────────
// One syscall returns both sections from a SINGLE scheduler-consistent
// snapshot (never two independent traversals). Bounded, explicit-width,
// no pointers.

/// Version of the combined process/thread snapshot ABI.
///
/// v2 (Phase 15-A.1) appends one authoritative CPU execution counter to each
/// process and thread record. v3 (MEM-PROC #274) appends `committed_bytes` and
/// `working_set_bytes` to the process record. Both changes are additive *in
/// serialized size* — fields are appended — but not byte-compatible with prior
/// versions, so the version is bumped and both sides reject the other's layout
/// via `version` + `process_entry_size` / `thread_entry_size`.
pub const PROC_SNAPSHOT_VERSION: u32 = 3;
/// Maximum name bytes exposed (matches the kernel `KernelName` bound).
pub const PROC_NAME_MAX: usize = 32;
/// `ProcSnapshotHeader.flags` bit0: at least one section was truncated.
pub const PROC_SNAPSHOT_FLAG_TRUNCATED: u32 = 1;

/// Header for `ObInfoClass::ProcessSnapshot`. 32 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcSnapshotHeader {
    pub version: u32,
    pub process_total: u32,
    pub process_returned: u32,
    pub thread_total: u32,
    pub thread_returned: u32,
    pub process_entry_size: u32,
    pub thread_entry_size: u32,
    /// See `PROC_SNAPSHOT_FLAG_*`.
    pub flags: u32,
}

/// One process record. 64 bytes:
/// `pid:u32 | name:[u8;32] | thread_count:u32 | cpu_time:u64 |
///  committed_bytes:u64 | working_set_bytes:u64`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcessInfoRaw {
    pub pid: u32,
    pub name: [u8; PROC_NAME_MAX],
    pub thread_count: u32,
    /// Phase 15-A.1: sum of the process's thread CPU execution counters, in
    /// timer intervals. Monotonic; Δ between two snapshots yields CPU%.
    pub cpu_time: u64,
    /// MEM-PROC (#274): reserved/allocated bytes (heap span + mmap regions).
    pub committed_bytes: u64,
    /// MEM-PROC (#274): resident bytes (mapped 4 KB heap pages × 4096). Idle
    /// processes report 0.
    pub working_set_bytes: u64,
}

/// One thread record. 56 bytes:
/// `tid:u32 | pid:u32 | name:[u8;32] | state:u8 | idle:u8 | is_current:u8 |
///  _pad:u8 | cpu:u32 | cpu_time:u64`. `state` uses `ThreadState::to_u8`:
/// 0=Ready, 1=Running, 2=Blocked, 3=Suspended, 4=Terminated.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ThreadInfoRaw {
    pub tid: u32,
    pub pid: u32,
    pub name: [u8; PROC_NAME_MAX],
    pub state: u8,
    pub idle: u8,
    pub is_current: u8,
    pub _pad: u8,
    pub cpu: u32,
    /// Phase 15-A.1: authoritative monotonic CPU execution counter (timer
    /// intervals). Idle threads report 0.
    pub cpu_time: u64,
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

/// Timezone configuration returned by `ObInfoClass::TimeZone` (#357).
#[repr(C)]
pub struct SysTimeZone {
    pub utc_offset_minutes: i32,
    pub dst_offset_minutes: i32,
    pub dst_enabled: u32,
    pub dst_start_month: u8,
    pub dst_start_day: u8,
    pub dst_end_month: u8,
    pub dst_end_day: u8,
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
