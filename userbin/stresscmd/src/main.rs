#![no_std]
#![no_main]

use libneodos::syscall;

const ARGS_ADDR: u64 = 0x41F000;
const TOTAL_CMDS: u32 = 300;

macro_rules! prog {
    ($path:literal) => { Prog { path: $path, args: b"" } };
    ($path:literal, $args:literal) => { Prog { path: $path, args: $args } };
}

struct Prog {
    path: &'static [u8],
    args: &'static [u8],
}

static PROGRAMS: &[Prog] = &[
    prog!(b"C:\\Programs\\ver.nxe"),
    prog!(b"C:\\Programs\\vol.nxe"),
    prog!(b"C:\\Programs\\drives.nxe"),
    prog!(b"C:\\Programs\\datetime.nxe"),
    prog!(b"C:\\Programs\\neomem.nxe"),
    prog!(b"C:\\Programs\\ps.nxe"),
    prog!(b"C:\\Programs\\colors.nxe"),
    prog!(b"C:\\Programs\\cpuinfo.nxe"),
    prog!(b"C:\\Programs\\corecls.nxe"),
    prog!(b"C:\\Programs\\coredir.nxe", b"C:\\"),
    prog!(b"C:\\Programs\\echo.nxe", b"stress test"),
    prog!(b"C:\\Programs\\neotop.nxe"),
    prog!(b"C:\\Programs\\tree.nxe", b"C:\\Temp"),
];

fn to_ob_path<'a>(vfs: &[u8], buf: &'a mut [u8; 512]) -> &'a str {
    let p = b"\\Global\\FileSystem\\";
    let t = p.len() + vfs.len();
    if t > 510 { return ""; }
    buf[..p.len()].copy_from_slice(p);
    buf[p.len()..t].copy_from_slice(vfs);
    buf[t] = 0;
    unsafe { core::str::from_utf8_unchecked(&buf[..t]) }
}

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn uint_to_str(mut n: u32) -> [u8; 10] {
    let mut buf = [0u8; 10];
    let mut i = 9;
    if n == 0 {
        buf[9] = b'0';
        return buf;
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    buf
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;

    for i in 0..TOTAL_CMDS {
        let p = &PROGRAMS[(i as usize) % PROGRAMS.len()];

        unsafe {
            let d = ARGS_ADDR as *mut u8;
            d.write_bytes(0, 256);
            if !p.args.is_empty() {
                let n = p.args.len().min(255);
                core::ptr::copy_nonoverlapping(p.args.as_ptr(), d, n);
                d.add(n).write(0);
            }
        }

        let mut ob_buf = [0u8; 512];
        let ob_path = to_ob_path(p.path, &mut ob_buf);

        let packed = 0xFFu64 | (0xFFu64 << 8) | (0xFFu64 << 16);

        match syscall::sys_ob_create(ob_path, syscall::ob_type::PROCESS, None, packed) {
            Ok(fd) => {
                let _ = syscall::sys_ob_wait(fd);
                let _ = syscall::sys_close(fd);
                passed += 1;
            }
            Err(_) => {
                failed += 1;
            }
        }

        if (i + 1) % 50 == 0 || i == TOTAL_CMDS - 1 {
            let nbuf = uint_to_str(i + 1);
            let start = nbuf.iter().position(|&b| b != 0).unwrap_or(9);
            write_str(b"[STRESS] ");
            write_str(&nbuf[start..]);
            write_str(b"/300 commands (P:");
            let pb = uint_to_str(passed);
            let ps = pb.iter().position(|&b| b != 0).unwrap_or(9);
            write_str(&pb[ps..]);
            write_str(b" F:");
            let fb = uint_to_str(failed);
            let fs = fb.iter().position(|&b| b != 0).unwrap_or(9);
            write_str(&fb[fs..]);
            write_str(b")\r\n");
        }
    }

    write_str(b"\r\n[STRESS] ");
    let pb = uint_to_str(passed);
    let ps = pb.iter().position(|&b| b != 0).unwrap_or(9);
    write_str(&pb[ps..]);
    write_str(b" passed, ");
    let fb = uint_to_str(failed);
    let fs = fb.iter().position(|&b| b != 0).unwrap_or(9);
    write_str(&fb[fs..]);
    write_str(b" failed\r\n");

    if failed == 0 {
        write_str(b"[STRESS] ALL_STRESS_TESTS_PASSED\r\n");
    } else {
        write_str(b"[STRESS] SOME_STRESS_TESTS_FAILED\r\n");
    }
    write_str(b"STRESSCMD_COMPLETE\r\n");
    syscall::sys_exit(if failed == 0 { 0 } else { 1 })
}
