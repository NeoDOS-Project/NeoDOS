use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_crash(&mut self, args: &[&str]) {
        if args.is_empty() {
            crate::crash::print_crash_dump_status();
            return;
        }
        let sub = args[0].to_uppercase();
        match sub.as_str() {
            "DUMP" => {
                crate::crash::print_crash_dump_full();
            }
            "STATUS" => {
                crate::crash::print_crash_dump_status();
            }
            "TRIGGER" => {
                crate::println!("[!] Triggering test crash dump...");
                crate::crash::dump_panic(0xdead, 0xbeef);
                crate::println!("[+] Crash dump written to serial and RAM buffer");
            }
            _ => {
                crate::println!("Usage: CRASH [DUMP|STATUS|TRIGGER]");
                crate::println!("  CRASH           - show crash dump status");
                crate::println!("  CRASH DUMP      - write full crash dump to serial");
                crate::println!("  CRASH STATUS    - show crash dump area status");
                crate::println!("  CRASH TRIGGER   - trigger a test crash dump (safe)");
            }
        }
    }
}
