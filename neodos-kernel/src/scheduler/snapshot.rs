//! Phase 14-B: read-only kernel process/thread inspection.
//!
//! This module produces a **scheduler-consistent snapshot** of the logical
//! process/thread objects so that future diagnostic tooling does not have to
//! walk live scheduler structures directly.
//!
//! Design constraints (see `docs/scheduler/scheduler.md`):
//! - Read-only: never enqueues/dequeues/migrates or mutates any scheduler field.
//! - The snapshot owns bounded copies of the data (`KernelName`, `ThreadState`,
//!   `u32`); it holds no references into live `Eprocess`/`Kthread` objects and
//!   no scheduler lock after it is returned.
//! - The authoritative source is `Scheduler.eprocesses` / `Scheduler.kthreads`
//!   (the logical registry used by lifecycle/reaping), **not** the run queues.
//! - Consistency: every field is copied while the global `SCHEDULER` mutex is
//!   held. `KPRCB.current_thread` is only written under that same mutex (except
//!   the pre-AP-scheduling bootstrap), so the CPU/ownership view is coherent
//!   with the process/thread state at snapshot time.
//! - Ordering: processes by `pid`, threads by `tid` (deterministic).
//! - Lifecycle: a reaped object is already removed from the registry and does
//!   not appear. A `Terminated` (not-yet-reaped) thread still appears with
//!   `state == Terminated`, matching the existing lifecycle.
//! - No user ABI is introduced here (Phase 15 owns the user-facing interface).

use crate::scheduler::types::{KernelName, ThreadState};
use crate::scheduler::Scheduler;

/// Hard upper bounds for a snapshot. The logical registry can grow without a
/// fixed limit, so enumeration truncates deterministically and sets
/// `truncated`.
pub const MAX_SNAPSHOT_PROCESSES: usize = 64;
pub const MAX_SNAPSHOT_THREADS: usize = 128;

/// Bounded, owned process record.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub name: KernelName,
    /// Number of live threads currently mapped to this pid in the snapshot.
    pub thread_count: u32,
    /// Phase 15-A.1: sum of the CPU execution counters of this process's
    /// threads, in timer intervals (Δ vs. a later snapshot yields CPU%).
    /// Idle threads are excluded, so idle CPU is never attributed here.
    pub cpu_time: u64,
    /// MEM-PROC (#274): reserved/allocated bytes (heap span + mmap regions).
    pub committed_bytes: u64,
    /// MEM-PROC (#274): resident bytes (mapped 4 KB heap pages × 4096).
    pub working_set_bytes: u64,
}

impl ProcessSnapshot {
    pub const EMPTY: ProcessSnapshot = ProcessSnapshot {
        pid: 0,
        name: KernelName::empty(),
        thread_count: 0,
        cpu_time: 0,
        committed_bytes: 0,
        working_set_bytes: 0,
    };
}

/// Bounded, owned thread record.
#[derive(Clone, Copy, PartialEq)]
pub struct ThreadSnapshot {
    pub tid: u32,
    pub pid: u32,
    pub name: KernelName,
    /// Existing scheduler state definition (no diagnostic-only enum).
    pub state: ThreadState,
    /// Logical CPU: for a Running thread this is the `KPRCB.current_thread`
    /// owner; otherwise the scheduler's `Kthread.cpu` assignment.
    pub cpu: u32,
    pub idle: bool,
    /// True when some CPU's `KPRCB.current_thread` is this thread.
    pub is_current: bool,
    /// Phase 15-A.1: authoritative monotonic CPU execution counter (timer
    /// intervals). Idle threads report 0.
    pub cpu_time: u64,
}

impl ThreadSnapshot {
    pub const EMPTY: ThreadSnapshot = ThreadSnapshot {
        tid: 0,
        pid: 0,
        name: KernelName::empty(),
        state: ThreadState::Ready,
        cpu: 0,
        idle: false,
        is_current: false,
        cpu_time: 0,
    };
}

/// Fixed-capacity snapshot container. This is ~9 KB; allocate it on the heap
/// (e.g. `Box::new(ProcSnapshot::empty())`) rather than on a kernel stack.
pub struct ProcSnapshot {
    pub processes: [ProcessSnapshot; MAX_SNAPSHOT_PROCESSES],
    pub process_count: usize,
    pub threads: [ThreadSnapshot; MAX_SNAPSHOT_THREADS],
    pub thread_count: usize,
    /// True when the registry exceeded the fixed capacity and was truncated.
    pub truncated: bool,
}

impl ProcSnapshot {
    pub const fn empty() -> Self {
        ProcSnapshot {
            processes: [ProcessSnapshot::EMPTY; MAX_SNAPSHOT_PROCESSES],
            process_count: 0,
            threads: [ThreadSnapshot::EMPTY; MAX_SNAPSHOT_THREADS],
            thread_count: 0,
            truncated: false,
        }
    }

    /// Read-only lookup by TID (snapshot is already detached from live state).
    pub fn thread(&self, tid: u32) -> Option<&ThreadSnapshot> {
        self.threads[..self.thread_count].iter().find(|t| t.tid == tid)
    }

    /// Read-only lookup by PID.
    pub fn process(&self, pid: u32) -> Option<&ProcessSnapshot> {
        self.processes[..self.process_count].iter().find(|p| p.pid == pid)
    }
}

