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

// ── Phase 293-B: current_thread write ring ────────────────────────────────
//
// Every write to `KPRCB.current_thread` is recorded with the old/new pointer
// and TIDs, plus a *static* site tag (no backtrace, no locks, no allocation).
pub const CTX_RING_SIZE: usize = 128;

/// Static site tags for `current_thread` / `rsp` writes.
pub const SITE_SYNC_SCHEDULE: u8 = 1;   // sync_per_cpu_current from schedule_with
pub const SITE_SYNC_STEAL: u8 = 2;      // schedule_with step 2
pub const SITE_SYNC_SCAN: u8 = 3;       // schedule_with step 3
pub const SITE_SYNC_IDLE: u8 = 4;       // schedule_with idle fallback
pub const SITE_SYNC_HANDOFF: u8 = 5;    // usermode handoff
pub const SITE_SET_IDT: u8 = 6;         // idt.rs timer/exception paths
pub const SITE_SET_RESCHED: u8 = 7;     // syscall/resched.rs
pub const SITE_SET_AP_IDLE: u8 = 8;     // smp.rs ap_enter_idle
pub const SITE_SET_RAW: u8 = 9;         // direct call without tag
pub const SITE_SET_RESCHED_IDLE_FALLBACK: u8 = 10; // resched.rs blocked/terminated idle fallback
pub const SITE_SET_RESCHED_CHOSEN: u8 = 11;       // resched.rs Ring3 candidate
pub const SITE_SET_RESCHED_NEXT: u8 = 12;         // resched.rs main next thread

#[derive(Clone, Copy)]
struct CtxEv {
    seq: u64,
    cpu: u8,
    site: u8,
    old_ptr: u64,
    new_ptr: u64,
    old_tid: u32,
    new_tid: u32,
    new_pid: u32,
    new_state: u8,
    new_cpu: u32,
    new_rsp: u64,
}

const CTX_ZERO: CtxEv = CtxEv {
    seq: 0, cpu: 0, site: 0, old_ptr: 0, new_ptr: 0,
    old_tid: 0, new_tid: 0, new_pid: 0, new_state: 0, new_cpu: 0, new_rsp: 0,
};

static mut CTX_RING: [CtxEv; CTX_RING_SIZE] = [CTX_ZERO; CTX_RING_SIZE];
static CTX_HEAD: AtomicU64 = AtomicU64::new(0);
static CTX_TRACE_ON: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

pub fn ctx_trace_enable(v: bool) { CTX_TRACE_ON.store(v, Ordering::Relaxed); }
#[inline]
pub fn ctx_trace_enabled() -> bool { CTX_TRACE_ON.load(Ordering::Relaxed) }

