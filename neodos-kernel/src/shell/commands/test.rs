use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_test(&mut self, _args: &[&str]) {
        println!("Running kernel self-tests...");
        let (passed, failed) = crate::testing::run_all();

        if failed == 0 {
            println!("\r\nAll {} kernel tests passed.", passed);
        } else {
            println!("\r\n{} passed, {} failed.", passed, failed);
        }

        println!("Executing user-mode SYSTEST.BIN...");
        self.cmd_run(&["SYSTEST.BIN"]);
    }
}
