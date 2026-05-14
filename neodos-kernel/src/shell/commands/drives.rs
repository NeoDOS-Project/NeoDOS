use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_drives(&mut self) {
        println!("Mounted drives:");
        crate::globals::with_vfs(|vfs| {
            for i in 0..26 {
                if vfs.drives[i].is_some() {
                    let drive = (b'A' + i as u8) as char;
                    match vfs.volume_label(drive) {
                        Ok(label) if !label.is_empty() => println!("  {}:  {}", drive, label),
                        Ok(_) => println!("  {}:  (no label)", drive),
                        Err(_) => println!("  {}:  (label unavailable)", drive),
                    }
                }
            }
        });
    }
}
