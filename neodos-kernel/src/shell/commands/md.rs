use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_md(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: MD DIRNAME");
            return;
        }

        let dirname = args[0];
        let full_path = self.resolve_absolute_path(dirname);

        crate::globals::with_vfs(|vfs| {
            match vfs.mkdir(&full_path) {
                Ok(_) => println!("Directory created"),
                Err(e) => println!("Error creating directory: {:?}", e),
            }
        });
    }
}
