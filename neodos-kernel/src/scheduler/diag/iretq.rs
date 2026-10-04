//! #476 IRETQ frame audit (diagnostic only).
//!
//! The scheduler commits a "next thread" and returns its saved `rsp`; the
//! context-switch ASM then executes `mov rsp, next_rsp; pop 15 GPRs; iretq`.
//! The frame consumed by `iretq` therefore lives at `next_rsp + 120`:
//!
//! ```text
//! [next_rsp + 120] = RIP
//! [next_rsp + 128] = CS
//! [next_rsp + 136] = RFLAGS
//! [next_rsp + 144] = RSP   (Ring-3 return only)
//! [next_rsp + 152] = SS    (Ring-3 return only)
//! ```
//!
//! This module verifies that this frame is (a) inside the selected thread's
//! kernel stack, (b) ring-coherent with the thread kind, and (c) a plausible
//! iretq frame (canonical RIP/RSP, valid CS/SS, RFLAGS bit1). It emits
//! `[IRETQ_BAD_FRAME]` on violation. It never changes scheduling behaviour.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::scheduler::Kthread;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IretqBad {
    FrameOutsideKstack = 0,
    InvalidRip = 1,
    InvalidCs = 2,
    InvalidRsp = 3,
    InvalidSs = 4,
    InvalidRflags = 5,
    RingMismatch = 6,
    FrameAlignment = 7,
    TidStackMismatch = 8,
}

impl IretqBad {
    pub fn as_str(self) -> &'static str {
        match self {
            IretqBad::FrameOutsideKstack => "FRAME_OUTSIDE_KSTACK",
            IretqBad::InvalidRip => "INVALID_RIP",
            IretqBad::InvalidCs => "INVALID_CS",
            IretqBad::InvalidRsp => "INVALID_RSP",
            IretqBad::InvalidSs => "INVALID_SS",
            IretqBad::InvalidRflags => "INVALID_RFLAGS",
            IretqBad::RingMismatch => "RING_MISMATCH",
            IretqBad::FrameAlignment => "FRAME_ALIGNMENT",
            IretqBad::TidStackMismatch => "TID_STACK_MISMATCH",
        }
    }
}

pub static IRETQ_BAD_COUNT: AtomicU64 = AtomicU64::new(0);
pub static IRETQ_BAD_KINDS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];
pub static IRETQ_CHECKED: AtomicU64 = AtomicU64::new(0);

const RING: usize = 32;
#[derive(Clone, Copy)]
struct Ev {
    kind: u8, cpu: u32, tid: u32, pid: u32, next_rsp: u64, frame_addr: u64,
    rip: u64, cs: u64, rflags: u64, rsp: u64, ss: u64, ks_top: u64, seq: u64,
}
const ZERO: Ev = Ev { kind: 0, cpu: 0, tid: 0, pid: 0, next_rsp: 0, frame_addr: 0, rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0, ks_top: 0, seq: 0 };
static mut RING_BUF: [Ev; RING] = [ZERO; RING];
static HEAD: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub struct Frame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

pub const KERNEL_CS: u64 = 0x08;
pub const USER_CS: u64 = 0x1B;
pub const USER_SS: u64 = 0x23;

#[inline]
pub fn canonical(a: u64) -> bool {
    (((a << 16) as i64) >> 16) as u64 == a
}

/// Pure validator (unit-testable). `frame_addr` is `next_rsp + 120`.
/// `expect_ring3` is true for user threads, false for kernel/idle threads.
pub fn validate(frame_addr: u64, ks_base: u64, ks_top: u64, f: &Frame, expect_ring3: bool) -> Option<IretqBad> {
    let need = if f.cs & 3 == 3 { 40 } else { 24 };
    if frame_addr < ks_base || frame_addr + need > ks_top {
        return Some(IretqBad::FrameOutsideKstack);
    }
    if frame_addr & 7 != 0 {
        return Some(IretqBad::FrameAlignment);
    }
    match f.cs {
        KERNEL_CS => {
            if expect_ring3 {
                return Some(IretqBad::RingMismatch);
            }
        }
        USER_CS => {
            if !expect_ring3 {
                return Some(IretqBad::RingMismatch);
            }
        }
        _ => return Some(IretqBad::InvalidCs),
    }
    // RFLAGS: bit1 is architecturally 1; bits 63:22 reserved.
    if f.rflags & 0x2 == 0 || (f.rflags >> 22) != 0 {
        return Some(IretqBad::InvalidRflags);
    }
    // RIP: non-zero, canonical; Ring-3 RIP must be in the low canonical half.
    if f.rip == 0 || !canonical(f.rip) {
        return Some(IretqBad::InvalidRip);
    }
    if f.cs & 3 == 3 && f.rip >= 0x0000_8000_0000_0000 {
        return Some(IretqBad::InvalidRip);
    }
    if f.cs & 3 == 3 {
        if f.ss != USER_SS {
            return Some(IretqBad::InvalidSs);
        }
        if f.rsp == 0 || !canonical(f.rsp) || f.rsp >= 0x0000_8000_0000_0000 {
            return Some(IretqBad::InvalidRsp);
        }
    }
    None
}

fn push(kind: IretqBad, cpu: u32, tid: u32, pid: u32, next_rsp: u64, frame_addr: u64, f: &Frame, ks_top: u64) {
    let seq = HEAD.fetch_add(1, Ordering::Relaxed);
    let e = Ev { kind: kind as u8, cpu, tid, pid, next_rsp, frame_addr, rip: f.rip, cs: f.cs, rflags: f.rflags, rsp: f.rsp, ss: f.ss, ks_top, seq };
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(RING_BUF[(seq as usize) % RING]), e); }
}

