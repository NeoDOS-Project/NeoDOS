#![no_std]
#![no_main]

use libneodos::syscall;

const SHELL_PATH: &[u8] = b"C:\\Programs\\NeoShell.nxe\0";
const NEOINIT_VERSION: &str = "NeoInit v0.1.0 (PID 1)";

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn set_vt(vt: u8) {
    if let Ok(fd) = syscall::sys_ob_open("\\Global\\Info\\VtInfo", libneodos::syscall::ob_access::WRITE) {
        let _ = syscall::sys_ob_set_info(fd, 17, &[vt; 1]);
        let _ = syscall::sys_close(fd);
    }
}

fn spawn(path: &[u8]) -> Result<u32, i64> {
    let path_ptr = path.as_ptr() as u64;
    let result: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "push r8",
            "mov rbx, rsi",
            "mov rcx, rdx",
            "mov r8, r9",
            "int 0x80",
            "pop r8",
            "pop rcx",
            "pop rbx",
            in("rax") 7u64,
            in("rsi") path_ptr,
            in("rdx") 0xFFu64,
            in("r9") 0xFFu64,
            lateout("rax") result,
        );
    }
    if result < 0 { Err(result) } else { Ok(result as u32) }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n");
    write_str(NEOINIT_VERSION.as_bytes());
    write_str(b"\r\n");
    write_str(b"----------------------------------------\r\n");

    write_str(b"[neoinit] entering spawn loop...\r\n");
    loop {
        write_str(b"[neoinit] spawning 'shell' on VT0...\r\n");
        set_vt(0);
        match spawn(SHELL_PATH) {
            Ok(pid) => {
                write_str(b"[neoinit] shell PID ");
                let mut buf = [0u8; 10];
                let mut i = 9;
                let mut v = pid as usize;
                while v > 0 {
                    buf[i] = b'0' + (v % 10) as u8;
                    v /= 10;
                    if i == 0 { break; }
                    i -= 1;
                }
                let start = if pid == 0 { 9 } else { i + 1 };
                let s = if pid == 0 { b"0" } else { &buf[start..=9] };
                write_str(s);
                write_str(b" exited, respawning...\r\n");
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
