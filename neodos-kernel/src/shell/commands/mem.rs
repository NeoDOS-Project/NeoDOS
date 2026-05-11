use crate::println;
use crate::shell::shell::DosShell;

fn format_kb(kib: u64) -> (u64, &'static str) {
    if kib >= 1024 * 1024 {
        (kib / (1024 * 1024), "MB")
    } else if kib >= 1024 {
        (kib / 1024, "KB")
    } else {
        (kib, "KB")
    }
}

impl<'a> DosShell<'a> {
    pub fn cmd_mem(&mut self, args: &[&str]) {
        let s = crate::memory::stats();
        
        // Check for /H flag
        let human = args.iter().any(|&a| a.eq_ignore_ascii_case("/H") || a.eq_ignore_ascii_case("-H"));
        
        if human {
            // Human readable format
            let (total, unit) = format_kb(s.total_kib);
            let (usable, _) = format_kb(s.usable_kib);
            let (free, _) = format_kb(s.free_kib);
            let (used, _) = format_kb(s.used_kib);
            let (reserved, _) = format_kb(s.reserved_kib);
            
            println!("Memory:");
            println!("  Total:    {} {}", total, unit);
            println!("  Usable:   {} {}", usable, unit);
            println!("  Free:     {} {}", free, unit);
            println!("  Used:     {} {}", used, unit);
            println!("  Reserved: {} {}", reserved, unit);
        } else {
            // Classic DOS format
            println!("Physical max: 0x{:x}", s.phys_max);
            println!("Total:    {} KiB", s.total_kib);
            println!("Usable:   {} KiB", s.usable_kib);
            println!("Free:     {} KiB", s.free_kib);
            println!("Used:     {} KiB", s.used_kib);
            println!("Reserved: {} KiB", s.reserved_kib);
        }
    }
}