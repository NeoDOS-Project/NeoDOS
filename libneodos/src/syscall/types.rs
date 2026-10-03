//! ABI mirror types shared by the syscall wrappers.

// ── CpuInfoFull (mirrors kernel cpu::CpuInfoFull) ──

#[repr(C)]
pub struct CpuInfoFull {
    pub vendor_id: [u8; 12],
    pub brand: [u8; 48],
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub cpu_type: u32,
    pub features_edx: u32,
    pub features_ecx: u32,
    pub ext_features_edx: u32,
    pub ext_features_ecx: u32,
    pub features_ebx_leaf7: u32,
    pub phys_addr_bits: u8,
    pub virt_addr_bits: u8,
    pub cpu_count: u32,
    pub apic_id: u32,
    pub cpu_id: u32,
    pub is_bsp: bool,
    pub tsc_khz: u64,
    pub timer_source: u8,
    pub tick_rate_hz: u64,
}

impl CpuInfoFull {
    pub fn vendor_str(&self) -> &str {
        core::str::from_utf8(&self.vendor_id).unwrap_or("Unknown")
    }

    pub fn brand_str(&self) -> &str {
        let mut end = self.brand.len();
        while end > 0 && (self.brand[end - 1] == 0 || self.brand[end - 1] == b' ') {
            end -= 1;
        }
        core::str::from_utf8(&self.brand[..end]).unwrap_or("Unknown")
    }

    pub fn cpu_type_str(&self) -> &'static str {
        match self.cpu_type {
            0 => "Reserved (overclocked)",
            1 => "Other",
            2 => "Unknown",
            3 => "Normal desktop/mobile",
            _ => "Unknown",
        }
    }

    pub fn has_sse(&self) -> bool { (self.features_edx >> 25) & 1 == 1 }
    pub fn has_sse2(&self) -> bool { (self.features_edx >> 26) & 1 == 1 }
    pub fn has_sse3(&self) -> bool { (self.features_ecx >> 0) & 1 == 1 }
    pub fn has_ssse3(&self) -> bool { (self.features_ecx >> 9) & 1 == 1 }
    pub fn has_sse41(&self) -> bool { (self.features_ecx >> 19) & 1 == 1 }
    pub fn has_sse42(&self) -> bool { (self.features_ecx >> 20) & 1 == 1 }
    pub fn has_avx(&self) -> bool { (self.features_ecx >> 28) & 1 == 1 }
    pub fn has_avx2(&self) -> bool { (self.features_ebx_leaf7 >> 5) & 1 == 1 }
    pub fn has_aes(&self) -> bool { (self.features_ecx >> 25) & 1 == 1 }
    pub fn has_fma(&self) -> bool { (self.features_ecx >> 12) & 1 == 1 }
    pub fn has_f16c(&self) -> bool { (self.features_ecx >> 29) & 1 == 1 }
    pub fn has_popcnt(&self) -> bool { (self.features_ecx >> 23) & 1 == 1 }
    pub fn has_xsave(&self) -> bool { (self.features_ecx >> 26) & 1 == 1 }
    pub fn has_osxsave(&self) -> bool { (self.features_ecx >> 27) & 1 == 1 }
    pub fn has_rdrand(&self) -> bool { (self.features_ecx >> 30) & 1 == 1 }
    pub fn has_pclmulqdq(&self) -> bool { (self.features_ecx >> 1) & 1 == 1 }
    pub fn has_fsgsbase(&self) -> bool { (self.features_ebx_leaf7 >> 0) & 1 == 1 }
    pub fn has_bmi1(&self) -> bool { (self.features_ebx_leaf7 >> 3) & 1 == 1 }
    pub fn has_bmi2(&self) -> bool { (self.features_ebx_leaf7 >> 8) & 1 == 1 }
    pub fn has_hle(&self) -> bool { (self.features_ebx_leaf7 >> 4) & 1 == 1 }
    pub fn has_rtm(&self) -> bool { (self.features_ebx_leaf7 >> 11) & 1 == 1 }
    pub fn has_smep(&self) -> bool { (self.features_ebx_leaf7 >> 7) & 1 == 1 }
    pub fn has_erms(&self) -> bool { (self.features_ebx_leaf7 >> 9) & 1 == 1 }
    pub fn has_invcpcid(&self) -> bool { (self.features_ebx_leaf7 >> 10) & 1 == 1 }
    pub fn has_x2apic(&self) -> bool { (self.features_ecx >> 21) & 1 == 1 }
    pub fn has_htt(&self) -> bool { (self.features_edx >> 28) & 1 == 1 }
    pub fn has_nx(&self) -> bool { (self.ext_features_edx >> 20) & 1 == 1 }
    pub fn has_long_mode(&self) -> bool { (self.ext_features_edx >> 29) & 1 == 1 }
    pub fn has_syscall(&self) -> bool { (self.ext_features_edx >> 11) & 1 == 1 }
    pub fn has_mmx(&self) -> bool { (self.features_edx >> 23) & 1 == 1 }
    pub fn has_fxsr(&self) -> bool { (self.features_edx >> 24) & 1 == 1 }
}

