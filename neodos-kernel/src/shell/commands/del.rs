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

        crate::globals::with_vfs(|vfs| {
            match vfs.remove_file(&full_path) {
                Ok(_) => {},
                Err(e) => println!("  Error: {:?}", e),
            }
        });
    }
}