/// Record a `current_thread` write. Lock-free; callers must be able to tolerate
/// a few relaxed stores (only enabled during the interactive phase).
///
/// Phase 293-C: also detects, at the instant of the write, whether the target
/// pointer is already the `KPRCB.current_thread` of another CPU. The FIRST such
/// occurrence freezes the ring (no further overwrite) and records the full
/// before/after state, so the exact first invalid publication is preserved.
#[inline]
pub fn ctx_ev(site: u8, old_ptr: u64, new_ptr: *const crate::scheduler::Kthread) {
    if !CTX_TRACE_ON.load(Ordering::Relaxed) { return; }
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as u8;
    let (new_tid, new_pid, new_state, new_cpu, new_rsp) = if new_ptr.is_null() {
        (0, 0, 0u8, 0u32, 0u64)
    } else {
        unsafe { ((*new_ptr).tid, (*new_ptr).pid, (*new_ptr).state.to_u8(), (*new_ptr).cpu, (*new_ptr).rsp) }
    };
    // BEFORE: the previous current_thread on this CPU (deref only if non-null;
    // best-effort — a stale pointer is not dereferenced for classification).
    let (old_k_cpu, old_rsp) = if old_ptr == 0 {
        (0u32, 0u64)
    } else {
        // Safe: old_ptr is the previous current of this CPU and is therefore a
        // live Kthread in the scheduler table.
        unsafe {
            let p = old_ptr as *const crate::scheduler::Kthread;
            ((*p).cpu, (*p).rsp)
        }
    };
    // Diagnostic-only cross-CPU ownership check at the moment of the write.
    let mut other_owner: i32 = -1;
    if !new_ptr.is_null() {
        for c in 0..crate::arch::x64::cpu_local::MAX_CPUS {
            if c as u8 == cpu { continue; }
            let cur = unsafe {
                match crate::arch::x64::cpu_local::kprcb_page(c) {
                    Some(p) => core::ptr::read_volatile(
                        (p + crate::arch::x64::cpu_local::OFFSET_CURRENT_THREAD as u64) as *const u64),
                    None => 0,
                }
            };
            if cur == new_ptr as u64 { other_owner = c as i32; }
        }
    }
    let seq = CTX_HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % CTX_RING_SIZE;
    let e = CtxEv {
        seq, cpu, site, old_ptr, new_ptr: new_ptr as u64,
        old_tid: 0, new_tid, new_pid, new_state, new_cpu, new_rsp,
    };
    // Frozen ring: once frozen only the retained window is dumped; do not
    // overwrite it so the pre-divergence sequence survives.
    if !CTX_FROZEN.load(Ordering::Relaxed) {
        unsafe { core::ptr::write_volatile(&mut CTX_RING[idx] as *mut CtxEv, e); }
        unsafe {
            CTX_OLD_KCPU[seq as usize % CTX_RING_SIZE] = old_k_cpu;
            CTX_OLD_RSP[seq as usize % CTX_RING_SIZE] = old_rsp;
        }
    }
    if other_owner >= 0 && !CTX_FROZEN.swap(true, Ordering::Relaxed) {
        // First invalid cross-CPU ownership: retain detail and stop overwriting.
        unsafe {
            CTX_FIRST.site = site;
            CTX_FIRST.cpu = cpu;
            CTX_FIRST.other_owner = other_owner as u8;
            CTX_FIRST.new_ptr = new_ptr as u64;
            CTX_FIRST.new_tid = new_tid;
            CTX_FIRST.new_pid = new_pid;
            CTX_FIRST.new_k_cpu = new_cpu;
            CTX_FIRST.new_rsp = new_rsp;
            CTX_FIRST.old_ptr = old_ptr;
            CTX_FIRST.old_k_cpu = old_k_cpu;
            CTX_FIRST.old_rsp = old_rsp;
            CTX_FIRST.seq = seq;
        }
        crate::raw_serial_println!(
            "[CTX_DOUBLE_OWNER] FIRST cpu={} other_owner={} site={} seq={} ptr=0x{:x} tid={} pid={} k.cpu={} k.rsp=0x{:x} old_ptr=0x{:x} old_k.cpu={} old_rsp=0x{:x}",
            cpu, other_owner, site, seq, new_ptr as u64, new_tid, new_pid, new_cpu, new_rsp,
            old_ptr, old_k_cpu, old_rsp);
    }
}

/// Frozen-state detail for the first invalid ownership publication.
#[derive(Clone, Copy)]
pub struct FirstOwner {
    pub seq: u64,
    pub cpu: u8,
    pub other_owner: u8,
    pub site: u8,
    pub new_ptr: u64,
    pub new_tid: u32,
    pub new_pid: u32,
    pub new_k_cpu: u32,
    pub new_rsp: u64,
    pub old_ptr: u64,
    pub old_k_cpu: u32,
    pub old_rsp: u64,
}

static mut CTX_FIRST: FirstOwner = FirstOwner {
    seq: 0, cpu: 0, other_owner: 0, site: 0, new_ptr: 0, new_tid: 0,
    new_pid: 0, new_k_cpu: 0, new_rsp: 0, old_ptr: 0, old_k_cpu: 0, old_rsp: 0,
};
static CTX_FROZEN: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
/// Per-slot BEFORE state (k.cpu / rsp of the previous current_thread).
static mut CTX_OLD_KCPU: [u32; CTX_RING_SIZE] = [0; CTX_RING_SIZE];
static mut CTX_OLD_RSP: [u64; CTX_RING_SIZE] = [0; CTX_RING_SIZE];

#[inline]
pub fn ctx_frozen() -> bool { CTX_FROZEN.load(Ordering::Relaxed) }

