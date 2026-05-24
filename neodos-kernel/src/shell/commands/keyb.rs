use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_keyb(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Keyboard layout switching is handled by the NEM ps2kbd driver.");
            println!("Currently defaults to Spanish (SP) layout.");
            println!("Usage: KEYB not yet implemented for NEM driver path.");
            return;
        }
        // TODO: route layout change through NEM ps2kbd driver's LAYOUT atomic
        // via the Event Bus or a dedicated kernel interface.
        println!("KEYB: NEM driver layout switch not yet wired.");
    }
}

