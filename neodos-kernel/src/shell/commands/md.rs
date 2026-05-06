use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub(super) fn cmd_md(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: MD DIRNAME");
            return;
        }

        let dirname = args[0];
        let (parent_path, leaf) = self.split_parent_and_leaf(dirname);
        if leaf.is_empty() || leaf == "." || leaf == ".." {
            println!("Invalid directory name");
            return;
        }

        let parent_inode = if parent_path.is_empty() {
            self.current_dir_inode
        } else {
            match self.resolve_directory_arg(parent_path) {
                Ok((inode, _, _)) => inode,
                Err(_) => {
                    println!("The system cannot find the path specified");
                    return;
                }
            }
        };

        match self
            .fs
            .create_directory_at(parent_inode, leaf, self.cache, self.ata)
        {
            Ok(_) => println!("Directory created"),
            Err(e) => println!("Error creating directory: {:?}", e),
        }
    }
}

