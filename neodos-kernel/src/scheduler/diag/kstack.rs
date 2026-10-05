//! #476 H1 experiment — per-CPU "switch-out kernel stack" tracking.
//!
//! H1 hypothesis: a terminated thread's kernel stack can be reclaimed by one
//! CPU while another CPU is still in the window between repointing
//! `KPRCB.current_thread` and executing the `mov rsp` that physically leaves
//! that stack.
//!
//! This module makes that window observable:
//!
//! * [`note`] is called at the instant a CPU repoints `KPRCB.current_thread`
//!   (immediately before it abandons the current kernel stack).
//! * [`switch_out_clear`] is called from the context-switch ASM **after** the
//!   `mov rsp` to the next stack.
//! * [`reclaim_conflict`] reports the stack a CPU is currently abandoning, so
//!   `recycle_terminated()` can refuse to free it and log
//!   `[KSTACK_RECLAIM_CONFLICT]`.
//!
//! Diagnostic only: it never changes scheduling behaviour except to *leak* a
//! conflicting stack so the experiment can continue without corrupting memory.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::arch::x64::cpu_local::MAX_CPUS;

/// True while `cpu` is between its KPRCB update and the `mov rsp`.
pub static ACTIVE: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
/// Kernel-stack top of the thread being abandoned by `cpu`.
pub static KS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
pub static TID: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
pub static PID: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
/// Live RSP captured at the note (still inside the abandoned stack).
pub static RSP: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
pub static SIZE: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
/// Kernel-stack top of the thread being switched to.
pub static NEXT_KS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];

pub static NOTED: AtomicU64 = AtomicU64::new(0);
pub static CLEARED: AtomicU64 = AtomicU64::new(0);
pub static CONFLICTS: AtomicU64 = AtomicU64::new(0);
/// #476 permanent regression: count of switch-out boundary violations
/// (`owner(live rsp) != KPRCB.current_thread`). Must stay 0.
pub static RSP_OWNER_MISMATCH: AtomicU64 = AtomicU64::new(0);
static RSP_OWNER_MISMATCH_LOGGED: AtomicU64 = AtomicU64::new(0);

const RING: usize = 256;

#[derive(Clone, Copy)]
struct Ev {
    kind: u8, // 1 = OUT, 2 = CLR, 3 = CONFLICT
    cpu: u8,
    tid: u32,
    pid: u32,
    ks: u64,
    rsp: u64,
    next_ks: u64,
    seq: u64,
}
const ZERO: Ev = Ev { kind: 0, cpu: 0, tid: 0, pid: 0, ks: 0, rsp: 0, next_ks: 0, seq: 0 };
static mut RING_BUF: [Ev; RING] = [ZERO; RING];
static HEAD: AtomicU64 = AtomicU64::new(0);

#[inline]
fn push(kind: u8, cpu: u8, tid: u32, pid: u32, ks: u64, rsp: u64, next_ks: u64) {
    let seq = HEAD.fetch_add(1, Ordering::Relaxed);
    let idx = (seq as usize) % RING;
    let e = Ev { kind, cpu, tid, pid, ks, rsp, next_ks, seq };
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(RING_BUF[idx]), e);
    }
}

/// Record that the current CPU is about to abandon `old`'s kernel stack and
/// run on `new` after the next `mov rsp`. Must be called immediately before the
/// `KPRCB.current_thread` write.
#[inline]
pub fn note(old: *const crate::scheduler::Kthread, new: *const crate::scheduler::Kthread, live_rsp: u64) {
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
    if cpu >= MAX_CPUS { return; }
    let (oks, otid, opid, osz) = if old.is_null() {
        (0u64, 0u64, 0u64, 0u64)
    } else {
        unsafe { ((*old).kernel_stack_top, (*old).tid as u64, (*old).pid as u64, (*old).kernel_stack_size as u64) }
    };
    let nks = if new.is_null() { 0u64 } else { unsafe { (*new).kernel_stack_top } };
    KS[cpu].store(oks, Ordering::Relaxed);
    TID[cpu].store(otid, Ordering::Relaxed);
    PID[cpu].store(opid, Ordering::Relaxed);
    RSP[cpu].store(live_rsp, Ordering::Relaxed);
    SIZE[cpu].store(osz, Ordering::Relaxed);
    NEXT_KS[cpu].store(nks, Ordering::Relaxed);
    ACTIVE[cpu].store(oks != 0, Ordering::Release);
    NOTED.fetch_add(1, Ordering::Relaxed);
    push(1, cpu as u8, otid as u32, opid as u32, oks, live_rsp, nks);
}

/// Clear the current CPU's switch-out marker. MUST be called from the ASM
/// *after* `mov rsp, next_rsp`, never from Rust before the stack switch.
#[no_mangle]
pub extern "C" fn switch_out_clear() {
    switch_out_clear_at(0);
}

