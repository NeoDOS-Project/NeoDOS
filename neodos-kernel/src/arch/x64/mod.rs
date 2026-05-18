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

use crate::arch::Platform;

/// x86_64 platform implementation.
pub struct X64Platform;

impl Platform for X64Platform {
    #[inline]
    fn halt() -> ! {
        loop {
            unsafe {
                core::arch::asm!("hlt", options(nostack, nomem));
            }
        }
    }

    /// Attempt to power off the system via VM debug ports or ACPI.
    fn poweroff() -> ! {
        Self::disable_interrupts();
        unsafe {
            for &(port, val) in &[(0x404u16, 0x2000u16), (0x604u16, 0x2000u16),
                                  (0xB004u16, 0x2000u16), (0x4004u16, 0x3400u16)] {
                core::arch::asm!("out dx, ax", in("dx") port, in("ax") val,
                    options(nomem, nostack, preserves_flags));
            }
            core::arch::asm!("out dx, al", in("dx") 0x64u16, in("al") 0xFEu8,
                options(nomem, nostack, preserves_flags));
        }
        Self::halt()
    }

    #[inline]
    fn enable_interrupts() {
        unsafe { core::arch::asm!("sti"); }
    }

    #[inline]
    fn disable_interrupts() {
        unsafe { core::arch::asm!("cli"); }
    }

    fn cpu_info() -> crate::cpu::CpuInfo {
        crate::cpu::get_cpu_info()
    }
}

// ── Convenience re-exports (phase toward Platform trait) ────────────

#[inline]
pub fn halt() -> ! {
    X64Platform::halt()
}

pub fn poweroff() -> ! {
    X64Platform::poweroff()
}

#[inline]
pub fn enable_interrupts() {
    X64Platform::enable_interrupts();
}

#[inline]
pub fn disable_interrupts() {
    X64Platform::disable_interrupts();
}
