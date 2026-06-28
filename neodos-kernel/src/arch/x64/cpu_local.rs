//! Per-CPU data structures — KPRCB (Kernel Processor Control Block).
//!
//! Each CPU has a 4 KB page containing its KPRCB. The GS segment base
//! is set to point at this page, allowing lock-free access to per-CPU
//! state via `gs:[offset]` addressing.
//!
//! Layout (4 KB page, `#[repr(C, align(4096))]`):
//!
//! | Offset | Size | Field                 |
//! |--------|------|-----------------------|
//! | 0x000  | 4    | cpu_id                |
//! | 0x004  | 4    | apic_id               |
//! | 0x008  | 8    | current_thread (ptr)   |
//! | 0x010  | 4    | current_pid           |
//! | 0x014  | 1    | idle                  |
//! | 0x015  | 1    | need_resched          |
//! | 0x016  | 1    | current_irql          |
//! | 0x017  | 1    | _pad0                 |
//! | 0x018  | 32   | run_queue (CpuRunQueue) |
//! | 0x038  | 288  | slab_caches[9] (PerCpuSlabCache) |
//! | 0x158  | 8    | interrupt_count       |
//! | 0x160  | 8    | context_switch_count  |
//! | 0x168  | 8    | timer_tick_count      |
//! | 0x170  | 64   | exit context (RSP, RIP, RBX, R12-R15, RBP) |
//! | 0x1B0  | 1    | exit_now              |
//! | 0x1B1  | 1    | _pad1                 |
//! | ...    | ...  | (remaining bytes)     |

use crate::scheduler::Kthread;

// ── Constants ────────────────────────────────────────────────────────────

/// Maximum number of CPUs supported.
pub const MAX_CPUS: usize = 16;

/// Number of per-CPU slab cache size classes.
pub const NUM_SLAB_CACHES: usize = 9;

/// Size classes for per-CPU slab caches (power-of-two, 8B to 2KB).
pub const SLAB_CACHE_SIZES: [usize; NUM_SLAB_CACHES] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];

/// Maximum number of free objects in a per-CPU slab cache hot list.
pub const SLAB_BATCH_SIZE: usize = 32;

// ── Per-CPU run queue ────────────────────────────────────────────────────

/// Simple per-CPU run queue: ring buffer of TIDs.
/// No locks needed — only the owning CPU accesses this.
#[repr(C)]
pub struct CpuRunQueue {
    /// Ring buffer of TIDs.
    pub entries: [u32; 64],
    pub head_idx: u16,
    pub tail_idx: u16,
    pub count: u16,
}

