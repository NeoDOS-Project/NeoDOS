//! A1.5 EPROCESS/KTHREAD split — Thread-based scheduler
//!
//! - EPROCESS: shared resources (address space, handle table, heap, mmap, CWD)
//! - KTHREAD: per-thread CPU context, priority, time slice, kernel stack
//! - Schedule operates on threads, lazy CR3 swap across EPROCESS boundaries

pub mod address_space;
pub mod types;
pub mod stack;
pub mod process;
pub mod thread;
pub mod queue;
pub mod smp;
pub mod aging;
pub mod lifecycle;
pub mod wake;
pub mod schedule;

pub use types::{Kthread, Eprocess, ThreadState, MmapRegion, KERNEL_STACK_SIZE, IDLE_TIME_SLICE, PRIORITY_HIGH, PRIORITY_ABOVE_NORMAL, PRIORITY_NORMAL, PRIORITY_IDLE, PRIORITY_COUNT, TIME_SLICES, BOOT_TID, IDLE_TID, AGING_INTERVAL_TICKS, MAX_STARVATION_TICKS, TEB_SIZE, STACK_CANARY};
pub use stack::{AlignedKStack, check_kernel_stack_canary, spawn_net_kthread, init_ring3_frame};
pub use schedule::{sched_forensic_enable, sched_forensic_verbose_enable, sched_forensic_verbose};

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::log::LogSubsys;

pub struct Scheduler {
    pub eprocesses: Vec<Option<Eprocess>>,
    pub kthreads: Vec<Option<Box<Kthread>>>,
    pub current_tid: u32,
    pub next_pid: u32,
    pub next_tid: u32,
    timer_ticks: u64,
    /// Schedule backstop: every 20 schedule() calls the idle thread
    /// is force-picked regardless of priority-scan outcome.  This ensures
    /// TID 0 always gets CPU even when higher-priority threads dominate
    /// the Ready queue.
    schedule_count: u64,
}

#[allow(unused_macros)]
macro_rules! with_current {
    ($sched:expr, $eproc:ident, $body:block) => {{
        let tid = $sched.current_tid_for_this_cpu();
        let pid = $sched.find_kthread(tid).map(|t| t.pid);
        if let Some(pid) = pid {
            if let Some($eproc) = $sched.find_eprocess_mut(pid) {
                $body
            }
        }
    }};
}

#[allow(clippy::new_without_default)]
impl Scheduler {
    pub fn find_eprocess_mut(&mut self, pid: u32) -> Option<&mut Eprocess> {
        self.eprocesses.iter_mut()
            .find(|e| e.as_ref().is_some_and(|ep| ep.pid == pid))
            .and_then(|e| e.as_mut())
    }

    pub fn find_eprocess(&self, pid: u32) -> Option<&Eprocess> {
        self.eprocesses.iter()
            .find(|e| e.as_ref().is_some_and(|ep| ep.pid == pid))
            .and_then(|e| e.as_ref())
    }

    pub fn find_kthread_mut(&mut self, tid: u32) -> Option<&mut Kthread> {
        self.kthreads.iter_mut()
            .find(|t| t.as_ref().is_some_and(|k| k.tid == tid))
            .and_then(|t| t.as_mut().map(|k| &mut **k))
    }

    pub fn find_kthread(&self, tid: u32) -> Option<&Kthread> {
        self.kthreads.iter()
            .find(|t| t.as_ref().is_some_and(|k| k.tid == tid))
            .and_then(|t| t.as_ref().map(|k| &**k))
    }

    /// Collect all TIDs belonging to an EPROCESS.
    pub fn thread_tids_for_pid(&self, pid: u32) -> Vec<u32> {
        self.kthreads.iter()
            .filter_map(|t| {
                if let Some(k) = t {
                    if k.pid == pid { Some(k.tid) } else { None }
                } else { None }
            })
            .collect()
    }

    /// Check if KPRCB current_thread pointer belongs to this Scheduler instance.
    /// Used to distinguish global SCHEDULER vs local test schedulers (F-01).
    fn kprcb_thread_in_self(&self) -> bool {
        if crate::hal::safe::GsBase::read() == 0 { return false; }
        let ptr = unsafe { crate::arch::x64::cpu_local::this_cpu_current_thread() };
        if ptr.is_null() { return false; }
        self.kthreads.iter().any(|t| {
            if let Some(k) = t {
                &**k as *const crate::scheduler::Kthread as *const u8 == ptr as *const u8
            } else { false }
        })
    }

