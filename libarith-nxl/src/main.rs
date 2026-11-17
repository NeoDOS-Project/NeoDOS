#![no_std]
#![no_main]

//! B2 spike producer: a PIE NXL that imports `math_add` from `math.nxl`.
//! The linker emits an undefined symbol + dynamic relocation that the kernel
//! loader resolves from the NXL symbol registry (`resolve_nxl_imports`).

use core::arch::asm;

extern "C" {
    fn math_add(a: i64, b: i64) -> i64;
}

#[no_mangle]
pub extern "C" fn arith_sum3(a: i64, b: i64, c: i64) -> i64 {
    // Two cross-library calls: exercises the imported symbol.
    unsafe { math_add(math_add(a, b), c) }
}

#[repr(C)]
pub struct ArithAbiTable {
    pub version: u32,
    pub sum3: extern "C" fn(i64, i64, i64) -> i64,
    _reserved: [u64; 4],
}

#[no_mangle]
#[link_section = ".export_table"]
pub static ARITH_EXPORT_TABLE: ArithAbiTable = ArithAbiTable {
    version: 1,
    sum3: arith_sum3,
    _reserved: [0; 4],
};

#[no_mangle]
pub extern "C" fn nxl_entry() -> ! {
    loop {
        unsafe { asm!("hlt"); }
    }
}

#[panic_handler]
fn nxl_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe { asm!("hlt"); }
    }
}
