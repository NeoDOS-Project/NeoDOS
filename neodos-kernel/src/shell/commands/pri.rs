use crate::println;
use crate::scheduler::current_scheduler;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_pri(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: PRI <pid> <priority>");
            println!("  Priority levels: 0=HIGH, 1=ABOVE_NORMAL, 2=NORMAL, 3=IDLE");
            return;
        }

        let pid: u32 = match args[0].parse() {
            Ok(n) => n,
            Err(_) => {
                println!("Invalid PID: {}", args[0]);
                return;
            }
        };

        let priority: u8 = match args[1].parse() {
            Ok(n) if n <= 3 => n,
            _ => {
                println!("Invalid priority: {} (use 0-3)", args[1]);
                return;
            }
        };

        let mut scheduler = current_scheduler().lock();
        if scheduler.set_process_priority(pid, priority) {
            let names = ["HIGH", "ABOVE_NORMAL", "NORMAL", "IDLE"];
            println!("Process {} priority set to {} ({})", pid, priority, names[priority as usize]);
        } else {
            println!("Process {} not found.", pid);
        }
    }
}
