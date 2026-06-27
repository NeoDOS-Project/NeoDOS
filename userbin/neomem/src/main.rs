#![no_std]
#![no_main]

use libneodos::syscall::{
    self, ob_access,
    sys_ob_open, sys_close, ObInfoClass,
};

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u64(v: u64) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = v;
    let mut i = 20;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    write_str(&buf[i..]);
}

/// Write a value in KiB with automatic unit conversion to KB/MB/GB/TB.
/// Uses integer arithmetic, shows one decimal for non-exact conversions.
fn write_kib_value(v: u64) {
    if v == 0 {
        write_str(b"0 B");
        return;
    }
    // 1 KiB = 1024 bytes, but the kernel reports in KiB.
    // We format based on magnitude:
    //   < 1     KiB -> bytes (multiply by 1024)
    //   < 1024  KiB -> show as KiB (v)
    //   < 1 MiB (1048576 KiB) -> show as MB (v / 1024)
    //   < 1 GiB -> show as GB (v / 1048576)
    //   else    -> show as TB (v / 1073741824)

    if v < 1024 {
        write_u64(v);
        write_str(b" KB");
    } else if v < 1048576 {
        // Show in MB
        let mb = v / 1024;
        let frac = ((v % 1024) * 10) / 1024;
        write_u64(mb);
        if frac > 0 {
            write_str(b".");
            write_u64(frac);
        }
        write_str(b" MB");
    } else if v < 1073741824 {
        // Show in GB
        let gb = v / 1048576;
        let frac = ((v % 1048576) * 10) / 1048576;
        write_u64(gb);
        if frac > 0 {
            write_str(b".");
            write_u64(frac);
        }
        write_str(b" GB");
    } else {
        // Show in TB
        let tb = v / 1073741824;
        let frac = ((v % 1073741824) * 10) / 1073741824;
        write_u64(tb);
        if frac > 0 {
            write_str(b".");
            write_u64(frac);
        }
        write_str(b" TB");
    }
}

fn write_line_kib(label: &[u8], value_kib: u64) {
    write_str(b"  ");
    write_str(label);
    write_str(b": ");
    write_kib_value(value_kib);
    write_str(b"\r\n");
}

fn write_line(label: &[u8], value: u64, suffix: &[u8]) {
    write_str(b"  ");
    write_str(label);
    write_str(b": ");
    write_u64(value);
    write_str(suffix);
    write_str(b"\r\n");
}

#[used]
#[link_section = ".rodata"]
static NEOMEM_HELP: &[u8] = b"::HELP::\
NEOMEM\r\n\
  NeoDOS Memory Diagnostics. Displays physical memory, kernel heap,\r\n\
  user memory, and paging statistics.\r\n\
\r\n\
NEOMEM /?\r\n\
  Show this help.\r\n\
::END::";

fn print_help() {
    write_str(b"\r\nNEOMEM - NeoDOS Memory Diagnostics v0.1\r\n");
    write_str(b"Shows physical memory, kernel heap, user memory, and paging stats.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    let fd = match sys_ob_open("\\Global\\Info\\Memory", ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"\r\nMemory info not available\r\n\r\n");
            syscall::sys_exit(1);
        }
    };

    // Read the extended MemoryStats struct (15 u64s = 120 bytes)
    let mut info = syscall::MemInfo {
        phys_max: 0, total_kib: 0, usable_kib: 0,
        free_kib: 0, used_kib: 0, reserved_kib: 0,
        kernel_heap_total_kib: 0, kernel_heap_used_kib: 0, kernel_heap_free_kib: 0,
        user_memory_total_kib: 0, user_memory_used_kib: 0, user_memory_free_kib: 0,
        total_pages: 0, free_pages: 0, used_pages: 0,
    };
    let info_sz = core::mem::size_of::<syscall::MemInfo>();
    let buf = unsafe {
        core::slice::from_raw_parts_mut(&mut info as *mut syscall::MemInfo as *mut u8, info_sz)
    };
    let n = match syscall::sys_ob_query_info(fd, ObInfoClass::Memory, buf) {
        Ok(n) => n,
        Err(_) => {
            let _ = sys_close(fd);
            write_str(b"\r\nMemory info read failed\r\n\r\n");
            syscall::sys_exit(1);
        }
    };

    let _ = sys_close(fd);

    if n < 48 {
        write_str(b"\r\nMemory info truncated\r\n\r\n");
        syscall::sys_exit(1);
    }

    // ── Physical Memory ──
    write_str(b"\r\n");
    write_str(b"Physical Memory\r\n");
    write_str(b"---------------\r\n");
    write_line_kib(b"Total", info.total_kib);
    write_line_kib(b"Used ", info.used_kib);
    write_line_kib(b"Free ", info.free_kib);
    write_str(b"\r\n");

    // ── Kernel Memory ──
    write_str(b"Kernel Memory\r\n");
    write_str(b"-------------\r\n");
    write_line_kib(b"Heap Total", info.kernel_heap_total_kib);
    write_line_kib(b"Heap Used ", info.kernel_heap_used_kib);
    write_line_kib(b"Heap Free ", info.kernel_heap_free_kib);
    write_str(b"\r\n");

    // ── User Memory ──
    write_str(b"User Memory\r\n");
    write_str(b"-----------\r\n");
    write_line_kib(b"Total", info.user_memory_total_kib);
    write_line_kib(b"Used ", info.user_memory_used_kib);
    write_line_kib(b"Free ", info.user_memory_free_kib);
    write_str(b"\r\n");

    // ── Paging ──
    if n >= info_sz {
        write_str(b"Paging\r\n");
        write_str(b"------\r\n");
        write_line(b"Total Pages", info.total_pages, b"");
        write_line(b"Used  Pages", info.used_pages, b"");
        write_line(b"Free  Pages", info.free_pages, b"");
        write_str(b"\r\n");
    }

    syscall::sys_exit(0)
}
