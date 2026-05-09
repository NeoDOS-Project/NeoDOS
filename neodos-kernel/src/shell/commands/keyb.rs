use crate::drivers::keyboard::{KeyboardDriver, KeyboardLayout};
use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub(super) fn cmd_keyb(&mut self, args: &[&str]) {
        if args.is_empty() {
            let current = match KeyboardDriver::layout() {
                KeyboardLayout::Us => "US",
                KeyboardLayout::Sp => "SP",
            };
            println!("Keyboard layout: {}", current);
            println!("Usage: KEYB US|SP");
            return;
        }

        let arg0 = args[0];
        if arg0 == "/?" || arg0 == "-h" || arg0 == "--help" {
            println!("KEYB - Change keyboard layout");
            println!("Usage: KEYB US|SP");
            return;
        }

        let mut buf = [0u8; 8];
        let mut len = 0usize;
        for (i, b) in arg0.as_bytes().iter().enumerate() {
            if i >= buf.len() {
                break;
            }
            let mut c = *b;
            if c.is_ascii_lowercase() {
                c = c.to_ascii_uppercase();
            }
            buf[i] = c;
            len += 1;
        }
        let key = core::str::from_utf8(&buf[..len]).unwrap_or("");

        let layout = match key {
            "US" | "EN" => Some(KeyboardLayout::Us),
            "SP" | "ES" => Some(KeyboardLayout::Sp),
            _ => None,
        };

        if let Some(layout) = layout {
            KeyboardDriver::set_layout(layout);
            println!("Keyboard layout set.");
        } else {
            println!("Invalid layout. Use: KEYB US|SP");
        }
    }
}

