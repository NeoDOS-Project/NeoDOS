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
mod drives;
mod fsck;
mod help;
mod keyb;
mod kill;
mod label;
mod ndreg;
mod crash;
mod pri;
mod ps;
mod run;

use crate::shell::handler::COMMANDS;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn dispatch_command(&mut self, cmd: &str, args: &[&str]) -> bool {
        COMMANDS.dispatch(cmd, args, self)
    }
}
