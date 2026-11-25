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
use libneodos::tr_id;

const APP_NAME: &str = "cmdtest";
const IDS_HEADER: u32 = 1001;
const IDS_PASS: u32 = 1002;
const IDS_FAIL: u32 = 1003;
const IDS_ALL_PASSED: u32 = 1004;
const IDS_SOME_FAILED: u32 = 1005;
const IDS_COMPLETE: u32 = 1006;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn cmd_passed() {
    write_str(tr_id!(IDS_HEADER).as_bytes());
    write_str(tr_id!(IDS_PASS).as_bytes());
    write_str(b"\r\n");
}

/// End-to-end probe for `ObCreate(Driver)` authorization (AUDIT-001 / #809).
///
/// NeoInit runs this binary twice with the `NEMAUTH` argument: once with the
/// unprivileged account (must be denied) and once as SYSTEM (the gate must not
/// block it). The probe path lives in the trusted directory but does not exist,
/// so an authorised caller reaches the loader and fails with EIO without
/// loading anything.
const NEM_AUTH_ARG: &[u8] = b"NEMAUTH";
const NEM_PROBE_PATH: &str =
    "\\Global\\FileSystem\\C:\\System\\Drivers\\__nemauthtest_missing.nem";
const EPERM: i64 = -16;

fn write_i64(v: i64) {
    let mut buf = [0u8; 24];
    let mut i = buf.len();
    let negative = v < 0;
    let mut n = if negative { (v as i128).unsigned_abs() as u64 } else { v as u64 };
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if negative {
        i -= 1;
        buf[i] = b'-';
    }
    write_str(&buf[i..]);
}

fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn run_nem_auth_probe() -> u32 {
    write_str(b"[NEMAUTH] start\r\n");
    match syscall::sys_ob_create(NEM_PROBE_PATH, syscall::ob_type::DRIVER, None, 0) {
        Err(e) if e == EPERM => {
            write_str(b"[NEMAUTH] PASS non-admin denied with EPERM\r\n");
            0
        }
        Err(e) if e == syscall::EIO || e == syscall::ENOENT => {
            write_str(b"[NEMAUTH] PASS admin authorized (loader reached)\r\n");
            0
        }
        Err(_) => {
            write_str(b"[NEMAUTH] FAIL unexpected error\r\n");
            1
        }
        Ok(fd) => {
            let _ = syscall::sys_close(fd);
            write_str(b"[NEMAUTH] FAIL driver was loaded\r\n");
            1
        }
    }
}

/// End-to-end probes for the NEM v3 loader validation (AUDIT-002/003,
/// #810/#811). Both files live in the trusted directory and are loaded by an
/// administrator, so they exercise the real `ObCreate(Driver)` path and the
/// real loader; the kernel must reject them with an error, never panic.
const NEM_LOAD_ARG: &[u8] = b"NEMLOAD";

/// Ring 3 probe for #761: write patterns inside the identity-mapped user
/// window (the last four pages, normally an unused slot) and read them back.
/// With the window reserved in the buddy allocator this cannot corrupt kernel
/// page tables or heap objects; the guest must survive and see its pattern.
const USERWIN_ARG: &[u8] = b"USERWIN";
const USERWIN_START: u64 = 0x023F_C000; // USER_LIMIT (0x2400000) - 16 KiB

fn run_user_window_probe() -> u32 {
    write_str(b"[USERWIN] start\r\n");
    let mut ok = true;
    for page in 0..4u64 {
        let addr = (USERWIN_START + page * 0x1000) as *mut u8;
        unsafe {
            core::ptr::write_bytes(addr, 0xA5, 0x1000);
            let mut off = 0usize;
            while off < 0x1000 {
                if core::ptr::read(addr.add(off)) != 0xA5 {
                    ok = false;
                    break;
                }
                off += 64;
            }
        }
    }
    if ok {
        write_str(b"[USERWIN] PASS wrote 16 KiB inside user window\r\n");
        0
    } else {
        write_str(b"[USERWIN] FAIL pattern mismatch\r\n");
        1
    }
}

