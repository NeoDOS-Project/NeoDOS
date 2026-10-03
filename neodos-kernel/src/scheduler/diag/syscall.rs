//! Syscall identity ring (#293 Phase 1).

use core::sync::atomic::{AtomicU64, Ordering};

// ── Syscall identity ring (Phase 1 of the #293 forensic protocol) ──────────
//
// Captures, for a *filtered* TID, the entry and exit of every syscall with the
// user RIP/RSP and kernel RSP, without allocating or taking the global
// scheduler lock. Dumped from the fault/panic path via the raw serial writer.
pub const SYS_RING_SIZE: usize = 96;

#[derive(Clone, Copy)]
struct SysEv {
    seq: u64,
    cpu: u8,
    phase: u8, // 0 = enter, 1 = exit
    nr: u32,
    tid: u32,
    pid: u32,
    user_rip: u64,
    user_rsp: u64,
    k_rsp: u64,
}

const SYS_ZERO: SysEv = SysEv {
    seq: 0, cpu: 0, phase: 0, nr: 0, tid: 0, pid: 0,
    user_rip: 0, user_rsp: 0, k_rsp: 0,
};

static mut SYS_RING: [SysEv; SYS_RING_SIZE] = [SYS_ZERO; SYS_RING_SIZE];
static SYS_HEAD: AtomicU64 = AtomicU64::new(0);
/// Runtime TID filter; 0 means "record nothing" (kept off until enabled).
static SYS_FILTER_TID: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

/// Enable syscall tracing for a specific TID (0 disables).
pub fn sys_trace_set_tid(tid: u32) {
    SYS_FILTER_TID.store(tid, Ordering::Relaxed);
}

#[inline]
pub fn sys_trace_enabled() -> bool {
    SYS_FILTER_TID.load(Ordering::Relaxed) != 0
}

/// Record one syscall event for the filtered TID. Lock-free, no allocation.
/// `want == u32::MAX` traces every TID (used during the interactive phase).
#[inline]
pub fn sys_ev(
    phase: u8, nr: u32, tid: u32, pid: u32,
    user_rip: u64, user_rsp: u64, k_rsp: u64,
) {
    let want = SYS_FILTER_TID.load(Ordering::Relaxed);
    if want == 0 || (want != u32::MAX && want != tid) {
        return;
    }
    let seq = SYS_HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % SYS_RING_SIZE;
    let e = SysEv {
        seq, cpu: unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as u8,
        phase, nr, tid, pid, user_rip, user_rsp, k_rsp,
    };
    unsafe { core::ptr::write_volatile(&mut SYS_RING[idx] as *mut SysEv, e); }
}

/// Dump the syscall ring (oldest → newest) through the raw serial writer.
pub fn sys_dump_raw() {
    let want = SYS_FILTER_TID.load(Ordering::Relaxed);
    let head = SYS_HEAD.load(Ordering::Relaxed);
    crate::raw_serial_println!(
        "[SYSCALL_TRACE] filter_tid={} head={} dump={}",
        want, head, head.min(SYS_RING_SIZE as u64));
    if head == 0 { return; }
    let count = head.min(SYS_RING_SIZE as u64) as usize;
    let start = if head > SYS_RING_SIZE as u64 {
        (head - SYS_RING_SIZE as u64) as usize
    } else { 0 };
    for i in 0..count {
        let idx = (start + i) % SYS_RING_SIZE;
        let e = unsafe { core::ptr::read_volatile(&SYS_RING[idx] as *const SysEv) };
        crate::raw_serial_println!(
            "[SYSCALL_TRACE] #{} {} cpu={} tid={} pid={} nr={} user_rip=0x{:x} user_rsp=0x{:x} k_rsp=0x{:x}",
            e.seq, if e.phase == 0 { "enter" } else { "exit" },
            e.cpu, e.tid, e.pid, e.nr, e.user_rip, e.user_rsp, e.k_rsp
        );
    }
}