pub fn ctx_dump_raw() {
    let head = CTX_HEAD.load(Ordering::Relaxed);
    crate::raw_serial_println!("[CTX_TRACE] on={} frozen={} head={} dump={}",
        CTX_TRACE_ON.load(Ordering::Relaxed), CTX_FROZEN.load(Ordering::Relaxed),
        head, head.min(CTX_RING_SIZE as u64));
    if CTX_FROZEN.load(Ordering::Relaxed) {
        let f = unsafe { CTX_FIRST };
        crate::raw_serial_println!(
            "[CTX_FIRST_INVALID] seq={} cpu={} other_owner={} site={} new=0x{:x} tid={} pid={} k.cpu={} rsp=0x{:x} old=0x{:x} old_k.cpu={} old_rsp=0x{:x}",
            f.seq, f.cpu, f.other_owner, f.site, f.new_ptr, f.new_tid, f.new_pid,
            f.new_k_cpu, f.new_rsp, f.old_ptr, f.old_k_cpu, f.old_rsp);
    }
    if head == 0 { return; }
    let count = head.min(CTX_RING_SIZE as u64) as usize;
    let start = if head > CTX_RING_SIZE as u64 {
        (head - CTX_RING_SIZE as u64) as usize
    } else { 0 };
    for i in 0..count {
        let idx = (start + i) % CTX_RING_SIZE;
        let e = unsafe { core::ptr::read_volatile(&CTX_RING[idx] as *const CtxEv) };
        let ok = unsafe { CTX_OLD_KCPU[idx] };
        let orsp = unsafe { CTX_OLD_RSP[idx] };
        crate::raw_serial_println!(
            "[CTX_TRACE] #{} cpu={} site={} old=0x{:x}(k.cpu={} rsp=0x{:x}) new=0x{:x}(tid={} pid={} state={} k.cpu={} rsp=0x{:x})",
            e.seq, e.cpu, e.site, e.old_ptr, ok, orsp, e.new_ptr, e.new_tid, e.new_pid,
            e.new_state, e.new_cpu, e.new_rsp
        );
    }
}

// ── Phase 293-B: Kthread.rsp write ring ───────────────────────────────────
pub const RSP_RING_SIZE: usize = 96;

pub const SITE_RSP_IDT_USER: u8 = 1;
pub const SITE_RSP_IDT_IDLE: u8 = 2;
pub const SITE_RSP_IDT_KERNEL: u8 = 3;
pub const SITE_RSP_RESCHED: u8 = 4;
pub const SITE_RSP_TIMESLICE: u8 = 5;

#[derive(Clone, Copy)]
struct RspEv {
    seq: u64,
    cpu: u8,
    site: u8,
    tid: u32,
    pid: u32,
    old_rsp: u64,
    new_rsp: u64,
    k_cpu: u32,
    k_state: u8,
    is_current: u8,
}

const RSP_ZERO: RspEv = RspEv {
    seq: 0, cpu: 0, site: 0, tid: 0, pid: 0,
    old_rsp: 0, new_rsp: 0, k_cpu: 0, k_state: 0, is_current: 0,
};

static mut RSP_RING: [RspEv; RSP_RING_SIZE] = [RSP_ZERO; RSP_RING_SIZE];
static RSP_HEAD: AtomicU64 = AtomicU64::new(0);
static RSP_TRACE_ON: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

pub fn rsp_trace_enable(v: bool) { RSP_TRACE_ON.store(v, Ordering::Relaxed); }
#[inline]
pub fn rsp_trace_enabled() -> bool { RSP_TRACE_ON.load(Ordering::Relaxed) }

/// Record a `k.rsp = new` write (called just before the store).
#[inline]
pub fn rsp_ev(site: u8, k: &crate::scheduler::Kthread, new_rsp: u64) {
    if !RSP_TRACE_ON.load(Ordering::Relaxed) { return; }
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as u8;
    let cur = unsafe { crate::arch::x64::cpu_local::this_cpu_current_thread() };
    let ptr = k as *const crate::scheduler::Kthread as *const u8;
    let is_current = (cur as *const u8 == ptr) as u8;
    let seq = RSP_HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % RSP_RING_SIZE;
    let e = RspEv {
        seq, cpu, site, tid: k.tid, pid: k.pid,
        old_rsp: k.rsp, new_rsp, k_cpu: k.cpu, k_state: k.state.to_u8(), is_current,
    };
    unsafe { core::ptr::write_volatile(&mut RSP_RING[idx] as *mut RspEv, e); }
}

pub fn rsp_dump_raw() {
    let head = RSP_HEAD.load(Ordering::Relaxed);
    crate::raw_serial_println!("[RSP_TRACE] on={} head={} dump={}",
        RSP_TRACE_ON.load(Ordering::Relaxed), head, head.min(RSP_RING_SIZE as u64));
    if head == 0 { return; }
    let count = head.min(RSP_RING_SIZE as u64) as usize;
    let start = if head > RSP_RING_SIZE as u64 {
        (head - RSP_RING_SIZE as u64) as usize
    } else { 0 };
    for i in 0..count {
        let idx = (start + i) % RSP_RING_SIZE;
        let e = unsafe { core::ptr::read_volatile(&RSP_RING[idx] as *const RspEv) };
        crate::raw_serial_println!(
            "[RSP_TRACE] #{} cpu={} site={} tid={} pid={} old=0x{:x} new=0x{:x} k.cpu={} state={} is_current={}",
            e.seq, e.cpu, e.site, e.tid, e.pid, e.old_rsp, e.new_rsp,
            e.k_cpu, e.k_state, e.is_current
        );
    }
}