    /// F-01: per-CPU view of current PID. If KPRCB is initialized (SMP) AND
    /// the KPRCB thread belongs to this Scheduler, returns per-CPU PID.
    /// Falls back to global current_tid for tests/local schedulers.
    pub fn current_pid(&self) -> u32 {
        if self.kprcb_thread_in_self() {
            if let Some(pid) = crate::arch::x64::cpu_local::try_per_cpu_pid() {
                return pid;
            }
        }
        self.find_kthread(self.current_tid).map(|t| t.pid).unwrap_or(0)
    }

    /// F-01: per-CPU helper to get current TID for THIS CPU if this is the
    /// global scheduler (KPRCB thread in self). Otherwise fallback.
    pub fn current_tid_for_this_cpu(&self) -> u32 {
        if self.kprcb_thread_in_self() {
            if let Some(tid) = crate::arch::x64::cpu_local::try_per_cpu_tid() {
                return tid;
            }
        }
        self.current_tid
    }

    pub fn current_eprocess_mut(&mut self) -> Option<&mut Eprocess> {
        // Use per-CPU only for global scheduler; local test schedulers use self.current_tid
        let tid = if self.kprcb_thread_in_self() {
            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
        } else {
            self.current_tid
        };
        let pid = self.find_kthread(tid).map(|t| t.pid)?;
        self.find_eprocess_mut(pid)
    }

    pub fn current_kthread_mut(&mut self) -> Option<&mut Kthread> {
        let tid = if self.kprcb_thread_in_self() {
            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
        } else {
            self.current_tid
        };
        self.find_kthread_mut(tid)
    }

    pub fn current_eprocess(&self) -> Option<&Eprocess> {
        let tid = if self.kprcb_thread_in_self() {
            crate::arch::x64::cpu_local::try_per_cpu_tid().unwrap_or(self.current_tid)
        } else {
            self.current_tid
        };
        let pid = self.find_kthread(tid).map(|t| t.pid)?;
        self.find_eprocess(pid)
    }

    // ── Construction ──
    pub fn new() -> Self {
        unsafe {
            if crate::arch::x64::cpu_local::KPRCB_PAGES[0] != 0 {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            }
        }
        let mut eprocesses = Vec::with_capacity(32);
        let mut kthreads = Vec::with_capacity(64);

        // Boot EPROCESS (PID 0) + boot KTHREAD (TID 0)
        // The boot thread is the initial execution context (rust_start).
        // It starts Running because it IS already executing on the BSP
        // stack — it does not need a saved iretq frame for first entry.
        // The scheduler will save its context when the first timer IRQ
        // preempts it, and restore it when switching back.
        //
        // kernel_stack_top is set to a non-zero sentinel so that the
        // timer handler never loads RSP0=0 when switching back to boot.
        // If RSP0=0, the very next Ring-3→Ring-0 interrupt triple-faults
        // because the CPU cannot push the exception frame at address 0.
        // TID 0 never actually uses this address as a kernel stack (it
        // runs on the BSP stack), so the exact value is irrelevant as
        // long as it is a valid, mapped address != 0.
        let boot_ks_top = crate::hal::bootstrap_stack_top();
        let boot_eproc = Eprocess::new_kernel(0);
        let boot_thread = Kthread {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0,
            r14: 0, r15: 0, rbp: 0,
            rsp: boot_ks_top,
            rip: 0,
            rflags: 0x202,
            tid: BOOT_TID,
            pid: 0,
            state: ThreadState::Running,
            cpu_ticks: 0,
            waiting_for: None,
            priority: PRIORITY_NORMAL,
            time_slice_remaining: TIME_SLICES[PRIORITY_NORMAL as usize],
            ticks_since_scheduled: 0,
            kernel_stack_top: boot_ks_top,
            kernel_stack: None,
            teb_base: 0,
            cpu: 0,
            obj_id: None,
            kernel_apc_queue: VecDeque::new(),
            user_apc_queue: VecDeque::new(),
            apc_pending: false,
            is_idle: false,
        };
        eprocesses.push(Some(boot_eproc));
        kthreads.push(Some(Box::new(boot_thread)));

        // Idle KTHREAD (TID 1) — runs the halt loop when nothing else is Ready.
        // Shares the PID 0 EPROCESS (no separate address space needed for idle).
        let idle_stack_top = unsafe { crate::scheduler::stack::IDLE_STACK.as_ptr().add(crate::scheduler::stack::IDLE_STACK_SIZE) as u64 } & !0xF;
        let idle_thread = Kthread::new_idle(
            IDLE_TID, 0,
            crate::scheduler::stack::idle_task as *const () as u64,
            idle_stack_top,
        );
        kthreads.push(Some(Box::new(idle_thread)));

        Scheduler {
            eprocesses,
            kthreads,
            current_tid: BOOT_TID,
            next_pid: 1,
            next_tid: 2,
            timer_ticks: 0,
            schedule_count: 0,
        }
    }
    pub fn has_non_idle_processes(&self) -> bool {
        self.eprocesses.iter().skip(1).any(|e| e.is_some())
    }

