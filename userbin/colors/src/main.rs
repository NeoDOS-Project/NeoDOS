#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::console;
use libneodos::syscall;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

#[used]
#[link_section = ".rodata"]
static COLORS_HELP: &[u8] = b"::HELP::\
COLORS\r\n\
  Display 256-color palette and test truecolor.\r\n\
::END::";

fn itoa(mut n: u8, buf: &mut [u8]) -> usize {
    if n == 0 { buf[0] = b'0'; return 1; }
    let mut i = 0;
    if n >= 100 { buf[i] = b'0' + n / 100; i += 1; n %= 100; }
    if n >= 10 || i > 0 { buf[i] = b'0' + n / 10; i += 1; n %= 10; }
    buf[i] = b'0' + n; i + 1
}

fn set_fg_256(index: u8) {
    let mut buf = [0u8; 16];
    buf[0] = 0x1B; buf[1] = b'['; buf[2] = b'3'; buf[3] = b'8';
    buf[4] = b';'; buf[5] = b'5'; buf[6] = b';';
    let mut tmp = [0u8; 4];
    let d = itoa(index, &mut tmp);
    buf[7..7 + d].copy_from_slice(&tmp[..d]);
    buf[7 + d] = b'm';
    write_str(&buf[..8 + d]);
}

fn set_bg_256(index: u8) {
    let mut buf = [0u8; 16];
    buf[0] = 0x1B; buf[1] = b'['; buf[2] = b'4'; buf[3] = b'8';
    buf[4] = b';'; buf[5] = b'5'; buf[6] = b';';
    let mut tmp = [0u8; 4];
    let d = itoa(index, &mut tmp);
    buf[7..7 + d].copy_from_slice(&tmp[..d]);
    buf[7 + d] = b'm';
    write_str(&buf[..8 + d]);
}

fn set_both_256(fg: u8, bg: u8) {
    let mut buf = [0u8; 28];
    buf[0] = 0x1B; buf[1] = b'['; buf[2] = b'3'; buf[3] = b'8';
    buf[4] = b';'; buf[5] = b'5'; buf[6] = b';';
    let mut pos = 7;
    let mut tmp = [0u8; 4];
    let d = itoa(fg, &mut tmp);
    buf[pos..pos + d].copy_from_slice(&tmp[..d]); pos += d;
    buf[pos] = b';'; pos += 1;
    buf[pos] = b'4'; pos += 1; buf[pos] = b'8'; pos += 1;
    buf[pos] = b';'; pos += 1; buf[pos] = b'5'; pos += 1; buf[pos] = b';'; pos += 1;
    let d = itoa(bg, &mut tmp);
    buf[pos..pos + d].copy_from_slice(&tmp[..d]); pos += d;
    buf[pos] = b'm'; pos += 1;
    write_str(&buf[..pos]);
}

fn set_bg_truecolor(r: u8, g: u8, b: u8) {
    let mut buf = [0u8; 28];
    buf[0] = 0x1B; buf[1] = b'['; buf[2] = b'4'; buf[3] = b'8';
    buf[4] = b';'; buf[5] = b'2'; buf[6] = b';';
    let mut pos = 7;
    let mut tmp = [0u8; 4];
    let d = itoa(r, &mut tmp);
    buf[pos..pos + d].copy_from_slice(&tmp[..d]); pos += d;
    buf[pos] = b';'; pos += 1;
    let d = itoa(g, &mut tmp);
    buf[pos..pos + d].copy_from_slice(&tmp[..d]); pos += d;
    buf[pos] = b';'; pos += 1;
    let d = itoa(b, &mut tmp);
    buf[pos..pos + d].copy_from_slice(&tmp[..d]); pos += d;
    buf[pos] = b'm'; pos += 1;
    write_str(&buf[..pos]);
}

fn show_palette_256() {
    write_str(b"\r\n=== 256-color palette (BG) ===\r\n\r\n");
    for row in 0..16 {
        for col in 0..16 {
            let index = (row * 16 + col) as u8;
            set_bg_256(index);
            write_str(b"  ");
        }
        write_str(b"\x1b[0m\r\n");
    }
    write_str(b"\x1b[0m\r\n");

    write_str(b"=== 256-color palette (FG) ===\r\n\r\n");
    write_str(b"\x1b[48;5;0m");
    for row in 0..16 {
        for col in 0..16 {
            let index = (row * 16 + col) as u8;
            set_fg_256(index);
            write_str(b"XX");
        }
        write_str(b"\x1b[0m\x1b[48;5;0m");
        if row < 15 { write_str(b"\r\n"); }
    }
    write_str(b"\x1b[0m\r\n\r\n");

    write_str(b"=== FG/BG combined samples ===\r\n\r\n");
    let samples: &[(u8, u8, &[u8])] = &[
        (15, 196, b" Red on coral "),
        (0, 226, b" Black on yellow "),
        (231, 21, b" White on blue "),
        (46, 89, b" Green on purple "),
        (226, 17, b" Yellow on navy "),
        (87, 52, b" Cyan on brown "),
    ];
    for &(fg, bg, label) in samples {
        set_both_256(fg, bg);
        write_str(label);
    }
    write_str(b"\x1b[0m\r\n\r\n");
}

fn show_truecolor_gradient() {
    write_str(b"Truecolor gradient (red -> green -> blue):\r\n\r\n");
    for step in 0..64 {
        let r = if step < 32 { 255 - step * 8 } else { 0 };
        let g = if step < 16 { step * 16 } else if step < 48 { 255 - (step - 16) * 8 } else { 0 };
        let b = if step < 32 { 0 } else { (step - 32) * 8 };
        set_bg_truecolor(r, g, b);
        write_str(b" ");
    }
    write_str(b"\x1b[0m\r\n\r\n");

    write_str(b"Color bars (R/G/B channels):\r\n\r\n");
    for channel in 0..3 {
        let label = if channel == 0 { b"R: " } else if channel == 1 { b"G: " } else { b"B: " };
        write_str(label);
        for step in 0..32 {
            let v = step * 8;
            let (r, g, b) = match channel {
                0 => (v, 0, 0),
                1 => (0, v, 0),
                _ => (0, 0, v),
            };
            set_bg_truecolor(r, g, b);
            write_str(b" ");
        }
        write_str(b"\x1b[0m\r\n");
    }
    write_str(b"\x1b[0m\r\n");
}

fn print_help() {
    write_str(b"\r\nCOLORS\r\n  Display 256-color palette and test truecolor.\r\n");
    write_str(b"  Usage:\r\n");
    write_str(b"    COLORS       show 256-color palette\r\n");
    write_str(b"    COLORS /256  show 256-color palette grid\r\n");
    write_str(b"    COLORS /true show truecolor gradient\r\n");
    write_str(b"    COLORS /all  show both\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    let args = libneodos::args::trim_ascii(&raw);

    if libneodos::args::is_help_flag(args) {
        print_help();
        syscall::sys_exit(0);
    }

    let show_all = args.is_empty() || args.eq_ignore_ascii_case(b"/ALL");
    let show_256 = show_all || args.eq_ignore_ascii_case(b"/256");
    let show_true = show_all || args.eq_ignore_ascii_case(b"/TRUE");

    if show_256 {
        show_palette_256();
    }
    if show_true {
        show_truecolor_gradient();
    }
    if !show_256 && !show_true {
        write_str(b"\r\nUnknown option. Use COLORS /? for help.\r\n");
    }

    syscall::sys_exit(0)
}