// ── Phase 293-B: DOUBLE_RUNNING / STACK_OWNER_MISMATCH rings ──────────────
pub const DR_RING_SIZE: usize = 32;

#[derive(Clone, Copy)]
struct DrEv {
    seq: u64,
    cpu: u32,
    a_tid: u32,
    a_pid: u32,
    a_rsp: u64,
    a_kcpu: u32,
    b_tid: u32,
    b_pid: u32,
    b_rsp: u64,
    b_kcpu: u32,
    sched_current: u32,
    kprcb_tid: u32,
}

const DR_ZERO: DrEv = DrEv {
    seq: 0, cpu: 0, a_tid: 0, a_pid: 0, a_rsp: 0, a_kcpu: 0,
    b_tid: 0, b_pid: 0, b_rsp: 0, b_kcpu: 0, sched_current: 0, kprcb_tid: 0,
};

static mut DR_RING: [DrEv; DR_RING_SIZE] = [DR_ZERO; DR_RING_SIZE];
static DR_HEAD: AtomicU64 = AtomicU64::new(0);
static DR_LAST_SEQ: AtomicU64 = AtomicU64::new(0);

/// Record a DOUBLE_RUNNING occurrence and return its sequence number.
#[inline]
pub fn dr_ev(
    cpu: u32,
    a: &crate::scheduler::Kthread, b: &crate::scheduler::Kthread,
    sched_current: u32, kprcb_tid: u32,
) -> u64 {
    let seq = DR_HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % DR_RING_SIZE;
    let e = DrEv {
        seq, cpu,
        a_tid: a.tid, a_pid: a.pid, a_rsp: a.rsp, a_kcpu: a.cpu,
        b_tid: b.tid, b_pid: b.pid, b_rsp: b.rsp, b_kcpu: b.cpu,
        sched_current, kprcb_tid,
    };
    unsafe { core::ptr::write_volatile(&mut DR_RING[idx] as *mut DrEv, e); }
    DR_LAST_SEQ.store(seq, Ordering::Relaxed);
    seq
}

#[inline]
pub fn dr_last_seq() -> u64 { DR_LAST_SEQ.load(Ordering::Relaxed) }

pub fn dr_dump_raw() {
    let head = DR_HEAD.load(Ordering::Relaxed);
    crate::raw_serial_println!("[DOUBLE_RUNNING] head={} dump={}",
        head, head.min(DR_RING_SIZE as u64));
    if head == 0 { return; }
    let count = head.min(DR_RING_SIZE as u64) as usize;
    let start = if head > DR_RING_SIZE as u64 {
        (head - DR_RING_SIZE as u64) as usize
    } else { 0 };
    for i in 0..count {
        let idx = (start + i) % DR_RING_SIZE;
        let e = unsafe { core::ptr::read_volatile(&DR_RING[idx] as *const DrEv) };
        crate::raw_serial_println!(
            "[DOUBLE_RUNNING] #{} cpu={} A(tid={} pid={} rsp=0x{:x} k.cpu={}) B(tid={} pid={} rsp=0x{:x} k.cpu={}) sched.current={} kprcb_tid={}",
            e.seq, e.cpu, e.a_tid, e.a_pid, e.a_rsp, e.a_kcpu,
            e.b_tid, e.b_pid, e.b_rsp, e.b_kcpu, e.sched_current, e.kprcb_tid
        );
    }
}

/// STACK_OWNER_MISMATCH: the same `Kthread` is the `KPRCB.current_thread` of
/// more than one CPU (two CPUs on one kernel stack). Recorded, never fixed.
static STACK_OWNER_MISMATCH: AtomicU64 = AtomicU64::new(0);

pub fn stack_owner_mismatch_count() -> u64 {
    STACK_OWNER_MISMATCH.load(Ordering::Relaxed)
}