    pub fn has_non_idle_threads(&self) -> bool {
        self.kthreads.iter().any(|t| {
            t.as_ref().is_some_and(|k| {
                !k.is_idle &&
                k.state != ThreadState::Terminated &&
                k.state != ThreadState::Suspended
            })
        })
    }

}

lazy_static! {
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

pub fn current_scheduler() -> &'static Mutex<Scheduler> {
    &SCHEDULER
}

// ── SMP test isolation ──
// When true, AP work-stealing and timer preemption are paused
// so that k18/k19 tests can manipulate global runqueues without
// concurrent AP interference. Set by the test harness.
pub(crate) static SCHED_TEST_MODE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Phase 13: APs enter the scheduler only after the BSP has finished the boot
/// test suite (set by `main.rs`), keeping the suite deterministic. This is a
/// bring-up gate, not a scheduling workaround: no thread is forced, no runqueue
/// is drained, no extra yield is injected.
static AP_SCHED_ACTIVE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Whether APs are allowed to run the scheduler.
pub fn ap_sched_active() -> bool {
    AP_SCHED_ACTIVE.load(core::sync::atomic::Ordering::Acquire)
}

/// Enable/disable AP scheduling. Must only be toggled by the BSP.
pub fn set_ap_sched_active(v: bool) {
    AP_SCHED_ACTIVE.store(v, core::sync::atomic::Ordering::Release);
}

/// Establish the BSP's per-CPU identity (KPRCB.current_thread = boot thread)
/// once GS is programmed. Without this, CPU0's KPRCB thread stays null and
/// `current_tid_for_this_cpu()` falls back to the global `current_tid`, which
/// APs also write — breaking Rule 6.1.5. No-op before GS is set.
pub fn sync_bsp_identity() {
    if crate::hal::safe::GsBase::read() == 0 {
        return;
    }
    let s = current_scheduler();
    let lock = s.lock();
    if let Some(k) = lock.find_kthread(BOOT_TID) {
        let ptr = k as *const Kthread as *mut Kthread;
        unsafe {
            crate::arch::x64::cpu_local::sync_per_cpu_current(ptr, k.pid);
        }
    }
}

/// Phase 7 diagnostic: dump every KTHREAD (tid/pid/state/cpu/rsp) plus each
/// per-CPU runqueue's TIDs. Read-only, used by Ctrl+Alt+V / syscall 99 to
/// localize a consumer that never wakes. No scheduler behavior change.
pub fn sched_dump() {
    let s = SCHEDULER.lock();
    crate::serial_println!("[SCHED_DUMP] current_tid={} next_tid={} schedule_count={}",
        s.current_tid, s.next_tid, s.schedule_count);
    for k_opt in s.kthreads.iter() {
        if let Some(k) = k_opt {
            let st = match k.state {
                ThreadState::Ready => "READY",
                ThreadState::Running => "RUNNING",
                ThreadState::Blocked { .. } => "BLOCKED",
                ThreadState::Suspended => "SUSPENDED",
                ThreadState::Terminated => "TERMINATED",
            };
            crate::serial_println!("[SCHED_DUMP] tid={} pid={} state={} cpu={} prio={} rsp=0x{:x} wait={:?}",
                k.tid, k.pid, st, k.cpu, k.priority, k.rsp, k.waiting_for);
        }
    }
    drop(s);
    // Best-effort lock-free read: this diagnostic is called from IRQ context
    // (Ctrl+Alt+V) and MUST NOT spin on RUNQUEUE_LOCKS held by the interrupted
    // context (self-deadlock). try_lock and skip if busy.
    for cpu in 0..crate::arch::x64::cpu_local::MAX_CPUS {
        let kprcb = unsafe { crate::arch::x64::cpu_local::KPRCB_PAGES[cpu] };
        if kprcb == 0 { continue; }
        let guard = crate::arch::x64::cpu_local::RUNQUEUE_LOCKS[cpu].try_lock();
        if guard.is_none() {
            crate::serial_println!("[SCHED_DUMP] cpu={} runqueue=BUSY (lock held by interrupted ctx)", cpu);
            continue;
        }
        let rq = unsafe {
            &*((kprcb + crate::arch::x64::cpu_local::OFFSET_RUN_QUEUE as u64)
                as *const crate::arch::x64::cpu_local::CpuRunQueue)
        };
        let cap = rq.entries.len();
        let mut v = alloc::vec::Vec::new();
        let mut idx = rq.head_idx as usize;
        for _ in 0..rq.count {
            v.push(rq.entries[idx % cap]);
            idx += 1;
        }
        crate::serial_println!("[SCHED_DUMP] cpu={} runqueue_tids={:?} count={}", cpu, v, rq.count);
        drop(guard);
    }
}



