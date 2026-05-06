use core::fmt;
use spin::Mutex;
use lazy_static::lazy_static;

const MAX_PROCESSES: usize = 4;
const IDLE_STACK_SIZE: usize = 4096;

static mut IDLE_STACK: [u8; IDLE_STACK_SIZE] = [0; IDLE_STACK_SIZE];

fn idle_task() -> ! {
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

#[repr(C)]
#[derive(Clone)]
pub struct Process {
    // Saved registers from context switch
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rbp: u64,
    
    // Control registers
    pub rsp: u64,
    pub rip: u64,
    pub rflags: u64,
    
    // Metadata
    pub pid: u32,
    pub state: ProcessState,
    pub cpu_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
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
            .finish()
    }
}

impl Process {
    pub fn new(pid: u32, entry: u64, stack_ptr: u64) -> Self {
        Process {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rbp: 0,
            rsp: stack_ptr,
            rip: entry,
            rflags: 0x202,  // IF bit set (interrupts enabled)
            pid,
            state: ProcessState::Ready,
            cpu_ticks: 0,
        }
    }
}

pub struct Scheduler {
    processes: [Option<Process>; MAX_PROCESSES],
    pub current_pid: u32,
    next_pid: u32,
    timer_ticks: u64,
}

impl Scheduler {
    pub fn new() -> Self {
        let mut scheduler = Scheduler {
            processes: [None, None, None, None],
            current_pid: 0,
            next_pid: 1,
            timer_ticks: 0,
        };
        
        // Create idle process (PID 0) that can always be scheduled
        // This prevents panic when timer fires before any processes are added
        let idle_stack_top = unsafe { IDLE_STACK.as_ptr().add(IDLE_STACK_SIZE) as u64 } & !0xF;
        let idle_rsp = init_interrupt_stack_frame(idle_stack_top, idle_task as u64);

        scheduler.processes[0] = Some(Process::new(0, idle_task as u64, idle_rsp));
        
        scheduler
    }

    pub fn has_non_idle_processes(&self) -> bool {
        self.processes
            .iter()
            .skip(1)
            .any(|p| p.as_ref().is_some_and(|proc| proc.state != ProcessState::Terminated))
    }

    pub fn add_process(&mut self, entry: u64, stack_base: u64) -> u32 {
        for i in 0..MAX_PROCESSES {
            if self.processes[i].is_none() {
                let pid = self.next_pid;
                self.next_pid += 1;
                
                // Initialize stack frame for context switch
                // The stack should look like it was interrupted
                let stack_ptr = init_interrupt_stack_frame(stack_base, entry);

                self.processes[i] = Some(Process::new(pid, entry, stack_ptr));
                return pid;
            }
        }
        panic!("Process table full");
    }

    pub fn current_process_mut(&mut self) -> Option<&mut Process> {
        let pid = self.current_pid;
        let idx = self.processes.iter().position(|p| p.as_ref().is_some_and(|proc| proc.pid == pid))?;
        self.processes[idx].as_mut()
    }

    pub fn current_process(&mut self) -> &mut Process {
        let pid = self.current_pid;
        if let Some(idx) = self
            .processes
            .iter()
            .position(|p| p.as_ref().is_some_and(|proc| proc.pid == pid))
        {
            return self.processes[idx].as_mut().unwrap();
        }

        // Fallback to idle process if the current PID is stale/corrupted.
        self.current_pid = 0;
        self.processes[0]
            .as_mut()
            .expect("Idle process missing from scheduler")
    }

    pub fn schedule(&mut self) -> *mut Process {
        // Round-robin scheduling with fallback to idle process (PID 0)
        let mut count = 0;
        let max_attempts = (self.next_pid as usize) + 10;
        
        loop {
            count += 1;
            
            // Prevent infinite loop - after many attempts, fall back to idle process
            if count > max_attempts {
                // Fallback to PID 0 (idle process) if no other process is ready
                if let Some(idle) = &mut self.processes[0] {
                    if idle.pid == 0 && idle.state != ProcessState::Terminated {
                        self.current_pid = 0;
                        idle.state = ProcessState::Running;
                        return idle as *mut Process;
                    }
                }
                panic!("No ready processes and idle process is unavailable");
            }
            
            self.current_pid += 1;
            if self.current_pid >= self.next_pid {
                self.current_pid = 0;  // Wrap to idle process
            }

            for proc in self.processes.iter_mut() {
                if let Some(p) = proc {
                    if p.pid == self.current_pid && p.state == ProcessState::Ready {
                        p.state = ProcessState::Running;
                        return p as *mut Process;
                    }
                }
            }
        }
    }

    pub fn on_timer_tick(&mut self) {
        self.timer_ticks += 1;
        
        // Every 100 ticks (10ms), switch process
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
                    proc.state = ProcessState::Ready;
                }
            }
        }
    }

    pub fn print_processes(&self) {
        crate::vga::print_str("[");
        for proc in self.processes.iter() {
            if let Some(p) = proc {
                crate::vga::print_str("P");
                crate::vga::print_decimal(p.pid as u64);
                crate::vga::print_str("] ");
            }
        }
        crate::vga::print_str("\r\n");
    }
}

lazy_static! {
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

pub fn init() {
    // Scheduler is initialized via lazy_static
}

pub fn current_scheduler() -> &'static Mutex<Scheduler> {
    &SCHEDULER
}

fn init_interrupt_stack_frame(stack_top: u64, entry: u64) -> u64 {
    // Timer ISR saves 15 registers (RAX..RBP) and ends with IRETQ.
    // We pre-build a compatible stack so the first "restore + iretq" lands at `entry`.
    let mut stack_ptr = stack_top & !0xF;
    unsafe {
        let stack = stack_ptr as *mut u64;

        // Interrupt frame pushed by CPU on entry (no privilege change in ring0):
        // RIP, CS, RFLAGS
        stack.offset(-1).write(0x202); // RFLAGS
        stack.offset(-2).write(0x08);  // CS
        stack.offset(-3).write(entry); // RIP

        // Software-saved regs by timer_handler_asm (15 pushes)
        for j in 4..19 {
            stack.offset(-(j as isize)).write(0);
        }

        // Point to saved RAX (top of software frame)
        stack_ptr -= 18 * 8;
    }
    stack_ptr
}
