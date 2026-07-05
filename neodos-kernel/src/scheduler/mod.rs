//! A1.5 EPROCESS/KTHREAD split — Thread-based scheduler
//!
//! - EPROCESS: shared resources (address space, handle table, heap, mmap, CWD)
//! - KTHREAD: per-thread CPU context, priority, time slice, kernel stack
//! - Schedule operates on threads, lazy CR3 swap across EPROCESS boundaries

pub mod address_space;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::object::{self, ObType, ObId};
use crate::security::token::Token;

// ── Constants ──

pub const KERNEL_STACK_SIZE: usize = 16384;
const IDLE_STACK_SIZE: usize = 4096;

pub const PRIORITY_HIGH: u8 = 0;
pub const PRIORITY_ABOVE_NORMAL: u8 = 1;
pub const PRIORITY_NORMAL: u8 = 2;
pub const PRIORITY_IDLE: u8 = 3;
pub const PRIORITY_COUNT: u8 = 4;

pub const TIME_SLICES: [u16; PRIORITY_COUNT as usize] = [400, 200, 100, 50];

pub const AGING_INTERVAL_TICKS: u64 = 500;
pub const MAX_STARVATION_TICKS: u64 = 5000;

/// TEB (Thread Environment Block) size: 4 KB page
pub const TEB_SIZE: u64 = 0x1000;

#[repr(align(16))]
pub struct AlignedKStack(pub [u8; KERNEL_STACK_SIZE]);

static mut IDLE_STACK: [u8; IDLE_STACK_SIZE] = [0; IDLE_STACK_SIZE];

// ── MmapRegion (unchanged) ──

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MmapRegion {
    pub base: u64,
    pub len: u64,
    pub prot: u16,
    pub flags: u16,
    pub drive: u8,
    pub inode: u32,
    pub file_size: u32,
}

// ── ThreadState ──

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked { waiting_for: u32 },
    Terminated,
}

impl ThreadState {
    pub fn to_u8(&self) -> u8 {
        match self {
            ThreadState::Ready => 0,
            ThreadState::Running => 1,
            ThreadState::Blocked { .. } => 2,
            ThreadState::Terminated => 3,
        }
    }
}

// ── KTHREAD ──

#[repr(C)]
pub struct Kthread {
    // CPU context (saved/restored on context switch)
    pub rax: u64,  rbx: u64,  rcx: u64,  rdx: u64,
    pub rsi: u64,  rdi: u64,  r8: u64,   r9: u64,
    pub r10: u64,  r11: u64,  r12: u64,  r13: u64,
    pub r14: u64,  r15: u64,  rbp: u64,
    pub rsp: u64,  pub rip: u64,  pub rflags: u64,

    // IDs
    pub tid: u32,
    pub pid: u32,     // backref → EPROCESS

    // Scheduling state
    pub state: ThreadState,
    pub cpu_ticks: u64,
    pub waiting_for: Option<u32>,
    pub priority: u8,
    pub time_slice_remaining: u16,
    pub ticks_since_scheduled: u64,

    // Kernel stack
    pub kernel_stack_top: u64,
    kernel_stack: Option<Box<AlignedKStack>>,

    // TEB (Thread Environment Block) — user-mode TLS area
    pub teb_base: u64,

    // CPU affinity
    pub cpu: u32,

    // KOBJ
    pub obj_id: Option<ObId>,

    // A4.5 — APC queues
    pub kernel_apc_queue: VecDeque<crate::apc::ApcEntry>,
    pub user_apc_queue: VecDeque<crate::apc::ApcEntry>,
    pub apc_pending: bool,
}

impl fmt::Debug for Kthread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kthread")
            .field("tid", &self.tid)
            .field("pid", &self.pid)
            .field("rip", &self.rip)
            .field("rsp", &self.rsp)
            .field("state", &self.state)
            .field("cpu_ticks", &self.cpu_ticks)
            .field("priority", &self.priority)
            .field("time_slice_remaining", &self.time_slice_remaining)
            .field("kernel_stack_top", &self.kernel_stack_top)
            .field("obj_id", &self.obj_id)
            .finish()
    }
}

impl Kthread {
    pub fn take_kernel_stack(&mut self) -> Option<Box<AlignedKStack>> {
        self.kernel_stack.take()
    }
}

// ── EPROCESS ──