/// DirEntry — matches kernel's DirEntryRaw (RAX=8).
#[repr(C)]
pub struct DirEntry {
    pub inode: u32,
    pub mode: u16,
    pub size: u32,
    pub name: [u8; 260],
}

impl DirEntry {
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(0);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

/// DateTime — matches kernel's SysDateTime (RAX=44).
#[repr(C)]
pub struct DateTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    pub valid: u8,
}

/// Timezone configuration — matches the kernel's `SysTimeZone` (RAX=44/#357).
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

/// MemInfo — matches kernel's extended MemoryStats (NeoMem v0.1).
/// Backward compatible: first 6 fields (48 bytes) match the v0.44 layout.
#[repr(C)]
pub struct MemInfo {
    // Physical memory (6 fields, backward compatible)
    pub phys_max: u64,
    pub total_kib: u64,
    pub usable_kib: u64,
    pub free_kib: u64,
    pub used_kib: u64,
    pub reserved_kib: u64,

    // Kernel heap (added in v0.46 / NeoMem v0.1)
    pub kernel_heap_total_kib: u64,
    pub kernel_heap_used_kib: u64,
    pub kernel_heap_free_kib: u64,

    // User memory pools
    pub user_memory_total_kib: u64,
    pub user_memory_used_kib: u64,
    pub user_memory_free_kib: u64,

    // Paging
    pub total_pages: u64,
    pub free_pages: u64,
    pub used_pages: u64,
}

/// DriveInfo — matches kernel's DriveInfoRaw (RAX=33).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DriveInfo {
    pub letter: u8,
    pub present: u8,
    pub fs_type: [u8; 16],
    pub label: [u8; 32],
    pub total_sectors: u64,
}

impl DriveInfo {
    pub fn fs_type_str(&self) -> &str {
        let end = self.fs_type.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.fs_type[..end]).unwrap_or("Unknown")
    }

    pub fn label_str(&self) -> &str {
        let end = self.label.iter().position(|&b| b == 0).unwrap_or(32);
        if end == 0 { return "(no label)"; }
        core::str::from_utf8(&self.label[..end]).unwrap_or("")
    }
}

/// DriverInfo — mirrors kernel's DriverInfoRaw (RAX=56).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DriverInfo {
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

impl DriverInfo {
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(8);
        core::str::from_utf8(&self.name[..end]).unwrap_or("<?>")
    }

    pub fn state_str(&self) -> &'static str {
        match self.state {
            0 => "Loaded",
            1 => "Initialized",
            2 => "Registered",
            3 => "Bound",
            4 => "Active",
            5 => "Faulted",
            6 => "Unloaded",
            7 => "Unloading",
            _ => "Unknown",
        }
    }

    pub fn category_str(&self) -> &'static str {
        match self.category {
            0 => "Boot",
            1 => "System",
            2 => "Demand",
            _ => "Unknown",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SMP observability — additive ABI (ObInfoClass::CpuStats / ThreadStats)
//
// Mirrors `neodos-kernel/src/syscall/ob/types.rs`. These are new structs;
// no existing layout changes. Snapshot protocol:
//   buffer = [StatsHeader][Entry; returned]
//   total    = objects available this snapshot
//   returned = objects copied (returned < total ⇒ truncated)
// ═══════════════════════════════════════════════════════════════════════

/// Version of the stats snapshot layout.
pub const STATS_VERSION: u32 = 1;

/// Common header for `CpuStats` / `ThreadStats` snapshots (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StatsHeader {
    pub version: u32,
    pub total: u32,
    pub returned: u32,
    pub entry_size: u32,
}

