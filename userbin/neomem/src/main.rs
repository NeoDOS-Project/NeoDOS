#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::i18n;
use libneodos::syscall;
use libneodos::syscall::{MemInfo, ObInfoClass, ob_access};
use libneodos::tr_id;

const APP_NAME: &str = "neomem";
const IDS_TOTAL: u32 = 1004;
const IDS_USED: u32 = 1005;
const IDS_FREE: u32 = 1006;
const IDS_PHYSICAL: u32 = 1007;
const IDS_KERNEL: u32 = 1008;
const IDS_USER: u32 = 1009;
const IDS_PAGING: u32 = 1010;
const IDS_UNAVAIL: u32 = 1015;
const IDS_READ_FAIL: u32 = 1016;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_num(n: u64) {
    if n == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut v = n;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    write_str(&buf[i..]);
}

/// Human-readable size, delegated to the shared units service in `math.nxl`.
fn write_size(bytes: u64) {
    let mut buf = [0u8; 32];
    let n = libmath::format_size(bytes, &mut buf);
    if n > 0 {
        write_str(&buf[..n]);
    }
}

/// KiB→bytes, delegated to the shared units service in `math.nxl`.
fn kib_to_bytes(kib: u64) -> u64 {
    libmath::kib_to_bytes(kib)
}

/// Print `Total / Used / Free` for a KiB-valued triple (kernel `MemoryStats`).
fn print_field_kib(label: &[u8], total_kib: u64, used_kib: u64, free_kib: u64) {
    write_str(b"  ");
    write_str(label);
    write_size(kib_to_bytes(total_kib));
    write_str(b", ");
    write_str(tr_id!(IDS_USED).as_bytes());
    write_size(kib_to_bytes(used_kib));
    write_str(b", ");
    write_str(tr_id!(IDS_FREE).as_bytes());
    write_size(kib_to_bytes(free_kib));
    write_str(b"\r\n");
}

/// Print `Total / Used / Free` for a page-count triple.
fn print_field_pages(total: u64, used: u64, free: u64) {
    write_str(b"  ");
    write_str(tr_id!(IDS_TOTAL).as_bytes());
    write_num(total);
    write_str(b", ");
    write_str(tr_id!(IDS_USED).as_bytes());
    write_num(used);
    write_str(b", ");
    write_str(tr_id!(IDS_FREE).as_bytes());
    write_num(free);
    write_str(b"\r\n");
}

fn print_help() {
    write_str(b"\r\nNEOMEM\r\n  Display system memory information.\r\n  Shows physical, kernel, user, and paging memory.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    let fd = match syscall::sys_ob_open("\\Global\\Info\\Memory", ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_UNAVAIL).as_bytes());
            write_str(b"\r\n\r\n");
            syscall::sys_exit(1);
        }
    };

    // Query straight into the shared `libneodos::syscall::MemInfo` so the layout
    // always matches the kernel's `MemoryStats` (15 fields / 120 bytes). The
    // previous private 13-field struct drifted from the ABI and produced garbage.
    let size = core::mem::size_of::<MemInfo>();
    let mut info: MemInfo = unsafe { core::mem::zeroed() };
    let buf = unsafe {
        core::slice::from_raw_parts_mut(&mut info as *mut MemInfo as *mut u8, size)
    };
    let n = match syscall::sys_ob_query_info(fd, ObInfoClass::Memory, buf) {
        Ok(n) => n,
        Err(_) => {
            let _ = syscall::sys_close(fd);
            write_str(b"\r\n");
            write_str(tr_id!(IDS_READ_FAIL).as_bytes());
            write_str(b"\r\n\r\n");
            syscall::sys_exit(1);
        }
    };
    let _ = syscall::sys_close(fd);

    if n < size {
        write_str(b"\r\n");
        write_str(tr_id!(IDS_READ_FAIL).as_bytes());
        write_str(b"\r\n\r\n");
        syscall::sys_exit(1);
    }

    write_str(b"\r\n");
    write_str(tr_id!(IDS_PHYSICAL).as_bytes());
    write_str(b"\r\n");
    print_field_kib(tr_id!(IDS_TOTAL).as_bytes(), info.total_kib, info.used_kib, info.free_kib);

    write_str(b"\r\n");
    write_str(tr_id!(IDS_KERNEL).as_bytes());
    write_str(b"\r\n");
    print_field_kib(
        tr_id!(IDS_TOTAL).as_bytes(),
        info.kernel_heap_total_kib,
        info.kernel_heap_used_kib,
        info.kernel_heap_free_kib,
    );

    write_str(b"\r\n");
    write_str(tr_id!(IDS_USER).as_bytes());
    write_str(b"\r\n");
    print_field_kib(
        tr_id!(IDS_TOTAL).as_bytes(),
        info.user_memory_total_kib,
        info.user_memory_used_kib,
        info.user_memory_free_kib,
    );

    write_str(b"\r\n");
    write_str(tr_id!(IDS_PAGING).as_bytes());
    write_str(b"\r\n");
    print_field_pages(info.total_pages, info.used_pages, info.free_pages);

    write_str(b"\r\n\r\n");
    syscall::sys_exit(0)
}
