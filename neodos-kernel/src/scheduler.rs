use core::fmt;
use spin::Mutex;
use lazy_static::lazy_static;

const MAX_PROCESSES: usize = 4;

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
        Scheduler {
            processes: [None, None, None, None],
            current_pid: 0,
            next_pid: 1,
            timer_ticks: 0,
        }
    }

    pub fn add_process(&mut self, entry: u64, stack_base: u64) -> u32 {
        for i in 0..MAX_PROCESSES {
            if self.processes[i].is_none() {
                let pid = self.next_pid;
                self.next_pid += 1;
                
                // Initialize stack frame for context switch
                // The stack should look like it was interrupted
                let mut stack_ptr = stack_base;
                unsafe {
                    let stack = stack_ptr as *mut u64;
                    
                    // Hardware frame (SS, RSP, RFLAGS, CS, RIP)
                    stack.offset(-1).write(0x10);        // SS
                    stack.offset(-2).write(stack_base);   // RSP
                    stack.offset(-3).write(0x202);       // RFLAGS (Interrupts enabled)
                    stack.offset(-4).write(0x08);        // CS
                    stack.offset(-5).write(entry);       // RIP
                    
                    // Software frame (RAX to RBP, 15 registers)
                    for j in 6..21 {
                        stack.offset(-j).write(0);
                    }
                    
                    stack_ptr -= 20 * 8; // Point to RAX
                }

                self.processes[i] = Some(Process::new(pid, entry, stack_ptr));
                return pid;
            }
        }
        panic!("Process table full");
    }

    pub fn current_process(&mut self) -> &mut Process {
        for proc in self.processes.iter_mut() {
            if let Some(p) = proc {
                if p.pid == self.current_pid {
                    return p;
                }
            }
        }
        panic!("Current process not found: {}", self.current_pid);
    }

    pub fn schedule(&mut self) -> *mut Process {
        // Simple round-robin: next Ready process
        let mut count = 0;
        loop {
            self.current_pid += 1;
            if self.current_pid >= self.next_pid {
                self.current_pid = 1;
            }
            
            count += 1;
            if count > 10 {
                panic!("No ready processes");
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
