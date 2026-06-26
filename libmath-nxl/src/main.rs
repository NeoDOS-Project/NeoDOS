#![no_std]
#![no_main]

mod math;

use core::arch::asm;

// ============================================================
// NXL entry point — never actually executed (passive library)
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_entry() -> ! {
    loop { unsafe { asm!("hlt"); } }
}

// ============================================================
// C ABI wrappers — delegate to math.rs, keep #[no_mangle] here
// ============================================================

#[no_mangle] pub extern "C" fn math_add(a: i64, b: i64) -> i64 { math::add(a, b) }
#[no_mangle] pub extern "C" fn math_sub(a: i64, b: i64) -> i64 { math::sub(a, b) }
#[no_mangle] pub extern "C" fn math_mul(a: i64, b: i64) -> i64 { math::mul(a, b) }
#[no_mangle] pub extern "C" fn math_div(a: i64, b: i64) -> i64 { math::div(a, b) }
#[no_mangle] pub extern "C" fn math_mod(a: i64, b: i64) -> i64 { math::modulo(a, b) }
#[no_mangle] pub extern "C" fn math_abs(x: i64) -> i64 { math::abs(x) }
#[no_mangle] pub extern "C" fn math_abs_f64(x: f64) -> f64 { math::abs_f64(x) }
#[no_mangle] pub extern "C" fn math_min(a: i64, b: i64) -> i64 { math::min(a, b) }
#[no_mangle] pub extern "C" fn math_max(a: i64, b: i64) -> i64 { math::max(a, b) }
#[no_mangle] pub extern "C" fn math_clamp(v: i64, lo: i64, hi: i64) -> i64 { math::clamp(v, lo, hi) }
#[no_mangle] pub extern "C" fn math_pow(base: i64, exp: u32) -> i64 { math::pow(base, exp) }
#[no_mangle] pub extern "C" fn math_sqrt_int(x: u64) -> u64 { math::sqrt_int(x) }
#[no_mangle] pub extern "C" fn math_sqrt_f64(x: f64) -> f64 { math::sqrt_f64(x) }
#[no_mangle] pub extern "C" fn math_sin(x: f64) -> f64 { math::sin(x) }
#[no_mangle] pub extern "C" fn math_cos(x: f64) -> f64 { math::cos(x) }
#[no_mangle] pub extern "C" fn math_tan(x: f64) -> f64 { math::tan(x) }
#[no_mangle] pub extern "C" fn math_log2(x: f64) -> f64 { math::log2(x) }
#[no_mangle] pub extern "C" fn math_log(x: f64) -> f64 { math::log(x) }
#[no_mangle] pub extern "C" fn math_exp(x: f64) -> f64 { math::exp(x) }

// ============================================================
// Export Table — placed in .export_table section at known offset
// ============================================================

use math::MathAbiTable;

#[no_mangle]
#[link_section = ".export_table"]
pub static MATH_EXPORT_TABLE: MathAbiTable = MathAbiTable {
    version: 1,
    add: math_add,
    sub: math_sub,
    mul: math_mul,
    abs: math_abs,
    abs_f64: math_abs_f64,
    min: math_min,
    max: math_max,
    clamp: math_clamp,
    pow: math_pow,
    modulo: math_mod,
    div: math_div,
    sqrt_int: math_sqrt_int,
    sqrt_f64: math_sqrt_f64,
    sin: math_sin,
    cos: math_cos,
    tan: math_tan,
    log2: math_log2,
    log: math_log,
    exp: math_exp,
    _reserved: [0; 8],
};

// ============================================================
// Panic handler (NXL version — loops on HLT)
// ============================================================
#[panic_handler]
fn nxl_panic(_info: &core::panic::PanicInfo) -> ! {
    loop { unsafe { asm!("hlt"); } }
}
