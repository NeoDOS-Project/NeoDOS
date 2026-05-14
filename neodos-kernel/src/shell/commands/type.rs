use crate::println;
use crate::print;
use crate::shell::shell::DosShell;
use crate::fs::vfs::MODE_FILE;

impl DosShell {
    pub fn cmd_type(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: TYPE [drive:][path]filename");
            return;
        }

        let full_path = self.resolve_absolute_path(args[0]);

        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(&full_path) {
                Ok((drive_idx, node)) => {
                    if (node.mode & MODE_FILE) == 0 {
                        println!("Not a file");
                        return;
                    }

                    let mut offset = 0;
                    let mut buf = [0u8; 512];
                    loop {
                        match vfs.read(drive_idx, node.inode, offset, &mut buf) {
                            Ok(0) => break,
                            Ok(n) => {
                                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                                    print!("{}", s);
                                }
                                offset += n as u64;
                            }
                            Err(e) => {
                                println!("\nError reading file: {:?}", e);
                                break;
                            }
                        }
                    }
                    println!();
                }
                Err(_) => {
                    println!("File not found");
                }
            }
        });
    }
}
