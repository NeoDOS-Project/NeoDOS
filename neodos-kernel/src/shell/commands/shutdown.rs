use crate::eventbus::{EVENT_SHUTDOWN, SOURCE_KERNEL, EVENT_BUS};
use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_shutdown(&mut self, _args: &[&str]) {
        println!("Shutting down...");
        crate::globals::flush_cache_if_needed();

        let _ = EVENT_BUS.push_event(
            EVENT_SHUTDOWN,
            SOURCE_KERNEL,
            0,
            0, 0, 0,
        );

        EVENT_BUS.dispatch_pending();

        // Fallback: if ACPI driver didn't power off via S5, try HAL poweroff
        // (QEMU debug port, PS/2 reset, known ACPI ports)
        crate::hal::poweroff();
    }
}