impl CpuRunQueue {
    pub const fn new() -> Self {
        CpuRunQueue {
            entries: [0u32; 64],
            head_idx: 0,
            tail_idx: 0,
            count: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, tid: u32) -> bool {
        if self.count as usize >= self.entries.len() {
            return false;
        }
        self.entries[self.tail_idx as usize] = tid;
        self.tail_idx = self.tail_idx.wrapping_add(1);
        self.count += 1;
        true
    }

    #[inline]
    pub fn pop(&mut self) -> Option<u32> {
        if self.count == 0 {
            return None;
        }
        let tid = self.entries[self.head_idx as usize];
        self.head_idx = self.head_idx.wrapping_add(1);
        self.count -= 1;
        Some(tid)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline]
    pub fn len(&self) -> u16 {
        self.count
    }

    /// Peek at the front without removing.
    #[inline]
    pub fn peek(&self) -> Option<u32> {
        if self.count == 0 {
            None
        } else {
            Some(self.entries[self.head_idx as usize])
        }
    }
}

// ── Per-CPU slab cache ───────────────────────────────────────────────────

/// Per-CPU slab cache for a single size class.
/// Hot objects are stored in `free_list` for O(1) alloc/free without locks.
#[repr(C)]
pub struct PerCpuSlabCache {
    /// Head of per-CPU slab page linked list (raw pointer, opaque to cpu_local).
    pub head: *mut u8,
    /// Hot cache of free objects (fast path).
    pub free_list: [*mut u8; SLAB_BATCH_SIZE],
    /// Number of objects in free_list.
    pub free_count: u16,
    /// Slot size for this cache.
    pub slot_size: u16,
    /// Total allocations from this cache (stats).
    pub total_allocated: u64,
    /// Total frees to this cache (stats).
    pub total_freed: u64,
}

impl PerCpuSlabCache {
    pub const fn new(slot_size: usize) -> Self {
        PerCpuSlabCache {
            head: core::ptr::null_mut(),
            free_list: [core::ptr::null_mut(); SLAB_BATCH_SIZE],
            free_count: 0,
            slot_size: slot_size as u16,
            total_allocated: 0,
            total_freed: 0,
        }
    }
}

// ── KPRCB (Kernel Processor Control Block) ───────────────────────────────

/// Per-CPU kernel data structure. One page (4 KB) per CPU.
/// Accessed via GS segment: `gs:0` points to the start of this struct.
///
/// # Layout (actual offsets, `#[repr(C, align(4096))]`):
///
/// ```text
/// 0x000: cpu_id          (u32)
/// 0x004: apic_id         (u32)
/// 0x008: current_thread  (*mut Kthread)
/// 0x010: current_pid     (u32)
/// 0x014: idle            (bool)
/// 0x015: need_resched    (bool)
/// 0x016: current_irql (u8)
/// 0x017: _pad0           (u8)
/// 0x018: run_queue       (CpuRunQueue, 264 bytes)
/// 0x120: slab_caches     ([PerCpuSlabCache; 9], 9 × 288 bytes)
/// 0xB40: interrupt_count (u64)
/// 0xB48: context_switch_count (u64)
/// 0xB50: timer_tick_count (u64)
/// 0xB58: exit_rsp        (u64)
/// 0xB60: exit_rip        (u64)
/// 0xB68: exit_rbx        (u64)
/// 0xB70: exit_r12        (u64)
/// 0xB78: exit_r13        (u64)
/// 0xB80: exit_r14        (u64)
/// 0xB88: exit_r15        (u64)
/// 0xB90: exit_rbp        (u64)
/// 0xB98: exit_now        (bool)
/// ```
///
/// Total data: ~2969 bytes. Tail padding to 4096 via `align(4096)`.
///
/// # Safety
/// Fields are accessed via raw GS-segment reads in the hot path.
/// Only the owning CPU should write to its own KPRCB (except during
/// AP startup when the BSP initializes the AP's KPRCB).
#[repr(C, align(4096))]
pub struct Kprcb {
    // ── Offset 0x000: CPU identification ──
    /// Logical CPU index (0 = BSP).
    pub cpu_id: u32,                           // 0x000
    /// APIC ID (hardware identifier).
    pub apic_id: u32,                          // 0x004

    // ── Offset 0x008: Current execution ──
    /// Raw pointer to the currently running Kthread.
    pub current_thread: *mut Kthread,          // 0x008
    /// PID of the current process.
    pub current_pid: u32,                      // 0x010
    /// Whether this CPU is running the idle task.
    pub idle: bool,                            // 0x014
    /// Per-CPU NEED_RESCHED flag (set by timer, cleared on syscall return).
    pub need_resched: bool,                    // 0x015
    /// Current IRQL level (0=PASSIVE, 1=APC, 2=DISPATCH, 3+=DIRQL, 15=HIGH).
    pub current_irql: u8,                      // 0x016
    _pad0: u8,                                // 0x017

    // ── Offset 0x018: Per-CPU run queue ──
    pub run_queue: CpuRunQueue,                // 0x018 (1024+ bytes)

    // ── Offset 0x418: Per-CPU slab caches ──
    pub slab_caches: [PerCpuSlabCache; NUM_SLAB_CACHES],  // 9 × ~40 bytes

