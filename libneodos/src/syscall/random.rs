//! Random bytes from the kernel CSPRNG (`\Device\Random`, #712/M8).

use super::{ob_access, sys_close, sys_ob_open, sys_ob_query_info, ObInfoClass};

/// Random source capabilities (`ObInfoClass::RandomCaps`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomCaps {
    /// 1 when the CPU implements RDRAND.
    pub hw_rdrand: u8,
    /// 1 when the CPU implements RDSEED.
    pub hw_rdseed: u8,
    /// 1 once the kernel pool is seeded.
    pub pool_ready: u8,
    pub _pad: u8,
}

impl RandomCaps {
    /// True when a hardware entropy source backs the pool. Consumers that need
    /// a CSPRNG (TLS nonces, key material) must fail closed when this is false.
    pub fn hardware_backed(&self) -> bool {
        self.pool_ready != 0 && (self.hw_rdrand != 0 || self.hw_rdseed != 0)
    }
}

/// Fill `buf` from the kernel CSPRNG. Returns the number of bytes written
/// (all of them, or an error).
pub fn ob_random(buf: &mut [u8]) -> Result<usize, i64> {
    if buf.is_empty() {
        return Ok(0);
    }
    let fd = sys_ob_open("\\Device\\Random", ob_access::READ)?;
    let r = sys_ob_query_info(fd, ObInfoClass::Random, buf);
    let _ = sys_close(fd);
    let n = r? as usize;
    if n != buf.len() {
        return Err(super::EINVAL);
    }
    Ok(n)
}

/// Read the kernel random source capabilities.
pub fn ob_random_caps() -> Result<RandomCaps, i64> {
    let fd = sys_ob_open("\\Device\\Random", ob_access::READ)?;
    let mut caps = RandomCaps {
        hw_rdrand: 0,
        hw_rdseed: 0,
        pool_ready: 0,
        _pad: 0,
    };
    let sz = core::mem::size_of::<RandomCaps>();
    // SAFETY: RandomCaps is #[repr(C)] and `sz` is its exact size.
    let buf = unsafe { core::slice::from_raw_parts_mut(&mut caps as *mut RandomCaps as *mut u8, sz) };
    let r = sys_ob_query_info(fd, ObInfoClass::RandomCaps, buf);
    let _ = sys_close(fd);
    let n = r? as usize;
    if n < sz {
        return Err(super::EINVAL);
    }
    Ok(caps)
}
