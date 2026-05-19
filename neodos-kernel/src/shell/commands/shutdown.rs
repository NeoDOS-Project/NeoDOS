use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_shutdown(&mut self, _args: &[&str]) {
        println!("Shutting down...");
        crate::globals::flush_cache_if_needed();
        
        // ACPI shutdown or just hlt
        println!("System halted.");
        loop {
            crate::hal::hlt_once();
        }
    }
}
