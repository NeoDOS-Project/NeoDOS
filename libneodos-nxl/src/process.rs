
use crate::error::ret;
use crate::syscall::{syscall_0, syscall_1};

// ============================================================
// Raw syscall wrappers — Process domain
// ============================================================
#[no_mangle]
pub extern "C" fn nxl_sys_exit(code: u32) -> ! {
    unsafe { syscall_1(0, code as u64); }
    loop { unsafe { core::arch::asm!("hlt"); } }
}

#[no_mangle]
pub extern "C" fn nxl_sys_getpid() -> u32 {
    // Use Ob API: open \Global\Info\Process, query ProcessId, close
    let fd = match crate::fs::nxl_sys_ob_open(b"\\Global\\Info\\Process\0" as *const u8, 1) {
        r if r >= 0 => r as u8,
        _ => return 0,
    };
    let mut pid = [0u8; 4];
    let r = crate::fs::nxl_sys_ob_query_info(fd, 34, pid.as_mut_ptr(), 4);
    let _ = crate::fs::nxl_sys_close(fd);
    if r < 0 { return 0; }
    u32::from_le_bytes(pid)
}

#[no_mangle]
pub extern "C" fn nxl_sys_yield() {
    unsafe { syscall_0(2); }
}

#[no_mangle]
pub extern "C" fn nxl_sys_loadlib(path: *const u8) -> i64 {
    ret(unsafe { syscall_1(21, path as u64) })
}


