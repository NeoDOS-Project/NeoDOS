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
use libneodos::syscall::{ObInfoClass, ob_access};
use libneodos::tr_id;

const APP_NAME: &str = "neomem";
const IDS_TOTAL: u32 = 1004;
const IDS_USED: u32 = 1005;
const IDS_FREE: u32 = 1006;
const IDS_PHYSICAL: u32 = 1007;
const IDS_KERNEL: u32 = 1008;
const IDS_USER: u32 = 1009;
const IDS_PAGING: u32 = 1010;
const IDS_HEAP: u32 = 1011;
const IDS_SLAB: u32 = 1012;
const IDS_PAGE_TABLES: u32 = 1013;
const IDS_CACHED: u32 = 1014;
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

fn write_size(bytes: u64) {
    if bytes >= 1024 * 1024 * 1024 {
        let gb = bytes / (1024 * 1024 * 1024);
        let rem = (bytes % (1024 * 1024 * 1024)) * 100 / (1024 * 1024 * 1024);
        write_num(gb);
        write_str(b".");
        if rem < 10 { write_str(b"0"); }
        write_num(rem);
        write_str(b" GB");
    } else if bytes >= 1024 * 1024 {
        let mb = bytes / (1024 * 1024);
        write_num(mb);
        write_str(b" MB");
    } else if bytes >= 1024 {
        let kb = bytes / 1024;
        write_num(kb);
        write_str(b" KB");
    } else {
        write_num(bytes);
        write_str(b" B");
    }
}

fn print_field(label: &[u8], total: u64, used: u64) {
    write_str(b"  ");
    write_str(label);
    write_str(tr_id!(IDS_TOTAL).as_bytes());
    write_size(total);
    write_str(b", ");
    write_str(tr_id!(IDS_USED).as_bytes());
    write_size(used);
    write_str(b", ");
    write_str(tr_id!(IDS_FREE).as_bytes());
    write_size(total - used);
    write_str(b"\r\n");
}

fn print_help() {
    write_str(b"\r\nNEOMEM\r\n  Display system memory information.\r\n  Shows physical, kernel, user, and paging memory.\r\n\r\n");
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemInfo {
    physical_total: u64,
    physical_used: u64,
    kernel_total: u64,
    kernel_used: u64,
    user_total: u64,
    user_used: u64,
    page_total: u64,
    page_used: u64,
    heap_total: u64,
    heap_used: u64,
    slab_used: u64,
    page_table_used: u64,
    cached: u64,
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

    let mut buf = [0u8; core::mem::size_of::<MemInfo>()];
    let n = match syscall::sys_ob_query_info(fd, ObInfoClass::Memory, &mut buf) {
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

    if n < core::mem::size_of::<MemInfo>() {
        write_str(b"\r\n");
        write_str(tr_id!(IDS_READ_FAIL).as_bytes());
        write_str(b"\r\n\r\n");
        syscall::sys_exit(1);
    }

    let info: &MemInfo = unsafe { &*(buf.as_ptr() as *const MemInfo) };

    write_str(b"\r\n");
    write_str(tr_id!(IDS_PHYSICAL).as_bytes());
    write_str(b"\r\n");
    print_field(tr_id!(IDS_TOTAL).as_bytes(), info.physical_total, info.physical_used);

    write_str(b"\r\n");
    write_str(tr_id!(IDS_KERNEL).as_bytes());
    write_str(b"\r\n");
    print_field(tr_id!(IDS_TOTAL).as_bytes(), info.kernel_total, info.kernel_used);

    write_str(b"\r\n");
    write_str(tr_id!(IDS_USER).as_bytes());
    write_str(b"\r\n");
    print_field(tr_id!(IDS_TOTAL).as_bytes(), info.user_total, info.user_used);

    write_str(b"\r\n");
    write_str(tr_id!(IDS_PAGING).as_bytes());
    write_str(b"\r\n");
    print_field(tr_id!(IDS_TOTAL).as_bytes(), info.page_total, info.page_used);

    write_str(b"\r\n");
    write_str(tr_id!(IDS_HEAP).as_bytes());
    write_str(b": ");
    write_size(info.heap_total);
    write_str(b" (");
    write_str(tr_id!(IDS_USED).as_bytes());
    write_size(info.heap_used);
    write_str(b")\r\n");

    write_str(tr_id!(IDS_SLAB).as_bytes());
    write_str(b": ");
    write_size(info.slab_used);
    write_str(b"\r\n");

    write_str(tr_id!(IDS_PAGE_TABLES).as_bytes());
    write_str(b": ");
    write_size(info.page_table_used);
    write_str(b"\r\n");

    write_str(tr_id!(IDS_CACHED).as_bytes());
    write_str(b": ");
    write_size(info.cached);
    write_str(b"\r\n\r\n");

    syscall::sys_exit(0)
}
