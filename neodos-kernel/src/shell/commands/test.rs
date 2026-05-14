use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_test(&mut self, _args: &[&str]) {
        println!("Running kernel self-tests...");
        crate::testing::run_all();
        
        println!("\r\nKernel tests complete. Executing user-mode SYSTEST.BIN...");
        self.cmd_run(&["SYSTEST.BIN"]);
    }
}
