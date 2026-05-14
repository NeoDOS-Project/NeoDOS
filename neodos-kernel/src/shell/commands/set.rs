use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_set(&mut self, args: &[&str]) {
        if args.is_empty() {
            for i in 0..self.environment.count {
                if let Ok(k) = core::str::from_utf8(&self.environment.keys[i]) {
                    if let Ok(v) = core::str::from_utf8(&self.environment.values[i]) {
                        println!("{}={}", k.trim_matches('\0'), v.trim_matches('\0'));
                    }
                }
            }
            return;
        }

        let mut found_eq = false;
        for arg in args {
            if let Some(pos) = arg.find('=') {
                let key = arg[..pos].trim();
                let val = arg[pos + 1..].trim();
                if !key.is_empty() {
                    self.environment.set(key, val);
                    found_eq = true;
                }
                break;
            }
        }

        if !found_eq && args.len() >= 2 {
            println!("Usage: SET VAR=VALUE");
        }
    }
}