/// Site-tagged variant (site passed by the ASM in `edi`): 1/2 = syscall
/// resched paths, 3 = timer, 0 = exception/other. At this instant the CPU has
/// executed `mov rsp`, so `owner(live rsp) == KPRCB.current_thread` MUST hold.
/// This is the permanent regression assertion for #476: it detects the
/// "KPRCB published before the physical stack switch" window.
#[no_mangle]
pub extern "C" fn switch_out_clear_at(site: u64) {
    let cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
    if cpu >= MAX_CPUS { return; }
    let ks = KS[cpu].load(Ordering::Relaxed);
    let nks = NEXT_KS[cpu].load(Ordering::Relaxed);
    ACTIVE[cpu].store(false, Ordering::Release);
    NEXT_KS[cpu].store(0, Ordering::Relaxed);
    CLEARED.fetch_add(1, Ordering::Relaxed);
    push(2, cpu as u8, 0, 0, ks, 0, nks);

    let cur = unsafe { crate::arch::x64::cpu_local::this_cpu_current_thread() };
    if !cur.is_null() {
        let tid = unsafe { (*cur).tid };
        let cks = unsafe { (*cur).kernel_stack_top };
        let csz = unsafe { (*cur).kernel_stack_size };
        if tid != crate::scheduler::BOOT_TID && cks != 0 && csz != 0 {
            let live = unsafe { crate::hal::raw::raw_read_rsp() };
            if !crate::scheduler::stack::rsp_in_kernel_stack(cks, csz, live) {
                let n = RSP_OWNER_MISMATCH.fetch_add(1, Ordering::Relaxed) + 1;
                if RSP_OWNER_MISMATCH_LOGGED.fetch_add(1, Ordering::Relaxed) < 16 {
                    crate::raw_serial_println!(
                        "[RSP_OWNER_MISMATCH] site={} cpu={} kprcb_tid={} kprcb_ks_top=0x{:x} ks_size={} live_rsp=0x{:x} n={}",
                        site, cpu, tid, cks, csz, live, n);
                }
            }
        }
    }
}

/// If some CPU is currently abandoning the stack `ks_top`, return
/// `(owner_cpu, tid, pid, live_rsp, next_ks)`.
pub fn reclaim_conflict(ks_top: u64) -> Option<(usize, u64, u64, u64, u64)> {
    if ks_top == 0 { return None; }
    for cpu in 0..MAX_CPUS {
        if ACTIVE[cpu].load(Ordering::Acquire) && KS[cpu].load(Ordering::Relaxed) == ks_top {
            return Some((
                cpu,
                TID[cpu].load(Ordering::Relaxed),
                PID[cpu].load(Ordering::Relaxed),
                RSP[cpu].load(Ordering::Relaxed),
                NEXT_KS[cpu].load(Ordering::Relaxed),
            ));
        }
    }
    None
}

/// Record an observed conflict. Called from `recycle_terminated` under the
/// scheduler lock, after it has decided to leak the conflicting stack.
pub fn record_conflict(
    reclaimer_cpu: usize,
    owner_cpu: usize,
    tid: u64,
    pid: u64,
    ks_top: u64,
    size: u64,
    rsp: u64,
    next_ks: u64,
) {
    let n = CONFLICTS.fetch_add(1, Ordering::Relaxed) + 1;
    crate::raw_serial_println!(
        "[KSTACK_RECLAIM_CONFLICT] reclaimer_cpu={} owner_cpu={} tid={} pid={} kstack_top=0x{:x} kstack_size={} live_rsp=0x{:x} next_ks=0x{:x} phase=SWITCH_OUT_KS conflicts={}",
        reclaimer_cpu, owner_cpu, tid, pid, ks_top, size, rsp, next_ks, n);
    push(3, owner_cpu as u8, tid as u32, pid as u32, ks_top, rsp, next_ks);
}

/// True if any CPU is currently mid-switch. Used by tests.
pub fn any_active() -> bool {
    (0..MAX_CPUS).any(|c| ACTIVE[c].load(Ordering::Acquire))
}

pub fn dump_raw() {
    let head = HEAD.load(Ordering::Relaxed);
    let n = core::cmp::min(head, RING as u64);
    crate::raw_serial_println!(
        "[KSTACK_RING] noted={} cleared={} conflicts={} head={} dump={}",
        NOTED.load(Ordering::Relaxed), CLEARED.load(Ordering::Relaxed),
        CONFLICTS.load(Ordering::Relaxed), head, n);
    let start = head.saturating_sub(n);
    for s in start..head {
        let idx = (s as usize) % RING;
        let e = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(RING_BUF[idx])) };
        let k = match e.kind { 1 => "OUT", 2 => "CLR", 3 => "CONFLICT", _ => "?" };
        crate::raw_serial_println!(
            "[KSTACK_RING] #{} {} cpu={} tid={} pid={} ks=0x{:x} rsp=0x{:x} next_ks=0x{:x}",
            e.seq, k, e.cpu, e.tid, e.pid, e.ks, e.rsp, e.next_ks);
    }
    for cpu in 0..MAX_CPUS {
        if ACTIVE[cpu].load(Ordering::Relaxed) {
            crate::raw_serial_println!(
                "[KSTACK_STATE] cpu={} ACTIVE ks=0x{:x} tid={} pid={} rsp=0x{:x} next_ks=0x{:x}",
                cpu, KS[cpu].load(Ordering::Relaxed), TID[cpu].load(Ordering::Relaxed),
                PID[cpu].load(Ordering::Relaxed), RSP[cpu].load(Ordering::Relaxed),
                NEXT_KS[cpu].load(Ordering::Relaxed));
        }
    }
}
