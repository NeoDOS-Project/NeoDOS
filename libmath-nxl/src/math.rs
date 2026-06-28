//! Pure math functions — no std, no syscalls.
//! Can be used from other NXLs or user binaries via the export table.

use core::f64;

// ── Constants ──

pub const PI: f64 = core::f64::consts::PI;
pub const LN2: f64 = core::f64::consts::LN_2;

// ── Integer arithmetic ──

pub fn add(a: i64, b: i64) -> i64 { a + b }
pub fn sub(a: i64, b: i64) -> i64 { a - b }
pub fn mul(a: i64, b: i64) -> i64 { a * b }
pub fn div(a: i64, b: i64) -> i64 { if b == 0 { 0 } else { a / b } }
pub fn modulo(a: i64, b: i64) -> i64 { if b == 0 { 0 } else { a % b } }

// ── Comparison ──

pub fn abs(x: i64) -> i64 { if x < 0 { -x } else { x } }
pub fn abs_f64(x: f64) -> f64 { if x < 0.0 { -x } else { x } }
pub fn min(a: i64, b: i64) -> i64 { if a < b { a } else { b } }
pub fn max(a: i64, b: i64) -> i64 { if a > b { a } else { b } }
pub fn clamp(value: i64, lo: i64, hi: i64) -> i64 {
    if value < lo { lo } else if value > hi { hi } else { value }
}

// ── Power / Root ──

pub fn pow(base: i64, exp: u32) -> i64 {
    let mut result: i64 = 1;
    let mut b = base;
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 { result = result.wrapping_mul(b); }
        b = b.wrapping_mul(b);
        e >>= 1;
    }
    result
}

pub fn sqrt_int(x: u64) -> u64 {
    if x == 0 { return 0; }
    let mut guess = x;
    let mut result = x.div_ceil(2);
    while result < guess {
        guess = result;
        result = (x / guess + guess) / 2;
    }
    guess
}

pub fn sqrt_f64(x: f64) -> f64 {
    if x <= 0.0 { return 0.0; }
    let mut guess = x;
    for _ in 0..50 {
        guess = (guess + x / guess) / 2.0;
    }
    guess
}

// ── Trigonometry ──

pub fn sin(x: f64) -> f64 {
    let normalized = x % (2.0 * PI);
    let mut result = 0.0;
    let mut term = normalized;
    for n in 0..10 {
        result += term;
        term *= -normalized * normalized / ((2 * n + 2) as f64 * (2 * n + 3) as f64);
    }
    result
}

pub fn cos(x: f64) -> f64 { sin(x + PI / 2.0) }

pub fn tan(x: f64) -> f64 {
    let c = cos(x);
    if c == 0.0 { return 0.0; }
    sin(x) / c
}

// ── Logarithm / Exponential ──

pub fn log2(x: f64) -> f64 {
    if x <= 0.0 { return 0.0; }
    let mut result = 0.0;
    let mut val = x;
    while val >= 2.0 { val /= 2.0; result += 1.0; }
    while val < 1.0 { val *= 2.0; result -= 1.0; }
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

pub fn log(x: f64) -> f64 { log2(x) * LN2 }

pub fn exp(x: f64) -> f64 {
    let k = (x / LN2) as i64;
    let f = x - (k as f64) * LN2;
    let mut result = 1.0;
    let mut term = 1.0;
    for n in 1..20 {
        term *= f * LN2 / (n as f64);
        result += term;
    }
    let mut pow2 = 1.0;
    let mut kk = if k >= 0 { k } else { -k };
    let mut base = 2.0_f64;
    while kk > 0 {
        if kk & 1 == 1 { pow2 *= base; }
        base *= base;
        kk >>= 1;
    }
    if k < 0 { result / pow2 } else { result * pow2 }
}

// ── Export table type ──

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