    // ── Offset 0x600+: Statistics ──
    pub interrupt_count: u64,                  // 0x600
    pub context_switch_count: u64,             // 0x608
    pub timer_tick_count: u64,                 // 0x610

    // ── Offset 0x618: Exit trampoline (per-CPU) ──
    pub exit_rsp: u64,                         // 0x618
    pub exit_rip: u64,                         // 0x620
    pub exit_rbx: u64,                         // 0x628
    pub exit_r12: u64,                         // 0x630
    pub exit_r13: u64,                         // 0x638
    pub exit_r14: u64,                         // 0x640
    pub exit_r15: u64,                         // 0x648
    pub exit_rbp: u64,                         // 0x650
    pub exit_now: bool,
}

impl Kprcb {
    /// Create a new KPRCB for the given CPU.
    pub fn new(cpu_id: u32, apic_id: u32) -> Self {
        Kprcb {
            cpu_id,
            apic_id,
            current_thread: core::ptr::null_mut(),
            current_pid: 0,
            idle: true,
            need_resched: false,
            current_irql: 0,
            _pad0: 0,
            run_queue: CpuRunQueue::new(),
            slab_caches: Self::new_slab_caches(),
            interrupt_count: 0,
            context_switch_count: 0,
            timer_tick_count: 0,
            exit_rsp: 0,
            exit_rip: 0,
            exit_rbx: 0,
            exit_r12: 0,
            exit_r13: 0,
            exit_r14: 0,
            exit_r15: 0,
            exit_rbp: 0,
            exit_now: false,
        }
    }

