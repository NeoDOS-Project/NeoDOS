use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub(super) fn cmd_copy(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: COPY SRC DST");
            return;
        }

        let src = args[0];
        let dst = args[1];

        match self.resolve_file_inode(src) {
            Ok(src_inode_num) => {
                let mut buf = [0u8; 16384];
                match self.fs.read_file_to_buf(src_inode_num, &mut buf, self.cache, self.ata) {
                    Ok(read) => {
                        let (parent_path, leaf) = self.split_parent_and_leaf(dst);
                        if leaf.is_empty() || leaf == "." || leaf == ".." {
                            println!("Invalid destination path");
                            return;
                        }

                        let parent_inode = if parent_path.is_empty() {
                            self.current_dir_inode
                        } else {
                            match self.resolve_directory_arg(parent_path) {
                                Ok((inode, _, _)) => inode,
                                Err(_) => {
                                    println!("The system cannot find the destination path");
                                    return;
                                }
                            }
                        };

                        match self
                            .fs
                            .create_file_at(parent_inode, leaf, self.cache, self.ata)
                        {
                            Ok(dst_inode_num) => {
                                if let Ok(written) = self
                                    .fs
                                    .write_file(dst_inode_num, &buf[..read], self.cache, self.ata)
                                {
                                    println!("{} bytes copied", written);
                                } else {
                                    println!("Error writing destination file");
                                }
                            }
                            Err(e) => println!("Error creating destination file: {:?}", e),
                        }
                    }
                    Err(e) => println!("Error reading source file: {:?}", e),
                }
            }
            Err(_) => println!("Source file not found"),
        }
    }
}

