//! NeoDOS `math.nxl` client — thin wrapper over the shared math/units service.
//!
//! Mirrors `libnet`/`console`: lazily loads `math.nxl` via `sys_loadlib` and
//! dispatches through its export table (`base + 0`). The units API
//! (`kib_to_bytes`, `bytes_to_kib`, `scale_size`, `format_size`) lives in the
//! NXL so every tool shares one implementation at runtime.

#![no_std]
#![allow(clippy::missing_safety_doc)]

use core::sync::atomic::{AtomicU64, Ordering};
use libneodos::loadlib;

const MATH_NXL_PATH: &str = "C:\\System\\Libraries\\math.nxl\0";
const EXPORT_TABLE_OFFSET: u64 = 0x00;

static MATH_BASE: AtomicU64 = AtomicU64::new(0);

/// Mirrors `MathAbiTable` from `libmath-nxl` (version 2).
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
    pub kib_to_bytes: extern "C" fn(u64) -> u64,
    pub bytes_to_kib: extern "C" fn(u64) -> u64,
    pub scale_size: extern "C" fn(u64) -> u64,
    pub format_size: extern "C" fn(u64, *mut u8, usize) -> usize,
    pub _reserved: [u64; 4],
}

fn get_table() -> Option<&'static MathAbiTable> {
    let base = MATH_BASE.load(Ordering::Relaxed);
    if base != 0 {
        return Some(unsafe { &*((base + EXPORT_TABLE_OFFSET) as *const MathAbiTable) });
    }
    match loadlib(MATH_NXL_PATH) {
        Ok(base) => {
            MATH_BASE.store(base, Ordering::Relaxed);
            Some(unsafe { &*((base + EXPORT_TABLE_OFFSET) as *const MathAbiTable) })
        }
        Err(_) => None,
    }
}

/// True once `math.nxl` has been loaded by this process.
pub fn is_loaded() -> bool {
    MATH_BASE.load(Ordering::Relaxed) != 0
}

// ── Units of measurement ──

/// Convert KiB to bytes (0 when `math.nxl` is unavailable).
pub fn kib_to_bytes(kib: u64) -> u64 {
    match get_table() {
        Some(t) => (t.kib_to_bytes)(kib),
        None => 0,
    }
}

/// Convert bytes to whole KiB.
pub fn bytes_to_kib(bytes: u64) -> u64 {
    match get_table() {
        Some(t) => (t.bytes_to_kib)(bytes),
        None => 0,
    }
}

/// `(unit << 32) | value_x100` for `bytes` (unit: 0=B, 1=KB, 2=MB, 3=GB, 4=TB).
pub fn scale_size(bytes: u64) -> u64 {
    match get_table() {
        Some(t) => (t.scale_size)(bytes),
        None => 0,
    }
}

/// Format `bytes` as a human-readable size into `buf`; returns bytes written.
pub fn format_size(bytes: u64, buf: &mut [u8]) -> usize {
    match get_table() {
        Some(t) => (t.format_size)(bytes, buf.as_mut_ptr(), buf.len()),
        None => 0,
    }
}
