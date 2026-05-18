use alloc::boxed::Box;
use alloc::string::{String, ToString};
use core::fmt;
use core::sync::atomic::AtomicU64;
use spin::Mutex;
use lazy_static::lazy_static;

pub const MAX_PROCESSES: usize = 16;
pub const KERNEL_STACK_SIZE: usize = 16384;
const IDLE_STACK_SIZE: usize = 4096;

#[repr(align(16))]
pub struct AlignedKStack(pub [u8; KERNEL_STACK_SIZE]);

static mut IDLE_STACK: [u8; IDLE_STACK_SIZE] = [0; IDLE_STACK_SIZE];

fn idle_task() -> ! {
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
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
    pub cwd_drive: u8,  pub cwd_path: String,
    pub heap_base: u64,  pub heap_break: u64,
    pub kernel_stack_top: u64,
    kernel_stack: Option<Box<AlignedKStack>>,
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
            .field("kernel_stack_top", &self.kernel_stack_top)
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
            cwd_drive: 2,
            cwd_path: String::from("\\"),
            heap_base: 0,
            heap_break: 0,
            kernel_stack_top: stack_top,
            kernel_stack: stack,
        }
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
            cwd_drive,
            cwd_path: cwd_path.to_string(),
            heap_base,
            heap_break: heap_base,
            kernel_stack_top,
            kernel_stack: Some(stack),
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
                let proc = Process::new_ring3(pid, entry, user_stack_top, slot_idx, cwd_drive, cwd_path, heap_base);
                self.processes[i] = Some(proc);
                crate::trace_sched!(1, pid, 0); // 1 = ADD_PROCESS
                return pid;
            }
        }
        panic!("Process table full");
    }

    pub fn kill_pid(&mut self, pid: u32) -> bool {
        for proc in self.processes.iter_mut() {
            if let Some(p) = proc {
                if p.pid == pid && pid > 0 {
                    p.state = ProcessState::Terminated;
                    if let Some(slot) = p.user_slot {
                        crate::arch::x64::paging::free_user_slot(slot);
                        p.user_slot = None;
                    }
                    crate::trace_sched!(2, pid, 0); // 2 = KILL_PROCESS
                    return true;
                }
            }
        }
        false
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
            return self.processes[idx].as_mut().unwrap();
        }
        self.current_pid = 0;
        self.processes[0]
            .as_mut()
            .expect("Idle process missing from scheduler")
    }

    pub fn schedule(&mut self) -> *mut Process {
        let start = (self.current_pid + 1) % self.next_pid.max(1);
        let end = start + self.next_pid;

        // Invariant: must not be called from inside timer IRQ handler
        if cfg!(feature = "validation") && crate::invariants::is_in_timer_irq() {
            crate::serial_println!("[SCHED] schedule() called from timer IRQ context!");
        }

        for pid in start..end {
            let check_pid = pid % self.next_pid;
            if check_pid == 0 {
                continue;
            }
            for proc in self.processes.iter_mut() {
                if let Some(p) = proc {
                    if p.pid == check_pid && p.state == ProcessState::Ready {
                        let prev_pid = self.current_pid;
                        self.current_pid = check_pid;
                        p.state = ProcessState::Running;
                        crate::trace_cswitch!(prev_pid, check_pid);
                        return p as *mut Process;
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
        if self.timer_ticks % 100 == 0 {
            if let Some(current) = self.processes.iter_mut().find(|p| {
                if let Some(proc) = p {
                    proc.pid == self.current_pid
                } else {
                    false
                }
            }) {
                if let Some(proc) = current {
                    proc.cpu_ticks += 1;
                    if proc.state == ProcessState::Running {
                        proc.state = ProcessState::Ready;
                    }
                }
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

pub static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