    const fn new_slab_caches() -> [PerCpuSlabCache; NUM_SLAB_CACHES] {
        [
            PerCpuSlabCache::new(8),
            PerCpuSlabCache::new(16),
            PerCpuSlabCache::new(32),
            PerCpuSlabCache::new(64),
            PerCpuSlabCache::new(128),
            PerCpuSlabCache::new(256),
            PerCpuSlabCache::new(512),
            PerCpuSlabCache::new(1024),
            PerCpuSlabCache::new(2048),
        ]
    }
}

// ── Compile-time offset verification ───────────────────────────────────────
// These assertions verify that the OFFSET_* constants match the actual
// struct field offsets. If the struct layout changes, the build will fail.
const _: () = assert!(OFFSET_CPU_ID as usize == core::mem::offset_of!(Kprcb, cpu_id));
const _: () = assert!(OFFSET_APIC_ID as usize == core::mem::offset_of!(Kprcb, apic_id));
const _: () = assert!(OFFSET_CURRENT_THREAD as usize == core::mem::offset_of!(Kprcb, current_thread));
const _: () = assert!(OFFSET_CURRENT_PID as usize == core::mem::offset_of!(Kprcb, current_pid));
const _: () = assert!(OFFSET_IDLE as usize == core::mem::offset_of!(Kprcb, idle));
const _: () = assert!(OFFSET_NEED_RESCHED as usize == core::mem::offset_of!(Kprcb, need_resched));
const _: () = assert!(OFFSET_CURRENT_IRQL as usize == core::mem::offset_of!(Kprcb, current_irql));
const _: () = assert!(OFFSET_RUN_QUEUE as usize == core::mem::offset_of!(Kprcb, run_queue));
const _: () = assert!(OFFSET_SLAB_CACHES as usize == core::mem::offset_of!(Kprcb, slab_caches));
const _: () = assert!(OFFSET_INTERRUPT_COUNT as usize == core::mem::offset_of!(Kprcb, interrupt_count));
const _: () = assert!(OFFSET_CONTEXT_SWITCH_COUNT as usize == core::mem::offset_of!(Kprcb, context_switch_count));
const _: () = assert!(OFFSET_TIMER_TICK_COUNT as usize == core::mem::offset_of!(Kprcb, timer_tick_count));
const _: () = assert!(OFFSET_EXIT_RSP as usize == core::mem::offset_of!(Kprcb, exit_rsp));
const _: () = assert!(OFFSET_EXIT_RIP as usize == core::mem::offset_of!(Kprcb, exit_rip));
const _: () = assert!(OFFSET_EXIT_RBX as usize == core::mem::offset_of!(Kprcb, exit_rbx));
const _: () = assert!(OFFSET_EXIT_R12 as usize == core::mem::offset_of!(Kprcb, exit_r12));
const _: () = assert!(OFFSET_EXIT_R13 as usize == core::mem::offset_of!(Kprcb, exit_r13));
const _: () = assert!(OFFSET_EXIT_R14 as usize == core::mem::offset_of!(Kprcb, exit_r14));
const _: () = assert!(OFFSET_EXIT_R15 as usize == core::mem::offset_of!(Kprcb, exit_r15));
const _: () = assert!(OFFSET_EXIT_RBP as usize == core::mem::offset_of!(Kprcb, exit_rbp));
const _: () = assert!(OFFSET_EXIT_NOW as usize == core::mem::offset_of!(Kprcb, exit_now));

// ── Static KPRCB storage
/// BSP (CPU 0) sets these during PHASE 2.2.
/// APs read their own entry during startup.
pub(crate) static mut KPRCB_PAGES: [u64; MAX_CPUS] = [0; MAX_CPUS];

/// Number of CPUs currently online.
static CPU_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Initialize KPRCB pages for all potential CPUs.
/// Called by BSP during boot (after heap is ready).
///
/// Allocates KPRCB structures from the kernel heap (which is already
/// identity-mapped read-write) rather than from the buddy allocator.
pub fn init_kprcb_pages() {
    unsafe {
        for (cpu, page) in KPRCB_PAGES.iter_mut().enumerate().take(MAX_CPUS) {
            // Allocate a 4 KB aligned KPRCB from the kernel heap.
            // Kprcb is #[repr(C, align(4096))] so Box gives page-aligned allocation.
            let kprcb = alloc::boxed::Box::new(Kprcb::new(cpu as u32, 0));
            let addr = alloc::boxed::Box::into_raw(kprcb) as u64;
            *page = addr;
            crate::serial_println!("[SMP] KPRCB[{}] at 0x{:x}", cpu, addr);
        }
    }
}

/// Get the physical address of a CPU's KPRCB page.
pub fn kprcb_page(cpu_id: usize) -> Option<u64> {
    if cpu_id >= MAX_CPUS {
        return None;
    }
    let addr = unsafe { KPRCB_PAGES[cpu_id] };
    if addr == 0 { None } else { Some(addr) }
}

/// Set the APIC ID for a CPU's KPRCB.
pub fn set_apic_id(cpu_id: usize, apic_id: u32) {
    if let Some(page) = kprcb_page(cpu_id) {
        unsafe {
            let kprcb = page as *mut Kprcb;
            (*kprcb).apic_id = apic_id;
        }
    }
}

/// Mark a CPU as online.
pub fn mark_cpu_online(cpu_id: u32) {
    CPU_COUNT.store(cpu_id + 1, core::sync::atomic::Ordering::Relaxed);
}

/// Get the number of CPUs currently online.
pub fn cpu_count() -> u32 {
    CPU_COUNT.load(core::sync::atomic::Ordering::Relaxed)
}

// ── GS-based per-CPU access ──────────────────────────────────────────────

/// Read a u64 from the current CPU's KPRCB at the given byte offset.
///
/// # Safety
/// Requires GS base to be set (via `wrmsr(IA32_GS_BASE, kprcb_addr)`).
/// The offset must be within the KPRCB page (0..4096) and aligned to 8 bytes
/// for atomic access.
#[inline(always)]
pub unsafe fn gs_read_u64(offset: u32) -> u64 {
    crate::hal::raw::raw_gs_read_u64(offset)
}

/// Read a u32 from the current CPU's KPRCB at the given byte offset.
#[inline(always)]
pub unsafe fn gs_read_u32(offset: u32) -> u32 {
    crate::hal::raw::raw_gs_read_u32(offset)
}

/// Read a u8 from the current CPU's KPRCB at the given byte offset.
#[inline(always)]
pub unsafe fn gs_read_u8(offset: u32) -> u8 {
    crate::hal::raw::raw_gs_read_u8(offset)
}

/// Write a u64 to the current CPU's KPRCB at the given byte offset.
///
/// # Safety
/// Same requirements as `gs_read_u64`. Only the owning CPU should call this.
#[inline(always)]
pub unsafe fn gs_write_u64(offset: u32, val: u64) {
    crate::hal::raw::raw_gs_write_u64(offset, val);
}

/// Write a u8 to the current CPU's KPRCB at the given byte offset.
#[inline(always)]
pub unsafe fn gs_write_u8(offset: u32, val: u8) {
    crate::hal::raw::raw_gs_write_u8(offset, val);
}

// ── KPRCB field offsets (for GS-segment access) ──────────────────────────
//
// These are byte offsets into Kprcb for use with gs_read_*/gs_write_*.
// Generated from the struct layout.

pub const OFFSET_CPU_ID: u32 = 0x000;
pub const OFFSET_APIC_ID: u32 = 0x004;
pub const OFFSET_CURRENT_THREAD: u32 = 0x008;
pub const OFFSET_CURRENT_PID: u32 = 0x010;
pub const OFFSET_IDLE: u32 = 0x014;
pub const OFFSET_NEED_RESCHED: u32 = 0x015;
pub const OFFSET_CURRENT_IRQL: u32 = 0x016;
pub const OFFSET_RUN_QUEUE: u32 = 0x018;
pub const OFFSET_SLAB_CACHES: u32 = 0x120;
pub const OFFSET_INTERRUPT_COUNT: u32 = 0xB40;
pub const OFFSET_CONTEXT_SWITCH_COUNT: u32 = 0xB48;
pub const OFFSET_TIMER_TICK_COUNT: u32 = 0xB50;
pub const OFFSET_EXIT_RSP: u32 = 0xB58;
pub const OFFSET_EXIT_RIP: u32 = 0xB60;
pub const OFFSET_EXIT_RBX: u32 = 0xB68;
pub const OFFSET_EXIT_R12: u32 = 0xB70;
pub const OFFSET_EXIT_R13: u32 = 0xB78;
pub const OFFSET_EXIT_R14: u32 = 0xB80;
pub const OFFSET_EXIT_R15: u32 = 0xB88;
pub const OFFSET_EXIT_RBP: u32 = 0xB90;
pub const OFFSET_EXIT_NOW: u32 = 0xB98;

// ── High-level per-CPU accessors ─────────────────────────────────────────

/// Get the current CPU's ID (reads from KPRCB via GS segment).
///
/// # Safety
/// Requires GS base to be set.
#[inline(always)]
pub unsafe fn this_cpu_id() -> u32 {
    gs_read_u32(OFFSET_CPU_ID)
}

/// Get the current CPU's APIC ID.
#[inline(always)]
pub unsafe fn this_cpu_apic_id() -> u32 {
    gs_read_u32(OFFSET_APIC_ID)
}

/// Get a raw pointer to the current CPU's Kthread.
#[inline(always)]
pub unsafe fn this_cpu_current_thread() -> *mut Kthread {
    gs_read_u64(OFFSET_CURRENT_THREAD) as *mut Kthread
}

/// Set the current CPU's Kthread pointer.
#[inline(always)]
pub unsafe fn this_cpu_set_current_thread(ptr: *mut Kthread) {
    gs_write_u64(OFFSET_CURRENT_THREAD, ptr as u64);
}

/// Get the current CPU's PID.
#[inline(always)]
pub unsafe fn this_cpu_current_pid() -> u32 {
    gs_read_u32(OFFSET_CURRENT_PID)
}

/// Set the current CPU's PID.
#[inline(always)]
pub unsafe fn this_cpu_set_current_pid(pid: u32) {
    gs_write_u64(OFFSET_CURRENT_PID, pid as u64);
}

/// Check if the current CPU is idle.
#[inline(always)]
pub unsafe fn this_cpu_is_idle() -> bool {
    gs_read_u8(OFFSET_IDLE) != 0
}

/// Set the idle flag on the current CPU.
#[inline(always)]
pub unsafe fn this_cpu_set_idle(val: bool) {
    gs_write_u8(OFFSET_IDLE, val as u8);
}

/// Check the per-CPU NEED_RESCHED flag.
#[inline(always)]
pub unsafe fn this_cpu_need_resched() -> bool {
    gs_read_u8(OFFSET_NEED_RESCHED) != 0
}

/// Set the per-CPU NEED_RESCHED flag.
#[inline(always)]
pub unsafe fn this_cpu_set_need_resched(val: bool) {
    gs_write_u8(OFFSET_NEED_RESCHED, val as u8);
}

/// Get the current CPU's IRQL level.
#[inline(always)]
pub unsafe fn this_cpu_irql() -> u8 {
    gs_read_u8(OFFSET_CURRENT_IRQL)
}

/// Set the current CPU's IRQL level.
#[inline(always)]
pub unsafe fn this_cpu_set_irql(level: u8) {
    gs_write_u8(OFFSET_CURRENT_IRQL, level);
}

/// Check if the current CPU is at DISPATCH_LEVEL or higher (IRQL >= 2).
#[inline(always)]
pub unsafe fn this_cpu_in_dispatch_level() -> bool {
    this_cpu_irql() >= 2
}

/// Increment the per-CPU interrupt count.
#[inline(always)]
pub unsafe fn this_cpu_inc_interrupt_count() {
    let offset = OFFSET_INTERRUPT_COUNT;
    let val = gs_read_u64(offset);
    gs_write_u64(offset, val + 1);
}

/// Increment the per-CPU context switch count.
#[inline(always)]
pub unsafe fn this_cpu_inc_context_switch_count() {
    let offset = OFFSET_CONTEXT_SWITCH_COUNT;
    let val = gs_read_u64(offset);
    gs_write_u64(offset, val + 1);
}

/// Increment the per-CPU timer tick count.
#[inline(always)]
pub unsafe fn this_cpu_inc_timer_tick_count() {
    let offset = OFFSET_TIMER_TICK_COUNT;
    let val = gs_read_u64(offset);
    gs_write_u64(offset, val + 1);
}

// ── Run queue accessors ──────────────────────────────────────────────────

/// Get a mutable reference to the current CPU's run queue.
///
/// # Safety
/// Requires GS base to be set. Only the owning CPU should call this
/// (except during bootstrap when BSP initializes AP's KPRCB).
#[inline(always)]
pub unsafe fn this_cpu_run_queue_mut() -> &'static mut CpuRunQueue {
    let kprcb_addr = gs_read_u64(0); // GS base points to KPRCB start
    let rq_ptr = (kprcb_addr + OFFSET_RUN_QUEUE as u64) as *mut CpuRunQueue;
    &mut *rq_ptr
}

