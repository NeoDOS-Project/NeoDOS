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

/// F-06: bounded zombie queue to prevent unbounded growth under storm.
/// Each zombie is an EPROCESS/KTHREAD that has been terminated but whose
/// slots (including kernel stacks) cannot be freed until no CPU is running
/// on that pid. Under rapid spawn/exit, defer_reap() pushes faster than
/// schedule() could previously pop (1 per schedule), leading to Vec growth
/// without bound and heap/user slot exhaustion.
const MAX_ZOMBIES: usize = 64;

lazy_static! {
    static ref ZOMBIE_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::with_capacity(MAX_ZOMBIES));
}

/// Defer EPROCESS slot recycling until after context switch.
/// The pid's Kthread stacks remain valid while current thread still executes.
/// F-DEV-02: fixed bounded queue without loss — previous drain discarded PIDs
/// without recycle_terminated, leaking eprocesses/kthreads/Ob objects.
/// Correctness requires queue with no silent loss: we never drain without
/// recycling. If hard cap exceeded we keep the queue oversized and warn;
/// reap_pending_zombies will drain all eligible on next schedule (F-06 loop).
/// Sync reclaim or spawn backpressure (see defer_reap_with_scheduler) bounds it.
pub fn defer_reap(pid: u32) {
    if pid == 0 { return; }
    let mut zombies = ZOMBIE_PIDS.lock();
    if zombies.len() >= MAX_ZOMBIES {
        kwarn!(crate::log::LogSubsys::Sched, "zombie backpressure: queue len {} >= MAX {}", zombies.len(), MAX_ZOMBIES);
    }
    zombies.push(pid);
    if zombies.len() > MAX_ZOMBIES * 4 {
        kwarn!(crate::log::LogSubsys::Sched, "zombie storm: queue len {} exceeds hard cap {} (awaiting reap, no loss)", zombies.len(), MAX_ZOMBIES * 4);
        // F-DEV-02: do NOT drain without recycle — that leaked PID slots forever.
        // Queue stays oversized until reap_pending_zombies drains eligible entries.
    }
}

/// F-DEV-02: sync reclaim variant called with scheduler lock held (terminate_current).
/// Enqueues pid via defer_reap, then synchronously reclaims oldest eligible zombies
/// while holding &mut Scheduler to keep the queue bounded without loss.
pub fn defer_reap_with_scheduler(sched: &mut Scheduler, pid: u32) {
    defer_reap(pid);
    // Bound the queue synchronously by recycling oldest not-running zombies.
    // This is called with SCHEDULER already locked, so recycle_terminated is safe.
    loop {
        let over = { ZOMBIE_PIDS.lock().len() > MAX_ZOMBIES * 2 };
        if !over { break; }
        let pos_opt = {
            let zombies = ZOMBIE_PIDS.lock();
            // Find first zombie that is not running on any CPU and not the newly queued pid
            // (new pid is still running on current CPU until context switch, so skip it)
            zombies.iter().position(|&p| p != pid && !crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(p))
        };
        let pos = match pos_opt {
            Some(p) => p,
            None => break, // all remaining zombies still running — cannot reclaim
        };
        let pid_to_reclaim = { ZOMBIE_PIDS.lock().remove(pos) };
        if crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(pid_to_reclaim) {
            ZOMBIE_PIDS.lock().push(pid_to_reclaim);
            break;
        }
        if !sched.recycle_terminated(pid_to_reclaim) {
            // Already gone or not found — continue to next
            continue;
        }
    }
}

/// Expose queue length for spawn backpressure checks.
pub fn zombie_queue_len() -> usize {
    ZOMBIE_PIDS.lock().len()
}

/// Check if spawn should be backpressured: queue at MAX and oldest still running.
/// Caller should attempt sync reclaim first; if still full, return NoMem.
pub fn is_zombie_backpressured() -> bool {
    let zombies = ZOMBIE_PIDS.lock();
    if zombies.len() < MAX_ZOMBIES { return false; }
    // If oldest eligible zombie is not running, we could reclaim, so not truly backpressured
    // Check if any zombie is reclaimable
    for &p in zombies.iter() {
        if !crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(p) {
            return false; // reclaimable, not backpressured
        }
    }
    true
}

