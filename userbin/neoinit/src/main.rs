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

// ── String IDs (from TOML) ──
const IDS_SHELL_SPAWN: u32 = 1001;

const APP_NAME: &str = "neoinit";
const NEOINIT_VERSION: &str = env!("CARGO_PKG_VERSION");
const REG_KEY_PATH: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\NeoInit";
const OB_FS_PREFIX: &[u8] = b"\\Global\\FileSystem\\";

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn set_vt(vt: u8) {
    if let Ok(fd) = syscall::sys_ob_open("\\Global\\Info\\VtInfo", syscall::ob_access::WRITE) {
        let _ = syscall::sys_ob_set_info(fd, syscall::ObSetInfoClass::SetProcessVt, &[vt; 1]);
        let _ = syscall::sys_close(fd);
    }
}

fn read_reg_str(key_fd: u8, name: &str, buf: &mut [u8]) -> Option<usize> {
    let mut reg_buf = [0u8; 512];
    let total = syscall::sys_cm_query_value(key_fd, name, &mut reg_buf).ok()?;
    if total < 8 {
        return None;
    }
    let data_len = u32::from_le_bytes([reg_buf[4], reg_buf[5], reg_buf[6], reg_buf[7]]) as usize;
    let copy_len = data_len.min(buf.len());
    if copy_len > 0 {
        buf[..copy_len].copy_from_slice(&reg_buf[8..8 + copy_len]);
    }
    Some(copy_len)
}

fn read_reg_dword(key_fd: u8, name: &str) -> Option<u32> {
    let mut reg_buf = [0u8; 12];
    let total = syscall::sys_cm_query_value(key_fd, name, &mut reg_buf).ok()?;
    if total < 12 {
        return None;
    }
    let value_type = u32::from_le_bytes([reg_buf[0], reg_buf[1], reg_buf[2], reg_buf[3]]);
    if value_type != syscall::REG_DWORD {
        return None;
    }
    Some(u32::from_le_bytes([reg_buf[8], reg_buf[9], reg_buf[10], reg_buf[11]]))
}

fn spawn_and_wait(path: &str) -> Result<u32, i64> {
    let attrs = 0xFFu64 | (0xFFu64 << 8) | (0xFFu64 << 16);
    let fd = syscall::sys_ob_create(path, 1, None, attrs)?;
    let _ = syscall::sys_ob_wait(fd);
    let _ = syscall::sys_close(fd);
    Ok(0)
}

