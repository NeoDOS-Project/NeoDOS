use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_help(&mut self, _args: &[&str]) {
        println!("Ring 0 HELP is a stub.");
        println!("Use HELP in the Ring 3 shell (neoshell) for full command help.");
        println!("Alternatively, run any .NXE with /? for its help text.");
        println!();
        println!("  Example:  run CLS.NXE");
        println!("  Example:  run COPY.NXE /?");
    }
}
