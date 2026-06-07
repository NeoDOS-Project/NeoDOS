use crate::println;
use crate::scheduler::current_scheduler;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_kill(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: KILL <pid>");
            return;
        }

        let pid: u32 = match args[0].parse() {
            Ok(n) => n,
            Err(_) => {
                println!("Invalid PID: {}", args[0]);
                return;
            }
        };

        if pid == 0 {
            println!("Cannot kill the idle process.");
            return;
        }

        let mut scheduler = current_scheduler().lock();
        if scheduler.kill_pid(pid) {
            scheduler.wake_waiters(pid);
            println!("Process {} terminated.", pid);
        } else {
            println!("Process {} not found.", pid);
        }
    }
}
