use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_drives(&mut self) {
        println!("Mounted drives:");
        let mut any = false;
        for i in 0..26u8 {
            let c = (b'A' + i) as char;
            if let Some(d) = self.drive_manager.get(c) {
                println!("  {}:  FsInstance {}", d.letter as char, d.fs.0);
                any = true;
            }
        }
        if !any {
            println!("  (none)");
        }
    }
}

