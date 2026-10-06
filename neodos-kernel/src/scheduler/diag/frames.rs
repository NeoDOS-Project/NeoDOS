//! Return-frame ring (#293 Phase 4).

use core::sync::atomic::{AtomicU64, Ordering};

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

