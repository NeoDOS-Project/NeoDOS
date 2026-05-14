use crate::println;
use crate::shell::shell::DosShell;
use alloc::string::String;

impl DosShell {
    pub fn cmd_label(&mut self, args: &[&str]) {
        let mut drive = self.current_drive;
        let mut label_start = 0;

        if let Some(first) = args.first() {
            if first.len() >= 2 && first.as_bytes()[1] == b':' {
                drive = first.chars().next().unwrap_or(self.current_drive).to_ascii_uppercase();
                if first.len() > 2 {
                    self.set_label_or_print_error(drive, &first[2..]);
                    return;
                }
                label_start = 1;
            }
        }

        if label_start >= args.len() {
            crate::globals::with_vfs(|vfs| {
                match vfs.volume_label(drive) {
                    Ok(label) if !label.is_empty() => println!(" Volume in drive {} is {}", drive, label),
                    Ok(_) => println!(" Volume in drive {} has no label", drive),
                    Err(e) => println!(" Volume information unavailable ({:?})", e),
                }
            });
            return;
        }

        let mut label = String::new();
        for (idx, part) in args[label_start..].iter().enumerate() {
            if idx > 0 {
                label.push(' ');
            }
            label.push_str(part);
        }
        self.set_label_or_print_error(drive, &label);
    }

    fn set_label_or_print_error(&mut self, drive: char, label: &str) {
        let label = label.trim();
        if label.len() > 11 {
            println!("Volume label must be 11 characters or fewer");
            return;
        }
        if !label.bytes().all(|b| b.is_ascii() && b >= 0x20) {
            println!("Invalid volume label");
            return;
        }

        crate::globals::with_vfs(|vfs| {
            match vfs.set_volume_label(drive, label) {
                Ok(()) => {
                    crate::globals::NEED_CACHE_FLUSH.store(true, core::sync::atomic::Ordering::Relaxed);
                    crate::globals::flush_cache_if_needed();
                    println!(" Volume in drive {} is now {}", drive, label);
                }
                Err(crate::fs::vfs::VfsError::PermissionDenied) => {
                    println!(" Cannot change label on drive {}", drive);
                }
                Err(e) => println!(" Error setting volume label ({:?})", e),
            }
        });
    }
}
