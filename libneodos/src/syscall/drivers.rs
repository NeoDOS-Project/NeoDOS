//! Driver management syscall wrappers.

use super::EINVAL;

/// sys_driver_unload (RAX=35): unload a NEM driver by name (admin).
/// force = true to force unload without waiting for ACK.
pub fn sys_driver_unload(name: &str, force: bool) -> Result<(), i64> {
    let bytes = name.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr();
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 35",
            "mov rbx, {ptr}",
            "mov rcx, {force}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            ptr = in(reg) ptr as u64,
            force = in(reg) force as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(()) }
}

