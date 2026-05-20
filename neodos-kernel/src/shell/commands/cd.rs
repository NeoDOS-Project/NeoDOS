use crate::println;
use crate::shell::shell::DosShell;
use crate::fs::vfs::MODE_DIR;
use alloc::string::String;
use alloc::vec::Vec;

fn normalize_path(path: &str) -> String {
    let mut comps: Vec<&str> = Vec::new();
    for comp in path.split('\\') {
        match comp {
            "" | "." => continue,
            ".." => { comps.pop(); }
            c => comps.push(c),
        }
    }
    if comps.is_empty() {
        String::from("\\")
    } else {
        let mut result = String::from("\\");
        result.push_str(&comps.join("\\"));
        result
    }
}

impl DosShell {
    pub fn cmd_cd(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("{}:{}> ", self.current_drive, self.current_dir);
            return;
        }

        let arg = args[0];
        
        // Handle drive change (e.g., "A:")
        if arg.len() == 2 && arg.ends_with(':') {
            let drive = (arg.as_bytes()[0] as char).to_ascii_uppercase();
            crate::globals::with_vfs(|vfs| {
                match vfs.resolve_path(&alloc::format!("{}:\\", drive)) {
                    Ok(_) => {
                        self.current_drive = drive;
                        self.current_dir = String::from("\\");
                        self.current_dir_inode = 0;
                    }
                    Err(_) => {
                        println!("Invalid drive");
                    }
                }
            });
            return;
        }

        let full_path = self.resolve_absolute_path(arg);
        
        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(&full_path) {
                Ok((_drive_idx, node)) => {
                    if (node.mode & MODE_DIR) == 0 {
                        println!("Not a directory");
                        return;
                    }
                    
                    if let Some(colon_idx) = full_path.find(':') {
                        self.current_drive = (full_path.as_bytes()[0] as char).to_ascii_uppercase();
                        let raw = &full_path[colon_idx + 1..];
                        self.current_dir = normalize_path(raw);
                        self.current_dir_inode = node.inode;
                    }
                }
                Err(_) => {
                    println!("Invalid directory");
                }
            }
        });
    }
}
