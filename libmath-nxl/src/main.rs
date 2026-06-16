#![no_std]
#![no_main]

use core::arch::asm;

// ============================================================
// NXL entry point — never actually executed (passive library)
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_entry() -> ! {
    loop { unsafe { asm!("hlt"); } }
}

// ============================================================
// Math functions — pure computation, no syscalls needed
// ============================================================

#[no_mangle]
pub extern "C" fn math_add(a: i64, b: i64) -> i64 {
    a + b
}

#[no_mangle]
pub extern "C" fn math_sub(a: i64, b: i64) -> i64 {
    a - b
}

#[no_mangle]
pub extern "C" fn math_mul(a: i64, b: i64) -> i64 {
    a * b
}

#[no_mangle]
pub extern "C" fn math_abs(x: i64) -> i64 {
    if x < 0 { -x } else { x }
}

#[no_mangle]
pub extern "C" fn math_abs_f64(x: f64) -> f64 {
    if x < 0.0 { -x } else { x }
}

#[no_mangle]
pub extern "C" fn math_min(a: i64, b: i64) -> i64 {
    if a < b { a } else { b }
}

#[no_mangle]
pub extern "C" fn math_max(a: i64, b: i64) -> i64 {
    if a > b { a } else { b }
}

#[no_mangle]
pub extern "C" fn math_clamp(value: i64, lo: i64, hi: i64) -> i64 {
    if value < lo { lo } else if value > hi { hi } else { value }
}

#[no_mangle]
pub extern "C" fn math_pow(base: i64, exp: u32) -> i64 {
    let mut result: i64 = 1;
    let mut b = base;
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 {
            result = result.wrapping_mul(b);
        }
        b = b.wrapping_mul(b);
        e >>= 1;
    }
    result
}

#[no_mangle]
pub extern "C" fn math_mod(a: i64, b: i64) -> i64 {
    if b == 0 { 0 } else { a % b }
}

#[no_mangle]
pub extern "C" fn math_div(a: i64, b: i64) -> i64 {
    if b == 0 { 0 } else { a / b }
}

#[no_mangle]
pub extern "C" fn math_sqrt_int(x: u64) -> u64 {
    if x == 0 { return 0; }
    let mut guess = x;
    let mut result = (x + 1) / 2;
    while result < guess {
        guess = result;
        result = (x / guess + guess) / 2;
    }
    guess
}

#[no_mangle]
pub extern "C" fn math_sqrt_f64(x: f64) -> f64 {
    if x < 0.0 { return 0.0; }
    if x == 0.0 { return 0.0; }
    // Newton's method
    let mut guess = x;
    for _ in 0..50 {
        guess = (guess + x / guess) / 2.0;
    }
    guess
}

#[no_mangle]
pub extern "C" fn math_sin(x: f64) -> f64 {
    // Taylor series approximation
    let normalized = x % (2.0 * PI);
    let mut result = 0.0;
    let mut term = normalized;
    for n in 0..10 {
        result += term;
        term *= -normalized * normalized / ((2 * n + 2) as f64 * (2 * n + 3) as f64);
    }
    result
}

#[no_mangle]
pub extern "C" fn math_cos(x: f64) -> f64 {
    math_sin(x + PI / 2.0)
}

#[no_mangle]
pub extern "C" fn math_tan(x: f64) -> f64 {
    let c = math_cos(x);
    if c == 0.0 { return 0.0; }
    math_sin(x) / c
}

#[no_mangle]
pub extern "C" fn math_log2(x: f64) -> f64 {
    if x <= 0.0 { return 0.0; }
    let mut result = 0.0;
    let mut val = x;
    while val >= 2.0 {
        val /= 2.0;
        result += 1.0;
    }
    while val < 1.0 {
        val *= 2.0;
        result -= 1.0;
    }
    // Newton's method for log2 of value in [1, 2)
    let mut frac = 0.5;
    val = (val - 1.0) / (val + 1.0);
    let mut term = val;
    for _ in 0..30 {
        result += frac * term;
        frac /= 2.0;
        term *= val * val;
    }
    result
}

#[no_mangle]
pub extern "C" fn math_log(x: f64) -> f64 {
    math_log2(x) * LN2
}

#[no_mangle]
pub extern "C" fn math_exp(x: f64) -> f64 {
    // exp(x) = 2^(x / ln2)
    let k = (x / LN2) as i64;
    let f = x - (k as f64) * LN2;
    // Approximate 2^f using Taylor series
    let mut result = 1.0;
    let mut term = 1.0;
    for n in 1..20 {
        term *= f * LN2 / (n as f64);
        result += term;
    }
    // Multiply by 2^k
    let mut pow2 = 1.0;
    let mut kk = if k >= 0 { k } else { -k };
    let mut base = 2.0_f64;
    while kk > 0 {
        if kk & 1 == 1 {
            pow2 *= base;
        }
        base *= base;
        kk >>= 1;
    }
    if k < 0 { result / pow2 } else { result * pow2 }
}

// Constants
const PI: f64 = 3.14159265358979323846;
const LN2: f64 = 0.69314718055994530942;

// ============================================================
// Export Table — placed in .export_table section at known offset
// ============================================================
#[repr(C)]
pub struct MathAbiTable {
    pub version: u32,
    pub add: extern "C" fn(i64, i64) -> i64,
    pub sub: extern "C" fn(i64, i64) -> i64,
    pub mul: extern "C" fn(i64, i64) -> i64,
    pub abs: extern "C" fn(i64) -> i64,
    pub abs_f64: extern "C" fn(f64) -> f64,
    pub min: extern "C" fn(i64, i64) -> i64,
    pub max: extern "C" fn(i64, i64) -> i64,
    pub clamp: extern "C" fn(i64, i64, i64) -> i64,
    pub pow: extern "C" fn(i64, u32) -> i64,
    pub modulo: extern "C" fn(i64, i64) -> i64,
    pub div: extern "C" fn(i64, i64) -> i64,
    pub sqrt_int: extern "C" fn(u64) -> u64,
    pub sqrt_f64: extern "C" fn(f64) -> f64,
    pub sin: extern "C" fn(f64) -> f64,
    pub cos: extern "C" fn(f64) -> f64,
    pub tan: extern "C" fn(f64) -> f64,
    pub log2: extern "C" fn(f64) -> f64,
    pub log: extern "C" fn(f64) -> f64,
    pub exp: extern "C" fn(f64) -> f64,
    pub _reserved: [u64; 8],
}

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
