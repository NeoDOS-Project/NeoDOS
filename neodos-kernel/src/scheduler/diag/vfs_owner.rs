//! #345 Phase 2B/2C VFS owner/waiter instrumentation.

use core::sync::atomic::{AtomicU64, Ordering};

// ── #345 Phase 2B: VFS owner / waiter instrumentation ─────────────────────
//
// Lock-free observation of who currently holds the VFS lock. Purely
// diagnostic: lock semantics are unchanged; a separate atomic is written just
// after a successful acquire and cleared just before release.
//
// Packed word: [63:32] seq | [31:16] tid | [15:0] pid

pub static VFS_OWNER: AtomicU64 = AtomicU64::new(0);
pub static VFS_OWNER_CPU: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);
pub static VFS_OWNER_RIP: AtomicU64 = AtomicU64::new(0); // site tag (not a code addr)
pub static VFS_OWNER_ACQ: AtomicU64 = AtomicU64::new(0);
/// Best-effort pointer to the owning Kthread (live state read lock-free).
pub static VFS_OWNER_KPTR: AtomicU64 = AtomicU64::new(0);
pub static VFS_WAITER: AtomicU64 = AtomicU64::new(0);
pub static VFS_WAITER_CPU: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);
pub static VFS_SEQ: AtomicU64 = AtomicU64::new(0);
pub static VFS_WAIT_COUNT: AtomicU64 = AtomicU64::new(0);
/// Wait duration (ticks) of the longest observed VFS wait, for duration evidence.
pub static VFS_WAIT_MAX_TICKS: AtomicU64 = AtomicU64::new(0);

/// Site tags for `with_vfs` callers (diagnostic only).
pub const VFS_SITE_OTHER: u64 = 0;
pub const VFS_SITE_CREATE_PROC: u64 = 1; // create_process_from_ob_path (NXE read)
pub const VFS_SITE_INIT_LOAD: u64 = 2;   // main.rs NeoInit loader
pub const VFS_SITE_MAIN: u64 = 3;

#[inline]
fn pack_id(tid: u32, pid: u32, seq: u64) -> u64 {
    ((seq & 0xFFFF_FFFF) << 32) | (((tid as u64) & 0xFFFF) << 16) | ((pid as u64) & 0xFFFF)
}

#[inline]
pub fn vfs_owner_word() -> u64 { VFS_OWNER.load(Ordering::Relaxed) }
#[inline]
pub fn vfs_owner_cpu() -> u32 { VFS_OWNER_CPU.load(Ordering::Relaxed) }
#[inline]
pub fn vfs_owner_rip() -> u64 { VFS_OWNER_RIP.load(Ordering::Relaxed) }
#[inline]
pub fn vfs_owner_acq() -> u64 { VFS_OWNER_ACQ.load(Ordering::Relaxed) }
#[inline]
pub fn vfs_waiter_word() -> u64 { VFS_WAITER.load(Ordering::Relaxed) }
#[inline]
pub fn vfs_waiter_cpu() -> u32 { VFS_WAITER_CPU.load(Ordering::Relaxed) }
#[inline]
pub fn vfs_wait_count() -> u64 { VFS_WAIT_COUNT.load(Ordering::Relaxed) }

#[inline]
pub fn vfs_owner_tid() -> u32 { ((VFS_OWNER.load(Ordering::Relaxed) >> 16) & 0xFFFF) as u32 }
#[inline]
pub fn vfs_owner_pid() -> u32 { (VFS_OWNER.load(Ordering::Relaxed) & 0xFFFF) as u32 }

/// Record that this CPU now owns VFS. `site` is a stable caller tag.
#[inline]
pub fn vfs_owner_acquired(site: u64) {
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
    let cur = unsafe { crate::arch::x64::cpu_local::this_cpu_current_thread() };
    let (tid, pid) = if cur.is_null() { (0u32, 0u32) } else { unsafe { ((*cur).tid, (*cur).pid) } };
    let seq = VFS_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    VFS_OWNER_KPTR.store(cur as u64, Ordering::Relaxed);
    VFS_OWNER_CPU.store(cpu, Ordering::Relaxed);
    VFS_OWNER_RIP.store(site, Ordering::Relaxed);
    VFS_OWNER_ACQ.store(crate::hal::get_ticks(), Ordering::Relaxed);
    VFS_OWNER.store(pack_id(tid, pid, seq), Ordering::Relaxed);
}

#[inline]
pub fn vfs_owner_released() {
    VFS_OWNER.store(0, Ordering::Relaxed);
    VFS_OWNER_KPTR.store(0, Ordering::Relaxed);
}

