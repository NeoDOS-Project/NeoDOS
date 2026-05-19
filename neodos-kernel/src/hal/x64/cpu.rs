use core::arch::asm;

pub extern "C" fn enable_interrupts() {
    unsafe { asm!("sti", options(nostack, nomem)); }
}

pub extern "C" fn disable_interrupts() {
    unsafe { asm!("cli", options(nostack, nomem)); }
}

pub extern "C" fn halt() -> ! {
    loop {
        unsafe { asm!("hlt", options(nostack, nomem)); }
    }
}

pub extern "C" fn poweroff() -> ! {
    disable_interrupts();
    unsafe {
        for &(port, val) in &[(0x404u16, 0x2000u16), (0x604u16, 0x2000u16),
                              (0xB004u16, 0x2000u16), (0x4004u16, 0x3400u16)] {
            asm!("out dx, ax", in("dx") port, in("ax") val,
                options(nomem, nostack, preserves_flags));
        }
        asm!("out dx, al", in("dx") 0x64u16, in("al") 0xFEu8,
            options(nomem, nostack, preserves_flags));
    }
    halt()
}

pub fn cpu_info() -> crate::cpu::CpuInfo {
    crate::cpu::get_cpu_info()
}