/// Get a mutable reference to a specific CPU's run queue by CPU index.
///
/// # Safety
/// Requires the target CPU's KPRCB to be initialized. Caller must ensure
/// no concurrent access to the same CPU's run queue (except for the
/// push-only cross-CPU case).
#[inline(always)]
pub unsafe fn cpu_run_queue_mut(cpu: usize) -> &'static mut CpuRunQueue {
    if cpu >= MAX_CPUS {
        panic!("cpu_run_queue_mut: cpu {} out of range", cpu);
    }
    let kprcb_addr = KPRCB_PAGES[cpu];
    if kprcb_addr == 0 {
        panic!("cpu_run_queue_mut: CPU {} KPRCB not initialized", cpu);
    }
    let rq_ptr = (kprcb_addr + OFFSET_RUN_QUEUE as u64) as *mut CpuRunQueue;
    &mut *rq_ptr
}

/// Drain all entries from a specific CPU's run queue into the caller's
/// local run queue. Used for work stealing.
///
/// # Safety
/// Requires both CPUs' KPRCBs to be initialized.
#[inline(always)]
pub unsafe fn steal_from_cpu_run_queue(from_cpu: usize, to_queue: &mut CpuRunQueue) -> u32 {
    let mut stolen = 0u32;
    if from_cpu >= MAX_CPUS { return stolen; }
    let src = cpu_run_queue_mut(from_cpu);
    while let Some(tid) = src.pop() {
        if to_queue.push(tid) {
            stolen += 1;
        } else {
            // Push back if destination is full
            src.push(tid);
            break;
        }
    }
    stolen
}

