use crate::println;
use crate::shell::shell::DosShell;
use crate::fs::vfs::MODE_DIR;

impl DosShell {
    pub fn cmd_dir(&mut self, args: &[&str]) {
        let path_arg = args.first().copied().unwrap_or(".");
        let full_path = self.resolve_absolute_path(path_arg);

        println!(" Directory of {}", full_path);
        println!();

        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(&full_path) {
                Ok((drive_idx, node)) => {
                    if (node.mode & MODE_DIR) == 0 {
                        println!("  Not a directory");
                        return;
                    }

                    let mut i = 0;
                    loop {
                        match vfs.readdir(drive_idx, node.inode, i) {
                            Ok(Some(entry)) => {
                                let type_str = if (entry.node.mode & MODE_DIR) != 0 { "<DIR>" } else { "     " };
                                println!("  {:<12} {:>5} {:>10}", entry.name, type_str, entry.node.size);
                                i += 1;
                            }
                            Ok(None) => break,
                            Err(e) => {
                                println!("  Error: {:?}", e);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("  The system cannot find the path specified ({:?})", e);
                }
            }
        });
    }
}
