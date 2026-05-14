use crate::fs::drive_manager::FsInstanceId;
use crate::fs::neodos_fs::ROOT_INODE;
use crate::println;
use crate::shell::shell::{vfs_path_from_drive_manager, DosShell};

impl<'a> DosShell<'a> {
    pub fn cmd_dir(&mut self, args: &[&str]) {
        let path_arg = args.first().copied();

        let (fs_id, vfs_opt) = match path_arg {
            Some(p) => match vfs_path_from_drive_manager(&self.drive_manager, p) {
                Ok((id, vfs)) => (id, Some(vfs)),
                Err(_) => (FsInstanceId::PRIMARY, None),
            },
            None => (FsInstanceId::PRIMARY, None),
        };

        if fs_id == FsInstanceId::FAT32_ESP {
            if let Some(vfs) = vfs_opt {
                let path_str = vfs.as_str().unwrap_or("/");
                let drive_letter = self.dir_display_drive(path_arg);
                println!(" Directory of {}:{}", drive_letter, path_str);
                println!();

                match &mut self.fat32 {
                    Some(fat) => {
                        if let Err(e) = fat.list_directory(self.ata, path_str) {
                            println!("Error reading directory: {:?}", e);
                        }
                    }
                    None => println!("Drive A: not available"),
                }
            }
            return;
        }

        if fs_id.0 >= 1 && fs_id.0 <= 3 {
            let path_str = match &vfs_opt {
                Some(vfs) => vfs.as_str().unwrap_or("\\"),
                None => "\\",
            };
            let drive_letter = self.dir_display_drive(path_arg);
            println!(" Directory of {}:{}", drive_letter, path_str);
            println!();
            let _ = self.with_volume(fs_id, |fs, cache, ata| {
                let (inode, _, _) = fs
                    .resolve_directory_path(ROOT_INODE, b"\\", 1, path_str, cache, ata)?;
                fs.list_directory(inode, cache, ata)
            });
            return;
        }

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
