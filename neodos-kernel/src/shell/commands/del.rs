use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_del(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: DEL filename");
            return;
        }

        let filename = args.last().unwrap();
        
        let parent_inode = self.current_dir_inode;
        let (_, leaf) = self.split_parent_and_leaf(filename);
        
        match self.resolve_file_inode(filename) {
            Ok(inode_num) => {
                let result = self.fs.delete_file_by_inode(parent_inode, leaf, inode_num, self.cache, self.ata);
                
                match result {
                    Ok(_) => {
                        let _ = self.cache.flush(self.ata);
                        println!("File deleted");
                    }
                    Err(e) => {
                        println!("Delete error: {:?}", e);
                    }
                }
            }
            Err(_) => {
                println!("File not found");
            }
        }
    }
}