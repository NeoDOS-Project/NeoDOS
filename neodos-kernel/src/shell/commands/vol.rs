use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_vol(&mut self, args: &[&str]) {
        let drive = if args.is_empty() {
            self.current_drive
        } else if args.len() == 1 && args[0].len() == 2 && args[0].as_bytes()[1] == b':' {
            args[0].chars().next().unwrap_or(self.current_drive).to_ascii_uppercase()
        } else {
            println!("Usage: VOL [drive:]");
            return;
        };

        crate::globals::with_vfs(|vfs| {
            match vfs.volume_label(drive) {
                Ok(label) if !label.is_empty() => {
                    println!(" Volume in drive {} is {}", drive, label);
                }
                Ok(_) => {
                    println!(" Volume in drive {} has no label", drive);
                }
                Err(e) => {
                    println!(" Volume information unavailable ({:?})", e);
                }
            }
        });
    }
}
