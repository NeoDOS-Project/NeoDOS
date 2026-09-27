//! Lock-free scheduler event ring for post-mortem forensics.
//!
//! Records the last `SCHED_EV_RING_SIZE` scheduling-relevant events (dispatch,
//! wake, block, context-save) into a fixed ring that never allocates and never
//! takes a lock. Dumped from the fault/panic path via [`dump_raw`] using the
//! lock-free raw serial writer, so it survives an exception-logger deadlock.
//!
//! Diagnostic only: the writes are a handful of relaxed stores on scheduling
//! transitions, not on the timer hot path.

use core::sync::atomic::{AtomicU64, Ordering};

pub const SCHED_EV_RING_SIZE: usize = 128;

/// Event kinds (kept in sync with `dump_raw`).
pub const EV_DISPATCH_RQ: u8 = 1;
pub const EV_DISPATCH_STEAL: u8 = 2;
pub const EV_DISPATCH_SCAN: u8 = 3;
pub const EV_WAKE_READY: u8 = 4;
pub const EV_BLOCK: u8 = 5;
pub const EV_RESCHED_SAVE: u8 = 6;
pub const EV_TIMER_SAVE: u8 = 7;
pub const EV_IDLE: u8 = 8;

#[derive(Clone, Copy)]
struct Ev {
    seq: u64,
    kind: u8,
    cpu: u8,
    tid: u32,
    rsp: u64,
    extra: u64,
}

const ZERO: Ev = Ev { seq: 0, kind: 0, cpu: 0, tid: 0, rsp: 0, extra: 0 };

static mut RING: [Ev; SCHED_EV_RING_SIZE] = [ZERO; SCHED_EV_RING_SIZE];
static HEAD: AtomicU64 = AtomicU64::new(0);

/// Record one scheduler event. Lock-free; safe from any context.
#[inline]
pub fn ev(kind: u8, cpu: u32, tid: u32, rsp: u64, extra: u64) {
    let seq = HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % SCHED_EV_RING_SIZE;
    let e = Ev { seq, kind, cpu: cpu as u8, tid, rsp, extra };
    unsafe {
        core::ptr::write_volatile(&mut RING[idx] as *mut Ev, e);
    }
}

/// Dump the ring (oldest → newest) through the lock-free raw serial writer.
pub fn dump_raw() {    let head = HEAD.load(Ordering::Relaxed);
    if head == 0 {
        crate::raw_serial_println!("[SCHED_EV] no events");
        return;
    }
    let count = head.min(SCHED_EV_RING_SIZE as u64) as usize;
    let start = if head > SCHED_EV_RING_SIZE as u64 {
        (head - SCHED_EV_RING_SIZE as u64) as usize
    } else {
        0
    };
    crate::raw_serial_println!("[SCHED_EV] head={} dump={} (kind 1=rq 2=steal 3=scan 4=wake 5=block 6=resched 7=timer 8=idle)",
        head, count);
    for i in 0..count {
        let idx = (start + i) % SCHED_EV_RING_SIZE;
        let e = unsafe { core::ptr::read_volatile(&RING[idx] as *const Ev) };
        crate::raw_serial_println!(
            "[SCHED_EV] #{} k={} cpu={} tid={} rsp=0x{:x} x=0x{:x}",
            e.seq, e.kind, e.cpu, e.tid, e.rsp, e.extra
        );
    }
}

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

// ── Return-frame ring (Phase 4 of the #293 forensic protocol) ──────────────
//
// Captures the *complete* syscall/iretq return frame (RIP/CS/RFLAGS/RSP/SS) at
// the syscall exit, tagged with the CPU/TID/PID that read it. Dumped raw so a
// torn/corrupt frame can be reconstructed even if the panic logger deadlocks.
pub const FRAME_RING_SIZE: usize = 32;

#[derive(Clone, Copy)]
struct FrameEv {
    seq: u64,
    phase: u8,
    cpu: u8,
    tid: u32,
    pid: u32,
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

const FRAME_ZERO: FrameEv = FrameEv {
    seq: 0, phase: 0, cpu: 0, tid: 0, pid: 0,
    rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
};

static mut FRAME_RING: [FrameEv; FRAME_RING_SIZE] = [FRAME_ZERO; FRAME_RING_SIZE];
static FRAME_HEAD: AtomicU64 = AtomicU64::new(0);

/// Record one full return frame. Lock-free; safe from the syscall exit path.
#[inline]
pub fn frame_ev(
    phase: u8, tid: u32, pid: u32,
    rip: u64, cs: u64, rflags: u64, rsp: u64, ss: u64,
) {
    let seq = FRAME_HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % FRAME_RING_SIZE;
    let e = FrameEv {
        seq, phase, cpu: unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as u8,
        tid, pid, rip, cs, rflags, rsp, ss,
    };
    unsafe { core::ptr::write_volatile(&mut FRAME_RING[idx] as *mut FrameEv, e); }
}

/// Dump the return-frame ring through the raw serial writer.
pub fn frame_dump_raw() {
    let head = FRAME_HEAD.load(Ordering::Relaxed);
    crate::raw_serial_println!("[FRAME_TRACE] head={} dump={}", head,
        head.min(FRAME_RING_SIZE as u64));
    if head == 0 { return; }
    let count = head.min(FRAME_RING_SIZE as u64) as usize;
    let start = if head > FRAME_RING_SIZE as u64 {
        (head - FRAME_RING_SIZE as u64) as usize
    } else { 0 };
    for i in 0..count {
        let idx = (start + i) % FRAME_RING_SIZE;
        let e = unsafe { core::ptr::read_volatile(&FRAME_RING[idx] as *const FrameEv) };
        crate::raw_serial_println!(
            "[FRAME_TRACE] #{} {} cpu={} tid={} pid={} RIP=0x{:x} CS=0x{:x} RFLAGS=0x{:x} RSP=0x{:x} SS=0x{:x}",
            e.seq, if e.phase == 0 { "enter" } else { "exit" },
            e.cpu, e.tid, e.pid, e.rip, e.cs, e.rflags, e.rsp, e.ss
        );
    }
}
