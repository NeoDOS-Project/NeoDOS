use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_shutdown(&mut self) {
        println!("Shutting down...");
        crate::serial_println!("[SHUTDOWN] Syncing block cache...");
        let _ = self.cache.flush(self.ata);
        crate::serial_println!("[SHUTDOWN] Syncing filesystem...");
        let _ = self.fs.sync(self.cache, self.ata);
        crate::serial_println!("[SHUTDOWN] Power off...");
        unsafe {
            // PIIX4 ACPI PM1_CNT: SCI_EN=1, SLP_TYP=S5(7), SLP_EN=1 => 0x3C01
            // Try both 16-bit and 32-bit writes
            core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x3C01u16, options(nomem, nostack, preserves_flags));
            core::arch::asm!("out dx, eax", in("dx") 0x604u16, in("eax") 0x3C01u32, options(nomem, nostack, preserves_flags));
            // Bochs/QEMU old shutdown port
            core::arch::asm!("out dx, ax", in("dx") 0xB004u16, in("ax") 0x2000u16, options(nomem, nostack, preserves_flags));
        }
        crate::serial_println!("[SHUTDOWN] ACPI poweroff failed, halting.");
        println!("Shutdown failed; halting.");
        crate::arch::disable_interrupts();
        loop {
            unsafe { core::arch::asm!("hlt") };
        }
    }
}
