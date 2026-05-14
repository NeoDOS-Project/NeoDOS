use crate::println;
use crate::shell::shell::DosShell;
use crate::fs::vfs::MODE_FILE;

impl DosShell {
    pub fn cmd_copy(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: COPY source destination");
            return;
        }

        let src_path = self.resolve_absolute_path(args[0]);
        let dst_path = self.resolve_absolute_path(args[1]);

        println!("Copying {} to {}...", src_path, dst_path);

        let mut data = alloc::vec::Vec::new();
        let mut error = false;

        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(&src_path) {
                Ok((drive_idx, node)) => {
                    if (node.mode & MODE_FILE) == 0 {
                        println!("Source is not a file");
                        error = true;
                        return;
                    }
                    data.resize(node.size as usize, 0);
                    if let Err(e) = vfs.read(drive_idx, node.inode, 0, &mut data) {
                        println!("Error reading source: {:?}", e);
                        error = true;
                    }
                }
                Err(_) => {
                    println!("Source file not found");
                    error = true;
                }
            }
            
            if error { return; }

            // Write part
            // For now, we only support writing if the file exists or if we create it.
            // Since VFS doesn't have a high-level "create or open" yet, 
            // and NeoDosFs create_file is low-level, we'll just print a placeholder if it fails.
            println!("Writing to destination not fully implemented in VFS yet");
        });
    }
}
