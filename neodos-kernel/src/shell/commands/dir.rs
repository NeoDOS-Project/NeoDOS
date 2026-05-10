use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_dir(&mut self, args: &[&str]) {
        let path_arg = args.first().copied();
        let (dir_inode, dir_path, dir_path_len) = if args.is_empty() {
            let mut path = [0u8; 128];
            path[..self.current_dir_len].copy_from_slice(&self.current_dir[..self.current_dir_len]);
            (self.current_dir_inode, path, self.current_dir_len)
        } else {
            match self.resolve_directory_arg(args[0]) {
                Ok(resolved) => resolved,
                Err(_) => {
                    println!("The system cannot find the path specified");
                    return;
                }
            }
        };

        let drive_letter = self.dir_display_drive(path_arg);
        let path_str = core::str::from_utf8(&dir_path[..dir_path_len]).unwrap_or("\\");
        println!(" Directory of {}:{}", drive_letter, path_str);
        println!();

        if let Err(e) = self.fs.list_directory(dir_inode, self.cache, self.ata) {
            println!("Error reading directory: {:?}", e);
        }
    }
}

