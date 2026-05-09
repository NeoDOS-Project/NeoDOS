//! Built-in DOS shell commands.
//!
//! Commands are split into individual files in this directory (one command per
//! file) to keep compile units small and changes localized.
//!
//! To add a new command:
//! 1) create `src/shell/commands/<name>.rs` with an `impl DosShell` method
//! 2) add `mod <name>;` here
//! 3) add a match arm in [`DosShell::dispatch_command`]
//! 4) (optional) add it to `HELP`

mod call;
mod cd;
mod copy;
mod cpuinfo;
mod devices;
mod dir;
mod drives;
mod echo;
mod help;
mod keyb;
mod md;
mod mem;
mod set;
mod shutdown;
mod test;
mod tsr;
mod r#type;
mod vol;

use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn dispatch_command(&mut self, cmd: &str, args: &[&str]) {
        match cmd {
            "HELP" => self.cmd_help(),
            "CLS" => crate::vga::clear_screen(),
            "DIR" => self.cmd_dir(args),
            "TYPE" => self.cmd_type(args),
            "ECHO" => self.cmd_echo(args),
            "SET" => self.cmd_set(args),
            "KEYB" => self.cmd_keyb(args),
            "CPUINFO" => self.cmd_cpuinfo(),
            "MEM" => self.cmd_mem(),
            "EXIT" => self.cmd_shutdown(),
            "SHUTDOWN" | "POWEROFF" => self.cmd_shutdown(),
            "CD" => self.cmd_cd(args),
            "CALL" => self.cmd_call(args),
            "COPY" => self.cmd_copy(args),
            "MD" => self.cmd_md(args),
            "VOL" => self.cmd_vol(args),
            "DRIVES" => self.cmd_drives(),
            "SYNC" => {
                println!("Syncing disk...");
                let _ = self.fs.sync(self.cache, self.ata);
            }
            "DEL" => println!("DEL not yet implemented"),
            "REN" => println!("REN not yet implemented"),
            "VER" => println!("NeoDOS v0.6"),
            "TSR" => self.cmd_tsr(args),
            "DEVICES" => self.cmd_devices(),
            "TEST" => self.cmd_test(args),
            _ => println!("Bad command or file name"),
        }
    }
}
