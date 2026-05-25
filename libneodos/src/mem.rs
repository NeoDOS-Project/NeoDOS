use crate::syscall;

pub fn brk(new_break: u64) -> Result<u64, i64> {
    syscall::sys_brk(new_break)
}

pub fn sbrk(increment: i64) -> Result<u64, i64> {
    let current = brk(0)?;
    if increment == 0 {
        return Ok(current);
    }
    let new = (current as i64).checked_add(increment).ok_or(syscall::EINVAL)?;
    if new < 0 {
        return Err(syscall::EINVAL);
    }
    let new = new as u64;
    brk(new).map(|_| current)
}

pub fn mmap(len: u64, prot: u16, flags: u16) -> Result<u64, i64> {
    syscall::sys_mmap(0, len, prot, flags, 0)
}

pub fn munmap(addr: u64, len: u64) -> Result<(), i64> {
    syscall::sys_munmap(addr, len)
}

pub const PROT_READ: u16 = 1;
pub const PROT_WRITE: u16 = 2;
pub const MAP_ANONYMOUS: u16 = 1;
pub const MAP_SHARED: u16 = 2;
