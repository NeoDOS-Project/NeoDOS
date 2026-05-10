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

        println!("PID  STATE       WAIT SLOT      RIP               RSP               TICKS");
        for proc in scheduler.processes.iter() {
            if let Some(p) = proc {
                let (state_str, wait_str, slot_str) = format_proc_info(p);

                println!("{:>3}  {:9}  {:>4} {:>4}  0x{:016x}  0x{:016x}  {}",
                    p.pid, state_str, wait_str, slot_str, p.rip, p.rsp, p.cpu_ticks);
            }
        }
    }
}

/// Format a process's state/wait/slot into static strings.
fn format_proc_info(p: &crate::scheduler::Process) -> (&'static str, &'static str, &'static str) {
    let state_str = match p.state {
        ProcessState::Ready => "Ready",
        ProcessState::Running => "Running",
        ProcessState::Blocked { .. } => "Blocked",
        ProcessState::Terminated => "Term.",
    };

    let wait_str = match p.state {
        ProcessState::Blocked { .. } => "yes",
        _ => "-",
    };

    let slot_str = match p.user_slot {
        Some(_) => "yes",
        None => "-",
    };

    (state_str, wait_str, slot_str)
}
