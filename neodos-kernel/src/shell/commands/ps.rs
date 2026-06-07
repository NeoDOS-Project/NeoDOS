use crate::println;
use crate::scheduler::{current_scheduler, ThreadState, MAX_THREADS};
use crate::shell::shell::DosShell;

#[derive(Copy, Clone)]
struct ThSnap {
    tid: u32,
    pid: u32,
    state: ThreadState,
    rip: u64,
    rsp: u64,
    cpu_ticks: u64,
    priority: u8,
    ticks_since_scheduled: u64,
}

impl DosShell {
    pub fn cmd_ps(&mut self) {
        let mut snap = [None; MAX_THREADS];

        {
            let scheduler = current_scheduler().lock();
            for (i, th) in scheduler.kthreads.iter().enumerate() {
                if let Some(k) = th {
                    snap[i] = Some(ThSnap {
                        tid: k.tid,
                        pid: k.pid,
                        state: k.state,
                        rip: k.rip,
                        rsp: k.rsp,
                        cpu_ticks: k.cpu_ticks,
                        priority: k.priority,
                        ticks_since_scheduled: k.ticks_since_scheduled,
                    });
                }
            }
        }

        let has_any = snap.iter().any(|e| e.is_some());
        if !has_any {
            println!("No threads");
            return;
        }

        println!("TID  PID  STATE       PRI      RIP               RSP               TICKS");
        for entry in snap.iter() {
            if let Some(t) = entry {
                let (state_str, pri_str) = format_th_info(t);
                println!("{:>3}  {:>3}  {:9}  {:>3}  0x{:016x}  0x{:016x}  {}",
                    t.tid, t.pid, state_str, pri_str, t.rip, t.rsp, t.cpu_ticks);
            }
        }
    }
}

fn format_th_info(t: &ThSnap) -> (&'static str, &'static str) {
    let state_str = match t.state {
        ThreadState::Ready => "Ready",
        ThreadState::Running => "Running",
        ThreadState::Blocked { .. } => "Blocked",
        ThreadState::Terminated => "Term.",
    };

    let pri_str = match t.priority {
        0 => "H",
        1 => "AN",
        2 => "N",
        3 => "I",
        _ => "?",
    };

    (state_str, pri_str)
}
