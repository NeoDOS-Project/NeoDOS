use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_rd(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: RD dirname");
            return;
        }

        let dirname = args.last().unwrap();
        
        let parent_inode = self.current_dir_inode;
        let (_, leaf) = self.split_parent_and_leaf(dirname);
        
        match self.fs.delete_directory(parent_inode, leaf, self.cache, self.ata) {
            Ok(_) => {
                let _ = self.cache.flush(self.ata);
                println!("Directory deleted");
            }
            Err(_) => {
                // Check if directory exists but is not empty
                match self.fs.find_dir_in_dir(parent_inode, leaf, self.cache, self.ata) {
                    Ok(_) => println!("Directory not empty"),
                    Err(_) => println!("Path not found"),
                }
            }
        }
    }
}