/// Phase 13 evidence: dump each online CPU's KPRCB current identity and its
/// run queue length. One-shot diagnostic (not a hot path).
pub fn dump_per_cpu_current() {
    let count = crate::arch::x64::cpu_local::cpu_count();
    for cpu in 0..count as usize {
        let kprcb = unsafe { crate::arch::x64::cpu_local::KPRCB_PAGES[cpu] };
        if kprcb == 0 { continue; }
        let cur_ptr = unsafe {
            core::ptr::read_volatile(
                (kprcb + crate::arch::x64::cpu_local::OFFSET_CURRENT_THREAD as u64) as *const u64
            )
        };
        let cur_pid = unsafe {
            core::ptr::read_volatile(
                (kprcb + crate::arch::x64::cpu_local::OFFSET_CURRENT_PID as u64) as *const u32
            )
        };
        let idle = unsafe {
            core::ptr::read_volatile(
                (kprcb + crate::arch::x64::cpu_local::OFFSET_IDLE as u64) as *const u8
            )
        };
        let tid = if cur_ptr == 0 { 0 } else {
            unsafe { (*(cur_ptr as *const Kthread)).tid }
        };
        let qlen = crate::arch::x64::cpu_local::with_runqueue(cpu, |rq| rq.len());
        crate::serial_println!(
            "[AP_EVIDENCE] cpu={} kprcb=0x{:x} current_tid={} current_pid={} idle={} qlen={}",
            cpu, kprcb, tid, cur_pid, idle, qlen);
    }
}

/// Recycle a terminated EPROCESS. External resources are released here
/// (idempotently) if the caller has not already done so.
pub fn cleanup_terminated_process(pid: u32) {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    {
        let mut sched = current_scheduler().lock();
        if crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(pid) {
            // F-02-B: the pid still has a thread executing on some CPU (SMP).
            // Do not drop its kernel stack now; defer reclaim to the reaper.
            crate::scheduler::lifecycle::defer_reap(pid);
        } else {
            sched.recycle_terminated(pid);
        }
    }
    unsafe { crate::hal::irql::lower_irql(old_irql) };
}

