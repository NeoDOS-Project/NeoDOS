#![no_std]
#![no_main]

use libneodos::syscall::{self, ObEnumEntry, ObProcessInfo};
use libneodos::console;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u32(v: u32, width: usize) {
    let mut buf = [0u8; 12];
    let mut i = 11;
    let mut n = v;
    if n == 0 {
        buf[i] = b'0';
        if i == 0 { write_str(&buf[0..1]); return; }
        i -= 1;
    } else {
        while n > 0 {
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if i == 0 { break; }
            i -= 1;
        }
    }
    let digits_end = 12;
    let digits_start = i + 1;
    let digits_len = digits_end - digits_start;
    if digits_len >= width {
        write_str(&buf[digits_start..digits_end]);
    } else {
        for _ in 0..(width - digits_len) {
            write_str(b" ");
        }
        write_str(&buf[digits_start..digits_end]);
    }
}

fn build_proc_path(pid: u32, buf: &mut [u8; 128]) -> &str {
    let prefix = b"\\Process\\";
    let plen = prefix.len();
    buf[..plen].copy_from_slice(prefix);
    let mut i = plen;
    let mut n = pid;
    if n == 0 {
        buf[i] = b'0';
        i += 1;
    } else {
        let mut digits = [0u8; 10];
        let mut di = 10;
        while n > 0 {
            di -= 1;
            digits[di] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        while di < 10 {
            buf[i] = digits[di];
            i += 1;
            di += 1;
        }
    }
    buf[i] = 0;
    unsafe { core::str::from_utf8_unchecked(&buf[..i]) }
}

fn parse_pid_from_name(name: &str) -> Option<u32> {
    let num_part = if let Some(pos) = name.find('/') {
        &name[pos + 1..]
    } else {
        name
    };
    let mut n: u32 = 0;
    for &b in num_part.as_bytes() {
        if b < b'0' || b > b'9' { return None; }
        n = n * 10 + (b - b'0') as u32;
    }
    Some(n)
}

fn render_snapshot() {
    write_str(b"\x1b[2J\x1b[H");
    write_str(b"============================================================\r\n");
    write_str(b"              NeoTOP - System Monitor\r\n");
    write_str(b"============================================================\r\n");

    let dir_fd = match syscall::sys_ob_open("\\Process", libneodos::syscall::ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"Cannot open process list\r\n");
            return;
        }
    };

    let mut entries: [ObEnumEntry; 64] = core::array::from_fn(|_| ObEnumEntry {
        id: 0, obj_type: 0, name: [0u8; 32], mode: 0, _pad: [0u8; 2], size: 0,
    });

    let count = match syscall::sys_ob_enum(dir_fd, &mut entries) {
        Ok(c) => c,
        Err(_) => { let _ = syscall::sys_close(dir_fd); return; }
    };
    let _ = syscall::sys_close(dir_fd);

    write_str(b"\r\n");
    write_str(b" PID  PPID PRI THR STATE     \r\n");
    write_str(b" ---- ---- --- --- ----------\r\n");

    let mut path_buf = [0u8; 128];
    for i in 0..count.min(64) {
        let e_name = entries[i].name_str();
        let pid = match parse_pid_from_name(&e_name) {
            Some(p) => p,
            None => continue,
        };

        let proc_path = build_proc_path(pid, &mut path_buf);
        let proc_fd = match syscall::sys_ob_open(proc_path, libneodos::syscall::ob_access::READ) {
            Ok(f) => f,
            Err(_) => continue,
        };

        let mut info_buf = [0u8; 20];
        let written = match syscall::sys_ob_query_info(proc_fd,
            libneodos::syscall::ObInfoClass::Process, &mut info_buf) {
            Ok(w) => w,
            Err(_) => { let _ = syscall::sys_close(proc_fd); continue; }
        };
        let _ = syscall::sys_close(proc_fd);

        if written < 20 { continue; }
        let info: ObProcessInfo = unsafe { core::ptr::read(info_buf.as_ptr() as *const ObProcessInfo) };

        write_str(b" ");
        write_u32(info.pid, 3);
        write_str(b"  ");
        write_u32(info.parent_pid, 3);
        write_str(b"  ");
        write_str(info.priority_str().as_bytes());
        write_str(b" ");
        write_u32(info.thread_count, 2);
        write_str(b" ");
        let state = info.state_str();
        write_str(state.as_bytes());
        for _ in 0..(10 - state.len()) { write_str(b" "); }
        write_str(b"\r\n");
    }

    // Memory stats with console.nxl progress bar
    if let Ok(fd) = syscall::sys_ob_open("\\Global\\Info\\Memory", libneodos::syscall::ob_access::READ) {
        let mut mibuf = [0u8; 48];
        if syscall::sys_ob_query_info(fd, libneodos::syscall::ObInfoClass::Memory, &mut mibuf).is_ok() {
            let arr = unsafe { &*(mibuf.as_ptr() as *const [u64; 6]) };
            let total_kib = arr[1]; // MemoryStats.total_kib at offset 8
            let free_kib  = arr[3]; // MemoryStats.free_kib  at offset 24
            let used_kib  = arr[4]; // MemoryStats.used_kib  at offset 32
            let mut msg = [0u8; 64];
            let mp = b"total=";
            msg[..mp.len()].copy_from_slice(mp);
            let mut pos = mp.len();
            let mut n = total_kib;
            let mut digits = [0u8; 20];
            let mut nd = 0;
            if n == 0 { digits[nd] = b'0'; nd = 1; }
            while n > 0 { digits[nd] = b'0' + (n % 10) as u8; n /= 10; nd += 1; }
            for i in (0..nd).rev() { msg[pos] = digits[i]; pos += 1; }
            let sp = b" free=";
            msg[pos..pos+sp.len()].copy_from_slice(sp); pos += sp.len();
            n = free_kib;
            nd = 0;
            if n == 0 { digits[nd] = b'0'; nd = 1; }
            while n > 0 { digits[nd] = b'0' + (n % 10) as u8; n /= 10; nd += 1; }
            for i in (0..nd).rev() { msg[pos] = digits[i]; pos += 1; }
            let mem_id = console::progress_create("MEM", total_kib);
            console::progress_update(mem_id, used_kib);
            console::progress_set_message(mem_id,
                unsafe { core::str::from_utf8_unchecked(&msg[..pos]) });
            console::progress_finish(mem_id);
        }
        let _ = syscall::sys_close(fd);
    }

    write_str(b"============================================================\r\n");
}

#[used]
#[link_section = ".rodata"]
static NEOTOP_HELP: &[u8] = b"::HELP::\
NEOTOP [/W]\r\n\
  System monitor. Shows processes and memory.\r\n\
  /W    Watch mode: refresh every ~1 second (press any key to exit).\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nNEOTOP [/W]\r\n");
    write_str(b"  System monitor. Shows processes and memory.\r\n");
    write_str(b"  /W    Watch mode: refresh every ~1 second (press any key to exit).\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }

    let watch = raw.iter().any(|&b| b == b'w' || b == b'W');

    if !watch {
        render_snapshot();
        syscall::sys_exit(0);
    }

    loop {
        render_snapshot();
        // Busy-wait ~1 sec: yield ~10000 times (1000 Hz timer tick)
        for _ in 0..10000 {
            syscall::sys_yield();
        }
        // Quick non-blocking stdin check
        let mut ch = [0u8; 1];
        if let Ok(1) = syscall::sys_read(0, &mut ch) {
            if ch[0] == b'q' || ch[0] == b'\x1b' { break; }
        }
    }

    syscall::sys_exit(0)
}
