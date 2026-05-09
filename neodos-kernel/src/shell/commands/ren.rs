use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_rename(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: REN source target");
            return;
        }

        let source = args[0];
        let target = args[1];

        match self.resolve_file_inode(source) {
            Ok(_) => {
                let (parent_path, leaf) = self.split_parent_and_leaf(source);
                let parent_inode = if parent_path.is_empty() {
                    self.current_dir_inode
                } else {
                    match self.resolve_directory_arg(parent_path) {
                        Ok((inode, _, _)) => inode,
                        Err(_) => {
                            println!("Path not found");
                            return;
                        }
                    }
                };

                match self.fs.rename_file(parent_inode, leaf, target, self.cache, self.ata) {
                    Ok(_) => {
                        let _ = self.cache.flush(self.ata);
                        println!("File renamed");
                    }
                    Err(e) => println!("Error: {:?}", e),
                }
            }
            Err(_) => println!("File not found - {}", source),
        }
    }
}