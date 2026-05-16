use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_rename(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: REN oldname newname");
            return;
        }

        let old_path = self.resolve_absolute_path(args[0]);
        let new_name = args[1];

        crate::globals::with_vfs(|vfs| {
            match vfs.rename(&old_path, new_name) {
                Ok(_) => {},
                Err(e) => println!("  Error: {:?}", e),
            }
        });
    }
}
