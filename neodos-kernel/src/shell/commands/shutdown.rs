use crate::println;
use crate::drivers::{acpi, pci};
use crate::drivers::acpi::Pm1aCntTarget;
use crate::shell::shell::DosShell;

fn build_sleep_value(slp_typ: u8) -> u16 {
    ((slp_typ as u16) << 10) | (1 << 13)
}

impl<'a> DosShell<'a> {
    pub fn cmd_shutdown(&mut self) {
        println!("Shutting down...");
        crate::serial_println!("[SHUTDOWN] Syncing block cache...");
        let _ = self.cache.flush(self.ata);
        crate::serial_println!("[SHUTDOWN] Syncing filesystem...");
        let _ = self.fs.sync(self.cache, self.ata);
        crate::serial_println!("[SHUTDOWN] Power off...");

        // SLP_TYP para S5 se define en el DSDT (objeto AML \_S5) y no podemos
        // parsear AML desde el kernel. Los valores típicos en Intel son 5 y 7.
        const SLP_TYP_TRIALS: &[u8] = &[5, 7];

        unsafe {
            for &slp_typ in SLP_TYP_TRIALS {
                let acpi_val = build_sleep_value(slp_typ);
                crate::serial_println!("[SHUTDOWN] Trying SLP_TYP={}", slp_typ);

                // 1. FADT-detected PM1a_CNT (ACPI tables)
                if let Some(target) = acpi::find_pm1a_cnt_target() {
                    match target {
                        Pm1aCntTarget::IoPort(port) => {
                            core::arch::asm!("out dx, ax", in("dx") port, in("ax") acpi_val,
                                options(nomem, nostack, preserves_flags));
                        },
                        Pm1aCntTarget::Mmio(addr) => {
                            (addr as *mut u16).write_volatile(acpi_val);
                        },
                    }
                }

                // 2. PCI-detected PM1_CNT (PIIX4 GPBASE, ICH9 ABASE…)
                if let Some(port) = pci::find_acpi_pm1_cnt_port() {
                    core::arch::asm!("out dx, ax", in("dx") port, in("ax") acpi_val,
                        options(nomem, nostack, preserves_flags));
                }

                // 3. Fallback ACPI PM1a_CNT ports
                for &port in &[0x404u16, 0x604] {
                    core::arch::asm!("out dx, ax", in("dx") port, in("ax") acpi_val,
                        options(nomem, nostack, preserves_flags));
                }
            }

            // 4. VM debug poweroff ports (Bochs 0x604, QEMU 0xB004)
            for &(port, val) in &[(0x604u16, 0x2000u16), (0xB004, 0x2000)] {
                crate::serial_println!("[SHUTDOWN] Trying VM poweroff port {:#x}", port);
                core::arch::asm!("out dx, ax", in("dx") port, in("ax") val,
                    options(nomem, nostack, preserves_flags));
            }
        }

        crate::serial_println!("[SHUTDOWN] ACPI poweroff failed, halting.");
        println!("Shutdown failed; halting.");
        crate::arch::disable_interrupts();
        loop {
            unsafe { core::arch::asm!("hlt") };
        }
    }
}
