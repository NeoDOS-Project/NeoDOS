use crate::println;
use crate::shell::shell::DosShell;
use crate::eventbus::{EVENT_KEYB_LAYOUT, SOURCE_KERNEL, EVENT_BUS};

impl DosShell {
    pub fn cmd_keyb(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: KEYB US|SP");
            println!("  US = English (United States)");
            println!("  SP = Spanish");
            return;
        }

        let layout = match args[0].to_uppercase().as_str() {
            "US" => 0u64,
            "SP" => 1u64,
            _ => {
                println!("Invalid layout. Use US or SP.");
                return;
            }
        };

        let name = if layout == 0 { "US" } else { "SP" };
        match EVENT_BUS.push_event(EVENT_KEYB_LAYOUT, SOURCE_KERNEL, 3, layout, 0, 0) {
            Ok(_) => println!("Keyboard layout changed to {} via Event Bus.", name),
            Err(_) => println!("Error: Event Bus full, layout change deferred."),
        }
    }
}

