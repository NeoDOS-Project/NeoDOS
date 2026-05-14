pub mod entry;
pub mod gdt;
pub mod idt;
pub mod pic;
pub mod serial;
pub mod paging;

pub use gdt::init as init_gdt;
pub use idt::init as init_idt;
pub use pic::init as init_pic;
pub use serial::init as init_serial;

// CPU control instructions
#[inline]
pub fn halt() -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nostack, nomem));
        }
    }
}

/// Attempt to power off the system via VM debug ports or ACPI.
/// If poweroff fails, falls back to an infinite HLT loop with CLI.
pub fn poweroff() -> ! {
    disable_interrupts();
    unsafe {
        // ACPI PM1a_CNT fallback ports
        for &(port, val) in &[(0x404u16, 0x2000u16), (0x604u16, 0x2000u16),
                              (0xB004u16, 0x2000u16), (0x4004u16, 0x3400u16)] {
            core::arch::asm!("out dx, ax", in("dx") port, in("ax") val,
                options(nomem, nostack, preserves_flags));
        }
        // PS/2 keyboard controller CPU reset
        core::arch::asm!("out dx, al", in("dx") 0x64u16, in("al") 0xFEu8,
            options(nomem, nostack, preserves_flags));
    }
    halt()
}

#[inline]
pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti");
    }
}

#[inline]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli");
    }
}
