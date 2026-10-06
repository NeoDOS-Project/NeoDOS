//! #345 KPRCB identity-fallback detector and spawn-stress event ring.

use core::sync::atomic::{AtomicU64, Ordering};

// ── #345: KPRCB per-CPU identity fallback detector ────────────────────────
//
// `current_tid_for_this_cpu` / `current_kthread_mut` silently fall back to the
// *shared global* `Scheduler.current_tid` when the per-CPU KPRCB identity is
// unavailable. On SMP that global field is written by every CPU, so such a
// fallback makes one CPU operate on another CPU's current thread. Record the
// first occurrences (bounded) with the reason.
static KPRCB_FB_COUNT: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn kprcb_fallback_ev(site: &'static str, gs: u64, cur_ptr: *const crate::scheduler::Kthread, global_tid: u32) {
    let n = KPRCB_FB_COUNT.fetch_add(1, Ordering::Relaxed);
    if n < 24 {
        let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        crate::raw_serial_println!(
            "[KPRCB_FALLBACK] #{} site={} cpu={} gs=0x{:x} cur_ptr=0x{:x} global_current_tid={}",
            n, site, cpu, gs, cur_ptr as u64, global_tid
        );
    }
}

#[inline]
pub fn kprcb_fallback_count() -> u64 { KPRCB_FB_COUNT.load(Ordering::Relaxed) }

// ── #345 Phase 2A: stress-harness event ring ──────────────────────────────
//
// Bounded, lock-free ring recording where the spawn-stress thread is in its
// iteration. No allocation, no serial per event. Dumped from the panic path and
// on demand so a stall can be classified from the last full transition.
pub const ST_RING_SIZE: usize = 128;

pub const ST_CREATE_BEGIN: u8 = 1;
pub const ST_CREATE_OK: u8 = 2;
pub const ST_CREATE_ERR: u8 = 3;
pub const ST_ACTIVATE_BEGIN: u8 = 4;
pub const ST_ACTIVATE_OK: u8 = 5;
pub const ST_YIELD_BEGIN: u8 = 6;
pub const ST_RESUME: u8 = 7;
pub const ST_ITER_BEGIN: u8 = 8;

#[derive(Clone, Copy)]
struct StEv {
    seq: u64,
    event: u8,
    cpu: u8,
    tid: u32,
    pid: u32,
    iteration: u64,
    child_pid: u32,
}

const ST_ZERO: StEv = StEv {
    seq: 0, event: 0, cpu: 0, tid: 0, pid: 0, iteration: 0, child_pid: 0,
};

static mut ST_RING: [StEv; ST_RING_SIZE] = [ST_ZERO; ST_RING_SIZE];
static ST_HEAD: AtomicU64 = AtomicU64::new(0);

/// Record one stress event. Lock-free; safe from any context.
#[inline]
pub fn st_ev(event: u8, iteration: u64, child_pid: u32) {
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as u8;
    let cur = unsafe { crate::arch::x64::cpu_local::this_cpu_current_thread() };
    let (tid, pid) = if cur.is_null() { (0u32, 0u32) } else { unsafe { ((*cur).tid, (*cur).pid) } };
    let seq = ST_HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % ST_RING_SIZE;
    let e = StEv { seq, event, cpu, tid, pid, iteration, child_pid };
    unsafe { core::ptr::write_volatile(&mut ST_RING[idx] as *mut StEv, e); }
}

pub fn st_dump_raw() {
    let head = ST_HEAD.load(Ordering::Relaxed);
    crate::raw_serial_println!("[ST_TRACE] head={} dump={} (1=CREATE_BEGIN 2=CREATE_OK 3=CREATE_ERR 4=ACTIVATE_BEGIN 5=ACTIVATE_OK 6=YIELD_BEGIN 7=RESUME 8=ITER_BEGIN)",
        head, head.min(ST_RING_SIZE as u64));
    if head == 0 { return; }
    let count = head.min(ST_RING_SIZE as u64) as usize;
    let start = if head > ST_RING_SIZE as u64 {
        (head - ST_RING_SIZE as u64) as usize
    } else {
        0
    };
    for i in 0..count {
        let idx = (start + i) % ST_RING_SIZE;
        let e = unsafe { core::ptr::read_volatile(&ST_RING[idx] as *const StEv) };
        crate::raw_serial_println!(
            "[ST_TRACE] #{} ev={} cpu={} tid={} pid={} iter={} child={}",
            e.seq, e.event, e.cpu, e.tid, e.pid, e.iteration, e.child_pid
        );
    }
}

