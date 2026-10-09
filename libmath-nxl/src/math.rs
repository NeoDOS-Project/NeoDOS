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

// ── Units of measurement ──
//
// Shared by userland tools (neomem, neotop, drives, ipconfig) so the
// bytes→human-readable-size logic lives in one place and is reused at runtime
// through `math.nxl` (see `libmath`).

/// Bytes per unit for B, KB, MB, GB, TB (binary multiples).
const UNIT_BASES: [u64; 5] = [
    1,
    1024,
    1024 * 1024,
    1024 * 1024 * 1024,
    1024 * 1024 * 1024 * 1024,
];

pub const UNIT_B: u32 = 0;
pub const UNIT_KB: u32 = 1;
pub const UNIT_MB: u32 = 2;
pub const UNIT_GB: u32 = 3;
pub const UNIT_TB: u32 = 4;

pub fn kib_to_bytes(kib: u64) -> u64 { kib.saturating_mul(1024) }
pub fn bytes_to_kib(bytes: u64) -> u64 { bytes / 1024 }

/// Choose the largest unit for `bytes` and return `(unit << 32) | value_x100`,
/// i.e. the value scaled by 100 (two decimals) packed with the unit index.
pub fn scale_size(bytes: u64) -> u64 {
    let mut unit: usize = 0;
    let mut i = UNIT_BASES.len() - 1;
    while i > 0 {
        if bytes >= UNIT_BASES[i] {
            unit = i;
            break;
        }
        i -= 1;
    }
    let value_x100 = ((bytes as u128) * 100 / UNIT_BASES[unit] as u128) as u64;
    ((unit as u64) << 32) | (value_x100 & 0xFFFF_FFFF)
}

fn put_dec(mut v: u64, buf: &mut [u8], pos: &mut usize) {
    if v == 0 {
        if *pos < buf.len() { buf[*pos] = b'0'; *pos += 1; }
        return;
    }
    let mut tmp = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        tmp[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
    }
    while n > 0 {
        n -= 1;
        if *pos < buf.len() { buf[*pos] = tmp[n]; *pos += 1; }
    }
}

fn put_str(s: &[u8], buf: &mut [u8], pos: &mut usize) {
    for &b in s {
        if *pos < buf.len() { buf[*pos] = b; *pos += 1; }
    }
}

/// Format `bytes` as a human-readable size into `buf`, returning bytes written
/// (truncated to `buf.len()`). Policy: two decimals for GB/TB, integer else.
pub fn format_size(bytes: u64, buf: &mut [u8]) -> usize {
    let packed = scale_size(bytes);
    let unit = (packed >> 32) as usize;
    let value_x100 = (packed & 0xFFFF_FFFF) as u64;
    let mut pos = 0usize;
    if unit >= UNIT_GB as usize {
        put_dec(value_x100 / 100, buf, &mut pos);
        put_str(b".", buf, &mut pos);
        let frac = value_x100 % 100;
        if frac < 10 { put_str(b"0", buf, &mut pos); }
        put_dec(frac, buf, &mut pos);
    } else {
        put_dec(value_x100 / 100, buf, &mut pos);
    }
    const SUFFIX: [&[u8]; 5] = [b" B", b" KB", b" MB", b" GB", b" TB"];
    put_str(SUFFIX[unit], buf, &mut pos);
    pos.min(buf.len())
}

/// Compact human size for fixed-width UI columns: `whole.frac<unit>` with a
/// single decimal and a minimum unit of K (KiB), e.g. `0.0K`, `1.5K`, `3.5M`.
/// No padding — column alignment is a UI concern.
pub fn format_size_compact(bytes: u64, buf: &mut [u8]) -> usize {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;
    let (base, unit): (u64, u8) = if bytes >= TIB {
        (TIB, b'T')
    } else if bytes >= GIB {
        (GIB, b'G')
    } else if bytes >= MIB {
        (MIB, b'M')
    } else {
        (KIB, b'K')
    };
    let val10 = ((bytes as u128) * 10 / base as u128) as u64;
    let mut pos = 0usize;
    put_dec(val10 / 10, buf, &mut pos);
    put_str(b".", buf, &mut pos);
    put_dec(val10 % 10, buf, &mut pos);
    put_str(&[unit], buf, &mut pos);
    pos.min(buf.len())
}

// ── Numeric formatting ──

/// Write a decimal `u64` into `buf` (left-aligned); returns bytes written.
pub fn format_u64(v: u64, buf: &mut [u8]) -> usize {
    let mut pos = 0usize;
    put_dec(v, buf, &mut pos);
    pos.min(buf.len())
}

/// Write a `×10` percentage (`1374` → `137.4`) into `buf`; returns bytes written.
pub fn format_percent_x10(x10: u64, buf: &mut [u8]) -> usize {
    let mut pos = 0usize;
    put_dec(x10 / 10, buf, &mut pos);
    put_str(b".", buf, &mut pos);
    put_dec(x10 % 10, buf, &mut pos);
    pos.min(buf.len())
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
    // Units of measurement
    pub kib_to_bytes: extern "C" fn(u64) -> u64,
    pub bytes_to_kib: extern "C" fn(u64) -> u64,
    pub scale_size: extern "C" fn(u64) -> u64,
    pub format_size: extern "C" fn(u64, *mut u8, usize) -> usize,
    pub format_size_compact: extern "C" fn(u64, *mut u8, usize) -> usize,
    pub format_u64: extern "C" fn(u64, *mut u8, usize) -> usize,
    pub format_percent_x10: extern "C" fn(u64, *mut u8, usize) -> usize,
    pub _reserved: [u64; 4],
}
