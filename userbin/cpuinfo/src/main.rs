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
use libneodos::syscall::{self, ob_access, ObInfoClass};
use libneodos::tr_id;
use core::mem::size_of;

const APP_NAME: &str = "cpuinfo";
const IDS_CPU_INFO: u32 = 1004;
const IDS_TOPOLOGY: u32 = 1005;
const IDS_FEATURES: u32 = 1006;
const IDS_VENDOR: u32 = 1007;
const IDS_BRAND: u32 = 1008;
const IDS_CORES: u32 = 1009;
const IDS_THREADS: u32 = 1010;
const IDS_FAMILY: u32 = 1011;
const IDS_MODEL: u32 = 1012;
const IDS_STEPPING: u32 = 1013;
const IDS_YES: u32 = 1014;
const IDS_NO: u32 = 1015;
const IDS_UNAVAIL: u32 = 1016;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u64(mut v: u64) {
    if v == 0 { write_str(b"0"); return; }
    let mut buf = [0u8; 20];
    let mut i = 19;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

fn write_u32(v: u32) {
    write_u64(v as u64);
}

fn str_from_bytes(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..end]).unwrap_or("")
}

#[repr(C)]
struct CpuInfoFull {
    pub vendor_id: [u8; 12],
    pub brand: [u8; 48],
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub cpu_type: u32,
    pub features_edx: u32,
    pub features_ecx: u32,
    pub ext_features_edx: u32,
    pub ext_features_ecx: u32,
    pub features_ebx_leaf7: u32,
    pub phys_addr_bits: u8,
    pub virt_addr_bits: u8,
    pub cpu_count: u32,
    pub apic_id: u32,
    pub cpu_id: u32,
    pub is_bsp: bool,
    pub tsc_khz: u64,
    pub timer_source: u8,
    pub tick_rate_hz: u64,
}

fn has_feature(info: &CpuInfoFull, feat: &str) -> bool {
    match feat {
        "SSE"    => (info.features_edx >> 25) & 1 == 1,
        "SSE2"   => (info.features_edx >> 26) & 1 == 1,
        "SSE3"   => (info.features_ecx >> 0) & 1 == 1,
        "SSSE3"  => (info.features_ecx >> 9) & 1 == 1,
        "SSE4.1" => (info.features_ecx >> 19) & 1 == 1,
        "SSE4.2" => (info.features_ecx >> 20) & 1 == 1,
        "AVX"    => (info.features_ecx >> 28) & 1 == 1,
        "AVX2"   => (info.features_ebx_leaf7 >> 5) & 1 == 1,
        "AES"    => (info.features_ecx >> 25) & 1 == 1,
        "RDRAND" => (info.features_ecx >> 30) & 1 == 1,
        _ => false,
    }
}

fn print_help() {
    write_str(b"\r\nCPUINFO\r\n  Displays CPU information.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }

    let fd = match syscall::sys_ob_open("\\Global\\Info\\CpuInfo", ob_access::READ) {
        Ok(f) => f,
        Err(_) => {
            write_str(b"\r\n");
            write_str(tr_id!(IDS_UNAVAIL).as_bytes());
            write_str(b"\r\n\r\n");
            syscall::sys_exit(1);
        }
    };

    let mut buf = [0u8; size_of::<CpuInfoFull>()];
    let n = match syscall::sys_ob_query_info(fd, ObInfoClass::CpuInfo, &mut buf) {
        Ok(n) => n,
        Err(_) => {
            let _ = syscall::sys_close(fd);
            write_str(b"\r\n");
            write_str(tr_id!(IDS_UNAVAIL).as_bytes());
            write_str(b"\r\n\r\n");
            syscall::sys_exit(1);
        }
    };
    let _ = syscall::sys_close(fd);

    if n < size_of::<CpuInfoFull>() {
        write_str(b"\r\n");
        write_str(tr_id!(IDS_UNAVAIL).as_bytes());
        write_str(b"\r\n\r\n");
        syscall::sys_exit(1);
    }

    let info: &CpuInfoFull = unsafe { &*(buf.as_ptr() as *const CpuInfoFull) };

    write_str(b"\r\n");
    write_str(tr_id!(IDS_CPU_INFO).as_bytes());
    write_str(b"\r\n");
    write_str(b"  ");
    write_str(tr_id!(IDS_VENDOR).as_bytes());
    write_str(str_from_bytes(&info.vendor_id).as_bytes());
    write_str(b"\r\n");
    write_str(b"  ");
    write_str(tr_id!(IDS_BRAND).as_bytes());
    write_str(str_from_bytes(&info.brand).as_bytes());
    write_str(b"\r\n");

    write_str(b"\r\n");
    write_str(tr_id!(IDS_TOPOLOGY).as_bytes());
    write_str(b"\r\n");
    write_str(b"  ");
    write_str(tr_id!(IDS_CORES).as_bytes());
    write_u32(info.cpu_count);
    write_str(b"\r\n");
    write_str(b"  ");
    write_str(tr_id!(IDS_FAMILY).as_bytes());
    write_u32(info.family);
    write_str(b"\r\n");
    write_str(b"  ");
    write_str(tr_id!(IDS_MODEL).as_bytes());
    write_u32(info.model);
    write_str(b"\r\n");
    write_str(b"  ");
    write_str(tr_id!(IDS_STEPPING).as_bytes());
    write_u32(info.stepping);
    write_str(b"\r\n");

    write_str(b"\r\n");
    write_str(tr_id!(IDS_FEATURES).as_bytes());
    write_str(b"\r\n");
    for feat in &["SSE", "SSE2", "SSE3", "SSSE3", "SSE4.1", "SSE4.2", "AVX", "AVX2", "AES", "RDRAND"] {
        write_str(b"    ");
        write_str(feat.as_bytes());
        write_str(b": ");
        if has_feature(info, feat) {
            write_str(tr_id!(IDS_YES).as_bytes());
        } else {
            write_str(tr_id!(IDS_NO).as_bytes());
        }
        write_str(b"\r\n");
    }
    write_str(b"\r\n");

    syscall::sys_exit(0)
}
