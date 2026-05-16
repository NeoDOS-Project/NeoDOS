use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_rd(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: RD dirname");
            return;
        }

        let dirname = args[0];
        let full_path = self.resolve_absolute_path(dirname);

        crate::globals::with_vfs(|vfs| {
            match vfs.remove_dir(&full_path) {
                Ok(_) => {},
                Err(e) => println!("  Error: {:?}", e),
            }
        });
    }
}