/// Try to reap *all* zombies that are not running on ANY CPU.
/// Called from schedule() with scheduler lock already held, after context
/// switch is decided. F-01 ensures we never free a pid still running on any
/// CPU; F-06 ensures we drain the whole eligible set per schedule, so a
/// burst of 1000 exits is reaped in one schedule, not 1000 schedules.
pub fn reap_pending_zombies(sched: &mut Scheduler, current_pid: u32) {
    // Quick check without lock to avoid taking ZOMBIE_PIDS when empty
    if ZOMBIE_PIDS.lock().is_empty() { return; }

    // Drain loop: keep trying to reap while there is an eligible zombie.
    loop {
        let pid_to_reap = {
            let mut zombies = ZOMBIE_PIDS.lock();
            if zombies.is_empty() { break; }
            // Find first zombie not running on any CPU and not the current pid
            let pos = zombies.iter().position(|&p| {
                if p == current_pid { return false; }
                !crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(p)
            });
            match pos {
                Some(pos) => Some(zombies.remove(pos)),
                None => None,
            }
        };
        match pid_to_reap {
            Some(pid) => {
                // Double-check after dropping ZOMBIE_PIDS lock
                if crate::arch::x64::cpu_local::is_pid_running_on_any_cpu(pid) {
                    ZOMBIE_PIDS.lock().push(pid);
                    continue;
                }
                // Recycle may take other locks (Ob), but not ZOMBIE_PIDS, so safe
                if !sched.recycle_terminated(pid) {
                    // If recycle failed (already gone), just continue to next
                    continue;
                }
                // Continue loop to reap next eligible zombie
            }
            None => break,
        }
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
        crate::serial_println!("[SPAWN] pid={} tid={} obj_id={:?} ob_id={:?} thread_obj_id={:?}", pid, tid, obj_id, ob_id, thread_obj_id);

        let mut eproc = Eprocess {
            pid,
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
            waiting_for: None,
            priority,
            time_slice_remaining: TIME_SLICES[priority as usize],
            ticks_since_scheduled: 0,
            kernel_stack_top,
            kernel_stack: Some(stack),
            teb_base: 0, cpu: 0,
            obj_id: None,
            kernel_apc_queue: VecDeque::new(),
            user_apc_queue: VecDeque::new(),
            apc_pending: false,
        };

        let ep_slot = self.alloc_eprocess_slot()?;
        self.eprocesses[ep_slot] = Some(Eprocess::new_kernel(self.next_pid));
        self.next_pid += 1;
        self.kthreads[th_slot] = Some(Box::new(kthread));
        // Fase 3 P1/P5: capturar frame inicial 18 slots y canary
        let (kptr, base, top, init_rsp, ent) = {
            let k = self.kthreads[th_slot].as_ref().unwrap();
            let b = k.kernel_stack_top.wrapping_sub(KERNEL_STACK_SIZE as u64);
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

        // Free resources from eprocess
        if let Some(ep_idx) = ep_idx {
            if let Some(mut eproc) = self.eprocesses[ep_idx].take() {
                // Free user slot
                if let Some(slot) = eproc.user_slot.take() {
                    crate::arch::x64::paging::free_user_slot(slot);
                }
                // Free heap pages + heap slot
                if eproc.heap_base != 0 {
                    crate::arch::x64::paging::heap_free_range(
                        eproc.heap_base,
                        eproc.heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE,
                    );
                    let heap_idx = ((eproc.heap_base
                        - crate::arch::x64::paging::PROCESS_HEAP_BASE)
                        / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as u8;
                    crate::arch::x64::paging::free_heap_slot(heap_idx);
                }
                // Free mmap regions
                for r in eproc.mmap_regions.iter() {
                    crate::arch::x64::paging::mmap_free_range(r.base, r.base + r.len);
                }
                // Close all handles
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
        }

        // Free all kernel stacks and unregister thread KOBJs
        for tid in &tids {
            if let Some(th) = self.find_kthread_mut(*tid) {
                if let Some(kid) = th.obj_id {
                    let _ = object::ob_destroy_object(kid);
                }
                // Kernel stack freed on drop
            }
            let th_idx = self.kthreads.iter().position(|t| {
                t.as_ref().is_some_and(|k| k.tid == *tid)
            });
            if let Some(th_idx) = th_idx {
                self.kthreads[th_idx] = None;
            }
        }

        crate::trace_sched!(2, pid, 0); // KILL_PROCESS
        true
    }

    /// Recycle a terminated EPROCESS (only when last thread exits).
    /// Caller must free EPROCESS resources first (user slot, heap, mmap, pipes).
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
            // Remove all remaining threads (should be 0 at this point)
            let tids: Vec<u32> = self.thread_tids_for_pid(pid);
            for tid in &tids {
                let th_idx = self.kthreads.iter().position(|t| {
                    t.as_ref().is_some_and(|k| k.tid == *tid)
                });
                if let Some(th_idx) = th_idx {
                    // Unregister thread Ob
                    if let Some(th) = &self.kthreads[th_idx] {
                        if let Some(kid) = th.obj_id {
                            let _ = object::ob_destroy_object(kid);
                        }
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
            self.kthreads[th_idx] = None;
            crate::trace_sched!(3, tid as u64, 1);
            true
        } else {
            false
        }
    }

    // ── Wake helpers ──

}
