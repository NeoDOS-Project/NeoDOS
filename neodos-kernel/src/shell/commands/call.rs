use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_call(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: CALL batchfile");
            return;
        }

        let full_path = self.resolve_absolute_path(args[0]);

        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(&full_path) {
                Ok((drive_idx, node)) => {
                    let mut buf = alloc::vec::Vec::new();
                    buf.resize(node.size as usize, 0);
                    if let Ok(read) = vfs.read(drive_idx, node.inode, 0, &mut buf) {
                        if let Ok(content) = core::str::from_utf8(&buf[..read]) {
                            self.execute_batch(content);
                        }
                    }
                }
                Err(_) => {
                    println!("Batch file not found");
                }
            }
        });
    }
}
