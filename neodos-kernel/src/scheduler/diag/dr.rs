//! DOUBLE_RUNNING / STACK_OWNER_MISMATCH rings.

use core::sync::atomic::{AtomicU64, Ordering};

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

