use crate::println;
use crate::shell::shell::DosShell;
use crate::fs::vfs::MODE_DIR;
use crate::fs::neodos_fs::{PERM_R, PERM_W, PERM_X, PERM_S, PERM_D};

fn fmt_perms(mode: u16) -> [u8; 5] {
    let mut p = [b'-'; 5];
    if mode & PERM_R != 0 { p[0] = b'R'; }
    if mode & PERM_W != 0 { p[1] = b'W'; }
    if mode & PERM_X != 0 { p[2] = b'X'; }
    if mode & PERM_S != 0 { p[3] = b'S'; }
    if mode & PERM_D != 0 { p[4] = b'D'; }
    p
}

impl DosShell {
    pub fn cmd_dir(&mut self, args: &[&str]) {
        let path_arg = args.first().copied().unwrap_or(".");
        let full_path = self.resolve_absolute_path(path_arg);

        println!(" Directory of {}", full_path);
        println!();

        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(&full_path) {
                Ok((drive_idx, node)) => {
                    if (node.mode & MODE_DIR) == 0 {
                        println!("  Not a directory");
                        return;
                    }

                    let mut i = 0;
                    loop {
                        match vfs.readdir(drive_idx, node.inode, i) {
                            Ok(Some(entry)) => {
                                let type_str = if (entry.node.mode & MODE_DIR) != 0 { "<DIR>" } else { "     " };
                                let perms = fmt_perms(entry.node.mode);
                                let perms_str = core::str::from_utf8(&perms).unwrap_or("-----");
                                println!("  {:<12} {:>5} {} {:>10}", entry.name, type_str, perms_str, entry.node.size);
                                i += 1;
                            }
                            Ok(None) => break,
                            Err(e) => {
                                println!("  Error: {:?}", e);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("  The system cannot find the path specified ({:?})", e);
                }
            }
        });
    }
}
