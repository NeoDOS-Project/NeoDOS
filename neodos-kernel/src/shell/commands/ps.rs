use crate::println;
use crate::scheduler::{current_scheduler, ProcessState};
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_ps(&mut self) {
        let scheduler = current_scheduler().lock();

        if scheduler.processes.iter().all(|p| p.is_none()) {
            println!("No processes");
            return;
        }

        println!("PID  STATE      RIP               RSP               TICKS");
        for proc in scheduler.processes.iter() {
            if let Some(p) = proc {
                let state_str = match p.state {
                    ProcessState::Ready => "Ready",
                    ProcessState::Running => "Running",
                    ProcessState::Blocked => "Blocked",
                    ProcessState::Terminated => "Terminated",
                };
                println!("{:>3}  {:9}  0x{:016x}  0x{:016x}  {}",
                    p.pid, state_str, p.rip, p.rsp, p.cpu_ticks);
            }
        }
    }
}
