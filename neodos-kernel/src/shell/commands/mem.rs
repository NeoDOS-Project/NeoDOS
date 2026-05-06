use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub(super) fn cmd_mem(&mut self) {
        let s = crate::memory::stats();
        println!("Physical max: 0x{:x}", s.phys_max);
        println!("Total:    {} KiB", s.total_kib);
        println!("Usable:   {} KiB", s.usable_kib);
        println!("Free:     {} KiB", s.free_kib);
        println!("Used:     {} KiB", s.used_kib);
        println!("Reserved: {} KiB", s.reserved_kib);
    }
}