pub struct Eprocess {
    pub pid: u32,
    pub parent_pid: u32,
    pub handle_table: crate::handle::HandleTable,
    pub cwd_drive: u8,
    pub cwd_path: String,
    pub heap_base: u64,
    pub heap_break: u64,
    pub user_slot: Option<u8>,
    pub mmap_regions: Vec<MmapRegion>,
    pub mmap_next: u64,
    pub thread_count: u32,
    pub exit_code: i64,
    pub obj_id: Option<ObId>,
    pub ob_id: Option<ObId>,
    pub address_space: address_space::AddressSpace,
    pub token: Token,
    pub vt_num: u8,
}

// ── Frame init helpers ──

fn init_ring0_frame(kernel_stack_top: u64, entry: u64) -> u64 {
    let mut sp = kernel_stack_top & !0xF;
    unsafe {
        let stack = sp as *mut u64;
        stack.offset(-1).write(0x202);
        stack.offset(-2).write(0x08);
        stack.offset(-3).write(entry);
        for j in 4..19 {
            stack.offset(-(j as isize)).write(0);
        }
        sp -= 18 * 8;
    }
    sp
}

pub fn init_ring3_frame(kernel_stack_top: u64, entry: u64, user_stack_top: u64) -> u64 {
    let mut sp = kernel_stack_top & !0xF;
    unsafe {
        let stack = sp as *mut u64;
        stack.offset(-1).write(0x23);
        stack.offset(-2).write(user_stack_top);
        stack.offset(-3).write(0x202);
        stack.offset(-4).write(0x1B);
        stack.offset(-5).write(entry);
        for j in 6..21 {
            stack.offset(-(j as isize)).write(0);
        }
        sp -= 20 * 8;
    }
    sp
}

// ── Idle task ──

fn idle_task() -> ! {
    loop {
        crate::hal::without_interrupts(|| {
            crate::work_queue::WORK_QUEUE.process_high();
        });
        crate::hal::without_interrupts(|| {
            crate::work_queue::WORK_QUEUE.process_low();
        });
        crate::eventbus::EVENT_BUS.dispatch_pending();
        crate::net::dhcp::dhcp_tick();
        crate::hal::hlt_once();
    }
}

// ── Constructors ──

impl Kthread {
    pub fn new_idle(tid: u32, pid: u32, entry: u64, stack_top: u64) -> Self {
        let rsp = init_ring0_frame(stack_top, entry);
        Kthread {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0,
            r14: 0, r15: 0, rbp: 0,
            rsp, rip: entry, rflags: 0x202,
            tid, pid,
            state: ThreadState::Ready,
            cpu_ticks: 0,
            waiting_for: None,
            priority: PRIORITY_NORMAL,
            time_slice_remaining: TIME_SLICES[PRIORITY_NORMAL as usize],
            ticks_since_scheduled: 0,
            kernel_stack_top: stack_top,
            kernel_stack: None,
            teb_base: 0,
            cpu: 0,
            obj_id: None,
            kernel_apc_queue: VecDeque::new(),
            user_apc_queue: VecDeque::new(),
            apc_pending: false,
        }
    }

    pub fn new_ring3(tid: u32, pid: u32, entry: u64, user_stack_top: u64) -> Self {
        let stack = Box::new(AlignedKStack([0u8; KERNEL_STACK_SIZE]));
        let kernel_stack_top = stack.0.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let rsp = init_ring3_frame(kernel_stack_top, entry, user_stack_top);
        // TEB: allocate page on first write via demand paging
        // The TID is used to derive a fixed address within the heap region
        let teb_base = 0;
        Kthread {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0,
            r14: 0, r15: 0, rbp: 0,
            rsp, rip: entry, rflags: 0x202,
            tid, pid,
            state: ThreadState::Ready,
            cpu_ticks: 0,
            waiting_for: None,
            priority: PRIORITY_NORMAL,
            time_slice_remaining: TIME_SLICES[PRIORITY_NORMAL as usize],
            ticks_since_scheduled: 0,
            kernel_stack_top,
            kernel_stack: Some(stack),
            teb_base,
            cpu: unsafe { crate::arch::x64::cpu_local::this_cpu_id() },
            obj_id: None,
            kernel_apc_queue: VecDeque::new(),
            user_apc_queue: VecDeque::new(),
            apc_pending: false,
        }
    }
}

