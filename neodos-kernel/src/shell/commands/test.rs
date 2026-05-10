use crate::print;
use crate::println;
use crate::serial_println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_test(&mut self, _args: &[&str]) {
        println!("Running NeoDOS self-tests...");
        println!();
        let (passed, failed) = crate::testing::run_all();
        println!();
        if failed == 0 {
            println!("All {} kernel tests passed.", passed);
            println!();
            println!("Running user-mode syscall tests (SYSTEST.BIN)...");
            println!();
            self.cmd_run(&["SYSTEST.BIN"]);
        } else {
            print!("{} passed, {} failed.", passed, failed);
            if failed > 0 {
                print!("  Check serial log for details.");
            }
            println!();
        }
    }
}
