//! Scheduler lifecycle — extracted from mod.rs
use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::log::LogSubsys;
use crate::object::{self, ObType};
use crate::object::ObId;
use crate::scheduler::types::{Eprocess, Kthread, ThreadState, PRIORITY_NORMAL, TIME_SLICES, KERNEL_STACK_SIZE};
use crate::scheduler::stack::{AlignedKStack, init_ring0_frame};
use crate::scheduler::Scheduler;
use core::sync::atomic::{AtomicU64, Ordering};

/// Soft watermark: queue length at which a spawn attempts a synchronous
/// reclaim before it is considered backpressured (see `spawn_usermode`).
pub const MAX_ZOMBIES: usize = 64;

/// Deterministic hard cap for the zombie queue (NEODOS-01 / #631).
///
/// A terminated PID is enqueued only while it still runs on some CPU, so the
/// number of *genuinely unreapable* entries is bounded by the CPU count.
/// Crossing this cap therefore means reaping lag or stale entries; it is
/// surfaced through `ZOMBIE_OVERFLOW` and an error log (never silently), and
/// the scheduler-aware paths force a synchronous reclaim to fall back under it.
pub const ZOMBIE_HARD_CAP: usize = MAX_ZOMBIES * 2;

/// NeoInit's PID. INV-10: it MUST NEVER BE KILLED (source-of-truth.md §INV-10).
pub const INIT_PID: u32 = 1;

// ── Zombie-lifecycle observability counters (NEODOS-01 / #631) ──────────────
static ZOMBIE_ENQUEUED: AtomicU64 = AtomicU64::new(0);
static ZOMBIE_DEDUP_SKIPPED: AtomicU64 = AtomicU64::new(0);
static ZOMBIE_REQUEUED: AtomicU64 = AtomicU64::new(0);
static ZOMBIE_STALE_DROPPED: AtomicU64 = AtomicU64::new(0);
static ZOMBIE_OVERFLOW: AtomicU64 = AtomicU64::new(0);
static ZOMBIE_BACKPRESSURE: AtomicU64 = AtomicU64::new(0);

/// Snapshot of the zombie queue state and its lifecycle counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ZombieStats {
    pub len: usize,
    pub enqueued: u64,
    pub dedup_skipped: u64,
    pub requeued: u64,
    pub stale_dropped: u64,
    pub overflow: u64,
    pub backpressure_hits: u64,
}

/// Zombie queue state machine.
///
/// Kept free of the "is this PID running?" side effect: that predicate is a
/// parameter of the query/reclaim helpers, so the policy can be unit-tested
/// deterministically (see `scheduler::tests::regressions`).
pub struct ZombieQueue {
    pids: Vec<u32>,
}

#[allow(clippy::new_without_default)]
impl ZombieQueue {
    pub fn new() -> Self {
        ZombieQueue { pids: Vec::with_capacity(MAX_ZOMBIES) }
    }

    #[inline]
    pub fn len(&self) -> usize { self.pids.len() }

    #[inline]
    pub fn is_empty(&self) -> bool { self.pids.is_empty() }

    #[inline]
    pub fn over_hard_cap(&self) -> bool { self.pids.len() >= ZOMBIE_HARD_CAP }