impl Scheduler {
    /// Fill `out` from this scheduler's logical registries under the caller's
    /// lock. Read-only: no run-queue or thread-state mutation occurs.
    ///
    /// Must be called with the `SCHEDULER` mutex held (or on a local test
    /// scheduler). The result contains no references into live objects.
    pub fn snapshot_into(&self, out: &mut ProcSnapshot) {
        out.process_count = 0;
        out.thread_count = 0;
        out.truncated = false;

        // Processes: PID + name + live thread count + aggregated CPU time.
        // CPU time is derived from the authoritative per-thread counters
        // (sum over threads), so there is no independent process counter to
        // drift. Idle threads contribute nothing.
        for ep in self.eprocesses.iter().flatten() {
            if out.process_count >= MAX_SNAPSHOT_PROCESSES {
                out.truncated = true;
                break;
            }
            let mut tc: u32 = 0;
            let mut cpu_time: u64 = 0;
            for k in self.kthreads.iter().flatten() {
                if k.pid == ep.pid {
                    tc += 1;
                    if !k.is_idle {
                        let ptr = &**k as *const crate::scheduler::Kthread;
                        let owner = crate::arch::x64::cpu_local::kthread_current_cpu(ptr);
                        cpu_time = cpu_time.saturating_add(
                            crate::scheduler::accounting::cpu_time_now(k, owner));
                    }
                }
            }
            // MEM-PROC (#274): committed = heap span + mmap regions;
            // working set = resident 4 KB heap pages of this process's slot.
            let heap_span = ep.heap_break.saturating_sub(ep.heap_base);
            let mmap_bytes: u64 = ep.mmap_regions.iter().map(|m| m.len).sum();
            let committed_bytes = heap_span.saturating_add(mmap_bytes);
            let working_set_bytes = if ep.heap_base == 0 {
                0
            } else {
                let slot = ((ep.heap_base - crate::arch::x64::paging::PROCESS_HEAP_BASE)
                    / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as usize;
                (crate::arch::x64::paging::heap_slot_resident_pages(slot) as u64)
                    .saturating_mul(crate::arch::x64::paging::PAGE_4K)
            };
            out.processes[out.process_count] = ProcessSnapshot {
                pid: ep.pid,
                name: ep.name,
                thread_count: tc,
                cpu_time,
                committed_bytes,
                working_set_bytes,
            };
            out.process_count += 1;
        }

        // Threads: identity + state + CPU + idle flag + CPU time.
        for k in self.kthreads.iter().flatten() {
            if out.thread_count >= MAX_SNAPSHOT_THREADS {
                out.truncated = true;
                break;
            }
            let ptr = &**k as *const crate::scheduler::Kthread;
            let owner = crate::arch::x64::cpu_local::kthread_current_cpu(ptr);
            out.threads[out.thread_count] = ThreadSnapshot {
                tid: k.tid,
                pid: k.pid,
                name: k.name,
                state: k.state,
                cpu: owner.unwrap_or(k.cpu),
                idle: k.is_idle,
                is_current: owner.is_some(),
                cpu_time: if k.is_idle { 0 } else {
                    crate::scheduler::accounting::cpu_time_now(k, owner)
                },
            };
            out.thread_count += 1;
        }

        // Deterministic ordering (unique keys → no allocation).
        out.processes[..out.process_count].sort_unstable_by_key(|p| p.pid);
        out.threads[..out.thread_count].sort_unstable_by_key(|t| t.tid);
    }
}

/// Copy a scheduler-consistent snapshot of the global scheduler.
/// The `SCHEDULER` lock is released before this returns.
pub fn kernel_snapshot_into(out: &mut ProcSnapshot) {
    let s = crate::scheduler::current_scheduler();
    let lock = s.lock();
    lock.snapshot_into(out);
}

fn state_name(s: ThreadState) -> &'static str {
    match s {
        ThreadState::Ready => "Ready",
        ThreadState::Running => "Running",
        ThreadState::Blocked { .. } => "Blocked",
        ThreadState::Suspended => "Suspended",
        ThreadState::Terminated => "Terminated",
    }
}

/// Explicit diagnostic dump of the global process/thread snapshot.
///
/// Locking discipline: the snapshot is copied under `SCHEDULER`, the lock is
/// released, and only then is anything printed (no console I/O under a lock).
pub fn kernel_snapshot_dump() {
    use alloc::boxed::Box;
    let mut snap = Box::new(ProcSnapshot::empty());
    kernel_snapshot_into(&mut snap);

    crate::serial_println!(
        "[PROC_SNAPSHOT] processes={} threads={} truncated={}",
        snap.process_count, snap.thread_count, snap.truncated);
    for p in snap.processes[..snap.process_count].iter() {
        crate::serial_println!(
            "[PROC_SNAPSHOT] pid={} name={} threads={}",
            p.pid, p.name, p.thread_count);
    }
    for t in snap.threads[..snap.thread_count].iter() {
        crate::serial_println!(
            "[PROC_SNAPSHOT]   tid={} pid={} name={} cpu={} state={} idle={} current={}",
            t.tid, t.pid, t.name, t.cpu, state_name(t.state), t.idle as u8, t.is_current as u8);
    }
}
