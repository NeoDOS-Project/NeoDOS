#![no_std]
#![no_main]

//! `uptime` — show the monotonic system uptime (#467).
//!
//! Unlike `datetime` (wall clock from the RTC), this reads the kernel's
//! monotonic uptime, which never decreases and is unaffected by clock steps.
//!
//! Usage:
//!   uptime          human form: "up 0h 3m 12s (192000 ms, tick 1000 Hz)"
//!   uptime -r       raw: the millisecond count only
//!   uptime -v       also print the high-resolution TSC clock

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use libneodos::syscall;

struct SbrkAlloc;
unsafe impl GlobalAlloc for SbrkAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(8) as i64;
        let ptr = libneodos::mem::sbrk(size).ok().unwrap_or(0) as *mut u8;
        if ptr.is_null() { core::ptr::null_mut() } else { ptr }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
#[global_allocator]
static ALLOC: SbrkAlloc = SbrkAlloc;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}
fn write_dec(mut v: u64) {
    let mut b = [0u8; 20];
    let mut i = b.len();
    if v == 0 { write_str(b"0"); return; }
    while v > 0 && i > 0 { i -= 1; b[i] = b'0' + (v % 10) as u8; v /= 10; }
    write_str(&b[i..]);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let args = libneodos::args::read_args();
    let raw = args.windows(2).any(|w| w == b"-r");
    let verbose = args.windows(2).any(|w| w == b"-v");

    let (ms, hz) = match syscall::ob_uptime() {
        Ok(v) => v,
        Err(_) => {
            write_str(b"[uptime] monotonic clock unavailable\r\n");
            syscall::sys_exit(1);
        }
    };

    if raw {
        write_dec(ms);
        write_str(b"\r\n");
    } else {
        let total_s = ms / 1000;
        let (d, h, m, s) = (
            total_s / 86400,
            (total_s % 86400) / 3600,
            (total_s % 3600) / 60,
            total_s % 60,
        );
        write_str(b"up ");
        if d > 0 { write_dec(d); write_str(b"d "); }
        write_dec(h); write_str(b"h ");
        write_dec(m); write_str(b"m ");
        write_dec(s); write_str(b"s");
        write_str(b" (monotonic ");
        write_dec(ms); write_str(b" ms, tick ");
        write_dec(hz as u64); write_str(b" Hz)\r\n");
    }

    if verbose {
        if let Ok((tsc, khz)) = syscall::ob_high_res_time() {
            write_str(b"high-res: tsc=");
            write_dec(tsc);
            write_str(b" tsc_khz=");
            write_dec(khz as u64);
            write_str(b"\r\n");
        }
    }
    syscall::sys_exit(0)
}
