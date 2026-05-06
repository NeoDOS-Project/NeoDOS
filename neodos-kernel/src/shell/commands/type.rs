use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub(super) fn cmd_type(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: TYPE FILENAME");
            return;
        }

        let filename = args[0];
        match self.resolve_file_inode(filename) {
            Ok(inode_num) => {
                if let Err(e) = self.fs.read_file(inode_num, self.cache, self.ata) {
                    println!("Error reading file: {:?}", e);
                }
                println!();
            }
            Err(_) => println!("File not found"),
        }
    }
}