fn try_spawn_test(path: &str, tag: &str) {
    let mut ob_path_buf = [0u8; 512];
    let bytes = path.as_bytes();
    let total = OB_FS_PREFIX.len() + bytes.len();
    if total > ob_path_buf.len() { return; }
    ob_path_buf[..OB_FS_PREFIX.len()].copy_from_slice(OB_FS_PREFIX);
    ob_path_buf[OB_FS_PREFIX.len()..total].copy_from_slice(bytes);
    let ob_path = match core::str::from_utf8(&ob_path_buf[..total]) {
        Ok(s) => s,
        Err(_) => return,
    };
    match spawn_and_wait(ob_path) {
        Ok(_) => write_str(b"[neoinit] test completed: "),
        Err(_) => write_str(b"[neoinit] test not found or failed: "),
    }
    write_str(tag.as_bytes());
    write_str(b"\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);

    write_str(b"\r\n");
    write_str(b"NeoInit v");
    write_str(NEOINIT_VERSION.as_bytes());
    write_str(b" (PID 1)\r\n");
    write_str(b"----------------------------------------\r\n");
    write_str(b"[neoinit] services managed by kernel Service Manager\r\n");

    // ── Open registry ──
    let reg_key = syscall::sys_cm_open_key(REG_KEY_PATH);
    let mut key_fd = 0xFFu8;
    match reg_key {
        Ok(fd) => {
            key_fd = fd;
            write_str(b"[neoinit] opened registry key\r\n");
        }
        Err(_) => {
            write_str(b"[neoinit] WARNING: registry key not found, using defaults\r\n");
        }
    }

    // ── Read DefaultShell ──
    let mut shell_buf = [0u8; 260];
    let shell_name = if key_fd != 0xFF {
        match read_reg_str(key_fd, "DefaultShell", &mut shell_buf) {
            Some(len) if len > 0 => {
                core::str::from_utf8(&shell_buf[..len]).unwrap_or("C:\\Programs\\neoshell.nxe")
            }
            _ => "C:\\Programs\\neoshell.nxe",
        }
    } else {
        "C:\\Programs\\neoshell.nxe"
    };

    // ── Read EnableVT ──
    let enable_vt = if key_fd != 0xFF {
        read_reg_dword(key_fd, "EnableVT").unwrap_or(1)
    } else {
        1
    };

    // ── Read WaitForNetwork ──
    let wait_for_network = if key_fd != 0xFF {
        read_reg_dword(key_fd, "WaitForNetwork").unwrap_or(0)
    } else {
        0
    };

    // ── Read EnableTests ──
    let enable_tests = if key_fd != 0xFF {
        read_reg_dword(key_fd, "EnableTests").unwrap_or(0)
    } else {
        0
    };

    // ── Read EnableNetworkTest ──
    let enable_net_test = if key_fd != 0xFF {
        read_reg_dword(key_fd, "EnableNetworkTest").unwrap_or(0)
    } else {
        0
    };

    // ── Close registry key ──
    if key_fd != 0xFF {
        let _ = syscall::sys_close(key_fd);
    }

    // ── Wait for network if configured ──
    if wait_for_network != 0 {
        write_str(b"[neoinit] waiting for network...\r\n");
        for _ in 0..100_000 {
            syscall::sys_yield();
        }
    }

    // ── Run user-mode test binaries (if enabled) ──
    if enable_tests != 0 {
        try_spawn_test("C:\\Programs\\cmdtest.nxe", "CMDTEST");
        try_spawn_test("C:\\Programs\\stresscmd.nxe", "STRESSCMD");
        try_spawn_test("C:\\Programs\\shtest.nxe", "SHTEST");
    }

    // ── Run network test if enabled (requires bridged networking) ──
    if enable_net_test != 0 {
        write_str(b"[neoinit] starting network test...\r\n");
        try_spawn_test("C:\\System\\Tools\\dhcptest.nxe", "DHCPTEST");
    }

    // ── Build Ob path for shell ──
    let mut ob_path_buf = [0u8; 512];
    let shell_bytes = shell_name.as_bytes();
    let total = OB_FS_PREFIX.len() + shell_bytes.len();
    if total > ob_path_buf.len() {
        write_str(b"[neoinit] ERROR: shell path too long\r\n");
        loop { syscall::sys_yield(); }
    }
    ob_path_buf[..OB_FS_PREFIX.len()].copy_from_slice(OB_FS_PREFIX);
    ob_path_buf[OB_FS_PREFIX.len()..total].copy_from_slice(shell_bytes);
    let ob_path = core::str::from_utf8(&ob_path_buf[..total]).unwrap();

    // ── Spawn loop ──
    write_str(b"[neoinit] entering spawn loop...\r\n");
    loop {
        write_str(b"[neoinit] ");
        write_str(tr_id!(IDS_SHELL_SPAWN).as_bytes());
        write_str(b"\r\n");
        if enable_vt != 0 {
            set_vt(0);
        }
        match spawn_and_wait(ob_path) {
            Ok(_pid) => {
                write_str(b"[neoinit] shell exited, respawning...\r\n");
            }
            Err(e) => {
                write_str(b"[neoinit] spawn FAILED: errno ");
                let mut eb = [0u8; 10];
                let mut i = 9;
                let mut v = (-e) as usize;
                while v > 0 {
                    eb[i] = b'0' + (v % 10) as u8;
                    v /= 10;
                    if i == 0 { break; }
                    i -= 1;
                }
                write_str(&eb[i..=9]);
                write_str(b"\r\n");
            }
        }
    }
}
