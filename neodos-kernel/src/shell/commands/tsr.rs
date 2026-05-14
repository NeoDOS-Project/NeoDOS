use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_tsr(&mut self, _args: &[&str]) {
        println!("TSR not implemented in VFS shell yet");
    }
}