// ── Per-CPU slab cache accessors (GS-segment) ───────────────────────────
//
// PerCpuSlabCache layout (from cpu_local.rs):
//   offset 0x00: head          (*mut u8, 8 bytes)
//   offset 0x08: free_list     ([*mut u8; 32], 256 bytes)
//   offset 0x108: free_count   (u16)
//   offset 0x10A: slot_size    (u16)
//   offset 0x10C: pad          (4 bytes)
//   offset 0x110: total_allocated (u64)
//   offset 0x118: total_freed  (u64)
//   Total: 288 bytes per cache

/// Size of one PerCpuSlabCache in bytes (must match struct layout).
const PER_CPU_SLAB_CACHE_SIZE: u32 = 288;

/// Offset within a single PerCpuSlabCache to its `free_count` field.
const SLAB_FREE_COUNT_OFFSET: u32 = 0x108;

/// Offset within a single PerCpuSlabCache to its `free_list[0]` element.
const SLAB_FREE_LIST_OFFSET: u32 = 0x008;

/// Maximum objects per-CPU hot cache.
pub const SLAB_BATCH_SIZE_USIZE: usize = 32;

/// Get the absolute GS-segment offset of the `free_count` for a given cache index.
#[inline(always)]
fn slab_free_count_offset(cache_idx: usize) -> u32 {
    OFFSET_SLAB_CACHES + (cache_idx as u32) * PER_CPU_SLAB_CACHE_SIZE + SLAB_FREE_COUNT_OFFSET
}

