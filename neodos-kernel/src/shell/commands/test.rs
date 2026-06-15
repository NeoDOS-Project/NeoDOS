use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_test(&mut self, args: &[&str]) {
        println!("Running kernel self-tests...");
        let (passed, failed) = crate::testing::run_all();

        if failed == 0 {
            println!("\r\nAll {} kernel tests passed.", passed);
        } else {
            println!("\r\n{} passed, {} failed.", passed, failed);
            println!("Kernel tests failed — skipping user-mode tests.");
            return;
        }

        let user_tests: &[&str] = if args.contains(&"quick") {
            &["SYSTEST.NXE"]
        } else {
            &["HELLO.NXE", "SYSTEST.NXE", "FILETEST.NXE", "ALLTEST.NXE", "CPUTEST.NXE", "TEST.NXE", "C:\\SYSTEM\\BIN\\CPUINFO.NXE", "C:\\SYSTEM\\BIN\\DIR.NXE"]
        };

        for bin in user_tests {
            println!("\r\n--- Running {} ---", bin);
            self.cmd_run(&[bin]);
            println!("--- {} done. ---", bin);
        }

        println!("\r\nALL_TESTS_COMPLETE");
    }
}
