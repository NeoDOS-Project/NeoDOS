use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_call(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: CALL FILENAME.BAT");
            return;
        }

        let filename = args[0];
        match self.resolve_file_inode(filename) {
            Ok(inode_num) => {
                let mut buf = [0u8; 4096];
                match self.fs.read_file_to_buf(inode_num, &mut buf, self.cache, self.ata) {
                    Ok(read) => {
                        if let Ok(content) = core::str::from_utf8(&buf[..read]) {
                            self.execute_batch(content);
                        } else {
                            println!("Error: Batch file is not valid UTF-8");
                        }
                    }
                    Err(e) => println!("Error reading batch file: {:?}", e),
                }
            }
            Err(_) => println!("File not found"),
        }
    }
}

