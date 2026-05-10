use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_vol(&mut self, args: &[&str]) {
        let drive_char = if args.is_empty() {
            self.current_drive as char
        } else {
            let a = args[0];
            let b = a.as_bytes();
            if b.len() != 2 || b[1] != b':' {
                println!("Usage: VOL [d:]");
                return;
            }
            match a.chars().next() {
                Some(c) => c.to_ascii_uppercase(),
                None => {
                    println!("Invalid drive specification");
                    return;
                }
            }
        };

        match self.drive_manager.get(drive_char) {
            Some(_) => {
                println!(
                    " Volume in drive {} is {}",
                    drive_char,
                    self.volume_label()
                );
                println!(" NeoDOS filesystem");
            }
            None => println!("Invalid drive specification"),
        }
    }
}

