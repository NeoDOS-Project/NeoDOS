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
