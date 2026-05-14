use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_attrib(&mut self, _args: &[&str]) {
        println!("ATTRIB not implemented in VFS yet");
    }
}