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

