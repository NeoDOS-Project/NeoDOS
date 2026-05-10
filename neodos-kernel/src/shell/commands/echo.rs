use crate::print;
use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_echo(&mut self, args: &[&str]) {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                print!(" ");
            }
            if arg.starts_with('%') && arg.ends_with('%') && arg.len() > 2 {
                let var = &arg[1..arg.len() - 1];
                if let Some(val) = self.environment.get(var) {
                    print!("{}", val);
                } else {
                    print!("{}", arg);
                }
            } else {
                print!("{}", arg);
            }
        }
        println!();
    }
}

