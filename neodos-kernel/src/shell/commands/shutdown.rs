use crate::println;
use crate::drivers::{acpi, pci};
use crate::drivers::acpi::Pm1aCntTarget;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_shutdown(&mut self) {
        println!("Shutting down...");
        crate::serial_println!("[SHUTDOWN] Syncing block cache...");
        let _ = self.cache.flush(self.ata);
        crate::serial_println!("[SHUTDOWN] Syncing filesystem...");
        let _ = self.fs.sync(self.cache, self.ata);
        crate::serial_println!("[SHUTDOWN] Power off...");

        let slp_val: u16 = 0x3C01;

        unsafe {
            // 1. FADT-detected PM1a_CNT (ACPI tables, any platform)
            if let Some(target) = acpi::find_pm1a_cnt_target() {
                match target {
                    Pm1aCntTarget::IoPort(port) => {
                        core::arch::asm!("out dx, ax", in("dx") port, in("ax") slp_val,
                            options(nomem, nostack, preserves_flags));
                        core::arch::asm!("out dx, eax", in("dx") port, in("eax") slp_val as u32,
                            options(nomem, nostack, preserves_flags));
                    },
                    Pm1aCntTarget::Mmio(addr) => {
                        (addr as *mut u16).write_volatile(slp_val);
                        (addr as *mut u32).write_volatile(slp_val as u32);
                    },
                }
            }

            // 2. PCI-detected PM1_CNT (PIIX4 GPBASE, QEMU-compat)
            if let Some(port) = pci::find_acpi_pm1_cnt_port() {
                core::arch::asm!("out dx, ax", in("dx") port, in("ax") slp_val,
                    options(nomem, nostack, preserves_flags));
                core::arch::asm!("out dx, eax", in("dx") port, in("eax") slp_val as u32,
                    options(nomem, nostack, preserves_flags));
            }

            // 3. Fallback known ports (QEMU Bochs, VirtualBox PIIX3, etc.)
            for &port in &[0x404u16, 0x604, 0xB004] {
                let val = if port == 0xB004 { 0x2000u16 } else { slp_val };
                core::arch::asm!("out dx, ax", in("dx") port, in("ax") val,
                    options(nomem, nostack, preserves_flags));
                if val == slp_val {
                    core::arch::asm!("out dx, eax", in("dx") port, in("eax") val as u32,
                        options(nomem, nostack, preserves_flags));
                }
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