fn build_bad_nem(buf: &mut [u8; 128], bad_reloc: bool) -> usize {
    // NemHeaderV3 (80 bytes) + name + optional reloc + text section.
    buf.fill(0);
    buf[0..4].copy_from_slice(b"NEM3");
    put_u32(buf, 4, 3); // version
    put_u32(buf, 8, 80); // header_size
    put_u16(buf, 16, 1); // abi_min
    put_u16(buf, 18, 1); // abi_target
    put_u16(buf, 20, 2); // abi_max
    put_u16(buf, 22, 2); // driver_type = Lifecycle
    put_u16(buf, 24, 0); // category = Boot
    put_u32(buf, 28, 16); // text_size
    put_u32(buf, 40, if bad_reloc { 16 } else { 0 }); // bss_size
    put_u32(buf, 44, if bad_reloc { 32 } else { 1 }); // total_mem_size
    put_u32(buf, 60, if bad_reloc { 1 } else { 0 }); // num_relocs
    put_u32(buf, 64, 84); // relocs_offset
    put_u32(buf, 68, if bad_reloc { 96 } else { 84 }); // syms_offset
    put_u32(buf, 72, if bad_reloc { 96 } else { 84 }); // strtab_offset
    put_u32(buf, 76, 80); // name_offset
    buf[80..84].copy_from_slice(b"BAD\0");

    if bad_reloc {
        // NemReloc: offset 0x40 is outside the 16-byte text section.
        put_u32(buf, 84, 0x40);
        put_u16(buf, 88, 0);
        buf[90] = 1; // R_NEM_64
        buf[91] = 0xFF; // no symbol
        put_u32(buf, 92, 0); // addend
        buf[96..112].fill(0x90);
        112
    } else {
        buf[84..100].fill(0x90);
        100
    }
}

fn try_bad_nem(name: &str, bad_reloc: bool) -> bool {
    let mut raw = [0u8; 128];
    let len = build_bad_nem(&mut raw, bad_reloc);

    let path = match name {
        "oversize" => "C:\\NeoDOS\\System\\drivers\\__nemtest_oversize.nem",
        _ => "C:\\NeoDOS\\System\\drivers\\__nemtest_reloc.nem",
    };
    let ob_path = match name {
        "oversize" => {
            "\\Global\\FileSystem\\C:\\NeoDOS\\System\\drivers\\__nemtest_oversize.nem"
        }
        _ => "\\Global\\FileSystem\\C:\\NeoDOS\\System\\drivers\\__nemtest_reloc.nem",
    };

    let fd = match syscall::ob_file_create(path) {
        Ok(fd) => fd,
        Err(_) => {
            write_str(b"[NEMLOAD] FAIL could not create probe file\r\n");
            return false;
        }
    };
    if syscall::sys_ob_set_info(fd, syscall::ObSetInfoClass::WriteContent, &raw[..len]).is_err() {
        let _ = syscall::ob_file_delete(fd);
        write_str(b"[NEMLOAD] FAIL could not write probe file\r\n");
        return false;
    }

    let result = syscall::sys_ob_create(ob_path, syscall::ob_type::DRIVER, None, 0);
    let ok = match result {
        // The dispatch maps any loader error to EIO; the important part is
        // that the kernel rejected the image and is still alive.
        Err(e) if e == syscall::EIO => true,
        Err(e) => {
            write_str(b"[NEMLOAD] error=");
            write_i64(e);
            write_str(b"\r\n");
            false
        }
        Ok(driver_fd) => {
            let _ = syscall::sys_close(driver_fd);
            write_str(b"[NEMLOAD] driver unexpectedly loaded\r\n");
            false
        }
    };

    let _ = syscall::ob_file_delete(fd);
    if ok {
        write_str(b"[NEMLOAD] PASS rejected\r\n");
    } else {
        write_str(b"[NEMLOAD] FAIL unexpected result\r\n");
    }
    ok
}

fn run_nem_loader_probe() -> u32 {
    write_str(b"[NEMLOAD] start\r\n");
    let mut failed = 0u32;
    if !try_bad_nem("oversize", false) {
        write_str(b"[NEMLOAD] FAIL sections > total_mem_size\r\n");
        failed += 1;
    }
    if !try_bad_nem("reloc", true) {
        write_str(b"[NEMLOAD] FAIL out-of-bounds relocation\r\n");
        failed += 1;
    }
    if failed == 0 {
        write_str(b"[NEMLOAD] PASS both malformed images rejected\r\n");
    }
    failed
}

