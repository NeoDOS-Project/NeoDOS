use crate::export;

pub fn brk(new_break: u64) -> Result<u64, i64> {
    let ret = (export::get_table().brk)(new_break);
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

pub fn sbrk(increment: i64) -> Result<u64, i64> {
    let ret = (export::get_table().sbrk)(increment);
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

pub fn mmap(len: u64, prot: u16, flags: u16) -> Result<u64, i64> {
    let ret = (export::get_table().mmap)(len, prot, flags);
    if ret < 0 { Err(ret) } else { Ok(ret as u64) }
}

pub fn munmap(addr: u64, len: u64) -> Result<(), i64> {
    let ret = (export::get_table().munmap)(addr, len);
    if ret < 0 { Err(ret) } else { Ok(()) }
}

pub const PROT_READ: u16 = 1;
pub const PROT_WRITE: u16 = 2;
pub const MAP_ANONYMOUS: u16 = 1;
pub const MAP_SHARED: u16 = 2;
