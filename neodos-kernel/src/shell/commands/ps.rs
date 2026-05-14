use crate::println;
use crate::scheduler::{current_scheduler, ProcessState, MAX_PROCESSES};
use crate::shell::shell::DosShell;

#[derive(Copy, Clone)]
struct ProcSnap {
    pid: u32,
    state: ProcessState,
    user_slot: Option<u8>,
    rip: u64,
    rsp: u64,
    cpu_ticks: u64,
}

impl DosShell {
    pub fn cmd_ps(&mut self) {
        let mut snap = [None; MAX_PROCESSES];

        {
            let scheduler = current_scheduler().lock();
            for (i, proc) in scheduler.processes.iter().enumerate() {
                if let Some(p) = proc {
                    snap[i] = Some(ProcSnap {
                        pid: p.pid,
                        state: p.state,
                        user_slot: p.user_slot,
                        rip: p.rip,
                        rsp: p.rsp,
                        cpu_ticks: p.cpu_ticks,
                    });
                }
            }
        }

        let has_any = snap.iter().any(|e| e.is_some());
        if !has_any {
            println!("No processes");
            return;
        }

        println!("PID  STATE       WAIT SLOT      RIP               RSP               TICKS");
        for entry in snap.iter() {
            if let Some(p) = entry {
                let (state_str, wait_str, slot_str) = format_proc_info(p);
                println!("{:>3}  {:9}  {:>4} {:>4}  0x{:016x}  0x{:016x}  {}",
                    p.pid, state_str, wait_str, slot_str, p.rip, p.rsp, p.cpu_ticks);
            }
        }
    }
}

fn format_proc_info(p: &ProcSnap) -> (&'static str, &'static str, &'static str) {
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
