use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_tsr(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: TSR FILENAME INT");
            println!("Example: TSR CLOCK.BIN 1C");
            return;
        }

        let filename = args[0];
        let int_hex = args[1];

        let mut int_num = 0;
        for b in int_hex.as_bytes() {
            let digit = match *b {
                b'0'..=b'9' => *b - b'0',
                b'a'..=b'f' => *b - b'a' + 10,
                b'A'..=b'F' => *b - b'A' + 10,
                _ => 0,
            };
            int_num = int_num * 16 + digit;
        }

        match crate::tsr::install_tsr(filename, int_num as u8, self.fs, self.cache, self.ata) {
            Ok(addr) => println!("TSR installed @ 0x{:x} (INT 0x{:x})", addr, int_num),
            Err(_) => println!("Error installing TSR"),
        }
    }
}

