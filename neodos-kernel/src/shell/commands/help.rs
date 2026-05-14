use crate::shell::handler::COMMANDS;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_help(&mut self) {
        COMMANDS.print_help();
    }
}