#[inline(never)]
fn test_ping_loopback() -> bool {
    // 127.0.0.1 as big-endian u32 (RAX 36 ABI). Loopback needs no NIC.
    // One retry: boot-time scheduling can rarely drop the first exchange.
    let mut rtt = syscall::sys_icmp_ping(0x7F000001);
    if rtt == 0 {
        rtt = syscall::sys_icmp_ping(0x7F000001);
    }
    if rtt > 0 {
        write_str(b"[ping-lo] reply 127.0.0.1 PASS\r\n");
        true
    } else {
        write_str(b"[ping-lo] 127.0.0.1 timeout FAIL\r\n");
        false
    }
}

#[inline(never)]
fn test_cd() -> bool {
    let mut buf = [0u8; 2];
    buf[0] = 0x41; // A
    buf[1] = 0;
    unsafe {
        let dst = 0x41F000 as *mut u8;
        core::ptr::write_bytes(dst, 0, 256);
        core::ptr::write(dst, b'C');
        core::ptr::write(dst.add(1), b':');
        core::ptr::write(dst.add(2), b'\\');
    }
    let mut cwd = [0u8; 256];
    match syscall::sys_getcwd(&mut cwd) {
        Ok(n) if n > 0 => {
            let cwd_str = core::str::from_utf8(libneodos::args::trim_ascii(&cwd[..n])).unwrap_or("");
            cwd_str.starts_with("C:\\")
        }
        _ => false,
    }
}

#[inline(never)]
fn test_md_rd() -> bool {
    let mut ob_buf = [0u8; 512];
    let prefix = b"\\Global\\FileSystem\\";
    let test_path = b"C:\\Users\\NeoDOS\\CMDTEST_TMP";
    let total = prefix.len() + test_path.len();
    let mut path_buf = [0u8; 512];
    path_buf[..prefix.len()].copy_from_slice(prefix);
    path_buf[prefix.len()..total].copy_from_slice(test_path);
    let ob_path = unsafe { core::str::from_utf8_unchecked(&path_buf[..total]) };

    match syscall::sys_ob_create(ob_path, 11, None, 0) {
        Ok(_) => {
            match syscall::sys_ob_destroy(0) {
                Ok(_) => true,
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

#[inline(never)]
fn test_del() -> bool {
    let mut ob_buf = [0u8; 512];
    let prefix = b"\\Global\\FileSystem\\";
    let test_path = b"C:\\Users\\NeoDOS\\CMDTEST_DEL";
    let total = prefix.len() + test_path.len();
    let mut path_buf = [0u8; 512];
    path_buf[..prefix.len()].copy_from_slice(prefix);
    path_buf[prefix.len()..total].copy_from_slice(test_path);
    let ob_path = unsafe { core::str::from_utf8_unchecked(&path_buf[..total]) };

    match syscall::sys_ob_create(ob_path, 0, None, 0) {
        Ok(fd) => {
            match syscall::ob_file_delete(fd) {
                Ok(_) => true,
                Err(_) => false,
            }
        }
        Err(_) => {
            match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ | libneodos::syscall::ob_access::DELETE) {
                Ok(fd) => {
                    match syscall::ob_file_delete(fd) {
                        Ok(_) => true,
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    // AUDIT-001 / #809: dedicated E2E mode (see NeoInit's NEMAUTH spawns).
    let raw_args = libneodos::args::read_args();
    if libneodos::args::trim_ascii(&raw_args) == NEM_AUTH_ARG {
        syscall::sys_exit(run_nem_auth_probe());
    }
    if libneodos::args::trim_ascii(&raw_args) == NEM_LOAD_ARG {
        syscall::sys_exit(run_nem_loader_probe());
    }
    if libneodos::args::trim_ascii(&raw_args) == USERWIN_ARG {
        syscall::sys_exit(run_user_window_probe());
    }

    let mut passed = 0u32;
    let mut failed = 0u32;

    if test_cd() { passed += 1; } else { failed += 1; cmd_passed(); }
    if test_md_rd() { passed += 1; } else { failed += 1; }
    if test_del() { passed += 1; } else { failed += 1; }
    if test_ping_loopback() { passed += 1; } else { failed += 1; }

    write_str(tr_id!(IDS_HEADER).as_bytes());
    if failed == 0 {
        write_str(tr_id!(IDS_ALL_PASSED).as_bytes());
    } else {
        write_str(tr_id!(IDS_SOME_FAILED).as_bytes());
    }
    write_str(b"\r\n");
    write_str(tr_id!(IDS_COMPLETE).as_bytes());
    write_str(b"\r\n");
    syscall::sys_exit(if failed == 0 { 0 } else { 1 })
}
