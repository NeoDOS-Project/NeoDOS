#![allow(dead_code)]

use crate::console;
use crate::serial_print;

// Process A: prints "A" in a loop
pub fn proc_a() -> ! {
    loop {
        console::print_str("A");
        serial_print!("A");
        for _ in 0..100000 {
            core::hint::spin_loop();
        }
    }
}

// Process B: prints "B"
pub fn proc_b() -> ! {
    loop {
        console::print_str("B");
        serial_print!("B");
        for _ in 0..100000 {
            core::hint::spin_loop();
        }
    }
}

// Process C: prints "C"
pub fn proc_c() -> ! {
    loop {
        console::print_str("C");
        serial_print!("C");
        for _ in 0..100000 {
            core::hint::spin_loop();
        }
    }
}

// Process D: prints "D"
pub fn proc_d() -> ! {
    loop {
        console::print_str("D");
        serial_print!("D");
        for _ in 0..100000 {
            core::hint::spin_loop();
        }
    }
}