/// State name for a `ThreadState` u8 (diag only).
fn st_name(s: u8) -> &'static str {
    match s { 0 => "Ready", 1 => "Running", 2 => "Blocked", 3 => "Suspended", 4 => "Terminated", _ => "?" }
}

/// #345 Phase 2C: capture the *live* state of the VFS owner at the instant a
/// waiter begins. Lock-free: reads the owner Kthread's fields with volatile
/// reads (no scheduler lock taken). Best-effort; the owner may change.
#[inline]
fn vfs_owner_snapshot() -> (u32, u32, u32, u32, u8, u8) {
    // returns (k_cpu, tid, pid, kprcb_cpu, state, is_kprcb_current)
    let kptr = VFS_OWNER_KPTR.load(Ordering::Relaxed) as *const crate::scheduler::Kthread;
    if kptr.is_null() {
        return (u32::MAX, 0, 0, u32::MAX, 0xFF, 0);
    }
    unsafe {
        let k_cpu = core::ptr::read_volatile(&(*kptr).cpu);
        let tid = core::ptr::read_volatile(&(*kptr).tid);
        let pid = core::ptr::read_volatile(&(*kptr).pid);
        let state = core::ptr::read_volatile(&(*kptr).state).to_u8();
        // Which CPU (if any) has this Kthread as KPRCB.current_thread?
        let mut kprcb_cpu = u32::MAX;
        for c in 0..crate::arch::x64::cpu_local::MAX_CPUS {
            let page = crate::arch::x64::cpu_local::kprcb_page(c);
            if let Some(p) = page {
                let cur = core::ptr::read_volatile(
                    (p + crate::arch::x64::cpu_local::OFFSET_CURRENT_THREAD as u64) as *const u64);
                if cur == kptr as u64 { kprcb_cpu = c as u32; break; }
            }
        }
        let is_current = (kprcb_cpu != u32::MAX) as u8;
        (k_cpu, tid, pid, kprcb_cpu, state, is_current)
    }
}

/// Record that this CPU is about to spin-wait for VFS, a bounded snapshot of
/// the owner (tid/pid/site/acq_t) plus the owner's LIVE state (#345 Phase 2C),
/// and (after acquisition) how long the wait took.
#[inline]
pub fn vfs_waiter_begin(site: u64) -> u64 {
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
    let cur = unsafe { crate::arch::x64::cpu_local::this_cpu_current_thread() };
    let (tid, pid) = if cur.is_null() { (0u32, 0u32) } else { unsafe { ((*cur).tid, (*cur).pid) } };
    let start = crate::hal::get_ticks();
    let seq = VFS_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    VFS_WAITER_CPU.store(cpu, Ordering::Relaxed);
    VFS_WAITER.store(pack_id(tid, pid, seq), Ordering::Relaxed);
    let n = VFS_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
    if n < 64 {
        let ow = VFS_OWNER.load(Ordering::Relaxed);
        let wtid = ((ow >> 16) & 0xFFFF) as u32;
        let wpid = (ow & 0xFFFF) as u32;
        let (okcpu, otid, opid, okprcb, ostate, oiscur) = vfs_owner_snapshot();
        let wpd = crate::scheduler::preempt_disabled() as u8;
        crate::raw_serial_println!(
            "[VFS_WAIT] #{n} waiter cpu={cpu} tid={tid} pid={pid} site={site} preempt={wpd} | owner cpu={} tid={} pid={} site={} acq_t={} | owner_live k.cpu={} tid={} pid={} state={} kprcb_cpu={} is_kprcb_current={} t={start}",
            vfs_owner_cpu(), wtid, wpid, vfs_owner_rip(), vfs_owner_acq(),
            okcpu, otid, opid, st_name(ostate), okprcb, oiscur);
    }
    start
}

/// Record the completion of a blocking VFS wait (duration in ticks).
#[inline]
pub fn vfs_wait_end(start: u64, site: u64) {
    let now = crate::hal::get_ticks();
    let d = now.wrapping_sub(start);
    let prev = VFS_WAIT_MAX_TICKS.fetch_max(d, Ordering::Relaxed);
    if d > prev {
        crate::raw_serial_println!("[VFS_WAIT_DONE] site={} duration_ticks={}", site, d);
    }
}

/// Best-effort caller return address. Kept for diagnostics; unreliable without
/// forced frame pointers. Callers pass explicit site tags instead.
#[inline(never)]
pub fn caller_rip() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mov {}, [rbp + 8]", out(reg) v, options(nomem, nostack, preserves_flags)); }
    v
}
