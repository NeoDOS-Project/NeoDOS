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
use crate::log::LogSubsys;

// ── Constants ──

pub const KERNEL_STACK_SIZE: usize = 16384;
const IDLE_STACK_SIZE: usize = 4096;
pub const IDLE_TIME_SLICE: u16 = 10;   // 10ms — idle runs briefly then yields to Ring 3

pub const PRIORITY_HIGH: u8 = 0;
pub const PRIORITY_ABOVE_NORMAL: u8 = 1;
pub const PRIORITY_NORMAL: u8 = 2;
pub const PRIORITY_IDLE: u8 = 3;
pub const PRIORITY_COUNT: u8 = 4;

pub const TIME_SLICES: [u16; PRIORITY_COUNT as usize] = [400, 200, 100, 50];

pub const BOOT_TID: u32 = 0;
pub const IDLE_TID: u32 = 1;

pub const AGING_INTERVAL_TICKS: u64 = 500;
pub const MAX_STARVATION_TICKS: u64 = 5000;

/// TEB (Thread Environment Block) size: 4 KB page
pub const TEB_SIZE: u64 = 0x1000;

/// Kernel stack canary value at offset 0 of each AlignedKStack
pub const STACK_CANARY: u64 = 0xDEAD_BEEF_CAFE_BABE;

#[repr(align(16))]
pub struct AlignedKStack(pub [u8; KERNEL_STACK_SIZE]);

impl AlignedKStack {
    pub fn new_boxed() -> Box<Self> {
        let mut stack = Box::new(AlignedKStack([0u8; KERNEL_STACK_SIZE]));
        unsafe {
            (stack.0.as_mut_ptr() as *mut u64).write(STACK_CANARY);
        }
        stack
    }
}

pub fn check_kernel_stack_canary(ks_top: u64, pid: u32, tid: u32, current_rsp: u64) {
    if ks_top == 0 { return; }
    let bottom = ks_top.saturating_sub(KERNEL_STACK_SIZE as u64);
    let canary = unsafe { *(bottom as *const u64) };
    if canary != STACK_CANARY {
        crate::serial_println!(
            "\n!!! CRITICAL KERNEL STACK OVERFLOW DETECTED !!!\n\
             PID={} TID={} ks_top=0x{:x} current_rsp=0x{:x} bottom=0x{:x} canary=0x{:x} expected=0x{:x}",
            pid, tid, ks_top, current_rsp, bottom, canary, STACK_CANARY
        );
        panic!("KERNEL STACK CANARY CORRUPTED FOR TID={}", tid);
    }
}

static mut IDLE_STACK: [u8; IDLE_STACK_SIZE] = [0; IDLE_STACK_SIZE];

pub fn spawn_net_kthread(entry: u64) -> Option<u32> {
    // Must disable interrupts when holding the scheduler lock: the timer
    // IRQ handler (timer_handler_inner) also acquires this lock, and with
    // interrupts enabled a timer tick would deadlock.  All other scheduler
    // lock acquisitions in the codebase already follow this pattern.
    crate::hal::without_interrupts(|| {
        current_scheduler()
            .lock()
            .spawn_kthread(entry, PRIORITY_NORMAL)
    })
}

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
    Suspended,
    Terminated,
}

