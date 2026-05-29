use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use spin::Mutex;
use lazy_static::lazy_static;
use crate::handle::{HandleTable, closed_handle_table, default_handle_table};
use crate::kobj::{self, KObjType};

pub const MAX_PROCESSES: usize = 16;
pub const KERNEL_STACK_SIZE: usize = 16384;
const IDLE_STACK_SIZE: usize = 4096;

pub const PRIORITY_HIGH: u8 = 0;
pub const PRIORITY_ABOVE_NORMAL: u8 = 1;
pub const PRIORITY_NORMAL: u8 = 2;
pub const PRIORITY_IDLE: u8 = 3;
pub const PRIORITY_COUNT: u8 = 4;

pub const TIME_SLICES: [u16; PRIORITY_COUNT as usize] = [400, 200, 100, 50];

pub const AGING_INTERVAL_TICKS: u64 = 100;
pub const MAX_STARVATION_TICKS: u64 = 1000;

#[repr(align(16))]
pub struct AlignedKStack(pub [u8; KERNEL_STACK_SIZE]);

static mut IDLE_STACK: [u8; IDLE_STACK_SIZE] = [0; IDLE_STACK_SIZE];

fn idle_task() -> ! {
    loop {
        crate::eventbus::EVENT_BUS.dispatch_pending();
        crate::hal::hlt_once();
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MmapRegion {
    pub base: u64,
    pub len: u64,
    pub prot: u16,      // 1=R, 2=W, 3=RW
    pub flags: u16,     // bit 0: 1=anonymous, 0=file-backed
    pub drive: u8,      // file-backed: drive index
    pub inode: u32,     // file-backed: inode number
    pub file_size: u32, // file-backed: total file size
}

#[repr(C)]
pub struct Process {
    pub rax: u64,  rbx: u64,  rcx: u64,  rdx: u64,
    pub rsi: u64,  rdi: u64,  r8: u64,   r9: u64,
    pub r10: u64,  r11: u64,  r12: u64,  r13: u64,
    pub r14: u64,  r15: u64,  rbp: u64,
    pub rsp: u64,  pub rip: u64,  pub rflags: u64,
    pub pid: u32,  pub state: ProcessState,  pub cpu_ticks: u64,
    pub user_slot: Option<u8>,  pub waiting_for: Option<u32>,
    pub priority: u8,
    pub time_slice_remaining: u16,
    pub ticks_since_scheduled: u64,
    pub cwd_drive: u8,  pub cwd_path: String,
    pub heap_base: u64,  pub heap_break: u64,
    pub kernel_stack_top: u64,
    kernel_stack: Option<Box<AlignedKStack>>,
    pub mmap_regions: Vec<MmapRegion>,
    pub mmap_next: u64,
    pub handle_table: HandleTable,
    pub kobj_id: Option<kobj::KObjId>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked { waiting_for: u32 },
    Terminated,
}

impl fmt::Debug for Process {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Process")
            .field("pid", &self.pid)
            .field("rip", &self.rip)
            .field("rsp", &self.rsp)
            .field("state", &self.state)
            .field("cpu_ticks", &self.cpu_ticks)
            .field("priority", &self.priority)
            .field("time_slice_remaining", &self.time_slice_remaining)
            .field("kernel_stack_top", &self.kernel_stack_top)
            .field("kobj_id", &self.kobj_id)
            .finish()
    }
}

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

impl Process {
    pub fn new_ring0(pid: u32, entry: u64, stack_top: u64, stack: Option<Box<AlignedKStack>>) -> Self {
        let rsp = init_ring0_frame(stack_top, entry);
        Process {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0,
            r14: 0, r15: 0, rbp: 0,
            rsp,
            rip: entry,
            rflags: 0x202,
            pid,
            state: ProcessState::Ready,
            cpu_ticks: 0,
            user_slot: None,
            waiting_for: None,
            priority: PRIORITY_NORMAL,
            time_slice_remaining: TIME_SLICES[PRIORITY_NORMAL as usize],
            ticks_since_scheduled: 0,
            cwd_drive: 2,
            cwd_path: String::from("\\"),
            heap_base: 0,
            heap_break: 0,
            kernel_stack_top: stack_top,
            kernel_stack: stack,
            mmap_regions: Vec::new(),
            mmap_next: crate::arch::x64::paging::MMAP_BASE,
            handle_table: closed_handle_table(),
            kobj_id: None,
        }
    }

    pub fn take_kernel_stack(&mut self) -> Option<Box<AlignedKStack>> {
        self.kernel_stack.take()
    }

    pub fn new_ring3(pid: u32, entry: u64, user_stack_top: u64, slot_idx: u8,
                     cwd_drive: u8, cwd_path: &str, heap_base: u64) -> Self {
        let stack = Box::new(AlignedKStack([0u8; KERNEL_STACK_SIZE]));
        let kernel_stack_top = stack.0.as_ptr() as u64 + KERNEL_STACK_SIZE as u64;
        let rsp = init_ring3_frame(kernel_stack_top, entry, user_stack_top);
        Process {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, r8: 0, r9: 0,
            r10: 0, r11: 0, r12: 0, r13: 0,
            r14: 0, r15: 0, rbp: 0,
            rsp,
            rip: entry,
            rflags: 0x202,
            pid,
            state: ProcessState::Ready,
            cpu_ticks: 0,
            user_slot: Some(slot_idx),
            waiting_for: None,
            priority: PRIORITY_NORMAL,
            time_slice_remaining: TIME_SLICES[PRIORITY_NORMAL as usize],
            ticks_since_scheduled: 0,
            cwd_drive,
            cwd_path: cwd_path.to_string(),
            heap_base,
            heap_break: heap_base,
            kernel_stack_top,
            kernel_stack: Some(stack),
            mmap_regions: Vec::new(),
            mmap_next: crate::arch::x64::paging::MMAP_BASE,
            handle_table: default_handle_table(),
            kobj_id: None,
        }
    }
}

pub struct Scheduler {
    pub processes: [Option<Process>; MAX_PROCESSES],
    pub current_pid: u32,
    pub next_pid: u32,
    timer_ticks: u64,
}

impl Scheduler {
    pub fn new() -> Self {
        const NONE: Option<Process> = None;
        let mut scheduler = Scheduler {
            processes: [NONE; MAX_PROCESSES],
            current_pid: 0,
            next_pid: 1,
            timer_ticks: 0,
        };

        let idle_stack_top = unsafe { IDLE_STACK.as_ptr().add(IDLE_STACK_SIZE) as u64 } & !0xF;
        scheduler.processes[0] = Some(Process::new_ring0(
            0,
            idle_task as *const () as u64,
            idle_stack_top,
            None,
        ));

        scheduler
    }

    pub fn has_non_idle_processes(&self) -> bool {
        self.processes
            .iter()
            .skip(1)
            .any(|p| p.as_ref().is_some_and(|proc| proc.state != ProcessState::Terminated))
    }

    pub fn add_ring3_process(
        &mut self,
        entry: u64,
        user_stack_top: u64,
        slot_idx: u8,
        cwd_drive: u8,
        cwd_path: &str,
        heap_base: u64,
    ) -> u32 {
        // Invariant: PID must be unique
        let pid = self.next_pid;
        for i in 0..MAX_PROCESSES {
            if self.processes[i].is_none() {
                self.next_pid += 1;
                let mut proc = Process::new_ring3(pid, entry, user_stack_top, slot_idx, cwd_drive, cwd_path, heap_base);
                let name = alloc::format!("proc/{}", pid);
                if let Ok(kid) = kobj::kobj_register(KObjType::Process, &name, pid as u64) {
                    proc.kobj_id = Some(kid);
                }
                self.processes[i] = Some(proc);
                crate::trace_sched!(1, pid, 0); // 1 = ADD_PROCESS
                return pid;
            }
        }
        panic!("Process table full");
    }

    pub fn kill_pid(&mut self, pid: u32) -> bool {
        // Unregister from KOBJ before freeing
        for p in self.processes.iter() {
            if let Some(proc) = p {
                if proc.pid == pid {
                    if let Some(kid) = proc.kobj_id {
                        kobj::kobj_unregister(kid);
                    }
                    break;
                }
            }
        }
        let idx = self.processes.iter().position(|p| {
            p.as_ref().is_some_and(|proc| proc.pid == pid && pid > 0)
        });
        if let Some(idx) = idx {
            if let Some(mut proc) = self.processes[idx].take() {
                // Free user slot (code+stack window)
                if let Some(slot) = proc.user_slot.take() {
                    crate::arch::x64::paging::free_user_slot(slot);
                }
                // Free heap pages + heap slot
                if proc.heap_base != 0 {
                    crate::arch::x64::paging::heap_free_range(
                        proc.heap_base,
                        proc.heap_base + crate::arch::x64::paging::PROCESS_HEAP_SIZE,
                    );
                    let heap_idx = ((proc.heap_base - crate::arch::x64::paging::PROCESS_HEAP_BASE)
                        / crate::arch::x64::paging::PROCESS_HEAP_SIZE) as u8;
                    crate::arch::x64::paging::free_heap_slot(heap_idx);
                }
                // Free mmap regions
                for r in proc.mmap_regions.iter() {
                    crate::arch::x64::paging::mmap_free_range(r.base, r.base + r.len);
                }
                // Close all handles (pipes, files, etc.)
                for h in proc.handle_table.iter_mut() {
                    match h.kind {
                        crate::handle::HANDLE_PIPE_READ => {
                            crate::pipe::PIPE_MANAGER.dec_read_ref(h.id as u8);
                        }
                        crate::handle::HANDLE_PIPE_WRITE => {
                            crate::pipe::PIPE_MANAGER.dec_write_ref(h.id as u8);
                        }
                        _ => {}
                    }
                    *h = crate::handle::HandleEntry::closed();
                }
                // proc dropped here: kernel_stack (Box<AlignedKStack>), cwd_path, etc. freed
                crate::trace_sched!(2, pid, 0); // 2 = KILL_PROCESS
                return true;
            }
        }
        false
    }

    /// Remove a terminated process from the scheduler table.
    /// Drops the process, freeing its kernel stack, cwd path, and other owned resources.
    /// External resources (user slot, heap, mmap, pipes) must already be freed.
    /// Returns true if the process was found and removed.
    pub fn recycle_terminated(&mut self, pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        // Unregister from KOBJ before freeing
        for p in self.processes.iter() {
            if let Some(proc) = p {
                if proc.pid == pid {
                    if let Some(kid) = proc.kobj_id {
                        kobj::kobj_unregister(kid);
                    }
                    break;
                }
            }
        }
        let idx = self.processes.iter().position(|p| {
            p.as_ref().is_some_and(|proc| proc.pid == pid)
        });
        if let Some(idx) = idx {
            self.processes[idx] = None;
            crate::trace_sched!(3, pid, 0); // 3 = RECYCLE_SLOT
            true
        } else {
            false
        }
    }

    pub fn wake_waiters(&mut self, pid: u32) {
        for proc in self.processes.iter_mut() {
            if let Some(p) = proc {
                if p.waiting_for == Some(pid) {
                    p.waiting_for = None;
                    if matches!(p.state, ProcessState::Blocked { .. }) {
                        p.state = ProcessState::Ready;
                    }
                }
            }
        }
    }

    pub fn current_process_mut(&mut self) -> Option<&mut Process> {
        let pid = self.current_pid;
        let idx = self.processes.iter().position(|p| p.as_ref().is_some_and(|proc| proc.pid == pid))?;
        self.processes[idx].as_mut()
    }

    #[allow(dead_code)]
    pub fn current_process(&mut self) -> &mut Process {
        let pid = self.current_pid;
        if let Some(idx) = self
            .processes
            .iter()
            .position(|p| p.as_ref().is_some_and(|proc| proc.pid == pid))
        {
            return self.processes[idx].as_mut()
                .expect("Scheduler: process vanished after position check");
        }
        self.current_pid = 0;
        self.processes[0]
            .as_mut()
            .expect("Idle process missing from scheduler")
    }

    /// Reset the current process's time slice to its priority's quantum.
    #[allow(dead_code)]
    pub fn reset_time_slice(&mut self) {
        if let Some(proc) = self.current_process_mut() {
            let idx = (proc.priority as usize).min(PRIORITY_COUNT as usize - 1);
            proc.time_slice_remaining = TIME_SLICES[idx];
            proc.ticks_since_scheduled = 0;
        }
    }

    pub fn set_process_priority(&mut self, pid: u32, priority: u8) -> bool {
        if priority >= PRIORITY_COUNT {
            return false;
        }
        for proc in self.processes.iter_mut().flatten() {
            if proc.pid == pid {
                proc.priority = priority;
                let idx = priority as usize;
                proc.time_slice_remaining = TIME_SLICES[idx];
                proc.ticks_since_scheduled = 0;
                return true;
            }
        }
        false
    }

    /// Apply aging: boost priority of starved Ready processes.
    fn apply_aging(&mut self) {
        for proc in self.processes.iter_mut().flatten() {
            if proc.pid > 0 && proc.state == ProcessState::Ready {
                proc.ticks_since_scheduled = proc.ticks_since_scheduled.saturating_add(AGING_INTERVAL_TICKS);
                if proc.ticks_since_scheduled >= MAX_STARVATION_TICKS && proc.priority > PRIORITY_HIGH {
                    proc.priority -= 1;
                    proc.ticks_since_scheduled = 0;
                    crate::serial_println!("[SCHED] Aging: PID {} boosted to priority {}", proc.pid, proc.priority);
                }
            }
        }
    }

    /// Priority-based schedule: scan from HIGHEST to LOWEST priority.
    /// Within the same priority level, round-robin from (current_pid + 1).
    /// Returns a mutable pointer to the selected Process.
    pub fn schedule(&mut self) -> *mut Process {
        // Invariant: must not be called from inside timer IRQ handler
        if cfg!(feature = "validation") && crate::invariants::is_in_timer_irq() {
            crate::serial_println!("[SCHED] schedule() called from timer IRQ context!");
        }

        let start = (self.current_pid + 1) % self.next_pid.max(1);

        // Scan by priority level: HIGHEST first
        for priority in 0..PRIORITY_COUNT {
            for offset in 0..self.next_pid {
                let check_pid = (start + offset) % self.next_pid.max(1);
                if check_pid == 0 {
                    continue;
                }
                for proc in self.processes.iter_mut() {
                    if let Some(p) = proc {
                        if p.pid == check_pid && p.state == ProcessState::Ready && p.priority == priority {
                            let prev_pid = self.current_pid;
                            self.current_pid = check_pid;
                            p.state = ProcessState::Running;
                            crate::trace_cswitch!(prev_pid, check_pid);
                            return p as *mut Process;
                        }
                    }
                }
            }
        }

        if let Some(idle) = &mut self.processes[0] {
            if idle.pid == 0 && idle.state != ProcessState::Terminated {
                let prev_pid = self.current_pid;
                self.current_pid = 0;
                idle.state = ProcessState::Running;
                crate::trace_cswitch!(prev_pid, 0);
                return idle as *mut Process;
            }
        }
        panic!("No ready processes and idle process is unavailable");
    }

    pub fn on_timer_tick(&mut self) {
        self.timer_ticks += 1;

        // Apply aging every AGING_INTERVAL_TICKS
        if self.timer_ticks % AGING_INTERVAL_TICKS == 0 {
            self.apply_aging();
        }

        let pid = self.current_pid;
        if pid == 0 {
            return;
        }

        if let Some(proc) = self.current_process_mut() {
            if proc.state != ProcessState::Running {
                return;
            }
            proc.cpu_ticks += 1;

            if proc.time_slice_remaining > 0 {
                proc.time_slice_remaining -= 1;
            }

            // Time slice expired: yield CPU
            if proc.time_slice_remaining == 0 {
                proc.state = ProcessState::Ready;
                crate::syscall::NEED_RESCHED.store(true, core::sync::atomic::Ordering::SeqCst);
            }
        }
    }
}

lazy_static! {
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

pub fn current_scheduler() -> &'static Mutex<Scheduler> {
    &SCHEDULER
}

/// Remove a terminated process from the scheduler table.
/// The process's kernel stack (Box<AlignedKStack>) is freed, its slot is recycled,
/// and all owned memory (cwd_path, mmap_regions Vec, handle_table) is released.
///
/// External resources (user slot, heap pages, mmap pages, pipe refcounts) must
/// already have been freed by the caller (e.g. syscall_dispatch for sys_exit).
///
/// Safe to call multiple times — second call is a no-op.
pub fn cleanup_terminated_process(pid: u32) {
    crate::hal::without_interrupts(|| {
        current_scheduler().lock().recycle_terminated(pid);
    });
}

pub fn get_current_cwd() -> (u8, String) {
    let mut lock = SCHEDULER.lock();
    if let Some(proc) = lock.current_process_mut() {
        (proc.cwd_drive, proc.cwd_path.clone())
    } else {
        (2, String::from("\\"))
    }
}

pub fn set_current_cwd(drive: u8, path: &str) {
    let mut lock = SCHEDULER.lock();
    if let Some(proc) = lock.current_process_mut() {
        proc.cwd_drive = drive;
        proc.cwd_path = path.to_string();
    }
}

pub fn current_process_heap_range() -> (u64, u64) {
    let mut lock = SCHEDULER.lock();
    if let Some(proc) = lock.current_process_mut() {
        (proc.heap_base, proc.heap_break)
    } else {
        (0, 0)
    }
}

pub fn set_current_heap_break(new_break: u64) {
    let mut lock = SCHEDULER.lock();
    if let Some(proc) = lock.current_process_mut() {
        proc.heap_break = new_break;
    }
}

pub fn current_process_mmap_regions() -> Vec<MmapRegion> {
    let mut lock = SCHEDULER.lock();
    if let Some(proc) = lock.current_process_mut() {
        proc.mmap_regions.clone()
    } else {
        Vec::new()
    }
}

pub fn add_current_mmap_region(region: MmapRegion) -> Option<u64> {
    let mut lock = SCHEDULER.lock();
    if let Some(proc) = lock.current_process_mut() {
        proc.mmap_regions.push(region);
        proc.mmap_next = region.base + region.len;
        Some(region.base)
    } else {
        None
    }
}

pub fn remove_current_mmap_region(base: u64) -> Option<MmapRegion> {
    let mut lock = SCHEDULER.lock();
    if let Some(proc) = lock.current_process_mut() {
        let idx = proc.mmap_regions.iter().position(|r| r.base == base);
        idx.map(|i| proc.mmap_regions.remove(i))
    } else {
        None
    }
}

pub fn free_current_mmap_pages(base: u64, len: u64) {
    use crate::arch::x64::paging::mmap_free_range;
    mmap_free_range(base, base + len);
}