/// Get the absolute GS-segment offset of `free_list[i]` for a given cache index.
#[inline(always)]
fn slab_free_list_elem_offset(cache_idx: usize, elem: usize) -> u32 {
    OFFSET_SLAB_CACHES + (cache_idx as u32) * PER_CPU_SLAB_CACHE_SIZE
        + SLAB_FREE_LIST_OFFSET + (elem as u32) * 8
}

/// Try to pop a free object from the per-CPU hot cache.
/// Returns the object pointer if available, or `None` if the cache is empty.
///
/// # Safety
/// Requires GS base to be set. Only the owning CPU should call this.
#[inline(always)]
pub unsafe fn this_cpu_slab_alloc_local(cache_idx: usize) -> Option<*mut u8> {
    let count_offset = slab_free_count_offset(cache_idx);
    let count = gs_read_u16(count_offset);
    if count == 0 {
        return None;
    }
    let idx = (count - 1) as usize;
    let ptr = gs_read_u64(slab_free_list_elem_offset(cache_idx, idx));
    gs_write_u16(count_offset, count - 1);
    Some(ptr as *mut u8)
}

/// Push a free object into the per-CPU hot cache.
/// Returns `Ok(())` on success, or `Err(ptr)` if the cache is full.
///
/// # Safety
/// Requires GS base to be set. Only the owning CPU should call this.
#[inline(always)]
pub unsafe fn this_cpu_slab_free_local(cache_idx: usize, ptr: *mut u8) -> Result<(), *mut u8> {
    let count_offset = slab_free_count_offset(cache_idx);
    let count = gs_read_u16(count_offset);
    if count as usize >= SLAB_BATCH_SIZE_USIZE {
        return Err(ptr);
    }
    let elem_offset = slab_free_list_elem_offset(cache_idx, count as usize);
    gs_write_u64(elem_offset, ptr as u64);
    gs_write_u16(count_offset, count + 1);
    Ok(())
}