impl ThreadState {
    pub fn to_u8(&self) -> u8 {
        match self {
            ThreadState::Ready => 0,
            ThreadState::Running => 1,
            ThreadState::Blocked { .. } => 2,
            ThreadState::Suspended => 3,
            ThreadState::Terminated => 4,
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
        stack.offset(-1).write(0x10); // SS (kernel_data)
        stack.offset(-2).write(kernel_stack_top); // RSP = ks_top (5-field IRETQ)
        stack.offset(-3).write(0x202); // RFLAGS
        stack.offset(-4).write(0x08); // CS
        stack.offset(-5).write(entry); // RIP
        for j in 6..21 {
            stack.offset(-(j as isize)).write(0);
        }
        sp -= 20 * 8;
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
            crate::work_queue::WORK_QUEUE.process_low();
        });
        crate::eventbus::EVENT_BUS.dispatch_pending();
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
            priority: PRIORITY_IDLE,
            time_slice_remaining: IDLE_TIME_SLICE,
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
        let stack = AlignedKStack::new_boxed();
        let kernel_stack_top = stack.0.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let rsp = init_ring3_frame(kernel_stack_top, entry, user_stack_top);
        Self::new_ring3_with_stack(tid, pid, entry, rsp, kernel_stack_top, stack)
    }

    /// Create a Ring 3 Kthread with a pre-allocated kernel stack.
    /// The caller is responsible for allocating the stack OUTSIDE the scheduler lock
    /// and computing kernel_stack_top and rsp via init_ring3_frame().
    /// This prevents heap allocations inside critical sections (without_interrupts +
    /// scheduler lock), which can corrupt the kernel heap.
    pub fn new_ring3_with_stack(
        tid: u32, pid: u32, entry: u64,
        rsp: u64, kernel_stack_top: u64, stack: Box<AlignedKStack>,
    ) -> Self {
        if kernel_stack_top == 0 {
            panic!("Kthread::new_ring3_with_stack: kernel_stack_top is 0 for TID={}", tid);
        }
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
            teb_base: 0,
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

    pub fn new_kernel(pid: u32) -> Self {
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
        };
        eprocesses.push(Some(boot_eproc));
        kthreads.push(Some(Box::new(boot_thread)));

        // Idle KTHREAD (TID 1) — runs the halt loop when nothing else is Ready.
        // Shares the PID 0 EPROCESS (no separate address space needed for idle).
        let idle_stack_top = unsafe { IDLE_STACK.as_ptr().add(IDLE_STACK_SIZE) as u64 } & !0xF;
        let idle_thread = Kthread::new_idle(
            IDLE_TID, 0,
            idle_task as *const () as u64,
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
                k.tid != IDLE_TID &&
                k.state != ThreadState::Terminated &&
                k.state != ThreadState::Suspended
            })
        })
    }

    /// Add a new EPROCESS + initial KTHREAD (Ring 3).
    /// The kernel stack is allocated internally by Kthread::new_ring3.
    /// Prefer add_ring3_process_with_stack when the caller has already
    /// allocated the stack outside the scheduler lock.
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
        obj_id: Option<ObId>,
        ob_id: Option<ObId>,
        thread_obj_id: Option<ObId>,
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
            address_space: address_space::AddressSpace::new(),
            token: parent_token,
            vt_num: 0,
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
    /// growing them now (outside the lock) so no realloc happens inside.
    pub fn ensure_slots(&mut self) {
        if self.eprocesses.iter().position(|e| e.is_none()).is_none() {
            self.eprocesses.push(None);
        }
        if self.kthreads.iter().position(|t| t.is_none()).is_none() {
            self.kthreads.push(None);
        }
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
                if matches!(k.state, ThreadState::Blocked { .. }) {
                    k.waiting_for = None;
                    Self::make_thread_ready(k);
                    #[cfg(feature = "forensic")]
                    {
                        let cpu_target = k.cpu;
                        let cpu_current = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
                        let rq_target_len = unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(cpu_target as usize).len() };
                        let rq_current_len = unsafe { crate::arch::x64::cpu_local::this_cpu_run_queue_mut().len() };
                        crate::serial_println!("[SMPSCHED] ENQUEUE pid={} tid={} cpu_target={} cpu_current={} rq_target_len={} rq_current_len={} equal={}", k.pid, k.tid, cpu_target, cpu_current, rq_target_len, rq_current_len, rq_target_len==rq_current_len);
                    }
                }
            }
        }
    }

    pub fn wake_blocked_on_magic(&mut self, magic: u32) {
        for k in self.kthreads.iter_mut().flatten() {
            if k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }) {
                k.waiting_for = None;
                Self::make_thread_ready(k);
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
            if k.tid != IDLE_TID && k.state == ThreadState::Ready {
                k.ticks_since_scheduled = k.ticks_since_scheduled.saturating_add(AGING_INTERVAL_TICKS);
                if k.ticks_since_scheduled >= MAX_STARVATION_TICKS && k.priority > PRIORITY_HIGH {
                    k.priority -= 1;
                    k.ticks_since_scheduled = 0;
                }
            }
        }
    }

    // ── Schedule ──

    /// Validate run queue invariants.
    /// Invariant: for each thread,
    ///   Ready    => exactly one entry in its CPU's run queue
    ///   !Ready   => zero entries in its CPU's run queue
    /// Returns Ok(count) on success, Err(message) on violation.
    pub fn validate_runqueue_invariants(&self) -> Result<usize, &'static str> {
        let current = self.find_kthread(self.current_tid)
            .ok_or("current_tid does not identify a thread")?;
        if current.state != ThreadState::Running {
            return Err("current_tid does not identify a Running thread");
        }
        let current_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() };
        if current.cpu != current_cpu {
            return Err("current thread belongs to another CPU");
        }

        let mut thread_tids = Vec::new();
        let mut running_cpus = Vec::new();
        for k in self.kthreads.iter().flatten() {
            if thread_tids.contains(&k.tid) {
                return Err("duplicate TID in scheduler thread table");
            }
            thread_tids.push(k.tid);
            if k.state == ThreadState::Running {
                if running_cpus.contains(&k.cpu) {
                    return Err("more than one Running thread on a CPU");
                }
                running_cpus.push(k.cpu);
            }
        }

        let mut queue_tids = Vec::new();
        let mut total_entries = 0usize;
        for cpu in 0..crate::arch::x64::cpu_local::MAX_CPUS {
            let queue_entries = unsafe {
                let rq = crate::arch::x64::cpu_local::cpu_run_queue_mut(cpu);
                let mut entries = Vec::new();
                let cap = rq.entries.len();
                let mut idx = rq.head_idx as usize;
                for _ in 0..rq.count {
                    entries.push(rq.entries[idx]);
                    idx = (idx + 1) % cap;
                }
                entries
            };

            for tid in queue_entries {
                if queue_tids.contains(&tid) {
                    return Err("duplicate TID in run queue");
                }
                let k = self.find_kthread(tid)
                    .ok_or("orphan TID in run queue")?;
                if tid == BOOT_TID || tid == IDLE_TID {
                    return Err("boot or idle thread found in run queue");
                }
                if k.cpu as usize != cpu {
                    return Err("run queue entry belongs to another CPU");
                }
                if k.state != ThreadState::Ready {
                    return Err("non-Ready thread found in run queue");
                }
                queue_tids.push(tid);
                total_entries += 1;
            }
        }

        for k in self.kthreads.iter().flatten() {
            let count = queue_tids.iter().filter(|&&tid| tid == k.tid).count();
            if k.tid == IDLE_TID || k.tid == BOOT_TID {
                if count != 0 {
                    return Err("special thread found in run queue");
                }
                continue;
            }
            if k.state == ThreadState::Ready && count != 1 {
                return Err("Ready thread not in run queue exactly once");
            }
            if k.state != ThreadState::Ready && count != 0 {
                return Err("Non-Ready thread found in run queue");
            }
        }
        Ok(total_entries)
    }

    /// Enqueue a thread to its assigned CPU's per-CPU run queue.
    /// Called when a thread transitions to Ready state.
    pub fn enqueue_to_cpu_run_queue(k: &Kthread) {
        if k.tid == BOOT_TID || k.tid == IDLE_TID {
            return;
        }
        let cpu = k.cpu as usize;
        if cpu >= crate::arch::x64::cpu_local::MAX_CPUS { return; }
        let my_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
        unsafe {
            let run_queue = crate::arch::x64::cpu_local::cpu_run_queue_mut(cpu);
            if run_queue.contains(k.tid) {
                return; // already in runqueue — avoid duplicate
            }
            run_queue.push(k.tid);
            #[cfg(feature = "forensic")]
            {
                if k.pid >= 4 {
                    let rq_len = run_queue.len();
                    crate::serial_println!("[SMPSCHED] ENQUEUE_OK pid={} tid={} cpu_target={} rq_len={}", k.pid, k.tid, cpu, rq_len);
                }
            }
        }
        // Send IPI_RESCHEDULE to the target CPU if it's a different CPU
        #[cfg(feature = "forensic")]
        if cpu != my_cpu && k.pid >= 4 {
            crate::serial_println!("[SMPSCHED] WAKE_NOTIFY from_cpu={} target_cpu={} pid={} tid={} action=IPI_RESCHEDULE", my_cpu, cpu, k.pid, k.tid);
        } else if k.pid >= 4 {
            #[cfg(feature = "forensic")]
            crate::serial_println!("[SMPSCHED] WAKE_NOTIFY from_cpu={} target_cpu={} pid={} tid={} action=same_cpu_no_ipi", my_cpu, cpu, k.pid, k.tid);
        }
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

    /// Transition a thread to Ready state and enqueue it exactly once.
    /// Safe to call if thread is already Ready (no-op, avoids duplicate enqueue).
    /// Must be called under scheduler lock + interrupts disabled.
    pub fn make_thread_ready(k: &mut Kthread) {
        if k.state == ThreadState::Ready {
            return;
        }
        k.state = ThreadState::Ready;
        let idx = (k.priority as usize).min(PRIORITY_COUNT as usize - 1);
        k.time_slice_remaining = TIME_SLICES[idx];
        k.ticks_since_scheduled = 0;
        Self::enqueue_to_cpu_run_queue(k);
    }

    /// Remove a thread from its assigned CPU's per-CPU run queue.
    /// Called when a thread transitions away from Ready state.
    /// Safe to call even if the thread is not currently in the run queue.
    /// Must be called under scheduler lock + interrupts disabled.
    pub fn remove_from_run_queue(k: &Kthread) {
        let cpu = k.cpu as usize;
        if cpu >= crate::arch::x64::cpu_local::MAX_CPUS {
            return;
        }
        unsafe {
            crate::arch::x64::cpu_local::remove_from_cpu_run_queue(cpu, k.tid);
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
    /// K20 fix: migration updates Kthread.cpu atomically under scheduler lock
    /// so physical queue owner and logical ownership stay consistent.
    fn try_work_steal(&mut self) -> Option<u32> {
        let my_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
        for victim in 0..crate::arch::x64::cpu_local::MAX_CPUS {
            if victim == my_cpu { continue; }
            let stolen = unsafe { self.steal_and_migrate(victim, my_cpu) };
            if stolen > 0 {
                // We stole at least one thread, pop from our queue
                return Self::try_dequeue_local();
            }
        }
        None
    }

    /// Steal TIDs from victim CPU's runqueue to thief CPU's runqueue,
    /// updating Kthread.cpu for each migrated thread.
    /// Must be called with scheduler lock held and interrupts disabled.
    /// Returns number of TIDs successfully migrated.
    /// On destination-full, restores old cpu and pushes TID back to victim.
    unsafe fn steal_and_migrate(&mut self, victim: usize, thief: usize) -> u32 {
        // SAFETY: KPRCB pages are initialized, queues are per-CPU and accessed
        // only while holding scheduler lock in schedule() path.
        let victim_rq = crate::arch::x64::cpu_local::cpu_run_queue_mut(victim);
        let thief_rq = crate::arch::x64::cpu_local::cpu_run_queue_mut(thief);
        let mut stolen: u32 = 0;
        while victim_rq.count > 0 {
            if (thief_rq.count as usize) >= thief_rq.entries.len() {
                break;
            }
            // Peek tid at victim head
            let tid = victim_rq.entries[(victim_rq.head_idx as usize) % victim_rq.entries.len()];
            // Pop victim
            let tid_popped = victim_rq.pop().unwrap();
            debug_assert_eq!(tid, tid_popped);
            // Update ownership optimistically
            let old_cpu: Option<u32> = if let Some(k) = self.find_kthread_mut(tid_popped) {
                let old = k.cpu;
                k.cpu = thief as u32;
                Some(old)
            } else {
                None
            };
            // Try push to thief
            if thief_rq.push(tid_popped) {
                stolen += 1;
            } else {
                // Rollback: restore cpu and push back to victim
                if let Some(old) = old_cpu {
                    if let Some(k) = self.find_kthread_mut(tid_popped) {
                        k.cpu = old;
                    }
                }
                let _ = victim_rq.push(tid_popped);
                break;
            }
        }
        stolen
    }

    /// Find a thread slot by TID, returning a raw pointer to the Kthread Box allocation (stable).
    fn find_kthread_ptr(&self, tid: u32) -> *mut Kthread {
        for th in self.kthreads.iter() {
            if let Some(k) = th {
                if k.tid == tid {
                    return &**k as *const Kthread as *mut Kthread;
                }
            }
        }
        core::ptr::null_mut()
    }

    /// Schedule the next thread.  Tries per-CPU run queue first, falls back
    /// to global priority scan.  Returns a `*mut Kthread` for RSP/stack access.
    pub fn schedule(&mut self) -> *mut Kthread {
        ktrace!(LogSubsys::Sched, "schedule entry");
        // Count every schedule decision, not just global-scan fallbacks.
        self.schedule_count += 1;

        // 1. Try per-CPU local run queue (fast path)
        if let Some(tid) = Self::try_dequeue_local() {
            let ptr = self.find_kthread_ptr(tid);
            if !ptr.is_null() {
                unsafe {
                    let k = &mut *ptr;
                    if k.state == ThreadState::Ready {
                        let prev = self.current_tid;
                        let prev_state = self.find_kthread(prev).map(|t| t.state.to_u8()).unwrap_or(255);
                        self.current_tid = tid;
                        k.state = ThreadState::Running;
                        kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=runqueue",
                            prev, tid);
                        crate::trace_cswitch!(prev as u64, tid as u64);
                        crate::trace_sched_switch!(prev, prev_state, tid, k.state.to_u8());
                        return ptr;
                    }
                }
            }
        }

        // 2. Try work stealing from another CPU
        if let Some(tid) = self.try_work_steal() {
            let ptr = self.find_kthread_ptr(tid);
            if !ptr.is_null() {
                unsafe {
                    let k = &mut *ptr;
                    if k.state == ThreadState::Ready {
                        let prev = self.current_tid;
                        let prev_state = self.find_kthread(prev).map(|t| t.state.to_u8()).unwrap_or(255);
                        self.current_tid = tid;
                        k.state = ThreadState::Running;
                        kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=steal",
                            prev, tid);
                        crate::trace_cswitch!(prev as u64, tid as u64);
                        crate::trace_sched_switch!(prev, prev_state, tid, k.state.to_u8());
                        return ptr;
                    }
                }
            }
        }

        // 3. Fallback: global priority scan (existing algorithm)
        let start = (self.current_tid + 1) % self.next_tid.max(1);

        for priority in 0..PRIORITY_COUNT {
            for offset in 0..self.next_tid {
                let check_tid = (start + offset) % self.next_tid.max(1);
                for k in self.kthreads.iter_mut().flatten() {
                    if k.tid == check_tid && k.state == ThreadState::Ready && k.priority == priority {
                        // P0-3 FIX: Remove from runqueue BEFORE setting state to Running.
                        Scheduler::remove_from_run_queue(&**k);
                        let prev = self.current_tid;
                        let prev_state = k.state.to_u8();
                        self.current_tid = check_tid;
                        k.state = ThreadState::Running;
                        kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=priority_scan prio={}",
                            prev, check_tid, priority);
                        crate::trace_cswitch!(prev as u64, check_tid as u64);
                        crate::trace_sched_switch!(prev, prev_state, check_tid, k.state.to_u8());
                        return &mut **k as *mut Kthread;
                    }
                }
            }
        }

        // Fallback to idle thread (TID 1, PRIORITY_IDLE).
        // NOTE: By design, the idle thread is created with state=Ready but is never
        // added to any runqueue. It is a special thread that only runs when no other
        // threads are ready. The remove_from_run_queue() call here is defensive: if
        // the idle thread were ever accidentally enqueued, we remove it to satisfy
        // the invariant (Running => runqueue_count == 0).
        {
            if !self.has_non_idle_threads() {
                kdebug!(LogSubsys::Sched, "[SCHED] idle_fallback: has_non_idle_threads=false (only idle or Suspended threads)");
            }
            let ptr = self.find_kthread_ptr(IDLE_TID);
            if !ptr.is_null() {
                unsafe {
                    let idle = &mut *ptr;
                    if idle.state != ThreadState::Terminated {
                        Scheduler::remove_from_run_queue(idle);
                        let prev = self.current_tid;
                        let prev_state = self.find_kthread(prev).map(|t| t.state.to_u8()).unwrap_or(255);
                        self.current_tid = IDLE_TID;
                        idle.state = ThreadState::Running;
                        idle.time_slice_remaining = IDLE_TIME_SLICE;
                        kdebug!(LogSubsys::Sched, "[SCHED] SWITCH old_tid={} new_tid={} reason=idle_fallback",
                            prev, IDLE_TID);
                        crate::trace_cswitch!(prev as u64, IDLE_TID as u64);
                        crate::trace_sched_switch!(prev, prev_state, IDLE_TID, idle.state.to_u8());
                        return ptr;
                    }
                }
            }
        }
        panic!("No ready threads and idle is unavailable");
    }

    // ── Timer tick ──

    pub fn on_timer_tick(&mut self, current_rsp: u64) {
        self.timer_ticks += 1;

        if self.timer_ticks.is_multiple_of(AGING_INTERVAL_TICKS) {
            self.apply_aging();
        }

        let _tid = self.current_tid;

        let mut needs_resched = false;
        let mut expired_priority: u8 = 0;
        if let Some(k) = self.current_kthread_mut() {
            let state_before = k.state.to_u8();
            if k.state == ThreadState::Running {
                k.cpu_ticks += 1;

                if k.time_slice_remaining > 0 {
                    k.time_slice_remaining -= 1;
                }

                if k.time_slice_remaining == 0 {
                    expired_priority = k.priority;
                    k.state = ThreadState::Ready;
                    k.rsp = current_rsp;
                    if k.tid != BOOT_TID && k.tid != IDLE_TID {
                        Self::enqueue_to_cpu_run_queue(k);
                    }
                    needs_resched = true;
                    crate::trace_sched_state!(k.tid, state_before, k.state.to_u8(), 2u8); // TIMESLICE_EXPIRED
                }
            }
        }

        if needs_resched {
            kdebug!(LogSubsys::Sched, "[SCHED] TIMESLICE_EXPIRED tid={} priority={}",
                _tid, expired_priority);
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

/// Yield execution of current thread cooperatively back to the scheduler.
pub fn yield_current_thread() {
    crate::hal::without_interrupts(|| {
        let s = current_scheduler();
        let mut lock = s.lock();
        let tid = lock.current_tid;
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

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_ne;
    use crate::test_true;

    // ── Process tests ──

    test_case!("kthread_new_initial_state", {
        let k = Kthread::new_idle(1, 0, 0x400000, 0x800000);
        test_eq!(k.tid, 1);
        test_eq!(k.rip, 0x400000);
        test_eq!(k.state, ThreadState::Ready);
        test_eq!(k.cpu_ticks, 0);
        test_eq!(k.pid, 0);
        test_eq!(k.priority, PRIORITY_IDLE);
        test_eq!(k.time_slice_remaining, IDLE_TIME_SLICE);
    });

    test_case!("kthread_state_debug", {
        let mut k = Kthread::new_idle(1, 0, 0x400000, 0x800000);
        test_eq!(k.state, ThreadState::Ready);
        k.state = ThreadState::Running;
        test_eq!(k.state, ThreadState::Running);
        k.state = ThreadState::Blocked { waiting_for: 42 };
        test_eq!(k.state, ThreadState::Blocked { waiting_for: 42 });
        k.state = ThreadState::Terminated;
        test_eq!(k.state, ThreadState::Terminated);
    });

    test_case!("kthread_state_partial_eq", {
        let s1 = ThreadState::Ready;
        let s2 = ThreadState::Ready;
        test_eq!(s1, s2);
        test_ne!(ThreadState::Ready, ThreadState::Running);
        test_ne!(ThreadState::Blocked { waiting_for: 1 }, ThreadState::Blocked { waiting_for: 2 });
    });

    test_case!("eprocess_new_ring3", {
        let ep = Eprocess::new_ring3(42, 1, 2, "\\", 0x10000000, 0);
        test_eq!(ep.pid, 42);
        test_eq!(ep.heap_base, 0x10000000);
        test_eq!(ep.heap_break, 0x10000000);
        test_eq!(ep.thread_count, 1);
        test_eq!(ep.cwd_drive, 2);
    });

    // ── Scheduler priority tests ──

    fn add_test_thread(sched: &mut Scheduler, tid: u32, pid: u32, entry: u64, priority: u8, state: ThreadState) {
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(tid, pid, entry, 0x800000);
        k.state = state;
        k.priority = priority;
        k.time_slice_remaining = TIME_SLICES[priority as usize];
        sched.kthreads[slot] = Some(Box::new(k));
        if sched.find_eprocess(pid).is_none() {
            let ep_slot = sched.alloc_eprocess_slot().unwrap();
            sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(pid, 0, 2, "\\", 0x10000000, 0));
        }
        if tid >= sched.next_tid {
            sched.next_tid = tid + 1;
        }
        let k = sched.kthreads.iter().flatten().find(|k| k.tid == tid).unwrap();
        Scheduler::remove_from_run_queue(k);
        if state == ThreadState::Ready {
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
    }

    fn set_test_current(sched: &mut Scheduler, tid: u32) {
        let previous = sched.current_tid;
        if previous != tid {
            if let Some(k) = sched.find_kthread_mut(previous) {
                if k.state == ThreadState::Running {
                    k.state = ThreadState::Blocked { waiting_for: 0 };
                }
            }
        }
        sched.current_tid = tid;
        let k = sched.find_kthread_mut(tid).unwrap();
        Scheduler::remove_from_run_queue(k);
        k.state = ThreadState::Running;
    }

    fn prepare_test_schedule(sched: &mut Scheduler) {
        if let Some(k) = sched.find_kthread_mut(sched.current_tid) {
            if k.state == ThreadState::Running {
                k.state = ThreadState::Blocked { waiting_for: 0 };
            }
        }
    }

    test_case!("sched_priority_high_picked_first", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        let next = sched.schedule();
        let picked_tid = unsafe { (*next).tid };
        test_eq!(picked_tid, 2);
    });

    test_case!("sched_priority_round_robin_same_level", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        let first = sched.schedule();
        let first_tid = unsafe { (*first).tid };
        test_ne!(first_tid, 0);
        let second = sched.schedule();
        let second_tid = unsafe { (*second).tid };
        test_ne!(second_tid, first_tid);
    });

    test_case!("sched_priority_idle_last", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 2);
    });

    test_case!("sched_time_slice_default_values", {
        let k = Kthread::new_ring3(1, 1, 0x400000, 0x800000);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_NORMAL as usize]);
        test_eq!(k.priority, PRIORITY_NORMAL);
    });

    test_case!("sched_on_timer_tick_decrements_slice", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;  // skip TID 0 (boot) and TID 1 (idle)
        sched.current_tid = 3;
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(3, 2, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.time_slice_remaining = 5;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(Box::new(k));
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        sched.on_timer_tick(0x700000);
        let remaining = sched.kthreads[slot].as_ref().unwrap().time_slice_remaining;
        test_eq!(remaining, 4);
    });

    test_case!("sched_on_timer_tick_expire_yields", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;  // skip TID 0 (boot) and TID 1 (idle)
        sched.current_tid = 3;
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(3, 2, 0x400000, 0x800000);
        k.state = ThreadState::Running;
        k.time_slice_remaining = 1;
        k.priority = PRIORITY_NORMAL;
        sched.kthreads[slot] = Some(Box::new(k));
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        sched.on_timer_tick(0x700000);
        let state = sched.kthreads[slot].as_ref().unwrap().state;
        test_eq!(state, ThreadState::Ready);
    });

    test_case!("sched_aging_boosts_starved", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;  // skip TID 0 (boot) and TID 1 (idle)
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(3, 2, 0x400000, 0x800000);
        k.state = ThreadState::Ready;
        k.priority = PRIORITY_IDLE;
        k.ticks_since_scheduled = MAX_STARVATION_TICKS + 1;
        k.time_slice_remaining = 50;
        sched.kthreads[slot] = Some(Box::new(k));
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(2, 0, 2, "\\", 0x10000000, 0));
        for _ in 0..AGING_INTERVAL_TICKS + 5 {
            sched.on_timer_tick(0x700000);
        }
        let boosted = sched.kthreads[slot].as_ref().unwrap();
        test_true!(boosted.priority < PRIORITY_IDLE);
    });

    test_case!("sched_set_process_priority", {
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        let slot = sched.alloc_kthread_slot().unwrap();
        let mut k = Kthread::new_ring3(1, 1, 0x400000, 0x800000);
        k.state = ThreadState::Ready;
        sched.kthreads[slot] = Some(Box::new(k));
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(1, 0, 2, "\\", 0x10000000, 0));
        test_true!(sched.set_process_priority(1, PRIORITY_HIGH));
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.priority, PRIORITY_HIGH);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_HIGH as usize]);
        test_true!(sched.set_process_priority(1, PRIORITY_IDLE));
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.priority, PRIORITY_IDLE);
        test_eq!(k.time_slice_remaining, TIME_SLICES[PRIORITY_IDLE as usize]);
        test_true!(!sched.set_process_priority(1, 99));
        let k = sched.kthreads[slot].as_ref().unwrap();
        test_eq!(k.priority, PRIORITY_IDLE);
        test_true!(!sched.set_process_priority(999, PRIORITY_HIGH));
    });

    test_case!("sched_priority_preempt_higher_ready", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 2;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        add_test_thread(&mut sched, 3, 3, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 1);
    });

    test_case!("sched_priority_blocked_ignored", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 2;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Blocked { waiting_for: 99 });
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        add_test_thread(&mut sched, 3, 3, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 3);
    });

    test_case!("sched_priority_unblock_picks_higher", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        sched.current_tid = 2;
        add_test_thread(&mut sched, 1, 1, 0x400000, PRIORITY_HIGH, ThreadState::Blocked { waiting_for: 0xFFFF_0000 });
        add_test_thread(&mut sched, 2, 2, 0x400000, PRIORITY_IDLE, ThreadState::Running);
        sched.kthreads.iter_mut().find(|t| t.as_ref().is_some_and(|k| k.tid == 1))
            .and_then(|t| t.as_mut()).unwrap().state = ThreadState::Ready;
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        test_eq!(picked, 1);
    });

    // ── Mmap tests ──

    test_case!("mmap_region_create", {
        let r = MmapRegion {
            base: 0x20000000, len: 0x1000, prot: 3, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        test_eq!(r.base, 0x20000000);
        test_eq!(r.len, 0x1000);
        test_eq!(r.prot, 3);
        test_eq!(r.flags, 1);
    });

    test_case!("mmap_region_anonymous", {
        let r = MmapRegion {
            base: 0x20001000, len: 0x4000, prot: 1, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        test_true!((r.flags & 1) != 0);
        test_eq!(r.prot & 2, 0);
        test_eq!(r.prot & 1, 1);
    });

    test_case!("mmap_region_file_backed", {
        let r = MmapRegion {
            base: 0x20010000, len: 0x2000, prot: 3, flags: 0,
            drive: 2, inode: 42, file_size: 8192,
        };
        test_eq!(r.flags & 1, 0);
        test_eq!(r.drive, 2);
        test_eq!(r.inode, 42);
        test_eq!(r.file_size, 8192);
    });

    test_case!("mmap_region_contains", {
        let r = MmapRegion {
            base: 0x20000000, len: 0x10000, prot: 3, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        test_true!(0x20000000 >= r.base && 0x20000000 < r.base + r.len);
        test_true!(0x2000FFF0 >= r.base && 0x2000FFF0 < r.base + r.len);
        test_true!(!(0x20010000 >= r.base && 0x20010000 < r.base + r.len));
    });

    test_case!("mmap_is_mmap_virtual_addr", {
        test_true!(crate::arch::x64::paging::is_mmap_virtual_addr(0x20000000));
        test_true!(crate::arch::x64::paging::is_mmap_virtual_addr(0x21FFFFFF));
        test_true!(!crate::arch::x64::paging::is_mmap_virtual_addr(0x1FFFFFFF));
        test_true!(!crate::arch::x64::paging::is_mmap_virtual_addr(0x22000000));
    });

    test_case!("mmap_process_add_remove", {
        let mut ep = Eprocess::new_ring3(99, 0, 2, "\\", 0x10000000, 0);
        test_eq!(ep.mmap_regions.len(), 0);
        let r1 = MmapRegion {
            base: 0x20000000, len: 0x1000, prot: 3, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        ep.mmap_regions.push(r1);
        test_eq!(ep.mmap_regions.len(), 1);
        test_eq!(ep.mmap_regions[0].base, 0x20000000);
        let r2 = MmapRegion {
            base: 0x20001000, len: 0x2000, prot: 1, flags: 1,
            drive: 0, inode: 0, file_size: 0,
        };
        ep.mmap_regions.push(r2);
        test_eq!(ep.mmap_regions.len(), 2);
        let idx = ep.mmap_regions.iter().position(|r| r.base == 0x20000000);
        test_true!(idx.is_some());
        ep.mmap_regions.remove(idx.unwrap());
        test_eq!(ep.mmap_regions.len(), 1);
        test_eq!(ep.mmap_regions[0].base, 0x20001000);
    });

    // ── Scheduler stress ──

    test_case!("stress_sched_rapid_yield", {
        for i in 0..500 {
            crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
            let prev = crate::syscall::clear_need_resched();
            test_true!(prev);
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            let _ = i;
        }
    });

    test_case!("stress_sched_state_transitions", {
        let mut p = Kthread::new_idle(99, 0, 0x400000, 0x800000);
        test_eq!(p.state, ThreadState::Ready);
        for _ in 0..200 {
            p.state = ThreadState::Running;
            p.state = ThreadState::Ready;
        }
        p.state = ThreadState::Terminated;
        test_eq!(p.state, ThreadState::Terminated);
    });

    // ── Run queue invariant tests (P0-3) ──

    test_case!("rq_invariant_enqueue_once", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });

    test_case!("rq_invariant_ready_to_blocked", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Blocked { waiting_for: 99 };
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });

    test_case!("rq_invariant_blocked_to_ready", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: 0x0005_0001 });
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });

    test_case!("rq_invariant_double_wake", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: 0x0005_0001 });
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            k.waiting_for = None;
            Scheduler::make_thread_ready(k);
        }
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });

    test_case!("rq_invariant_suspended_to_ready", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Suspended);
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });

    test_case!("rq_invariant_running_no_entry", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        set_test_current(&mut sched, 2);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });

    test_case!("rq_invariant_terminated_no_entry", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Terminated);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });

    test_case!("rq_invariant_stress_mixed_transitions", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        for _ in 0..1000 {
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                k.state = ThreadState::Running;
            }
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                Scheduler::make_thread_ready(k);
            }
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                Scheduler::remove_from_run_queue(k);
                k.state = ThreadState::Blocked { waiting_for: 99 };
            }
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                Scheduler::make_thread_ready(k);
            }
        }
        set_test_current(&mut sched, IDLE_TID);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });

    test_case!("rq_invariant_multi_thread", {
        let mut sched = Scheduler::new();
        sched.next_tid = 6;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 4, 3, 0x400000, PRIORITY_NORMAL, ThreadState::Blocked { waiting_for: 42 });
        add_test_thread(&mut sched, 5, 4, 0x400000, PRIORITY_IDLE, ThreadState::Running);
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Running;
        }
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 3).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Blocked { waiting_for: 43 };
        }
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 4).unwrap();
            Scheduler::make_thread_ready(k);
        }
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 5).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        set_test_current(&mut sched, 2);
        let result = sched.validate_runqueue_invariants();
        match result {
            Ok(count) => test_eq!(count, 1),
            Err(msg) => {
                crate::serial_println!("rq_invariant_multi_thread: {}", msg);
                return Err(msg);
            }
        }
    });

    test_case!("rq_invariant_cpu_runqueue_remove", {
        use crate::arch::x64::cpu_local::CpuRunQueue;
        let mut rq = CpuRunQueue::new();
        rq.push(10);
        rq.push(20);
        rq.push(30);
        test_eq!(rq.len(), 3);
        test_true!(rq.contains(20));
        test_true!(rq.remove(20));
        test_eq!(rq.len(), 2);
        test_true!(!rq.contains(20));
        test_true!(rq.contains(10));
        test_true!(rq.contains(30));
        test_true!(rq.remove(10));
        test_eq!(rq.len(), 1);
        test_true!(rq.contains(30));
        test_true!(rq.remove(30));
        test_eq!(rq.len(), 0);
        test_true!(!rq.contains(30));
        test_true!(!rq.remove(99));
    });

    test_case!("rq_invariant_full_regression", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked = unsafe { (*next).tid };
        let result = sched.validate_runqueue_invariants();
        if let Err(msg) = result {
            crate::serial_println!("rq_invariant_full_regression: {}", msg);
            return Err(msg);
        }

        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked).unwrap();
            Scheduler::make_thread_ready(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        let result = sched.validate_runqueue_invariants();

        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Blocked { waiting_for: 99 };
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());

        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked).unwrap();
            k.waiting_for = None;
            Scheduler::make_thread_ready(k);
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());

        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
    });

    // ── P0-3 Priority scan regression tests ──

    test_case!("rq_priority_scan_removes_from_runqueue", {
        // Test: Priority scan must remove thread from runqueue before setting to Running.
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 0;
        // Add a high-priority thread (TID 2) and a normal thread (TID 1).
        // TID 0 is boot, TID 1 is idle, so we start from TID 2.
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        // Verify initial invariants
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 2); // 2 Ready threads in runqueue

        // Schedule — should pick TID 2 (high priority) via priority scan
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked_tid = unsafe { (*next).tid };
        test_eq!(picked_tid, 2);

        // Verify invariant: Running thread must have 0 runqueue entries
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1); // Only TID 3 remains in runqueue
    });

    test_case!("rq_priority_scan_stress_100_iterations", {
        // Stress test: Repeat priority scan 100 times, verify invariant each time.
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        for i in 0..100 {
            // Make both threads Ready again
            {
                let k2 = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
                k2.state = ThreadState::Ready;
                Scheduler::enqueue_to_cpu_run_queue(k2);
            }
            {
                let k3 = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 3).unwrap();
                k3.state = ThreadState::Ready;
                Scheduler::enqueue_to_cpu_run_queue(k3);
            }

            // Schedule — should pick TID 2 (high priority)
            prepare_test_schedule(&mut sched);
            let next = sched.schedule();
            let picked_tid = unsafe { (*next).tid };
            test_eq!(picked_tid, 2);

            // Verify invariant: Running thread must have 0 runqueue entries
            let result = sched.validate_runqueue_invariants();
            test_true!(result.is_ok());
            let _ = i; // suppress unused warning
        }
    });

    test_case!("rq_priority_scan_return_to_ready", {
        // Test: After priority scan selects X, X can yield back to Ready,
        // then be scheduled again via priority scan with invariant preserved.
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        // Schedule TID 2
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked_tid = unsafe { (*next).tid };
        test_eq!(picked_tid, 2);

        // Verify invariant: Running => runqueue_count == 0
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());

        // Simulate yield: Running -> Ready (enqueue)
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        set_test_current(&mut sched, IDLE_TID);

        // Verify invariant: Ready => runqueue_count == 1
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);

        // Schedule again
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked_tid2 = unsafe { (*next).tid };
        test_eq!(picked_tid2, 2);

        // Verify invariant: Running => runqueue_count == 0
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });

    test_case!("rq_priority_scan_multiple_threads", {
        // Test: Multiple threads with different priorities, verify invariant
        // holds for all threads after each schedule.
        let mut sched = Scheduler::new();
        sched.next_tid = 6;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 4, 3, 0x400000, PRIORITY_ABOVE_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 5, 4, 0x400000, PRIORITY_IDLE, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        // Schedule 4 times, each time verify invariants
        for _ in 0..4 {
            prepare_test_schedule(&mut sched);
            let next = sched.schedule();
            let picked_tid = unsafe { (*next).tid };

            // Verify invariant
            let result = sched.validate_runqueue_invariants();
            test_true!(result.is_ok());

            // Mark as Ready again for next iteration
            {
                let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == picked_tid).unwrap();
                Scheduler::make_thread_ready(k);
            }
        }
    });

    test_case!("rq_priority_scan_duplicate_protection", {
        // Test: Fix doesn't break duplicate enqueue protection.
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);

        // Try to enqueue twice
        {
            let k = sched.kthreads.iter_mut().flatten().find(|k| k.tid == 2).unwrap();
            Scheduler::enqueue_to_cpu_run_queue(k);
        }

        // Verify only 1 entry
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });

    test_case!("rq_priority_scan_idle_thread_no_runqueue", {
        // Test: Idle thread is never in runqueue by design.
        // Verify it can be scheduled via priority scan without issues.
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        sched.current_tid = 0;

        // The idle thread (TID 1) is created in Scheduler::new() with state=Ready
        // but is NOT in any runqueue by design.
        // Schedule — should fall back to idle thread
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let picked_tid = unsafe { (*next).tid };
        test_eq!(picked_tid, 1); // IDLE_TID

        // Verify invariant: idle thread (Running) must have 0 runqueue entries
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 0);
    });

    test_case!("rq_timer_expiration_requeues_once", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Running);
        set_test_current(&mut sched, 2);
        sched.kthreads.iter_mut().flatten()
            .find(|k| k.tid == 2).unwrap().time_slice_remaining = 1;

        sched.on_timer_tick(0x700000);
        let k = sched.find_kthread(2).unwrap();
        test_eq!(k.state, ThreadState::Ready);
        test_eq!(k.rsp, 0x700000);
        unsafe {
            let rq = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            test_eq!(rq.len(), 1);
            test_true!(rq.contains(2));
        }
        set_test_current(&mut sched, IDLE_TID);
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
    });

    test_case!("rq_spawn_kthread_enqueues_once", {
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        let tid = sched.spawn_kthread(0x400000, PRIORITY_NORMAL).unwrap();
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(tid));
        }
    });

    test_case!("rq_add_thread_enqueues_once", {
        let mut sched = Scheduler::new();
        sched.next_tid = 2;
        let ep_slot = sched.alloc_eprocess_slot().unwrap();
        sched.eprocesses[ep_slot] = Some(Eprocess::new_ring3(42, 0, 2, "\\", 0x10000000, 0));
        let tid = sched.add_thread_to_process(42, 0x400000, 0x800000).unwrap();
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_ok());
        test_eq!(result.unwrap(), 1);
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(tid));
        }
    });

    test_case!("rq_validator_rejects_invalid_current", {
        let mut sched = Scheduler::new();
        sched.current_tid = 999;
        let result = sched.validate_runqueue_invariants();
        test_true!(result.is_err());
    });

    // ── K17 Gap A/B: kwait_block / kwait_wake real path (isolated Scheduler) ──

    test_case!("k17_kwait_block_wake_single_entry", {
        // Running/Ready thread → kwait_block (remove+Blocked) → kwait_wake (make_ready) → single entry
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        // TID 2 Ready via helper (enqueued)
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // Make TID 2 current Running (remove from queue)
        set_test_current(&mut sched, 2);
        // Validate Running has 0 entries
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 0);

        // Simulate kwait_block: remove + Blocked
        let magic: u32 = 0x0005_0063; // Event 99
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Blocked { waiting_for: magic };
            k.waiting_for = Some(magic);
        }
        // Switch current to idle to allow validation (current must be Running)
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 0);
        test_eq!(sched.find_kthread(2).unwrap().state, ThreadState::Blocked { waiting_for: magic });

        // Simulate kwait_wake: scan + make_thread_ready
        {
            // replicate kwait_wake logic
            let m = magic;
            // collect tids to wake to avoid borrow issues
            let to_wake: Vec<u32> = sched.kthreads.iter().flatten()
                .filter(|k| k.waiting_for == Some(m) && matches!(k.state, ThreadState::Blocked { .. }))
                .map(|k| k.tid)
                .collect();
            for tid in to_wake {
                if let Some(k) = sched.find_kthread_mut(tid) {
                    k.waiting_for = None;
                    Scheduler::make_thread_ready(k);
                }
            }
        }
        // Ready must be exactly once
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        test_eq!(sched.find_kthread(2).unwrap().state, ThreadState::Ready);
        test_eq!(sched.find_kthread(2).unwrap().waiting_for, None);
    });

    test_case!("k17_kwait_double_wake_idempotent", {
        // Blocked → wake → wake again → still single entry
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: 0x0005_0063 });
        // manually set waiting_for to match blocked magic
        sched.find_kthread_mut(2).unwrap().waiting_for = Some(0x0005_0063);
        // first wake
        {
            let magic = 0x0005_0063;
            let tids: Vec<u32> = sched.kthreads.iter().flatten()
                .filter(|k| k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }))
                .map(|k| k.tid).collect();
            for tid in tids {
                if let Some(k) = sched.find_kthread_mut(tid) {
                    k.waiting_for = None;
                    Scheduler::make_thread_ready(k);
                }
            }
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        // second wake — should be no-op
        {
            let magic = 0x0005_0063;
            let tids: Vec<u32> = sched.kthreads.iter().flatten()
                .filter(|k| k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }))
                .map(|k| k.tid).collect();
            for tid in tids {
                if let Some(k) = sched.find_kthread_mut(tid) {
                    k.waiting_for = None;
                    Scheduler::make_thread_ready(k);
                }
            }
        }
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        // also direct make_thread_ready idempotent
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
    });

    test_case!("k17_kwait_wake_multiple_threads_same_magic", {
        // Two Blocked threads waiting on same magic → single wake wakes both
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        let magic: u32 = 0x0006_000A; // Timer 10
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: magic });
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL,
            ThreadState::Blocked { waiting_for: magic });
        sched.find_kthread_mut(2).unwrap().waiting_for = Some(magic);
        sched.find_kthread_mut(3).unwrap().waiting_for = Some(magic);
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 0);
        // wake all
        {
            let tids: Vec<u32> = sched.kthreads.iter().flatten()
                .filter(|k| k.waiting_for == Some(magic) && matches!(k.state, ThreadState::Blocked { .. }))
                .map(|k| k.tid).collect();
            for tid in tids {
                if let Some(k) = sched.find_kthread_mut(tid) {
                    k.waiting_for = None;
                    Scheduler::make_thread_ready(k);
                }
            }
        }
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 2);
        test_eq!(sched.find_kthread(2).unwrap().state, ThreadState::Ready);
        test_eq!(sched.find_kthread(3).unwrap().state, ThreadState::Ready);
    });

    // ── K17 Gap 2: Terminated lifecycle & stale entry ──

    test_case!("k17_terminated_stale_runqueue_detected_and_recycled", {
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, 2);
        // Simulate buggy exit: set Terminated WITHOUT removing from runqueue
        // First, make it Ready again so it's in queue
        {
            let k = sched.find_kthread_mut(2).unwrap();
            // already Running, make Ready to re-enqueue
            Scheduler::make_thread_ready(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        let ok = sched.validate_runqueue_invariants();
        test_true!(ok.is_ok());
        // Now fake bug: Terminated while still in queue (skip remove)
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.state = ThreadState::Terminated;
            // keep in queue intentionally — do NOT call remove
        }
        // Need a Running current for validation: idle is Running
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_err()); // stale non-Ready in queue must be detected
        // Correct path: remove and validate passes
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
        }
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 0);
        // recycle_thread must free slot and keep invariants
        let freed = sched.recycle_thread(2);
        test_true!(freed);
        test_true!(sched.find_kthread(2).is_none());
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
    });

    test_case!("k17_terminated_recycle_keeps_invariants", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, 2);
        // Terminate current correctly (remove then Terminated)
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1); // only TID3 remains Ready
        // recycle terminated thread
        test_true!(sched.recycle_thread(2));
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        test_true!(sched.find_kthread(2).is_none());
        // schedule should still pick TID3
        prepare_test_schedule(&mut sched);
        let next = sched.schedule();
        let tid = unsafe { (*next).tid };
        test_eq!(tid, 3);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
    });

    // ── K17 Gap 3: Same-priority round-robin sustained ──

    test_case!("k17_same_prio_round_robin_sustained_20", {
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);

        let mut picks: Vec<u32> = Vec::new();
        for _ in 0..20 {
            prepare_test_schedule(&mut sched);
            let next = sched.schedule();
            let tid = unsafe { (*next).tid };
            test_true!(tid == 2 || tid == 3);
            picks.push(tid);
            // validate Running not in queue
            let r = sched.validate_runqueue_invariants();
            test_true!(r.is_ok());
            // yield back to Ready (re-enqueue) and set idle as current for next iteration
            {
                let k = sched.find_kthread_mut(tid).unwrap();
                Scheduler::make_thread_ready(k);
            }
            set_test_current(&mut sched, IDLE_TID);
            let r = sched.validate_runqueue_invariants();
            test_true!(r.is_ok());
            test_eq!(r.unwrap(), 2);
        }
        // Both threads must have been scheduled at least 8 times (no starvation)
        let c2 = picks.iter().filter(|&&t| t == 2).count();
        let c3 = picks.iter().filter(|&&t| t == 3).count();
        test_true!(c2 >= 8);
        test_true!(c3 >= 8);
        // Must have alternated at least once (no permanent exclusion)
        let mut alternated = false;
        for w in picks.windows(2) {
            if w[0] != w[1] { alternated = true; break; }
        }
        test_true!(alternated);
    });

    test_case!("k17_same_prio_three_threads_round_robin", {
        let mut sched = Scheduler::new();
        sched.next_tid = 5;
        sched.current_tid = 0;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 4, 3, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        set_test_current(&mut sched, IDLE_TID);
        let mut picks = Vec::new();
        for _ in 0..30 {
            prepare_test_schedule(&mut sched);
            let next = sched.schedule();
            let tid = unsafe { (*next).tid };
            test_true!(tid == 2 || tid == 3 || tid == 4);
            picks.push(tid);
            {
                let k = sched.find_kthread_mut(tid).unwrap();
                Scheduler::make_thread_ready(k);
            }
            set_test_current(&mut sched, IDLE_TID);
        }
        let c2 = picks.iter().filter(|&&t| t == 2).count();
        let c3 = picks.iter().filter(|&&t| t == 3).count();
        let c4 = picks.iter().filter(|&&t| t == 4).count();
        // Each at least 5 times in 30 picks
        test_true!(c2 >= 5);
        test_true!(c3 >= 5);
        test_true!(c4 >= 5);
    });

    // ── K18 Gap 4: Work stealing / cross-CPU runqueue ──
    // Helpers for cross-CPU queue manipulation (avoid IPI side-effects in tests)
    // These tests deliberately use unsafe cpu_run_queue_mut and direct state
    // mutation to isolate the work-stealing contract without modifying production.

    test_case!("k18_steal_drains_victim_to_thief_preserves_order", {
        // Setup: victim CPU1 with 3 Ready threads, thief CPU0 empty
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(0).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            }
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 6;
        // Create 3 threads affine to CPU1 (manual cpu override after helper)
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 3, 2, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        add_test_thread(&mut sched, 4, 3, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // Move them to CPU1: remove from CPU0, set cpu=1, push to CPU1
        for tid in [2u32, 3, 4] {
            if let Some(k) = sched.find_kthread_mut(tid) {
                unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, tid); }
                k.cpu = 1;
                Scheduler::enqueue_to_cpu_run_queue(k);
            }
        }
        // Verify victim has 3, thief empty
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 3);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 0);
        }
        // Steal: drain victim 1 into thief 0 via production steal_from_cpu_run_queue
        let stolen = unsafe {
            let dst = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            crate::arch::x64::cpu_local::steal_from_cpu_run_queue(1, dst)
        };
        test_eq!(stolen, 3);
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 0);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 3);
            // Order preserved: victim pushed 2,3,4 head->tail so thief should pop 2 first
            let rq0 = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            test_eq!(rq0.pop(), Some(2));
            test_eq!(rq0.pop(), Some(3));
            test_eq!(rq0.pop(), Some(4));
        }
        // Cleanup invariant: restore queues empty and threads Ready but not queued
        // Need to re-enqueue for validation? Instead remove and set state
        for tid in [2u32, 3, 4] {
            if let Some(k) = sched.find_kthread_mut(tid) {
                // after pop they are not in queue; ensure state still Ready
                test_eq!(k.state, ThreadState::Ready);
                // reset cpu to 0 for next tests
                k.cpu = 0;
                unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, tid); }
                k.state = ThreadState::Blocked { waiting_for: 0 };
            }
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
        }
        // restore terminated for isolation
        for tid in [2u32, 3, 4] {
            sched.recycle_thread(tid);
        }
    });

    test_case!("k18_steal_affinity_mismatch_stale_cpu_detected", {
        // K20 regression: stolen thread must have k.cpu updated to thief,
        // so validate passes. Previously this test demonstrated stale cpu bug.
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(0).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            }
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_HIGH, ThreadState::Ready);
        // Move TID2 to CPU1
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);

        // Scheduler-level steal must migrate ownership: use try_work_steal path
        // which now updates Kthread.cpu atomically.
        // Call steal_and_migrate directly to isolate migration without dequeue
        let stolen = unsafe { sched.steal_and_migrate(1, 0) };
        test_eq!(stolen, 1);
        // Now TID2 in CPU0 queue and k.cpu==0 → validate must pass
        test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(2));
            test_true!(!crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
        }

        // Pop and become Running on CPU0 → validate still passes
        let tid = unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(0).pop().unwrap() };
        test_eq!(tid, 2);
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.state = ThreadState::Running;
        }
        sched.current_tid = 2;
        test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());

        // Cleanup
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.state = ThreadState::Terminated;
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });

    test_case!("k18_schedule_via_steal_sets_current_tid_and_removes", {
        // Verify schedule() steal path (try_dequeue_local fails, try_work_steal succeeds)
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 4;
        sched.current_tid = IDLE_TID;
        // Prepare idle as Blocked so schedule must pick something else
        prepare_test_schedule(&mut sched); // idle Running -> Blocked
        // Create victim thread on CPU1
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            // Push directly to CPU1 to avoid IPI in test setup
            unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(1).push(2); }
        }
        // Ensure local queue empty
        unsafe { test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 0); }
        // schedule should steal from CPU1 to CPU0 and return TID2 as Running
        // Note: schedule's try_work_steal drains all from victim then pops from thief.
        // After steal, thief has the entry, pop yields TID2, state becomes Running.
        let next = sched.schedule();
        let tid = unsafe { (*next).tid };
        test_eq!(tid, 2);
        test_eq!(sched.current_tid, 2);
        test_eq!(unsafe { (*next).state }, ThreadState::Running);
        // Victim emptied
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 0);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 0);
        }
        // K20 fixed: k.cpu must have been migrated to thief (0) and validate passes
        test_eq!(unsafe { (*next).cpu }, 0);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        // Cleanup: make Running thread Ready (should stay on thief) then remove
        {
            let k = sched.find_kthread_mut(2).unwrap();
            // schedule already removed from queue
            k.state = ThreadState::Ready;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        // cleanup
        if let Some(k) = sched.find_kthread_mut(2) {
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });

    test_case!("k18_cross_cpu_enqueue_respects_thread_cpu_and_validate", {
        // enqueue_to_cpu_run_queue must place thread on its cpu's queue, not current cpu's
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Blocked { waiting_for: 99 });
        // Make it Ready with cpu=1; enqueue should go to CPU1
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.cpu = 1;
            Scheduler::make_thread_ready(k);
        }
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
            test_true!(!crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(2));
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        test_eq!(r.unwrap(), 1);
        // Now change cpu to 0 but leave entry on CPU1 → validate must detect
        sched.find_kthread_mut(2).unwrap().cpu = 0;
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_err());
        // fix and cleanup
        sched.find_kthread_mut(2).unwrap().cpu = 1;
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
        }
    });

    test_case!("k18_steal_empty_victim_returns_zero_and_leaves_local_intact", {
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // TID2 is on CPU0 (default), CPU1 empty
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 0);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 1);
        }
        let stolen = unsafe {
            let dst = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            crate::arch::x64::cpu_local::steal_from_cpu_run_queue(1, dst)
        };
        test_eq!(stolen, 0);
        unsafe {
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).len(), 1);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 0);
        }
        // cleanup
        sched.find_kthread_mut(2).unwrap().state = ThreadState::Terminated;
        unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
        sched.recycle_thread(2);
        set_test_current(&mut sched, IDLE_TID);
        unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear(); }
    });

    test_case!("k18_steal_then_requeue_bounces_to_victim_cpu", {
        // K20 regression: after scheduler-level steal, requeue must stay on thief (0), not bounce to victim (1)
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // Move to CPU1
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        // Steal via scheduler (migrates ownership)
        let stolen = unsafe { sched.steal_and_migrate(1, 0) };
        test_eq!(stolen, 1);
        test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
        let tid = unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(0).pop().unwrap() };
        test_eq!(tid, 2);
        // Simulate Running on thief
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.state = ThreadState::Running;
            sched.current_tid = 2;
        }
        test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
        // Yield: Running → Ready should stay on thief (0)
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::make_thread_ready(k);
        }
        unsafe {
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(2));
            test_true!(!crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        // cleanup
        {
            let k = sched.find_kthread_mut(2).unwrap();
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });

    // ── K19: destination-full and repeated-steal evidence ──

    test_case!("k19_dest_full_steal_pushback_no_loss", {
        // Fill thief CPU0 to capacity (64), victim CPU1 has 1, steal must not lose TID
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(0).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            }
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        // Move TID2 to victim CPU1
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        // Fill thief CPU0 with 64 dummy TIDs (no Kthread backing, just queue entries)
        // Use raw queue API to avoid needing Kthreads; validate is not used for these dummies
        // Instead fill with 64 entries that are not part of scheduler, then attempt steal
        // To keep validation valid, we fill with fake tids that are NOT in scheduler table
        // but we will clear afterwards. For this test we instead fill thief via direct push
        // of valid tids? Simpler: fill thief with 64 copies of a valid TID2 duplicate check
        // will prevent duplicates, so we directly manipulate queue without scheduler threads:
        unsafe {
            let rq0 = crate::arch::x64::cpu_local::cpu_run_queue_mut(0);
            // Ensure empty then fill with distinct dummy tids 1000..1063
            rq0.clear();
            for i in 0..64u32 {
                rq0.push(1000 + i);
            }
            test_eq!(rq0.len(), 64);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 1);
            let stolen = crate::arch::x64::cpu_local::steal_from_cpu_run_queue(1, rq0);
            // Destination full → stolen == 0, victim retains entry, no loss, push-back
            test_eq!(stolen, 0);
            test_eq!(rq0.len(), 64);
            test_eq!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).len(), 1);
            test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
            // Cleanup
            rq0.clear();
            crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
        }
        // Validate scheduler still consistent (TID2 Ready on CPU1, no entry on full thief after clear)
        // Need to re-ensure TID2 on CPU1 for validation
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(1).push(2);
        }
        set_test_current(&mut sched, IDLE_TID);
        let r = sched.validate_runqueue_invariants();
        test_true!(r.is_ok());
        // cleanup
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.cpu = 0;
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });

    test_case!("k19_repeated_steal_requeue_bounce_5_cycles", {
        // 5 cycles: victim→thief→Running(cpu=1)→Ready→bounce to victim, repeat
        unsafe {
            if crate::arch::x64::cpu_local::kprcb_page(0).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            }
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
        let mut sched = Scheduler::new();
        sched.next_tid = 3;
        add_test_thread(&mut sched, 2, 1, 0x400000, PRIORITY_NORMAL, ThreadState::Ready);
        {
            let k = sched.find_kthread_mut(2).unwrap();
            unsafe { crate::arch::x64::cpu_local::remove_from_cpu_run_queue(0, 2); }
            k.cpu = 1;
            Scheduler::enqueue_to_cpu_run_queue(k);
        }
        set_test_current(&mut sched, IDLE_TID);
        for _ in 0..5 {
            // steal via scheduler (migrates ownership to thief)
            let stolen = unsafe { sched.steal_and_migrate(1, 0) };
            test_eq!(stolen, 1);
            test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
            // pop and run on thief
            let tid = unsafe { crate::arch::x64::cpu_local::cpu_run_queue_mut(0).pop().unwrap() };
            test_eq!(tid, 2);
            {
                let k = sched.find_kthread_mut(2).unwrap();
                k.state = ThreadState::Running;
            }
            sched.current_tid = 2;
            test_eq!(sched.find_kthread(2).unwrap().cpu, 0);
            // yield back → should stay on thief (0) after fix
            {
                let k = sched.find_kthread_mut(2).unwrap();
                Scheduler::make_thread_ready(k);
            }
            unsafe {
                test_true!(crate::arch::x64::cpu_local::cpu_run_queue_mut(0).contains(2));
                test_true!(!crate::arch::x64::cpu_local::cpu_run_queue_mut(1).contains(2));
            }
            set_test_current(&mut sched, IDLE_TID);
            let r = sched.validate_runqueue_invariants();
            test_true!(r.is_ok());
        }
        // cleanup
        {
            let k = sched.find_kthread_mut(2).unwrap();
            k.cpu = 0;
            Scheduler::remove_from_run_queue(k);
            k.state = ThreadState::Terminated;
        }
        sched.recycle_thread(2);
        unsafe {
            crate::arch::x64::cpu_local::cpu_run_queue_mut(0).clear();
            if crate::arch::x64::cpu_local::kprcb_page(1).is_some() {
                crate::arch::x64::cpu_local::cpu_run_queue_mut(1).clear();
            }
        }
    });
}
