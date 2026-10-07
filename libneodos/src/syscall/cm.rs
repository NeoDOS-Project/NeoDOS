//! Registry (Cm) syscall wrappers.

use super::{ret, EINVAL};

// ═══════════════════════════════════════════════════════════════════════
// Registry (Cm) — RAX 67–76
// ═══════════════════════════════════════════════════════════════════════

pub const REG_NONE: u32 = 0;
pub const REG_SZ: u32 = 1;
pub const REG_DWORD: u32 = 2;
pub const REG_BINARY: u32 = 3;

/// sys_cm_open_key (RAX=67): open a registry key by full Ob path.
/// Returns fd (>=3) on success.
pub fn sys_cm_open_key(path: &str) -> Result<u8, i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr() as u64;
    let r = unsafe { ob_syscall_2!(50, ptr, 0u64) };
    ret(r).map(|v| v as u8)
}

/// sys_cm_create_key: create subkey `name` under open key `fd`
/// via ob_set_info(RegistryCreateKey = 23). Ok if it already exists
/// (the kernel answers Exist, which we also accept).
pub fn sys_cm_create_key(fd: u8, name: &str) -> Result<(), i64> {
    super::ob::sys_ob_set_info(fd, super::ob::ObSetInfoClass::RegistryCreateKey, name.as_bytes())
}

/// sys_cm_query_value (RAX=69): query a value on a registry key by fd.
/// buf receives: [type: u32 LE, data_len: u32 LE, data...]
/// Returns total_size (8 + data_len) regardless of buf capacity.
pub fn sys_cm_query_value(fd: u8, name: &str, buf: &mut [u8]) -> Result<usize, i64> {
    let bytes = name.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut name_buf = [0u8; 256];
    name_buf[..bytes.len()].copy_from_slice(bytes);
    let name_ptr = name_buf.as_ptr() as u64;
    let buf_ptr = buf.as_mut_ptr() as u64;
    let buf_len = buf.len() as u64;
    let r = unsafe { ob_syscall_4!(52, fd as u64, name_ptr, buf_ptr, buf_len) };
    ret(r).map(|v| v as usize)
}


/// sys_cm_set_value (RAX=70): set a value on a registry key by fd.
/// fd = key handle from sys_cm_open_key, name = value name, value_type = REG_* constant.
pub fn sys_cm_set_value(fd: u8, name: &str, value_type: u32, data: &[u8]) -> Result<(), i64> {
    let bytes = name.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut name_buf = [0u8; 256];
    name_buf[..bytes.len()].copy_from_slice(bytes);
    let name_ptr = name_buf.as_ptr() as u64;
    let data_ptr = data.as_ptr() as u64;
    let data_len = data.len() as u64;
    let r = unsafe { ob_syscall_5!(53, fd as u64, name_ptr, value_type as u64, data_ptr, data_len) };
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_cm_flush_key (RAX=57): persist a registry key's hive to disk.
/// Serializes the hive containing `fd` to `C:\System\Registry\<name>.hiv` if it
/// has pending changes, so values set via `sys_cm_set_value` survive a reboot.
pub fn sys_cm_flush_key(fd: u8) -> Result<(), i64> {
    let r = unsafe { ob_syscall_2!(57, fd as u64, 0u64) };
    ret(r).map(|_| ())
}

