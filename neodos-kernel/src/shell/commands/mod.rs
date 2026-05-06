// src/shell/commands/mod.rs

mod call;
mod cd;
mod copy;
mod cpuinfo;
mod devices;
mod dir;
mod drives;
mod echo;
mod help;
mod md;
mod mem;
mod set;
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
            "CPUINFO" => self.cmd_cpuinfo(),
            "MEM" => self.cmd_mem(),
            "EXIT" => {
                let _ = self.fs.sync(self.cache, self.ata);
                self.running = false;
            }
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
            _ => println!("Bad command or file name"),
        }
    }
}