/// Validate the dispatch frame the context-switch ASM is about to consume for
/// `rsp` (base of the 15-GPR block). `k` is the selected thread (or the current
/// one for a no-switch return). Must be called with the scheduler lock held.
pub fn audit(
    sched: &crate::scheduler::Scheduler,
    rsp: u64,
    k: *const Kthread,
    expect_ring3: bool,
    site: &'static str,
) {
    audit_core(Some(sched), rsp, k, expect_ring3, site);
}

/// Lock-free variant for the ASM trace hooks (no scheduler scan, no ring
/// expectation: the frame's own CS decides the ring, so a user thread caught in
/// Ring 0 inside a syscall is not a false positive). Validates everything else.
pub fn audit_self(rsp: u64, k: *const Kthread, site: &'static str) {
    if rsp < 0x1000 {
        return;
    }
    let cs = unsafe { core::ptr::read_volatile((rsp + 128) as *const u64) };
    audit_core(None, rsp, k, (cs & 3) == 3, site);
}

fn audit_core(
    sched: Option<&crate::scheduler::Scheduler>,
    rsp: u64,
    k: *const Kthread,
    expect_ring3: bool,
    site: &'static str,
) {
    if rsp < 0x1000 || k.is_null() {
        return;
    }
    let (ks_top, ks_size, tid, pid, is_kernel) = unsafe {
        ((*k).kernel_stack_top, (*k).kernel_stack_size, (*k).tid, (*k).pid, (*k).is_idle)
    };
    if ks_top == 0 {
        return;
    }
    let ks_base = ks_top.saturating_sub(ks_size as u64);
    let frame_addr = rsp + 120;
    let f = unsafe {
        Frame {
            rip: core::ptr::read_volatile(frame_addr as *const u64),
            cs: core::ptr::read_volatile((frame_addr + 8) as *const u64),
            rflags: core::ptr::read_volatile((frame_addr + 16) as *const u64),
            rsp: core::ptr::read_volatile((frame_addr + 24) as *const u64),
            ss: core::ptr::read_volatile((frame_addr + 32) as *const u64),
        }
    };
    IRETQ_CHECKED.fetch_add(1, Ordering::Relaxed);
    let mut bad = validate(frame_addr, ks_base, ks_top, &f, expect_ring3);
    // Cross-thread stack ownership: is `rsp` inside a *different* live thread's
    // kernel stack?
    if bad.is_none() {
        if let Some(sched) = sched {
            for t in sched.kthreads.iter().flatten() {
                if t.tid == tid || t.kernel_stack_top == 0 { continue; }
                let b = t.kernel_stack_top.saturating_sub(t.kernel_stack_size as u64);
                if rsp >= b && rsp < t.kernel_stack_top {
                    bad = Some(IretqBad::TidStackMismatch);
                    break;
                }
            }
        }
    }
    if let Some(kind) = bad {
        IRETQ_BAD_COUNT.fetch_add(1, Ordering::Relaxed);
        IRETQ_BAD_KINDS[kind as usize].fetch_add(1, Ordering::Relaxed);
        let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        let name = unsafe { (*(k)).name() };
        let k_cpu = unsafe { (*k).cpu };
        let k_state = unsafe { (*k).state };
        let kprcb_tid = crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(u32::MAX);
        crate::raw_serial_println!(
            "[IRETQ_BAD_FRAME] kind={} site={} cpu={} tid={} pid={} name={} is_kernel={} k_cpu={} k_state={:?} kprcb_tid={} next_rsp=0x{:x} ks_base=0x{:x} ks_top=0x{:x} frame_addr=0x{:x} rip=0x{:x} cs=0x{:x} rflags=0x{:x} user_rsp=0x{:x} ss=0x{:x}",
            kind.as_str(), site, cpu, tid, pid, name, is_kernel, k_cpu, k_state, kprcb_tid, rsp, ks_base, ks_top, frame_addr,
            f.rip, f.cs, f.rflags, f.rsp, f.ss);
        // Dump the recent `k.rsp` write history and switch-out ring so the
        // writer that produced this rsp can be reconstructed.
        crate::scheduler::diag::rsp_dump_raw();
        crate::scheduler::diag::kstack::dump_raw();
        push(kind, cpu, tid, pid, rsp, frame_addr, &f, ks_top);
    }
}

pub fn dump_raw() {
    let mut kinds = [0u64; 9];
    for (i, k) in IRETQ_BAD_KINDS.iter().enumerate() { kinds[i] = k.load(Ordering::Relaxed); }
    crate::raw_serial_println!(
        "[IRETQ_AUDIT] checked={} bad={} kinds=[OUTSIDE={} RIP={} CS={} RSP={} SS={} RFLAGS={} RING={} ALIGN={} TIDSTACK={}]",
        IRETQ_CHECKED.load(Ordering::Relaxed), IRETQ_BAD_COUNT.load(Ordering::Relaxed),
        kinds[0], kinds[1], kinds[2], kinds[3], kinds[4], kinds[5], kinds[6], kinds[7], kinds[8]);
    let head = HEAD.load(Ordering::Relaxed);
    let n = core::cmp::min(head, RING as u64);
    for s in head.saturating_sub(n)..head {
        let e = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(RING_BUF[(s as usize) % RING])) };
        if e.kind != 0 {
            crate::raw_serial_println!(
                "[IRETQ_BAD_RING] #{} kind={} cpu={} tid={} pid={} next_rsp=0x{:x} frame=0x{:x} rip=0x{:x} cs=0x{:x} rflags=0x{:x} user_rsp=0x{:x} ss=0x{:x} ks_top=0x{:x}",
                e.seq, e.kind, e.cpu, e.tid, e.pid, e.next_rsp, e.frame_addr, e.rip, e.cs, e.rflags, e.rsp, e.ss, e.ks_top);
        }
    }
}