/// Get current thread's EPROCESS CWD.
pub fn get_current_cwd() -> (u8, String) {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let lock = SCHEDULER.lock();
    let result = if let Some(ep) = lock.current_eprocess() {
        (ep.cwd_drive, ep.cwd_path.clone())
    } else {
        (2, String::from("\\"))
    };
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn set_current_cwd(drive: u8, path: &str) {
    let current_pid = current_pid();
    let _ = set_cwd_for_pid(current_pid, drive, path);
}

pub fn set_cwd_for_pid(pid: u32, drive: u8, path: &str) -> bool {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let mut lock = SCHEDULER.lock();
    let result = if let Some(ep) = lock.find_eprocess_mut(pid) {
        ep.cwd_drive = drive;
        ep.cwd_path = path.to_string();
        true
    } else {
        false
    };
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn current_process_heap_range() -> (u64, u64) {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let lock = SCHEDULER.lock();
    let result = if let Some(ep) = lock.current_eprocess() {
        (ep.heap_base, ep.heap_break)
    } else {
        (0, 0)
    };
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn current_vt_num() -> u8 {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let lock = SCHEDULER.lock();
    let result = if let Some(ep) = lock.current_eprocess() { ep.vt_num } else { 0 };
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn set_current_heap_break(new_break: u64) {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let mut lock = SCHEDULER.lock();
    if let Some(ep) = lock.current_eprocess_mut() {
        ep.heap_break = new_break;
    }
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
}

pub fn current_process_mmap_regions() -> Vec<MmapRegion> {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let lock = SCHEDULER.lock();
    let result = if let Some(ep) = lock.current_eprocess() {
        ep.mmap_regions.clone()
    } else {
        Vec::new()
    };
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn add_current_mmap_region(region: MmapRegion) -> Option<u64> {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let mut lock = SCHEDULER.lock();
    let _mem_guard = crate::syscall::util::USER_MEMORY_LOCK.lock();
    let result = if let Some(ep) = lock.current_eprocess_mut() {
        // Use try_reserve to avoid panic on OOM (P0.2)
        if ep.mmap_regions.try_reserve(1).is_err() {
            None
        } else {
            ep.mmap_regions.push(region);
            ep.mmap_next = region.base + region.len;
            Some(region.base)
        }
    } else {
        None
    };
    drop(_mem_guard);
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn remove_current_mmap_region(base: u64) -> Option<MmapRegion> {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let mut lock = SCHEDULER.lock();
    let _mem_guard = crate::syscall::util::USER_MEMORY_LOCK.lock();
    let result = if let Some(ep) = lock.current_eprocess_mut() {
        let idx = ep.mmap_regions.iter().position(|r| r.base == base);
        idx.map(|i| ep.mmap_regions.remove(i))
    } else {
        None
    };
    drop(_mem_guard);
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn free_current_mmap_pages(base: u64, len: u64) {
    crate::arch::x64::paging::mmap_free_range(base, base + len);
}

/// Find a thread's TEB base address.
pub fn current_teb_base() -> u64 {
    // F-01: try per-CPU first
    if let Some(tid) = crate::arch::x64::cpu_local::try_per_cpu_tid() {
        let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
        let lock = SCHEDULER.lock();
        let result = lock.find_kthread(tid).map(|k| k.teb_base).unwrap_or(0);
        drop(lock);
        unsafe { crate::hal::irql::lower_irql(old_irql) };
        if result != 0 { return result; }
    }
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let lock = SCHEDULER.lock();
    let result = lock.find_kthread(lock.current_tid).map(|k| k.teb_base).unwrap_or(0);
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

// ── Convenience: current PID/TID (F-01: per-CPU via KPRCB, fallback to global) ──

pub fn current_pid() -> u32 {
    if let Some(pid) = crate::arch::x64::cpu_local::try_per_cpu_pid() {
        return pid;
    }
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let lock = SCHEDULER.lock();
    let result = lock.current_pid();
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn current_tid() -> u32 {
    if let Some(tid) = crate::arch::x64::cpu_local::try_per_cpu_tid() {
        return tid;
    }
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let result = SCHEDULER.lock().current_tid;
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

/// Yield execution of current thread cooperatively back to the scheduler.
pub fn yield_current_thread() {
    crate::hal::without_interrupts(|| {
        let s = current_scheduler();
        let mut lock = s.lock();
        let tid = lock.current_tid_for_this_cpu();
        if tid > 0 {
            if let Some(k) = lock.current_kthread_mut() {
                let before = k.state.to_u8();
                Scheduler::make_thread_ready(k);
                crate::trace_sched_state!(tid, before, k.state.to_u8(), 1u8);
            }
        }
        // Signal reschedule so the yield is not a no-op.
        // Without this, kernel threads (notably netd) set state=Ready but
        // continue running until the next timer tick catches them in
        // Running state.  On a busy system this can starve other threads.
        crate::syscall::set_need_resched();
    });
}

/// For thread_join: block current thread until target TID terminates (via KWait, OB-031).
pub fn block_current_for_thread(tid: u32) {
    crate::kwait::kwait_block(crate::kwait::WaitReason::ThreadJoin { tid });
}

/// Wake a thread blocked on join (via KWait, OB-031).
pub fn wake_thread_joiner(tid: u32) {
    crate::kwait::kwait_wake(&crate::kwait::WaitReason::ThreadJoin { tid });
}



pub mod tests;
pub use tests::register_tests;
