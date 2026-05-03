use crate::vga;
use crate::serial_print;

// Process A: prints "A" in a loop
pub fn proc_a() -> ! {
    loop {
        vga::print_str("A");
        serial_print!("A");
        for _ in 0..100000 {
            unsafe { core::arch::asm!("nop") };
        }
    }
}

// Process B: prints "B"
pub fn proc_b() -> ! {
    loop {
        vga::print_str("B");
        serial_print!("B");
        for _ in 0..100000 {
            unsafe { core::arch::asm!("nop") };
        }
    }
}

// Process C: prints "C"
pub fn proc_c() -> ! {
    loop {
        vga::print_str("C");
        serial_print!("C");
        for _ in 0..100000 {
            unsafe { core::arch::asm!("nop") };
        }
    }
}

// Process D: prints "D"
pub fn proc_d() -> ! {
    loop {
        vga::print_str("D");
        serial_print!("D");
        for _ in 0..100000 {
            unsafe { core::arch::asm!("nop") };
        }
    }
}
