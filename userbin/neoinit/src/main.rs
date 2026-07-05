#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

use libneodos::syscall;

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

fn spawn_detached(path: &str) -> Result<u32, i64> {
    let attrs = 0xFFu64 | (0xFFu64 << 8) | (0xFFu64 << 16);
    let fd = syscall::sys_ob_create(path, 1, None, attrs)?;
    let pid = fd as u32;
    let _ = syscall::sys_close(fd);
    Ok(pid)
}

fn spawn_and_wait(path: &str) -> Result<u32, i64> {
    let attrs = 0xFFu64 | (0xFFu64 << 8) | (0xFFu64 << 16);
    let fd = syscall::sys_ob_create(path, 1, None, attrs)?;
    let _ = syscall::sys_ob_wait(fd);
    let _ = syscall::sys_close(fd);
    Ok(0)
}

fn spawn_service(path: &str) {
    if path.is_empty() { return; }
    let mut svc_path_buf = [0u8; 512];
    let svc_bytes = path.as_bytes();
    let svc_total = OB_FS_PREFIX.len() + svc_bytes.len();
    if svc_total > svc_path_buf.len() {
        write_str(b"[neoinit] WARNING: service path too long, skipping\r\n");
        return;
    }
    svc_path_buf[..OB_FS_PREFIX.len()].copy_from_slice(OB_FS_PREFIX);
    svc_path_buf[OB_FS_PREFIX.len()..svc_total].copy_from_slice(svc_bytes);
    let svc_ob_path = core::str::from_utf8(&svc_path_buf[..svc_total]).unwrap();
    match spawn_detached(svc_ob_path) {
        Ok(pid) => {
            write_str(b"[neoinit] started service PID ");
            let mut pb = [0u8; 10];
            let mut i = 9;
            let mut v = pid as usize;
            while v > 0 {
                pb[i] = b'0' + (v % 10) as u8;
                v /= 10;
                if i == 0 { break; }
                i -= 1;
            }
            let start = if pid == 0 { 9 } else { i + 1 };
            write_str(&pb[start..=9]);
            write_str(b"\r\n");
        }
        Err(e) => {
            write_str(b"[neoinit] service spawn FAILED: errno ");
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

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n");
    write_str(b"NeoInit v");
    write_str(NEOINIT_VERSION.as_bytes());
    write_str(b" (PID 1)\r\n");
    write_str(b"----------------------------------------\r\n");

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
                core::str::from_utf8(&shell_buf[..len]).unwrap_or("C:\\Programs\\NeoShell.nxe")
            }
            _ => "C:\\Programs\\NeoShell.nxe",
        }
    } else {
        "C:\\Programs\\NeoShell.nxe"
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

    // ── Read AutoStartServices (semicolon-separated) ──
    let mut services_buf = [0u8; 512];
    let services_str = if key_fd != 0xFF {
        read_reg_str(key_fd, "AutoStartServices", &mut services_buf)
            .and_then(|len| {
                if len > 0 {
                    core::str::from_utf8(&services_buf[..len]).ok()
                } else {
                    None
                }
            })
            .unwrap_or("")
    } else {
        ""
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

    // ── Auto-start services from registry ──
    if !services_str.is_empty() {
        write_str(b"[neoinit] auto-starting services...\r\n");
        for svc in services_str.split(';') {
            spawn_service(svc.trim());
        }
    }

    // ── Always start netcfg (network configuration daemon) ──
    spawn_service("C:\\Programs\\netcfg.nxe");

    // ── Spawn loop ──
    write_str(b"[neoinit] entering spawn loop...\r\n");
    loop {
        write_str(b"[neoinit] spawning shell...\r\n");
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
