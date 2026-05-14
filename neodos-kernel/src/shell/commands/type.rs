use crate::fs::drive_manager::FsInstanceId;
use crate::fs::neodos_fs::ROOT_INODE;
use crate::print;
use crate::println;
use crate::shell::shell::{vfs_path_from_drive_manager, DosShell};

impl<'a> DosShell<'a> {
    pub fn cmd_type(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: TYPE FILENAME");
            return;
        }

        let filename = args[0];

        let dm = self.drive_manager;
        if let Ok((fs_id, vfs)) = vfs_path_from_drive_manager(&dm, filename) {
            if fs_id == FsInstanceId::FAT32_ESP {
                let path_str = match vfs.as_str() {
                    Ok(s) => s,
                    Err(_) => {
                        println!("File not found");
                        return;
                    }
                };
                match &mut self.fat32 {
                    Some(fat) => {
                        let mut buf = [0u8; 4096];
                        match fat.read_file_by_path(self.ata, path_str, &mut buf) {
                            Ok(size) => {
                                if let Ok(s) = core::str::from_utf8(&buf[..size]) {
                                    print!("{}", s);
                                }
                                println!();
                            }
                            Err(_) => println!("File not found"),
                        }
                    }
                    None => println!("Drive A: not available"),
                }
                return;
            }

            if fs_id.0 >= 1 && fs_id.0 <= 3 {
                let path_str = match vfs.as_str() {
                    Ok(s) => s,
                    Err(_) => {
                        println!("File not found");
                        return;
                    }
                };
                let _ = self.with_volume(fs_id, |fs, cache, ata| {
                    let inode = fs.find_file_in_directory(ROOT_INODE, path_str, cache, ata)?;
                    fs.read_file(inode, cache, ata)
                });
                println!();
                return;
            }
        }

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
