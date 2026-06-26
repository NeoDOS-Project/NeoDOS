#![no_std]
#![no_main]

use libneodos::console;
use libneodos::syscall;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u32(mut v: u32) {
    if v == 0 { write_str(b"0"); return; }
    let mut buf = [0u8; 10];
    let mut i = 9;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

#[used]
#[link_section = ".rodata"]
static PROGRESS_HELP: &[u8] = b"::HELP::\
PROGRESS [n]\r\n\
  Demo de barras de progreso.\r\n\
  n  Numero de barras (1-8, default 3).\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nPROGRESS [n]\r\n");
    write_str(b"  Demo de barras de progreso.\r\n");
    write_str(b"  n  Numero de barras (1-8, default 3).\r\n\r\n");
}

fn args_to_slice(buf: &[u8; 256]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    &buf[..end]
}

fn parse_u32(s: &[u8]) -> Option<u32> {
    if s.is_empty() { return None; }
    let mut n: u32 = 0;
    for &b in s {
        if b < b'0' || b > b'9' { return None; }
        n = n.saturating_mul(10).saturating_add((b - b'0') as u32);
    }
    Some(n)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }
    let args = args_to_slice(&raw);

    let num_bars = if args.is_empty() {
        3u32
    } else {
        match parse_u32(libneodos::args::trim_ascii(args)) {
            Some(n) if n >= 1 && n <= 8 => n,
            _ => {
                write_str(b"\r\nUso: PROGRESS [n]  (n = 1-8)\r\n");
                syscall::sys_exit(1);
            }
        }
    };

    write_str(b"\r\nBarras de progreso: ");
    write_u32(num_bars);
    write_str(b"\r\n\r\n");

    let titles: &[&str] = &[
        "Alfa  ",
        "Beta  ",
        "Gamma ",
        "Delta ",
        "Epsilon",
        "Zeta  ",
        "Eta   ",
        "Theta ",
    ][..num_bars as usize];

    let totals: [u64; 8] = [100, 80, 60, 50, 40, 30, 20, 10];

    let mut ids = [0i32; 8];
    let mut currents = [0u64; 8];
    let mut done = 0u32;

    for (i, t) in titles.iter().enumerate() {
        ids[i] = console::progress_create(t, totals[i]);
    }

    while done < num_bars {
        for i in 0..num_bars as usize {
            if currents[i] < totals[i] {
                currents[i] = core::cmp::min(currents[i] + 1, totals[i]);
                console::progress_update(ids[i], currents[i]);
                if currents[i] >= totals[i] {
                    console::progress_set_message(ids[i], "listo!");
                    done += 1;
                }
            }
        }
        for _ in 0..3 {
            syscall::sys_yield();
        }
    }

    for i in 0..num_bars as usize {
        console::progress_set_message(ids[i], "completado");
        console::progress_finish(ids[i]);
    }

    write_str(b"\r\nTodo listo!\r\n");
    syscall::sys_exit(0)
}