impl StatsHeader {
    /// True when the kernel had more objects than fit in the caller's buffer.
    pub fn truncated(&self) -> bool {
        self.returned < self.total
    }
}

/// Per-CPU counters snapshot (40 bytes). `timer_tick_count` counts timer
/// interrupts, NOT CPU busy time. `interrupt_count` is currently not
/// incremented by the kernel and reads 0.
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

impl CpuStatsEntry {
    pub fn is_online(&self) -> bool {
        self.online != 0
    }
}

/// Per-thread snapshot (24 bytes). `cpu_id` is the CPU the thread is currently
/// enqueued/running on (`Kthread.cpu`).
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

impl ThreadStatsEntry {
    /// Kernel `ThreadState::to_u8` encoding.
    pub fn state_str(&self) -> &'static str {
        match self.state {
            0 => "Ready",
            1 => "Running",
            2 => "Blocked",
            3 => "Suspended",
            4 => "Terminated",
            _ => "?",
        }
    }

    pub fn priority_str(&self) -> &'static str {
        match self.priority {
            0 => "HIGH",
            1 => "ABOVE_NORMAL",
            2 => "NORMAL",
            3 => "IDLE",
            _ => "?",
        }
    }
}

// ── Phase 15-A: coherent process/thread snapshot ABI ──
// Mirrors `neodos-kernel/src/syscall/ob/types.rs`. One query returns both
// sections from a single scheduler-consistent snapshot:
//   buffer = [ProcSnapshotHeader][ProcessInfoRaw; process_returned]
//                              [ThreadInfoRaw; thread_returned]
/// Version of the process/thread snapshot ABI.
///
/// v2 (Phase 15-A.1) appends the authoritative per-process/thread CPU execution
/// counter. Not byte-compatible with v1 (header + record sizes differ), so both
/// sides validate the version and entry sizes before parsing.
pub const PROC_SNAPSHOT_VERSION: u32 = 3;
/// Maximum name bytes exposed (matches the kernel `KernelName` bound).
pub const PROC_NAME_MAX: usize = 32;
/// `ProcSnapshotHeader.flags` bit0: at least one section was truncated.
pub const PROC_SNAPSHOT_FLAG_TRUNCATED: u32 = 1;

/// Header for `ObInfoClass::ProcessSnapshot` (32 bytes).
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
    pub flags: u32,
}

impl ProcSnapshotHeader {
    /// True when either section was truncated.
    pub fn truncated(&self) -> bool {
        self.flags & PROC_SNAPSHOT_FLAG_TRUNCATED != 0
            || self.process_returned < self.process_total
            || self.thread_returned < self.thread_total
    }
}

/// One process record (64 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcessInfoRaw {
    pub pid: u32,
    pub name: [u8; PROC_NAME_MAX],
    pub thread_count: u32,
    /// CPU execution counter (timer intervals); Δ between snapshots → CPU%.
    pub cpu_time: u64,
    /// MEM-PROC (#274): reserved/allocated bytes (heap span + mmap regions).
    pub committed_bytes: u64,
    /// MEM-PROC (#274): resident bytes (mapped 4 KB heap pages × 4096).
    pub working_set_bytes: u64,
}

impl ProcessInfoRaw {
    pub fn name_str(&self) -> &str {
        bytes_to_str(&self.name)
    }
}

/// One thread record (56 bytes). `state` uses the kernel `ThreadState::to_u8`
/// encoding (0 Ready, 1 Running, 2 Blocked, 3 Suspended, 4 Terminated).
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
    /// CPU execution counter (timer intervals). Idle threads report 0.
    pub cpu_time: u64,
}

impl ThreadInfoRaw {
    pub fn name_str(&self) -> &str {
        bytes_to_str(&self.name)
    }
    pub fn state_str(&self) -> &'static str {
        match self.state {
            0 => "Ready",
            1 => "Running",
            2 => "Blocked",
            3 => "Suspended",
            4 => "Terminated",
            _ => "?",
        }
    }
    pub fn is_idle(&self) -> bool { self.idle != 0 }
    pub fn is_current_thread(&self) -> bool { self.is_current != 0 }
}

fn bytes_to_str(n: &[u8; PROC_NAME_MAX]) -> &str {
    let end = n.iter().position(|&b| b == 0).unwrap_or(PROC_NAME_MAX);
    core::str::from_utf8(&n[..end]).unwrap_or("")
}