/// Get the per-CPU slab cache `head` pointer for a given cache index.
#[inline(always)]
pub unsafe fn this_cpu_slab_head(cache_idx: usize) -> *mut u8 {
    let offset = OFFSET_SLAB_CACHES + (cache_idx as u32) * PER_CPU_SLAB_CACHE_SIZE;
    gs_read_u64(offset) as *mut u8
}

/// Set the per-CPU slab cache `head` pointer for a given cache index.
#[inline(always)]
pub unsafe fn this_cpu_set_slab_head(cache_idx: usize, head: *mut u8) {
    let offset = OFFSET_SLAB_CACHES + (cache_idx as u32) * PER_CPU_SLAB_CACHE_SIZE;
    gs_write_u64(offset, head as u64);
}

/// Read a u16 from the current CPU's KPRCB at the given byte offset.
#[inline(always)]
pub unsafe fn gs_read_u16(offset: u32) -> u16 {
    crate::hal::raw::raw_gs_read_u16(offset)
}

/// Write a u16 to the current CPU's KPRCB at the given byte offset.
#[inline(always)]
pub unsafe fn gs_write_u16(offset: u32, val: u16) {
    crate::hal::raw::raw_gs_write_u16(offset, val);
}

// ── Tests ────────────────────────────────────────────────────────────────

pub fn register_cpu_local_tests() {
    crate::testing::register("cpu_local_kprcb_size", || {
        // KPRCB must be at most one page; with align(4096) it rounds up to 4096
        crate::test_true!(core::mem::size_of::<Kprcb>() <= 4096);
        Ok(())
    });
    crate::testing::register("cpu_local_slab_cache_count", || {
        crate::test_eq!(core::mem::size_of::<[PerCpuSlabCache; NUM_SLAB_CACHES]>() % core::mem::size_of::<PerCpuSlabCache>(), 0);
        Ok(())
    });
    crate::testing::register("cpu_local_run_queue_ops", || {
        let mut q = CpuRunQueue::new();
        crate::test_true!(q.is_empty());
        crate::test_true!(q.push(1));
        crate::test_true!(q.push(2));
        crate::test_eq!(q.len(), 2);
        crate::test_eq!(q.pop(), Some(1));
        crate::test_eq!(q.pop(), Some(2));
        crate::test_true!(q.is_empty());
        crate::test_eq!(q.pop(), None);
        Ok(())
    });
    crate::testing::register("cpu_local_kprcb_init", || {
        let k = Kprcb::new(42, 0xAB);
        crate::test_eq!(k.cpu_id, 42u32);
        crate::test_eq!(k.apic_id, 0xABu32);
        crate::test_true!(k.current_thread.is_null());
        crate::test_eq!(k.current_pid, 0u32);
        crate::test_true!(k.idle);
        crate::test_true!(!k.need_resched);
        Ok(())
    });
    crate::testing::register("cpu_local_offset_sanity", || {
        crate::test_eq!(OFFSET_CPU_ID, 0x000u32);
        crate::test_eq!(OFFSET_CURRENT_THREAD, 0x008u32);
        crate::test_eq!(OFFSET_NEED_RESCHED, 0x015u32);
        crate::test_eq!(OFFSET_CURRENT_IRQL, 0x016u32);
        crate::test_eq!(OFFSET_EXIT_RSP, 0xB58u32);
        crate::test_eq!(OFFSET_EXIT_NOW, 0xB98u32);
        Ok(())
    });
}
