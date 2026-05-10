//! Built-in DOS shell commands.
//!
//! Commands are split into individual files in this directory (one command per
//! file) to keep compile units small and changes localized.
//!
//! To add a new command:
//! 1) create `src/shell/commands/<name>.rs` with an `impl DosShell` method
//! 2) add `mod <name>;` here
//! 3) add a `CommandEntry` to `handler::COMMANDS` in handler.rs
//!    Help is automatic — the entry's `category` and `description` appear in HELP.

mod call;
mod cd;
mod copy;
mod cpuinfo;
mod date;
mod del;
mod devices;
mod dir;
mod drives;
mod echo;
mod help;
mod keyb;
mod label;
mod md;
mod mem;
mod ren;
mod rd;
mod set;
mod shutdown;
mod test;
mod tsr;
mod r#type;
mod vol;
mod attrib;
mod ps;

use crate::shell::handler::COMMANDS;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn dispatch_command(&mut self, cmd: &str, args: &[&str]) {
        COMMANDS.dispatch(cmd, args, self);
    }
}