    /// Enqueue `pid` at most once. Returns `false` when it was a duplicate.
    pub fn enqueue(&mut self, pid: u32) -> bool {
        if pid == 0 { return false; }
        if self.pids.iter().any(|&p| p == pid) {
            ZOMBIE_DEDUP_SKIPPED.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.pids.push(pid);
        ZOMBIE_ENQUEUED.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Re-queue a PID that is still alive (dedup; counted separately).
    pub fn requeue(&mut self, pid: u32) {
        if pid == 0 { return; }
        if !self.pids.iter().any(|&p| p == pid) {
            self.pids.push(pid);
            ZOMBIE_REQUEUED.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Index of the first enqueued PID that is neither `exclude` nor currently
    /// running, according to `is_running`.
    pub fn find_reclaimable<F: Fn(u32) -> bool>(
        &self,
        exclude: u32,
        is_running: F,
    ) -> Option<usize> {
        self.pids.iter().position(|&p| p != exclude && !is_running(p))
    }

    /// Remove and return the PID at `pos`, if any.
    pub fn take_at(&mut self, pos: usize) -> Option<u32> {
        if pos < self.pids.len() { Some(self.pids.remove(pos)) } else { None }
    }

    /// True when the queue is at the soft watermark and no entry can be
    /// reclaimed (every entry is still running).
    pub fn backpressured<F: Fn(u32) -> bool>(&self, is_running: F) -> bool {
        if self.pids.len() < MAX_ZOMBIES { return false; }
        !self.pids.iter().any(|&p| !is_running(p))
    }
}

lazy_static! {
    static ref ZOMBIE_QUEUE: Mutex<ZombieQueue> = Mutex::new(ZombieQueue::new());
}

/// Snapshot the zombie queue state and lifecycle counters.
pub fn zombie_queue_stats() -> ZombieStats {
    let q = ZOMBIE_QUEUE.lock();
    ZombieStats {
        len: q.len(),
        enqueued: ZOMBIE_ENQUEUED.load(Ordering::Relaxed),
        dedup_skipped: ZOMBIE_DEDUP_SKIPPED.load(Ordering::Relaxed),
        requeued: ZOMBIE_REQUEUED.load(Ordering::Relaxed),
        stale_dropped: ZOMBIE_STALE_DROPPED.load(Ordering::Relaxed),
        overflow: ZOMBIE_OVERFLOW.load(Ordering::Relaxed),
        backpressure_hits: ZOMBIE_BACKPRESSURE.load(Ordering::Relaxed),
    }
}

/// Defer EPROCESS slot recycling until after context switch (NEODOS-01 / #631).
///
/// The PID's KTHREAD stacks remain valid while the current thread still
/// executes, so the slot cannot be recycled until no CPU runs the PID. Called
/// with the scheduler lock held by every enqueue path (`terminate_current`,
/// `kill_pid`, `cleanup_terminated_process`). It enqueues the PID (dedup) and
/// then synchronously reclaims the oldest not-running zombies until the queue
/// falls back under `ZOMBIE_HARD_CAP`, so the cap is actually enforced.
///
/// The queue must never silently lose a live PID (F-DEV-02), so enqueue is
/// infallible; unreclaimable overflow is surfaced through `ZOMBIE_OVERFLOW`.
pub fn defer_reap_with_scheduler(sched: &mut Scheduler, pid: u32) {
    if pid == 0 { return; }
    ZOMBIE_QUEUE.lock().enqueue(pid);

    loop {
        let pos = {
            let q = ZOMBIE_QUEUE.lock();
            if !q.over_hard_cap() { break; }
            // Skip the newly queued pid: it is still running on this CPU until
            // the context switch, so it must not be reclaimed here.
            q.find_reclaimable(pid, |p| crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(p))
        };
        let pos = match pos {
            Some(p) => p,
            None => {
                // Every remaining zombie still executes on some CPU: it cannot
                // be reclaimed here. The queue is bounded by the CPU count in
                // practice, so this is an observable anomaly, not a loss.
                ZOMBIE_OVERFLOW.fetch_add(1, Ordering::Relaxed);
                kerror!(LogSubsys::Sched,
                    "zombie hard cap {} reached; all {} entries still running (pid {})",
                    ZOMBIE_HARD_CAP, zombie_queue_len(), pid);
                break;
            }
        };
        let pid_to_reclaim = match ZOMBIE_QUEUE.lock().take_at(pos) {
            Some(p) => p,
            None => break,
        };
        // Double-check after dropping the queue lock.
        if crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(pid_to_reclaim) {
            ZOMBIE_QUEUE.lock().requeue(pid_to_reclaim);
            break;
        }
        if !sched.recycle_terminated(pid_to_reclaim) {
            // A stale queue entry is not a silent loss: keep it only if the PID
            // is still live, otherwise drop it as a stale record.
            if crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(pid_to_reclaim) {
                ZOMBIE_QUEUE.lock().requeue(pid_to_reclaim);
            } else {
                ZOMBIE_STALE_DROPPED.fetch_add(1, Ordering::Relaxed);
                kwarn!(LogSubsys::Sched,
                    "stale zombie pid {} dropped from queue after reclaim miss",
                    pid_to_reclaim);
            }
            continue;
        }
    }
}

/// Expose queue length for spawn backpressure checks.
pub fn zombie_queue_len() -> usize {
    ZOMBIE_QUEUE.lock().len()
}

/// Check if spawn should be backpressured: the queue is at the soft watermark
/// and no zombie can be reclaimed (all still running). Records a backpressure
/// hit so the condition is observable.
pub fn is_zombie_backpressured() -> bool {
    let bp = ZOMBIE_QUEUE.lock()
        .backpressured(|p| crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(p));
    if bp {
        ZOMBIE_BACKPRESSURE.fetch_add(1, Ordering::Relaxed);
    }
    bp
}

/// Try to reap *all* zombies that are not running on ANY CPU.
/// Called from schedule() with scheduler lock already held, after the next
/// thread has been committed but BEFORE the caller switches RSP to it.
/// `exclude_pid` is the pid currently executing on this CPU (the context being
/// switched away from): its kernel stack is still under us until the caller's
/// `mov rsp`/`iretq`, so it MUST NOT be reclaimed here (F-02-A). Any other
/// pid is safe to reclaim.
/// F-01 ensures we never free a pid still running on any CPU; F-06 ensures we
/// drain the whole eligible set per schedule, so a burst of 1000 exits is reaped
/// in one schedule, not 1000 schedules.
pub fn reap_pending_zombies(sched: &mut Scheduler, exclude_pid: u32) {
    // NEODOS-03 (#633): free any quarantine stacks no CPU is mid-switch on.
    drain_kstack_quarantine();
    // Quick check without the queue lock when there is nothing to do.
    if ZOMBIE_QUEUE.lock().is_empty() { return; }

    // Drain loop: keep reaping while there is an eligible zombie.
    loop {
        let pos = {
            let q = ZOMBIE_QUEUE.lock();
            if q.is_empty() { break; }
            // First zombie not running on any CPU and not the stacked pid.
            q.find_reclaimable(exclude_pid, |p| {
                crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(p)
            })
        };
        let pid = match pos {
            Some(p) => match ZOMBIE_QUEUE.lock().take_at(p) {
                Some(pid) => pid,
                None => break,
            },
            None => break,
        };
        // Double-check after dropping the queue lock.
        if crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(pid) {
            ZOMBIE_QUEUE.lock().requeue(pid);
            continue;
        }
        // Recycle may take other locks (Ob), but not the zombie queue, so safe.
        if !sched.recycle_terminated(pid) {
            // A stale queue entry is not a silent loss: if the PID is still live
            // somewhere, requeue it; otherwise drop the stale record.
            if crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(pid) {
                ZOMBIE_QUEUE.lock().requeue(pid);
            } else {
                ZOMBIE_STALE_DROPPED.fetch_add(1, Ordering::Relaxed);
                kwarn!(LogSubsys::Sched,
                    "stale zombie pid {} already reaped; dropping stale queue entry",
                    pid);
            }
            continue;
        }
        // Continue loop to reap the next eligible zombie.
    }
}

// ── NEODOS-03 (#633): kernel-stack quarantine ───────────────────────────────
//
// A terminated thread's kernel stack must not be freed while another CPU is in
// the window between repointing `KPRCB.current_thread` and executing the
// `mov rsp` that leaves that stack (#476). Instead of *leaking* the stack
// (`core::mem::forget`, the previous workaround), it is retained here and freed
// by `drain_kstack_quarantine` once no CPU is mid-switch on it.
pub(crate) struct QuarantinedStack {
    top: u64,
    stack: Box<AlignedKStack>,
}

/// Bounded: at most one stack per CPU can be mid-switch at a time, so the cap is
/// generous; crossing it means an owner CPU is stuck and is surfaced via metrics.
const KSTACK_QUARANTINE_CAP: usize = crate::arch::x64::cpu_local::MAX_CPUS * 4;

static KSTACK_QUARANTINE: Mutex<Vec<QuarantinedStack>> = Mutex::new(Vec::new());
static KSTACK_Q_CURRENT: AtomicU64 = AtomicU64::new(0);
static KSTACK_Q_PUSHES: AtomicU64 = AtomicU64::new(0);
static KSTACK_Q_DRAINED: AtomicU64 = AtomicU64::new(0);
static KSTACK_Q_MAX: AtomicU64 = AtomicU64::new(0);
static KSTACK_Q_OVERFLOW: AtomicU64 = AtomicU64::new(0);

fn quarantine_push(top: u64, stack: Box<AlignedKStack>) {
    let mut q = KSTACK_QUARANTINE.lock();
    if q.len() >= KSTACK_QUARANTINE_CAP {
        // Drain first: a conflicting stack is only mid-switch on a CPU for a
        // few instructions, so entries should become drainable immediately.
        q.retain(|e| crate::scheduler::diag::kstack::reclaim_conflict(e.top).is_some());
        if q.len() >= KSTACK_QUARANTINE_CAP {
            KSTACK_Q_OVERFLOW.fetch_add(1, Ordering::Relaxed);
            kerror!(LogSubsys::Sched,
                "kstack quarantine overflow: {} entries (cap {}); retaining, not freeing a possibly-live stack",
                q.len(), KSTACK_QUARANTINE_CAP);
        }
    }
    q.push(QuarantinedStack { top, stack });
    KSTACK_Q_PUSHES.fetch_add(1, Ordering::Relaxed);
    let cur = q.len() as u64;
    KSTACK_Q_CURRENT.store(cur, Ordering::Relaxed);
    if cur > KSTACK_Q_MAX.load(Ordering::Relaxed) {
        KSTACK_Q_MAX.store(cur, Ordering::Relaxed);
    }
}

/// Free every quarantined stack that no CPU is still abandoning. Cheap when the
/// quarantine is empty (single atomic load). Called on every schedule from
/// `reap_pending_zombies`, with the scheduler lock held.
pub fn drain_kstack_quarantine() {
    if KSTACK_Q_CURRENT.load(Ordering::Relaxed) == 0 {
        return;
    }
    let mut q = KSTACK_QUARANTINE.lock();
    let before = q.len();
    // Keep (retain) only stacks still mid-switch on some CPU; the rest are freed.
    q.retain(|e| crate::scheduler::diag::kstack::reclaim_conflict(e.top).is_some());
    let drained = (before - q.len()) as u64;
    if drained > 0 {
        KSTACK_Q_DRAINED.fetch_add(drained, Ordering::Relaxed);
        KSTACK_Q_CURRENT.store(q.len() as u64, Ordering::Relaxed);
    }
}

/// (current, pushes, drained, high_water, overflow). `current` must return to 0
/// (no leak); `overflow` must stay 0 in normal operation.
pub fn kstack_quarantine_stats() -> (u64, u64, u64, u64, u64) {
    (
        KSTACK_Q_CURRENT.load(Ordering::Relaxed),
        KSTACK_Q_PUSHES.load(Ordering::Relaxed),
        KSTACK_Q_DRAINED.load(Ordering::Relaxed),
        KSTACK_Q_MAX.load(Ordering::Relaxed),
        KSTACK_Q_OVERFLOW.load(Ordering::Relaxed),
    )
}

/// Test-only: allocate a stack and enqueue it, returning the queue length.
#[doc(hidden)]
pub fn quarantine_push_for_test() -> u64 {
    let stack = AlignedKStack::new_boxed();
    let top = stack.0.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
    quarantine_push(top, stack);
    KSTACK_Q_CURRENT.load(Ordering::Relaxed)
}

/// Free all external resources owned by an EPROCESS: user memory slot, demand
/// paging heap, mmap regions and handle-table entries.
///
/// F-02-B: idempotent — safe to call after `terminate_current` already released
/// them (guards on `user_slot.take()`, `heap_base != 0`, empty mmap, closed
/// handles). Used by `recycle_terminated` and `kill_pid` so resources are freed
/// exactly once regardless of which path reclaims the process.
fn free_eprocess_resources(eproc: &mut Eprocess) {
    if let Some(slot) = eproc.user_slot.take() {
        crate::arch::x64::paging::free_user_slot(slot);
    }
    if eproc.heap_base != 0 {
        crate::arch::x64::paging::heap_free_range(
            eproc.heap_base,
            eproc.heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE,
        );
        let heap_idx = ((eproc.heap_base - crate::arch::x64::paging::PROCESS_HEAP_BASE)
            / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as u8;
        crate::arch::x64::paging::heap_slot_reset(heap_idx as usize);
        crate::arch::x64::paging::free_heap_slot(heap_idx);
        eproc.heap_base = 0;
        eproc.heap_break = 0;
    }
    for r in eproc.mmap_regions.iter() {
        crate::arch::x64::paging::mmap_free_range(r.base, r.base + r.len);
    }
    eproc.mmap_regions.clear();
    eproc.mmap_next = crate::arch::x64::paging::MMAP_BASE;
    for i in 0..eproc.handle_table.len() {
        let h = eproc.handle_table[i];
        if h.is_pipe_read() {
            crate::object::pipe::PIPE_MANAGER.dec_read_ref(h.native_id().unwrap_or(0) as u8);
        } else if h.is_pipe_write() {
            crate::object::pipe::PIPE_MANAGER.dec_write_ref(h.native_id().unwrap_or(0) as u8);
        } else if h.has_ob_object() {
            let _ = crate::object::ob_close_object(h.object_id);
        }
        eproc.handle_table.set(i as u8, crate::handle::HandleEntry::closed());
    }
}

impl Scheduler {
    /// Find the first free slot index in eprocesses vec, growing if full.
    /// P0.2: use try_reserve to avoid panic on OOM (was push() panic).
    pub fn alloc_eprocess_slot(&mut self) -> Option<usize> {
        if let Some(pos) = self.eprocesses.iter().position(|e| e.is_none()) {
            Some(pos)
        } else {
            let idx = self.eprocesses.len();
            if self.eprocesses.try_reserve(1).is_err() { return None; }
            self.eprocesses.push(None);
            Some(idx)
        }
    }

    /// Find the first free slot index in kthreads vec, growing if full.
    /// P0.2: try_reserve for OOM safety.
    pub fn alloc_kthread_slot(&mut self) -> Option<usize> {
        if let Some(pos) = self.kthreads.iter().position(|t| t.is_none()) {
            Some(pos)
        } else {
            let idx = self.kthreads.len();
            if self.kthreads.try_reserve(1).is_err() { return None; }
            self.kthreads.push(None);
            Some(idx)
        }
    }

    pub fn add_ring3_process(
        &mut self,
        entry: u64,
        user_stack_top: u64,
        slot_idx: u8,
        cwd_drive: u8,
        cwd_path: &str,
        heap_base: u64,
        parent_pid: u32,
    ) -> Result<u32, &'static str> {
        // Find free slots first before consuming PID/TID
        let ep_slot = self.alloc_eprocess_slot()
            .ok_or("EPROCESS table full")?;
        let th_slot = self.alloc_kthread_slot()
            .ok_or("KTHREAD table full")?;

        let pid = self.next_pid;
        self.next_pid += 1;

        let tid = self.next_tid;
        self.next_tid += 1;

        let mut eproc = Eprocess::new_ring3(pid, slot_idx, cwd_drive, cwd_path, heap_base, parent_pid);
        let mut thread = Kthread::new_ring3(tid, pid, entry, user_stack_top);

        let name = alloc::format!("eproc/{}", pid);
        if let Ok(kid) = object::ob_create_object(ObType::Process, &name, pid as u64, 0, None) {
            eproc.obj_id = Some(kid);
        }

        // OB-046: Register process in Ob namespace
        let ob_name = alloc::format!("proc/{}", pid);
        match object::ob_create_object(ObType::Process, &ob_name, pid as u64, 0, None) {
            Ok(ob_id) => {
                let ns_path = alloc::format!("\\Process\\{}", pid);
                match crate::object::namespace::ob_insert_object(&ns_path, ob_id) {
                    Ok(_) => {
                        kinfo!(LogSubsys::Sched, "PID {} -> \\Process\\{} OK (ob_id={})", pid, pid, ob_id);
                        eproc.ob_id = Some(ob_id);
                    }
                    Err(e) => {
                        kerror!(LogSubsys::Sched, "PID {} -> \\Process\\{} FAILED: {}", pid, pid, e);
                        let _ = object::ob_close_object(ob_id);
                    }
                }
            }
            Err(e) => {
                kerror!(LogSubsys::Sched, "PID {} ob_create FAILED: {:?}", pid, e);
            }
        }

        let tname = alloc::format!("kthread/{}", tid);
        if let Ok(kid) = object::ob_create_object(ObType::Thread, &tname, tid as u64, 0, None) {
            thread.obj_id = Some(kid);
        }

        eproc.thread_count = 1;

        // NT6.1: Inherit token from parent process
        if parent_pid > 0 {
            if let Some(parent_ep) = self.find_eprocess(parent_pid) {
                eproc.token = parent_ep.token.clone();
                eproc.vt_num = parent_ep.vt_num;
            }
        }

        self.eprocesses[ep_slot] = Some(eproc);
        thread.state = ThreadState::Suspended;
        self.kthreads[th_slot] = Some(Box::new(thread));

        kdebug!(LogSubsys::Sched, "[SCHED] CREATE TID={} PID={} priority={} state=Suspended (Ring 3)",
            tid, pid, PRIORITY_NORMAL);

        crate::trace_sched!(1, pid, 0); // ADD_PROCESS
        Ok(pid)
    }

    /// Add a new EPROCESS + initial KTHREAD (Ring 3) with ALL resources
    /// pre-allocated outside the scheduler lock.
    ///
    /// The caller MUST:
    /// 1. Allocate kernel_stack via Box::new before entering the lock
    /// 2. Pre-compute rsp = init_ring3_frame(kernel_stack_top, entry, user_stack_top)
    /// 3. Ensure scheduler Vecs have capacity (call ensure_slots())
    ///
    /// Inside the lock we only:
    /// - Assign PID/TID
    /// - Move eproc + thread into the Vecs
    /// - Update states
    /// NO heap allocations, NO Ob operations, NO string formatting.
    #[allow(clippy::too_many_arguments)]
    pub fn add_ring3_process_with_stack(
        &mut self,
        entry: u64,
        slot_idx: u8,
        cwd_drive: u8,
        cwd_path: &str,
        heap_base: u64,
        parent_pid: u32,
        name: &str,
        rsp: u64,
        kernel_stack_top: u64,
        kernel_stack: Box<AlignedKStack>,
        mut obj_id: Option<ObId>,
        mut ob_id: Option<ObId>,
        mut thread_obj_id: Option<ObId>,
        parent_token: crate::security::token::Token,
    ) -> Result<u32, &'static str> {
        if kernel_stack_top == 0 {
            kerror!(LogSubsys::Sched, "[BUGCHECK] TID=NEW kernel_stack_top=0");
            return Err("kernel_stack_top is 0");
        }

        let pid = self.next_pid;
        self.next_pid += 1;

        let tid = self.next_tid;
        self.next_tid += 1;

        // F-04: create Ob objects inside the lock with the *real* pid/tid.
        // Previously they were created outside with a guessed pid (peek), causing
        // duplicate names and native_id drift under concurrent spawns.
        // Now we create them here atomically, so no race and no leak on failure.
        if obj_id.is_none() {
            let name = alloc::format!("eproc/{}", pid);
            if let Ok(id) = object::ob_create_object(object::ObType::Process, &name, pid as u64, 0, None) {
                obj_id = Some(id);
            }
        }
        if ob_id.is_none() {
            let ob_name = alloc::format!("proc/{}", pid);
            if let Ok(id) = object::ob_create_object(object::ObType::Process, &ob_name, pid as u64, 0, None) {
                let ns_path = alloc::format!("\\Process\\{}", pid);
                let _ = crate::object::namespace::ob_insert_object(&ns_path, id);
                ob_id = Some(id);
            }
        }
        if thread_obj_id.is_none() {
            let tname = alloc::format!("kthread/{}", tid);
            if let Ok(id) = object::ob_create_object(object::ObType::Thread, &tname, tid as u64, 0, None) {
                thread_obj_id = Some(id);
            }
        }
        crate::serial_println!("[SPAWN] pid={} tid={} name={} obj_id={:?} ob_id={:?} thread_obj_id={:?}", pid, tid, name, obj_id, ob_id, thread_obj_id);

        let mut eproc = Eprocess {
            pid,
            name: crate::scheduler::types::KernelName::from_path(name),
            parent_pid,
            handle_table: crate::handle::HandleTable::with_defaults(),
            cwd_drive,
            cwd_path: cwd_path.to_string(),
            heap_base,
            heap_break: heap_base,
            user_slot: Some(slot_idx),
            mmap_regions: alloc::vec::Vec::new(),
            mmap_next: crate::arch::x64::paging::MMAP_BASE,
            thread_count: 1,
            exit_code: 0,
            obj_id,
            ob_id,
            address_space: crate::scheduler::address_space::AddressSpace::new(),
            token: parent_token,
            vt_num: 0,
            args: [0u8; 256],
        };

        let mut thread = Kthread::new_ring3_with_stack(tid, pid, entry, rsp, kernel_stack_top, kernel_stack);
        thread.obj_id = thread_obj_id;
        // Phase 14-A: the process's initial thread inherits the process name.
        thread.name = crate::scheduler::types::KernelName::from_path(name);
        thread.state = ThreadState::Suspended;

        // Find slots (no alloc — we pre-reserved via ensure_slots)
        let ep_slot = self.resolve_eprocess_slot();
        let th_slot = self.resolve_kthread_slot();
        self.eprocesses[ep_slot] = Some(eproc);
        self.kthreads[th_slot] = Some(Box::new(thread));

        kinfo!(LogSubsys::Sched, "PID {} -> \\Process\\{} OK", pid, pid);
        crate::trace_sched!(1, pid, 0);
        Ok(pid)
    }

    /// Publish a freshly-created process's initial thread as `Ready`
    /// (`Suspended -> Ready`). Idempotent: returns `true` only when a
    /// `Suspended` thread for `pid` was activated. This is the single
    /// activation path shared by the `ObWait` hand-off and the Service
    /// Manager; it does not enqueue twice (a thread already `Ready`/`Running`
    /// is left untouched).
    pub fn activate_suspended_process(&mut self, pid: u32) -> bool {
        let mut activated = false;
        for k in self.kthreads.iter_mut().flatten() {
            if k.pid == pid && k.state == ThreadState::Suspended {
                Self::make_thread_ready(k);
                activated = true;
            }
        }
        activated
    }

    /// #501: mark the Ring-3 bootstrap hand-off *target* thread `Running`.
    ///
    /// Resolves the thread by `target_tid`, never via `current_kthread_mut()`:
    /// since #482 the per-CPU `KPRCB.current_thread` still points at the
    /// bootstrap thread until the deferred publication that happens just
    /// before the Ring-3 `iretq`, so `current_kthread_mut()` resolves to boot
    /// and would leave the target `Suspended`. `schedule()` only ever commits
    /// `Ready` candidates, so a `Suspended` target is never scheduled again —
    /// NeoInit never reaches its first syscall and the shell never starts.
    ///
    /// Returns `true` when a thread with `target_tid` was found.
    pub fn mark_handoff_target_running(&mut self, target_tid: u32) -> bool {
        match self.find_kthread_mut(target_tid) {
            Some(k) => {
                crate::scheduler::diag::run_ev(crate::scheduler::diag::RUN_SITE_USERMODE, k);
                k.state = ThreadState::Running;
                true
            }
            None => false,
        }
    }

    /// Ensure the eprocesses and kthreads Vecs have at least one free slot,
    /// growing them now so no realloc happens inside the critical section.
    /// P0.2: use try_reserve to avoid panic on OOM (was push() panic).
    pub fn ensure_slots(&mut self) -> Result<(), &'static str> {
        if self.eprocesses.iter().position(|e| e.is_none()).is_none() {
            self.eprocesses.try_reserve(1).map_err(|_| "NoMem for eprocess slot")?;
            self.eprocesses.push(None);
        }
        if self.kthreads.iter().position(|t| t.is_none()).is_none() {
            self.kthreads.try_reserve(1).map_err(|_| "NoMem for kthread slot")?;
            self.kthreads.push(None);
        }
        Ok(())
    }

    /// Resolve a free eprocess slot (must exist — caller called ensure_slots).
    fn resolve_eprocess_slot(&mut self) -> usize {
        self.eprocesses.iter().position(|e| e.is_none())
            .expect("ensure_slots guarantees a free eprocess slot")
    }

    /// Resolve a free kthread slot (must exist — caller called ensure_slots).
    fn resolve_kthread_slot(&mut self) -> usize {
        self.kthreads.iter().position(|t| t.is_none())
            .expect("ensure_slots guarantees a free kthread slot")
    }

    /// Add an additional thread to an existing EPROCESS (Ring 3).
    pub fn add_thread_to_process(&mut self, pid: u32, entry: u64, user_stack: u64) -> Option<u32> {
        let tid = self.next_tid;
        self.next_tid += 1;

        let th_slot = self.alloc_kthread_slot()?;

        let mut thread = Kthread::new_ring3(tid, pid, entry, user_stack);

        let tname = alloc::format!("kthread/{}", tid);
        if let Ok(kid) = object::ob_create_object(ObType::Thread, &tname, tid as u64, 0, None) {
            thread.obj_id = Some(kid);
        }

        // Now borrow eprocess to update thread_count and retrieve user_slot
        let _slot_idx = {
            let eproc = self.find_eprocess_mut(pid)?;
            eproc.thread_count += 1;
            eproc.user_slot?
        };

        // Additional threads become runnable through the common transition.
        thread.state = ThreadState::Suspended;
        self.kthreads[th_slot] = Some(Box::new(thread));
        if let Some(k) = self.kthreads[th_slot].as_mut() {
            Self::make_thread_ready(k);
        }

        Some(tid)
    }

    pub fn spawn_kthread(&mut self, entry: u64, priority: u8) -> Option<u32> {
        self.spawn_kthread_named(entry, priority, "kthread")
    }

    /// Phase 14-A: kernel-thread spawn with an explicit bounded name.
    pub fn spawn_kthread_named(&mut self, entry: u64, priority: u8, name: &str) -> Option<u32> {
        let th_slot = self.alloc_kthread_slot()?;
        let tid = self.next_tid;
        self.next_tid += 1;

        // Heap-allocated kernel stack: avoids BSS linker aliasing that
        // corrupted the initial iretq frame when a static array was used.
        let stack = AlignedKStack::new_boxed();
        let kernel_stack_top = stack.0.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let rsp = init_ring0_frame(kernel_stack_top, entry);

        let kthread = Kthread {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0,
            r14: 0, r15: 0, rbp: 0,
            rsp,
            rip: entry, rflags: 0x202,
            tid,
            pid: self.next_pid,
            state: ThreadState::Suspended,
            cpu_ticks: 0,
            cpu_time: 0,
            cpu_time_base: Kthread::CPU_TIME_UNSET,
            waiting_for: None,
            priority,
            base_priority: priority,
            time_slice_remaining: TIME_SLICES[priority as usize],
            ticks_since_scheduled: 0,
            kernel_stack_top,
            kernel_stack_size: KERNEL_STACK_SIZE,
            kernel_stack: Some(stack),
            teb_base: 0, cpu: 0,
            obj_id: None,
            kernel_apc_queue: VecDeque::new(),
            user_apc_queue: VecDeque::new(),
            apc_pending: false,
            is_idle: false,
            is_kernel: true,
            yield_requested: false,
            name: crate::scheduler::types::KernelName::from_path(name),
        };

        let ep_slot = self.alloc_eprocess_slot()?;
        self.eprocesses[ep_slot] = Some(Eprocess::new_kernel(self.next_pid));
        self.next_pid += 1;
        self.kthreads[th_slot] = Some(Box::new(kthread));
        // Fase 3 P1/P5: capturar frame inicial 18 slots y canary
        let (kptr, base, top, init_rsp, ent) = {
            let k = self.kthreads[th_slot].as_ref().unwrap();
            // #348: derive the canary base from the thread's actual stack size.
            let b = k.kernel_stack_top.wrapping_sub(k.kernel_stack_size as u64);
            (&**k as *const Kthread as u64, b, k.kernel_stack_top, k.rsp, k.rip)
        };
        if let Some(k) = self.kthreads[th_slot].as_mut() {
            Self::make_thread_ready(k);
        }
        crate::arch::x64::idt::netd_record_create(kptr, base, top, init_rsp, ent);

        kdebug!(LogSubsys::Sched, "[SCHED] CREATE TID={} PID={} priority={} state=Ready",
            tid, self.next_pid - 1, priority);

        // Netd is found by the global priority scan, not the run queue.
        // This prevents netd from starving the boot thread (TID 0).
        Some(tid)
    }

    // ── Kill / Recycle ──

    /// Kill an entire EPROCESS and all its threads.
    pub fn kill_pid(&mut self, pid: u32) -> bool {
        if pid == 0 { return false; }
        // INV-10 (source-of-truth.md): NeoInit (PID 1) must never be killed.
        // Refuse silently; callers treat `false` as "not killed".
        if pid == INIT_PID { return false; }

        // Unregister EPROCESS from Ob (OB-046)
        for ep in self.eprocesses.iter().flatten() {
            if ep.pid == pid {
                if let Some(kid) = ep.obj_id {
                    let _ = object::ob_destroy_object(kid);
                }
                if let Some(ob_id) = ep.ob_id {
                    let _ = object::ob_close_object(ob_id);
                    let ns_path = alloc::format!("\\Process\\{}", pid);
                    let _ = crate::object::namespace::ob_remove_object(&ns_path);
                }
                break;
            }
        }

        // Collect thread TIDs
        let tids = self.thread_tids_for_pid(pid);
        if tids.is_empty() { return false; }

        // Find eprocess slot
        let ep_idx = self.eprocesses.iter().position(|e| {
            e.as_ref().is_some_and(|ep| ep.pid == pid)
        });

        // F-02-B: if any thread of this pid is still executing on a CPU, we must
        // not drop its Kthread/kernel stack here. Free the eprocess resources
        // (idempotent) but keep the slot so `recycle_terminated` can drop the
        // threads/stacks once no CPU is executing on them.
        let running = crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(pid);

        // Free resources from eprocess
        if let Some(ep_idx) = ep_idx {
            if running {
                if let Some(ep) = self.eprocesses[ep_idx].as_mut() {
                    free_eprocess_resources(ep);
                }
            } else if let Some(mut eproc) = self.eprocesses[ep_idx].take() {
                free_eprocess_resources(&mut eproc);
            }
        }

        // Free all kernel stacks and unregister thread KOBJs
        for tid in &tids {
            if let Some(th) = self.find_kthread_mut(*tid) {
                if let Some(kid) = th.obj_id {
                    let _ = object::ob_destroy_object(kid);
                }
                if running {
                    // Do not drop the Kthread (and its kernel stack) while it may
                    // still be executing; leave it Terminated for the reaper.
                    Self::remove_from_run_queue(th);
                    th.state = ThreadState::Terminated;
                }
                // Kernel stack freed on drop (non-running path below).
            }
            if running {
                continue;
            }
            let th_idx = self.kthreads.iter().position(|t| {
                t.as_ref().is_some_and(|k| k.tid == *tid)
            });
            if let Some(th_idx) = th_idx {
                // #476 H1 experiment: guard the stack before dropping.
                if let Some(th) = self.kthreads[th_idx].as_mut() {
                    Self::guard_kstack_reclaim(th);
                }
                self.kthreads[th_idx] = None;
            }
        }

        if running {
            // Scheduler lock is held here: use the bounded, scheduler-aware
            // reclaim so the hard cap is enforced synchronously.
            defer_reap_with_scheduler(self, pid);
        }

        // #358: forced termination must converge through the same deferred
        // process-exit path as a voluntary exit (#374), so the Service Manager
        // observes a killed service and finalizes it. `terminate_current` does
        // this for voluntary exits/exceptions; `kill_pid` is the forced path and
        // never runs `terminate_current`, so it notifies here. The notification
        // is bounded, allocation-free and lock-safe (called with the scheduler
        // lock held), exactly like the `terminate_current` call site. `-1`
        // denotes an abnormal/forced termination.
        crate::services::notify_process_exit(pid, -1);

        crate::trace_sched!(2, pid, 0); // KILL_PROCESS
        true
    }

    /// Never free a kernel stack that another CPU is still abandoning (KPRCB
    /// repointed, `mov rsp` not yet executed). Instead of leaking it
    /// (`core::mem::forget`, the #476 H1 workaround), the stack is moved to the
    /// bounded quarantine and freed later by `drain_kstack_quarantine` once no
    /// CPU is mid-switch on it (NEODOS-03 / #633). Returns true on conflict.
    fn guard_kstack_reclaim(th: &mut Kthread) -> bool {
        let top = th.kernel_stack_top;
        if let Some((owner, _otid, _opid, orsp, nks)) =
            crate::scheduler::diag::kstack::reclaim_conflict(top)
        {
            let reclaimer = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
            crate::scheduler::diag::kstack::record_conflict(
                reclaimer, owner, th.tid as u64, th.pid as u64,
                top, th.kernel_stack_size as u64, orsp, nks);
            if let Some(b) = th.take_kernel_stack() {
                quarantine_push(top, b);
            }
            true
        } else {
            false
        }
    }

    /// Recycle a terminated EPROCESS (only when last thread exits).
    /// F-02-B: releases EPROCESS resources itself (idempotent), so it is safe
    /// even when the caller did not free them first.
    pub fn recycle_terminated(&mut self, pid: u32) -> bool {
        if pid == 0 { return false; }

        // Unregister from Ob (OB-046)
        for ep in self.eprocesses.iter().flatten() {
            if ep.pid == pid {
                if let Some(kid) = ep.obj_id {
                    let _ = object::ob_destroy_object(kid);
                }
                if let Some(ob_id) = ep.ob_id {
                    let _ = object::ob_close_object(ob_id);
                    let ns_path = alloc::format!("\\Process\\{}", pid);
                    let _ = crate::object::namespace::ob_remove_object(&ns_path);
                }
                break;
            }
        }

        // Remove eprocess slot
        let ep_idx = self.eprocesses.iter().position(|e| {
            e.as_ref().is_some_and(|ep| ep.pid == pid)
        });
        if let Some(ep_idx) = ep_idx {
            // F-02-B: release external resources exactly once (idempotent; may
            // already be done by terminate_current).
            if let Some(ep) = self.eprocesses[ep_idx].as_mut() {
                free_eprocess_resources(ep);
            }
            // Remove all remaining threads (should be 0 at this point)
            let tids: Vec<u32> = self.thread_tids_for_pid(pid);
            for tid in &tids {
                let th_idx = self.kthreads.iter().position(|t| {
                    t.as_ref().is_some_and(|k| k.tid == *tid)
                });
                if let Some(th_idx) = th_idx {
                    // Unregister thread Ob
                    if let Some(th) = self.kthreads[th_idx].as_mut() {
                        if let Some(kid) = th.obj_id {
                            let _ = object::ob_destroy_object(kid);
                        }
                        // #476 H1 experiment: do not free a stack another CPU
                        // is still abandoning (leaked instead).
                        Self::guard_kstack_reclaim(th);
                    }
                    self.kthreads[th_idx] = None;
                }
            }
            // Drop eprocess (frees handle_table Vec, mmap_regions Vec, cwd_path String)
            self.eprocesses[ep_idx] = None;
            crate::trace_sched!(3, pid, 0); // RECYCLE_SLOT
            true
        } else {
            false
        }
    }

    /// Centralized termination for current thread/process (used by sys_exit and exception path).
    /// Mirrors handler_exit logic: decrement thread_count, free resources if last thread, wake waiters, defer reap.
    /// Must be called with scheduler lock held and interrupts disabled. Caller must set need_resched after.
    /// F-01: uses per-CPU identity (KPRCB) when available and belongs to this Scheduler, not global current_tid.
    pub fn terminate_current(&mut self, exit_code: i64) -> Option<u32> {
        // P0.2: also take USER_MEMORY_LOCK (order SCHEDULER -> USER_MEMORY_LOCK)
        // to make free/unmap atomic against copy_user_string validation+read.
        let _mem_guard = crate::syscall::util::USER_MEMORY_LOCK.lock();
        // F-01: per-CPU current thread (SMP) — fallback to global for tests/early boot/local schedulers
        let (tid, pid) = if self.kprcb_thread_in_self() {
            if let Some(t) = crate::arch::x64::cpu_local::try_per_cpu_tid() {
                let p = crate::arch::x64::cpu_local::try_per_cpu_pid().unwrap_or_else(|| self.current_pid());
                (t, p)
            } else {
                (self.current_tid, self.current_pid())
            }
        } else {
            (self.current_tid, self.current_pid())
        };
        if tid == 0 || pid == 0 { return None; }
        // Remove from runqueue using the correct Kthread (per-CPU tid)
        if let Some(k) = self.find_kthread_mut(tid) {
            Self::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        } else if let Some(k) = self.current_kthread_mut() {
            // Fallback (should not happen)
            Self::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        let mut do_reap: Option<u32> = None;
        if let Some(ep) = self.find_eprocess_mut(pid) {
            ep.thread_count = ep.thread_count.saturating_sub(1);
            ep.exit_code = exit_code;
            if ep.thread_count == 0 {
                // Free user slot, heap, mmap, handles — same as handler_exit
                if let Some(slot) = ep.user_slot.take() {
                    crate::arch::x64::paging::free_user_slot(slot);
                }
                if ep.heap_base != 0 {
                    crate::arch::x64::paging::heap_free_range(ep.heap_base, ep.heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE);
                    let heap_idx = ((ep.heap_base - crate::arch::x64::paging::PROCESS_HEAP_BASE) / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as u8;
                    crate::arch::x64::paging::free_heap_slot(heap_idx);
                    ep.heap_base = 0;
                    ep.heap_break = 0;
                }
                for r in ep.mmap_regions.iter() {
                    crate::arch::x64::paging::mmap_free_range(r.base, r.base + r.len);
                }
                ep.mmap_regions.clear();
                ep.mmap_next = crate::arch::x64::paging::MMAP_BASE;
                for i in 0..ep.handle_table.len() {
                    let h = ep.handle_table[i];
                    if h.is_pipe_read() {
                        crate::object::pipe::PIPE_MANAGER.dec_read_ref(h.native_id().unwrap_or(0) as u8);
                    } else if h.is_pipe_write() {
                        crate::object::pipe::PIPE_MANAGER.dec_write_ref(h.native_id().unwrap_or(0) as u8);
                    } else if h.has_ob_object() {
                        let _ = crate::object::ob_close_object(h.object_id);
                    }
                    ep.handle_table.set(i as u8, crate::handle::HandleEntry::closed());
                }
                // Wake ChildExit waiters
                let ce_magic = crate::kwait::WaitReason::ChildExit { pid }.encode_magic();
                for k in self.kthreads.iter_mut().flatten() {
                    if k.waiting_for == Some(ce_magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                        k.waiting_for = None;
                        Self::make_thread_ready(k);
                    }
                }
                // #374: notify the Service Manager that this process exited.
                // Deferred and lock-free here (we hold the scheduler lock); the
                // restart policy is applied later from syscall context.
                crate::services::notify_process_exit(pid, exit_code);
                do_reap = Some(pid);
            }
        }
        // Wake ThreadJoin waiters for this tid
        let tj_magic = crate::kwait::WaitReason::ThreadJoin { tid }.encode_magic();
        for k in self.kthreads.iter_mut().flatten() {
            if k.waiting_for == Some(tj_magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                k.waiting_for = None;
                Self::make_thread_ready(k);
            }
        }
        // Check waitpid global
        if let Some(ep) = self.find_eprocess(pid) {
            if ep.thread_count == 0 && pid == crate::usermode::current_wait_pid() {
                crate::usermode::request_exit_to_kernel();
            }
        }
        if let Some(pid) = do_reap {
            defer_reap_with_scheduler(self, pid);
        }
        Some(pid)
    }

    /// Remove a single terminated thread.  Returns true if the thread was found.
    /// Does NOT free EPROCESS resources — only frees the kernel stack.
    pub fn recycle_thread(&mut self, tid: u32) -> bool {
        // Unregister thread Ob
        if let Some(th) = self.find_kthread(tid) {
            if let Some(kid) = th.obj_id {
                let _ = object::ob_destroy_object(kid);
            }
        }
        let th_idx = self.kthreads.iter().position(|t| {
            t.as_ref().is_some_and(|k| k.tid == tid)
        });
        if let Some(th_idx) = th_idx {
            // #476 H1 experiment: guard the stack before dropping.
            if let Some(th) = self.kthreads[th_idx].as_mut() {
                Self::guard_kstack_reclaim(th);
            }
            self.kthreads[th_idx] = None;
            crate::trace_sched!(3, tid as u64, 1);
            true
        } else {
            false
        }
    }

    // ── Wake helpers ──

}
