//! Thread/CPU-state publication rings (CTX, RSP, KCPU, RUN).

use core::sync::atomic::{AtomicU64, Ordering};

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

// ── #345: transition-to-Running ring ──────────────────────────────────────
//
// Records every site that commits a Kthread `Running`, together with the
// state/cpu it had *before* the write and the per-CPU KPRCB view at that
// instant. Dumped once on the first `TWO+ Running` detection so the exact
// transition that created the second `Running` on a CPU is preserved even if
// the failure is non-fatal (no panic dump). Lock-free, no allocation.
pub const RUN_RING_SIZE: usize = 160;

pub const RUN_SITE_FAST: u8 = 1;        // schedule_with local runqueue fast path
pub const RUN_SITE_STEAL: u8 = 2;       // schedule_with work-steal
pub const RUN_SITE_SCAN: u8 = 3;        // schedule_with global priority scan
pub const RUN_SITE_DISPATCH_IDLE: u8 = 4; // dispatch_idle (idle fallback / #355)
pub const RUN_SITE_RESUME_REJECT: u8 = 5; // resume_current_after_rejected_dispatch
pub const RUN_SITE_AP_IDLE: u8 = 6;     // register_ap_idle
pub const RUN_SITE_RESCHED_CHOSEN: u8 = 7; // resched select_fallback_ring3
pub const RUN_SITE_RESCHED_IDLE: u8 = 8;   // resched idle fallback
pub const RUN_SITE_IDT_REVERT: u8 = 9;  // idt user-preempt revert to current
pub const RUN_SITE_IDT_SAME: u8 = 10;   // idt same-tid restore
pub const RUN_SITE_IDT_RESTORE: u8 = 11; // idt kernel-mode restore Running
pub const RUN_SITE_USERMODE: u8 = 12;   // usermode wait_for_process activation

#[derive(Clone, Copy)]
struct RunEv {
    seq: u64,
    site: u8,
    cpu: u8,
    tid: u32,
    pid: u32,
    prev_state: u8,
    new_state: u8,
    k_cpu_before: u32,
    rsp: u64,
    kprcb_tid: u32,
}

const RUN_ZERO: RunEv = RunEv {
    seq: 0, site: 0, cpu: 0, tid: 0, pid: 0, prev_state: 0, new_state: 0,
    k_cpu_before: 0, rsp: 0, kprcb_tid: 0,
};

static mut RUN_RING: [RunEv; RUN_RING_SIZE] = [RUN_ZERO; RUN_RING_SIZE];
static RUN_HEAD: AtomicU64 = AtomicU64::new(0);
static RUN_FIRST_DUMPED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Record a transition to `Running`. `prev_state` is the state *before* the
/// write; callers pass `k` before assigning `k.state = Running`.
#[inline]
pub fn run_ev(site: u8, k: &crate::scheduler::Kthread) {
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as u8;
    let cur = unsafe { crate::arch::x64::cpu_local::this_cpu_current_thread() };
    let kprcb_tid = if cur.is_null() { u32::MAX } else { unsafe { (*cur).tid } };
    let seq = RUN_HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % RUN_RING_SIZE;
    let e = RunEv {
        seq, site, cpu, tid: k.tid, pid: k.pid,
        prev_state: k.state.to_u8(), new_state: crate::scheduler::types::ThreadState::Running.to_u8(),
        k_cpu_before: k.cpu, rsp: k.rsp, kprcb_tid,
    };
    unsafe { core::ptr::write_volatile(&mut RUN_RING[idx] as *mut RunEv, e); }
}

/// Dump the transition ring once. Called from `consistency_check` on the first
/// `TWO+ Running`. Safe from IRQ context (raw serial, no locks, no alloc).
pub fn run_dump_first() {
    if RUN_FIRST_DUMPED.swap(true, Ordering::Relaxed) {
        return;
    }
    run_dump_raw();
}

pub fn run_dump_raw() {
    let head = RUN_HEAD.load(Ordering::Relaxed);
    crate::raw_serial_println!("[RUN_TRACE] head={} dump={}", head, head.min(RUN_RING_SIZE as u64));
    if head == 0 { return; }
    let count = head.min(RUN_RING_SIZE as u64) as usize;
    let start = if head > RUN_RING_SIZE as u64 {
        (head - RUN_RING_SIZE as u64) as usize
    } else { 0 };
    for i in 0..count {
        let idx = (start + i) % RUN_RING_SIZE;
        let e = unsafe { core::ptr::read_volatile(&RUN_RING[idx] as *const RunEv) };
        crate::raw_serial_println!(
            "[RUN_TRACE] #{} site={} cpu={} tid={} pid={} {}->{} k.cpu_before={} rsp=0x{:x} kprcb_tid={}",
            e.seq, e.site, e.cpu, e.tid, e.pid, e.prev_state, e.new_state,
            e.k_cpu_before, e.rsp, e.kprcb_tid
        );
    }
}


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

