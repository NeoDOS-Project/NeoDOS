use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_del(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: DEL filename");
            return;
        }

        let filename = args[0];
        let full_path = self.resolve_absolute_path(filename);

        println!("DELETE not implemented in VFS yet: {}", full_path);
    }
}
