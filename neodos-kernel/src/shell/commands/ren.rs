use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_rename(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: REN oldname newname");
            return;
        }
        println!("RENAME not implemented in VFS yet");
    }
}