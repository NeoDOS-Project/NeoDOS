use crate::println;
use crate::shell::shell::{vfs_path_from_drive_manager, DosShell, ShellPathError};

impl<'a> DosShell<'a> {
    pub fn cmd_cd(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!(
                "{}",
                core::str::from_utf8(&self.current_dir[..self.current_dir_len]).unwrap_or("\\")
            );
            return;
        }

        let path = args[0];
        let b = path.as_bytes();

        if b.len() == 2 && b[1] == b':' {
            let letter = match path.chars().next() {
                Some(c) => c,
                None => {
                    println!("Invalid drive specification");
                    return;
                }
            };
            match self.drive_manager.get(letter) {
                Some(d) => {
                    self.current_drive = d.letter;
                }
                None => println!("Invalid drive specification"),
            }
            return;
        }

        let dm = self.drive_manager;
        let vfs = match vfs_path_from_drive_manager(&dm, path) {
            Err(ShellPathError::UnsupportedVolume) => {
                println!("Drive not ready");
                return;
            }
            Err(_) => {
                println!("Invalid path");
                return;
            }
            Ok(v) => v,
        };

        match self.resolve_directory_arg_from_vfs(vfs) {
            Ok((new_inode, new_path, new_path_len)) => {
                if b.len() >= 2 && b[1] == b':' {
                    if let Some(c) = path.chars().next() {
                        if let Some(d) = self.drive_manager.get(c) {
                            self.current_drive = d.letter;
                        }
                    }
                }
                self.current_dir_inode = new_inode;
                self.current_dir.fill(0);
                self.current_dir[..new_path_len].copy_from_slice(&new_path[..new_path_len]);
                self.current_dir_len = new_path_len;
            }
            Err(_) => println!("The system cannot find the path specified"),
        }
    }
}

