#![no_std]
#![no_main]

use libneodos::syscall;

const SHELL_PATH: &[u8] = b"C:\\NEOSHELL.NXE\0";
const NEOINIT_VERSION: &str = "NeoInit v0.1.0 (PID 1)";

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

/// Invoke sys_spawn (RAX=7) directly via INT 0x80.
/// RBX = path_ptr, RCX = stdin_fd, RDX = stdout_fd, R8 = stderr_fd.
/// 0xFF = inherit from parent.
fn spawn(path: &[u8]) -> Result<u32, i64> {
    let path_ptr = path.as_ptr() as u64;
    let result: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov rbx, rsi",
            "int 0x80",
            "pop rbx",
            in("rax") 7u64,
            in("rsi") path_ptr,
            in("rcx") 0xFFu64,
            in("rdx") 0xFFu64,
            in("r8") 0xFFu64,
            lateout("rax") result,
            options(nostack),
        );
    }
    if result < 0 {
        Err(result)
    } else {
        Ok(result as u32)
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write_str(b"\r\n");
    write_str(NEOINIT_VERSION.as_bytes());
    write_str(b"\r\n");
    write_str(b"----------------------------------------\r\n");

    // Main loop: spawn shell, when it exits (spawn returns), respawn
    write_str(b"[neoinit] entering spawn loop...\r\n");
    loop {
        write_str(b"[neoinit] spawning 'shell'...\r\n");
        match spawn(SHELL_PATH) {
            Ok(pid) => {
                write_str(b"[neoinit] shell PID ");
                // Simple PID printing
                let mut buf = [0u8; 10];
                let mut i = 9;
                let mut v = pid as usize;
                while v > 0 {
                    buf[i] = b'0' + (v % 10) as u8;
                    v /= 10;
                    if i == 0 { break; }
                    i -= 1;
                }
                let start = if buf.iter().all(|&c| c == 0) { 9 } else { i + 1 };
                // Handle pid == 0
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
