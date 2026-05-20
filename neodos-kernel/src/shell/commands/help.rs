use crate::println;
use crate::shell::handler::COMMANDS;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_help(&mut self, args: &[&str]) {
        if args.is_empty() {
            COMMANDS.print_help();
        } else {
            if !COMMANDS.print_command_help(args[0]) {
                println!("No help available for '{}'", args[0]);
            }
        }
    }
}