impl Eprocess {
    pub fn new_idle(pid: u32) -> Self {
        Eprocess {
            pid,
            parent_pid: 0,
            handle_table: crate::handle::HandleTable::new(),
            cwd_drive: 2,
            cwd_path: String::from("\\"),
            heap_base: 0,
            heap_break: 0,
            user_slot: None,
            mmap_regions: Vec::new(),
            mmap_next: 0,
            thread_count: 0,
            exit_code: 0,
            obj_id: None,
            ob_id: None,
            address_space: address_space::AddressSpace::new(),
            token: crate::security::DEFAULT_ADMIN_TOKEN.clone(),
            vt_num: 0,
        }
    }

    pub fn new_ring3(pid: u32, slot_idx: u8, cwd_drive: u8, cwd_path: &str, heap_base: u64, parent_pid: u32) -> Self {
        Eprocess {
            pid,
            parent_pid,
            handle_table: crate::handle::HandleTable::with_defaults(),
            cwd_drive,
            cwd_path: cwd_path.to_string(),
            heap_base,
            heap_break: heap_base,
            user_slot: Some(slot_idx),
            mmap_regions: Vec::new(),
            mmap_next: crate::arch::x64::paging::MMAP_BASE,
            thread_count: 1,
            exit_code: 0,
            obj_id: None,
            ob_id: None,
            address_space: address_space::AddressSpace::new(),
            token: crate::security::DEFAULT_ADMIN_TOKEN.clone(),
            vt_num: 0,
        }
    }
}

// ── Scheduler ──

pub struct Scheduler {
    pub eprocesses: Vec<Option<Eprocess>>,
    pub kthreads: Vec<Option<Kthread>>,
    pub current_tid: u32,
    pub next_pid: u32,
    pub next_tid: u32,
    timer_ticks: u64,
}

