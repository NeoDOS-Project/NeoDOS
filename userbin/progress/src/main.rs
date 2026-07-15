#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use core::fmt::Write;
use libneodos::i18n;
use libneodos::syscall;
use libneodos::tr_id;

const APP_NAME: &str = "progress";
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_USAGE_LINE3: u32 = 1003;
const IDS_TITLE: u32 = 1004;
const IDS_DONE: u32 = 1005;
const IDS_COMPLETED: u32 = 1006;
const IDS_ALL_DONE: u32 = 1007;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

struct BufWriter<'a>(&'a mut [u8], &'a mut usize);

impl<'a> Write for BufWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let avail = self.0.len() - *self.1;
        let n = bytes.len().min(avail);
        self.0[*self.1..*self.1 + n].copy_from_slice(&bytes[..n]);
        *self.1 += n;
        Ok(())
    }
}

fn progress_bar(percent: u8) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let mut pos = 0;
    buf[pos] = b'['; pos += 1;
    let filled = (percent as usize) / 5;
    let mut i = 0;
    while i < filled && i < 20 {
        buf[pos] = b'#'; pos += 1; i += 1;
    }
    while i < 20 {
        buf[pos] = b'.'; pos += 1; i += 1;
    }
    buf[pos] = b']'; pos += 1;
    buf[pos] = b' '; pos += 1;
    let tens = percent / 10;
    let ones = percent % 10;
    buf[pos] = b'0' + tens; pos += 1;
    buf[pos] = b'0' + ones; pos += 1;
    buf[pos] = b'%'; pos += 1;
    buf
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        write_str(b"\r\n");
        write_str(tr_id!(IDS_USAGE).as_bytes());
        write_str(b"\r\n");
        write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
        write_str(b"\r\n");
        write_str(tr_id!(IDS_USAGE_LINE3).as_bytes());
        write_str(b"\r\n\r\n");
        syscall::sys_exit(0);
    }

    let mut nbars = 3usize;
    let arg_slice = libneodos::args::trim_ascii(&raw);
    if !arg_slice.is_empty() {
        if let Ok(v) = core::str::from_utf8(arg_slice).unwrap_or("").parse::<usize>() {
            if v >= 1 && v <= 8 { nbars = v; }
        }
    }

    write_str(b"\r\n");
    write_str(tr_id!(IDS_TITLE).as_bytes());
    write_str(b"\r\n\r\n");

    for bar in 0..nbars {
        let label = [b"A", b"B", b"C", b"D", b"E", b"F", b"G", b"H"][bar];
        write_str(b"  [");
        write_str(&[label[0]]);
        write_str(b"] ");
        for p in 0..=100 {
            let bar_bytes = progress_bar(p as u8);
            write_str(b"\r");
            write_str(&bar_bytes);
            for _ in 0..10000000 { core::hint::spin_loop(); }
        }
        write_str(tr_id!(IDS_DONE).as_bytes());
        write_str(b"\r\n");
    }

    write_str(b"\r\n");
    write_str(tr_id!(IDS_ALL_DONE).as_bytes());
    write_str(b"\r\n\r\n");
    syscall::sys_exit(0)
}