/// Scan all CPU KPRCBs and count Kthreads owned by >1 CPU. Called under the
/// scheduler lock from the consistency check; lock-free reads of KPRCB.
pub fn stack_owner_scan() {
    let max = crate::arch::x64::cpu_local::MAX_CPUS;
    for cpu in 0..max {
        let ptr = unsafe {
            let page = crate::arch::x64::cpu_local::kprcb_page(cpu);
            match page {
                Some(p) => core::ptr::read_volatile(
                    (p + crate::arch::x64::cpu_local::OFFSET_CURRENT_THREAD as u64) as *const u64),
                None => 0,
            }
        };
        if ptr == 0 { continue; }
        // Count how many CPUs hold this same pointer.
        let mut owners = 0u32;
        for other in 0..max {
            if other == cpu { continue; }
            let p2 = unsafe {
                let page = crate::arch::x64::cpu_local::kprcb_page(other);
                match page {
                    Some(p) => core::ptr::read_volatile(
                        (p + crate::arch::x64::cpu_local::OFFSET_CURRENT_THREAD as u64) as *const u64),
                    None => 0,
                }
            };
            if p2 == ptr { owners += 1; }
        }
        if owners > 0 {
            let n = STACK_OWNER_MISMATCH.fetch_add(1, Ordering::Relaxed);
            if n < 16 {
                crate::raw_serial_println!(
                    "[STACK_OWNER_MISMATCH] ptr=0x{:x} held by cpu={} and {} other cpu(s)",
                    ptr, cpu, owners);
            }
        }
    }
}

// ── Phase 293-C: Kthread.cpu write ring ────────────────────────────────────
pub const KCPU_RING_SIZE: usize = 128;
pub const SITE_KCPU_SCAN: u8 = 1;      // schedule_with global scan
pub const SITE_KCPU_TIMESLICE: u8 = 2; // on_timer_tick expiry re-home
pub const SITE_KCPU_STEAL: u8 = 3;     // smp steal_and_migrate
pub const SITE_KCPU_STEAL_REVERT: u8 = 4;
pub const SITE_KCPU_IDLE_INIT: u8 = 5;

#[derive(Clone, Copy)]
struct KcpuEv {
    seq: u64,
    cpu: u8,
    site: u8,
    tid: u32,
    pid: u32,
    old_kcpu: u32,
    new_kcpu: u32,
    rsp: u64,
    is_current_here: u8,
}

const KCPU_ZERO: KcpuEv = KcpuEv {
    seq: 0, cpu: 0, site: 0, tid: 0, pid: 0,
    old_kcpu: 0, new_kcpu: 0, rsp: 0, is_current_here: 0,
};

static mut KCPU_RING: [KcpuEv; KCPU_RING_SIZE] = [KCPU_ZERO; KCPU_RING_SIZE];
static KCPU_HEAD: AtomicU64 = AtomicU64::new(0);
static KCPU_TRACE_ON: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

pub fn kcpu_trace_enable(v: bool) { KCPU_TRACE_ON.store(v, Ordering::Relaxed); }
#[inline]
pub fn kcpu_trace_enabled() -> bool { KCPU_TRACE_ON.load(Ordering::Relaxed) }

#[inline]
pub fn kcpu_ev(site: u8, k: &crate::scheduler::Kthread, new_kcpu: u32) {
    if !KCPU_TRACE_ON.load(Ordering::Relaxed) { return; }
    if CTX_FROZEN.load(Ordering::Relaxed) { return; }
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as u8;
    let cur = unsafe { crate::arch::x64::cpu_local::this_cpu_current_thread() };
    let is_current_here = (cur as *const u8 == k as *const crate::scheduler::Kthread as *const u8) as u8;
    let seq = KCPU_HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % KCPU_RING_SIZE;
    let e = KcpuEv {
        seq, cpu, site, tid: k.tid, pid: k.pid,
        old_kcpu: k.cpu, new_kcpu, rsp: k.rsp, is_current_here,
    };
    unsafe { core::ptr::write_volatile(&mut KCPU_RING[idx] as *mut KcpuEv, e); }
}

pub fn kcpu_dump_raw() {
    let head = KCPU_HEAD.load(Ordering::Relaxed);
    crate::raw_serial_println!("[KCPU_TRACE] head={} dump={}", head, head.min(KCPU_RING_SIZE as u64));
    if head == 0 { return; }
    let count = head.min(KCPU_RING_SIZE as u64) as usize;
    let start = if head > KCPU_RING_SIZE as u64 {
        (head - KCPU_RING_SIZE as u64) as usize
    } else { 0 };
    for i in 0..count {
        let idx = (start + i) % KCPU_RING_SIZE;
        let e = unsafe { core::ptr::read_volatile(&KCPU_RING[idx] as *const KcpuEv) };
        crate::raw_serial_println!(
            "[KCPU_TRACE] #{} cpu={} site={} tid={} pid={} old_k.cpu={} new_k.cpu={} rsp=0x{:x} is_current_here={}",
            e.seq, e.cpu, e.site, e.tid, e.pid, e.old_kcpu, e.new_kcpu, e.rsp, e.is_current_here
        );
    }
}