#[allow(unused_macros)]
macro_rules! with_current {
    ($sched:expr, $eproc:ident, $body:block) => {{
        let tid = $sched.current_tid;
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
    // ── Lookup helpers ──

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
            .and_then(|t| t.as_mut())
    }

    pub fn find_kthread(&self, tid: u32) -> Option<&Kthread> {
        self.kthreads.iter()
            .find(|t| t.as_ref().is_some_and(|k| k.tid == tid))
            .and_then(|t| t.as_ref())
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

    /// Find the first free slot index in eprocesses vec, growing if full.
    pub fn alloc_eprocess_slot(&mut self) -> Option<usize> {
        let pos = self.eprocesses.iter().position(|e| e.is_none());
        if pos.is_some() {
            pos
        } else {
            let idx = self.eprocesses.len();
            self.eprocesses.push(None);
            Some(idx)
        }
    }

    /// Find the first free slot index in kthreads vec, growing if full.
    pub fn alloc_kthread_slot(&mut self) -> Option<usize> {
        let pos = self.kthreads.iter().position(|t| t.is_none());
        if pos.is_some() {
            pos
        } else {
            let idx = self.kthreads.len();
            self.kthreads.push(None);
            Some(idx)
        }
    }

    /// Current TID convenience
    pub fn current_pid(&self) -> u32 {
        self.find_kthread(self.current_tid).map(|t| t.pid).unwrap_or(0)
    }

    pub fn current_eprocess_mut(&mut self) -> Option<&mut Eprocess> {
        let tid = self.current_tid;
        let pid = self.find_kthread(tid).map(|t| t.pid)?;
        self.find_eprocess_mut(pid)
    }

    pub fn current_kthread_mut(&mut self) -> Option<&mut Kthread> {
        self.find_kthread_mut(self.current_tid)
    }

    pub fn current_eprocess(&self) -> Option<&Eprocess> {
        let pid = self.find_kthread(self.current_tid).map(|t| t.pid)?;
        self.find_eprocess(pid)
    }

    // ── Construction ──

    pub fn new() -> Self {
        let mut eprocesses = Vec::with_capacity(32);
        let mut kthreads = Vec::with_capacity(64);

        // Idle EPROCESS (PID 0) + idle KTHREAD (TID 0)
        let idle_stack_top = unsafe { IDLE_STACK.as_ptr().add(IDLE_STACK_SIZE) as u64 } & !0xF;
        let idle_eproc = Eprocess::new_idle(0);
        let idle_thread = Kthread::new_idle(
            0, 0,
            idle_task as *const () as u64,
            idle_stack_top,
        );
        eprocesses.push(Some(idle_eproc));
        kthreads.push(Some(idle_thread));

        Scheduler {
            eprocesses,
            kthreads,
            current_tid: 0,
            next_pid: 1,
            next_tid: 1,
            timer_ticks: 0,
        }
    }

    pub fn has_non_idle_processes(&self) -> bool {
        self.eprocesses.iter().skip(1).any(|e| e.is_some())
    }

    pub fn has_non_idle_threads(&self) -> bool {
        self.kthreads.iter().skip(1).any(|t| {
            t.as_ref().is_some_and(|k| k.state != ThreadState::Terminated)
        })
    }

    /// Add a new EPROCESS + initial KTHREAD (Ring 3).
    #[allow(clippy::too_many_arguments)]
    pub fn add_ring3_process(
        &mut self,
        entry: u64,
        user_stack_top: u64,
        slot_idx: u8,
        cwd_drive: u8,
        cwd_path: &str,
        heap_base: u64,
        parent_pid: u32,
    ) -> u32 {
        let pid = self.next_pid;
        self.next_pid += 1;

        let tid = self.next_tid;
        self.next_tid += 1;

        // Find free slots
        let ep_slot = self.alloc_eprocess_slot()
            .expect("EPROCESS table full");
        let th_slot = self.alloc_kthread_slot()
            .expect("KTHREAD table full");

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
                        crate::serial_println!("[SCHED] PID {} -> \\Process\\{} OK (ob_id={})", pid, pid, ob_id);
                        eproc.ob_id = Some(ob_id);
                    }
                    Err(e) => {
                        crate::serial_println!("[SCHED] PID {} -> \\Process\\{} FAILED: {}", pid, pid, e);
                        let _ = object::ob_close_object(ob_id);
                    }
                }
            }
            Err(e) => {
                crate::serial_println!("[SCHED] PID {} ob_create FAILED: {:?}", pid, e);
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
        self.kthreads[th_slot] = Some(thread);

        // Enqueue new thread to its CPU's run queue
        if let Some(k) = &self.kthreads[th_slot] {
            Self::enqueue_to_cpu_run_queue(k);
        }

        crate::trace_sched!(1, pid, 0); // ADD_PROCESS
        pid
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

        self.kthreads[th_slot] = Some(thread);

        // Enqueue new thread to its CPU's run queue
        if let Some(k) = &self.kthreads[th_slot] {
            Self::enqueue_to_cpu_run_queue(k);
        }

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

    pub fn wake_waiters(&mut self, pid: u32) {
        // Legacy magic waitpid (0x8000_0000 | pid)
        let legacy_magic = pid | 0x8000_0000;
        // KWait ChildExit magic
        let kwait_magic = crate::kwait::WaitReason::ChildExit { pid }.encode_magic();
        for k in self.kthreads.iter_mut().flatten() {
            if k.waiting_for == Some(legacy_magic) || k.waiting_for == Some(kwait_magic) {
                k.waiting_for = None;
                if matches!(k.state, ThreadState::Blocked { .. }) {
                    k.state = ThreadState::Ready;
                    Self::enqueue_to_cpu_run_queue(k);
                }
            }
        }
    }

    pub fn wake_blocked_on_magic(&mut self, magic: u32) {
        for k in self.kthreads.iter_mut().flatten() {
            if k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                k.waiting_for = None;
                k.state = ThreadState::Ready;
                // Enqueue to its CPU's run queue
                Self::enqueue_to_cpu_run_queue(k);
            }
        }
    }

    // ── Priority ──

    pub fn set_process_priority(&mut self, pid: u32, priority: u8) -> bool {
        if priority >= PRIORITY_COUNT { return false; }
        let mut found = false;
        for k in self.kthreads.iter_mut().flatten() {
            if k.pid == pid {
                k.priority = priority;
                let idx = priority as usize;
                k.time_slice_remaining = TIME_SLICES[idx];
                k.ticks_since_scheduled = 0;
                found = true;
            }
        }
        found
    }

    pub fn reset_time_slice(&mut self) {
        if let Some(k) = self.current_kthread_mut() {
            let idx = (k.priority as usize).min(PRIORITY_COUNT as usize - 1);
            k.time_slice_remaining = TIME_SLICES[idx];
            k.ticks_since_scheduled = 0;
        }
    }

    // ── Aging ──

    fn apply_aging(&mut self) {
        for k in self.kthreads.iter_mut().flatten() {
            if k.tid > 0 && k.state == ThreadState::Ready {
                k.ticks_since_scheduled = k.ticks_since_scheduled.saturating_add(AGING_INTERVAL_TICKS);
                if k.ticks_since_scheduled >= MAX_STARVATION_TICKS && k.priority > PRIORITY_HIGH {
                    k.priority -= 1;
                    k.ticks_since_scheduled = 0;
                }
            }
        }
    }

    // ── Schedule ──

    /// Enqueue a thread to its assigned CPU's per-CPU run queue.
    /// Called when a thread transitions to Ready state.
    pub fn enqueue_to_cpu_run_queue(k: &Kthread) {
        let cpu = k.cpu as usize;
        if cpu >= crate::arch::x64::cpu_local::MAX_CPUS { return; }
        let my_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
        unsafe {
            let run_queue = crate::arch::x64::cpu_local::cpu_run_queue_mut(cpu);
            run_queue.push(k.tid);
        }
        // Send IPI_RESCHEDULE to the target CPU if it's a different CPU
        if cpu != my_cpu {
            unsafe {
                let kprcb = crate::arch::x64::cpu_local::kprcb_page(cpu);
                if let Some(kprcb_addr) = kprcb {
                    let apic_id = core::ptr::read_volatile(
                        (kprcb_addr + 4) as *const u32 // apic_id at offset 0x004
                    );
                    crate::arch::x64::ipi::send_ipi(
                        apic_id,
                        crate::arch::x64::ipi::IPI_RESCHEDULE,
                    );
                }
            }
        }
    }

    /// Try to dequeue the next thread from the current CPU's local run queue.
    /// Returns the TID if found, or None if the queue is empty.
    fn try_dequeue_local() -> Option<u32> {
        unsafe {
            let run_queue = crate::arch::x64::cpu_local::this_cpu_run_queue_mut();
            run_queue.pop()
        }
    }

    /// Try to steal a thread from another CPU's run queue.
    /// Returns the TID if found, or None if all queues are empty.
    fn try_work_steal() -> Option<u32> {
        let my_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
        for cpu in 0..crate::arch::x64::cpu_local::MAX_CPUS {
            if cpu == my_cpu { continue; }
            unsafe {
                let stolen = crate::arch::x64::cpu_local::steal_from_cpu_run_queue(
                    cpu, crate::arch::x64::cpu_local::this_cpu_run_queue_mut());
                if stolen > 0 {
                    // We stole at least one thread, pop from our queue
                    return Self::try_dequeue_local();
                }
            }
        }
        None
    }

    /// Find a thread slot by TID, returning a raw pointer.
    fn find_kthread_ptr(&self, tid: u32) -> *mut Option<Kthread> {
        for th in self.kthreads.iter() {
            if let Some(k) = th {
                if k.tid == tid {
                    return th as *const Option<Kthread> as *mut Option<Kthread>;
                }
            }
        }
        core::ptr::null_mut()
    }

    /// Schedule the next thread.  Tries per-CPU run queue first, falls back
    /// to global priority scan.  Returns a `*mut Kthread` for RSP/stack access.
    pub fn schedule(&mut self) -> *mut Kthread {
        // 1. Try per-CPU local run queue (fast path)
        if let Some(tid) = Self::try_dequeue_local() {
            let ptr = self.find_kthread_ptr(tid);
            if !ptr.is_null() {
                unsafe {
                    if let Some(k) = &mut *ptr {
                        if k.state == ThreadState::Ready {
                            let prev = self.current_tid;
                            self.current_tid = tid;
                            k.state = ThreadState::Running;
                            crate::trace_cswitch!(prev as u64, tid as u64);
                            return k as *mut Kthread;
                        }
                    }
                }
            }
        }

        // 2. Try work stealing from another CPU
        if let Some(tid) = Self::try_work_steal() {
            let ptr = self.find_kthread_ptr(tid);
            if !ptr.is_null() {
                unsafe {
                    if let Some(k) = &mut *ptr {
                        if k.state == ThreadState::Ready {
                            let prev = self.current_tid;
                            self.current_tid = tid;
                            k.state = ThreadState::Running;
                            crate::trace_cswitch!(prev as u64, tid as u64);
                            return k as *mut Kthread;
                        }
                    }
                }
            }
        }

        // 3. Fallback: global priority scan (existing algorithm)
        let start = (self.current_tid + 1) % self.next_tid.max(1);

        for priority in 0..PRIORITY_COUNT {
            for offset in 0..self.next_tid {
                let check_tid = (start + offset) % self.next_tid.max(1);
                if check_tid == 0 { continue; }
                for k in self.kthreads.iter_mut().flatten() {
                    if k.tid == check_tid && k.state == ThreadState::Ready && k.priority == priority {
                        let prev = self.current_tid;
                        self.current_tid = check_tid;
                        k.state = ThreadState::Running;
                        crate::trace_cswitch!(prev as u64, check_tid as u64);
                        return k as *mut Kthread;
                    }
                }
            }
        }

        // Fallback to idle thread (TID 0)
        if let Some(idle) = &mut self.kthreads[0] {
            if idle.tid == 0 && idle.state != ThreadState::Terminated {
                let prev = self.current_tid;
                self.current_tid = 0;
                idle.state = ThreadState::Running;
                crate::trace_cswitch!(prev as u64, 0);
                return idle as *mut Kthread;
            }
        }
        panic!("No ready threads and idle is unavailable");
    }

    // ── Timer tick ──

    pub fn on_timer_tick(&mut self) {
        self.timer_ticks += 1;

        if self.timer_ticks.is_multiple_of(AGING_INTERVAL_TICKS) {
            self.apply_aging();
        }

        let tid = self.current_tid;
        if tid == 0 { return; }

        let mut needs_resched = false;
        if let Some(k) = self.current_kthread_mut() {
            if k.state == ThreadState::Running {
                k.cpu_ticks += 1;

                if k.time_slice_remaining > 0 {
                    k.time_slice_remaining -= 1;
                }

                if k.time_slice_remaining == 0 {
                    k.state = ThreadState::Ready;
                    needs_resched = true;
                }
            }
        }

        if needs_resched {
            // Enqueue back to its CPU's run queue (avoid borrow conflict)
            if let Some(k) = self.find_kthread(tid) {
                Self::enqueue_to_cpu_run_queue(k);
            }
            crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
        }
    }
}

