use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_rd(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: RD dirname");
            return;
        }
        println!("RMDIR not implemented in VFS yet");
    }
}