// ── Global scheduler ──

lazy_static! {
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

pub fn current_scheduler() -> &'static Mutex<Scheduler> {
    &SCHEDULER
}

// ── Global helper functions (thread-aware) ──

/// Recycle a terminated EPROCESS. External resources should already be freed.
pub fn cleanup_terminated_process(pid: u32) {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    current_scheduler().lock().recycle_terminated(pid);
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
    let result = if let Some(ep) = lock.current_eprocess_mut() {
        ep.mmap_regions.push(region);
        ep.mmap_next = region.base + region.len;
        Some(region.base)
    } else {
        None
    };
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn remove_current_mmap_region(base: u64) -> Option<MmapRegion> {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let mut lock = SCHEDULER.lock();
    let result = if let Some(ep) = lock.current_eprocess_mut() {
        let idx = ep.mmap_regions.iter().position(|r| r.base == base);
        idx.map(|i| ep.mmap_regions.remove(i))
    } else {
        None
    };
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn free_current_mmap_pages(base: u64, len: u64) {
    crate::arch::x64::paging::mmap_free_range(base, base + len);
}

/// Find a thread's TEB base address.
pub fn current_teb_base() -> u64 {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let lock = SCHEDULER.lock();
    let result = lock.find_kthread(lock.current_tid).map(|k| k.teb_base).unwrap_or(0);
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

// ── Convenience: current PID (deprecated, prefer current_tid) ──

pub fn current_pid() -> u32 {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let lock = SCHEDULER.lock();
    let result = lock.current_pid();
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

pub fn current_tid() -> u32 {
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let result = SCHEDULER.lock().current_tid;
    unsafe { crate::hal::irql::lower_irql(old_irql) };
    result
}

/// For thread_join: block current thread until target TID terminates (via KWait, OB-031).
pub fn block_current_for_thread(tid: u32) {
    crate::kwait::kwait_block(crate::kwait::WaitReason::ThreadJoin { tid });
}

/// Wake a thread blocked on join (via KWait, OB-031).
pub fn wake_thread_joiner(tid: u32) {
    crate::kwait::kwait_wake(&crate::kwait::WaitReason::ThreadJoin { tid });
}
