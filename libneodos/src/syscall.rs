use core::arch::asm;

pub const EINVAL: i64 = -1;
pub const ENOENT: i64 = -2;
pub const ENOMEM: i64 = -3;
pub const EACCES: i64 = -4;
pub const EBADF: i64 = -5;
pub const EFAULT: i64 = -6;
pub const ENOSYS: i64 = -7;
pub const EAGAIN: i64 = -8;
pub const EPIPE: i64 = -9;
pub const EEXIST: i64 = -10;
pub const ENOTDIR: i64 = -11;
pub const EISDIR: i64 = -12;
pub const EIO: i64 = -13;
pub const ENODEV: i64 = -14;
pub const EBUSY: i64 = -15;

fn ret(val: u64) -> Result<u64, i64> {
    let signed = val as i64;
    if signed < 0 { Err(signed) } else { Ok(val) }
}

fn ret_unit(val: u64) -> Result<(), i64> {
    let signed = val as i64;
    if signed < 0 { Err(signed) } else { Ok(()) }
}

unsafe fn syscall_0(n: u64) -> u64 {
    let r: u64;
    asm!("mov rax, {}", "int 0x80", in(reg) n, out("rax") r);
    r
}

unsafe fn syscall_1(n: u64, a0: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx",
        "mov rax, {n}", "mov rbx, {a0}", "int 0x80",
        "pop rbx",
        n = in(reg) n, a0 = in(reg) a0,
        out("rax") r,
    );
    r
}

unsafe fn syscall_2(n: u64, a0: u64, a1: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "int 0x80",
        "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1,
        out("rax") r,
    );
    r
}

unsafe fn syscall_3(n: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx", "push rdx",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "mov rdx, {a2}", "int 0x80",
        "pop rdx", "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1, a2 = in(reg) a2,
        out("rax") r,
    );
    r
}

unsafe fn syscall_5(n: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    let r: u64;
    asm!(
        "push rbx", "push rcx", "push rdx", "push r8", "push r9",
        "mov rax, {n}", "mov rbx, {a0}", "mov rcx, {a1}", "mov rdx, {a2}",
        "mov r8, {a3}", "mov r9, {a4}", "int 0x80",
        "pop r9", "pop r8", "pop rdx", "pop rcx", "pop rbx",
        n = in(reg) n, a0 = in(reg) a0, a1 = in(reg) a1,
        a2 = in(reg) a2, a3 = in(reg) a3, a4 = in(reg) a4,
        out("rax") r,
    );
    r
}

pub fn sys_exit(code: u32) -> ! {
    unsafe { syscall_1(0, code as u64); }
    loop { unsafe { asm!("hlt"); } }
}

pub fn sys_write(fd: u8, buf: &[u8]) -> Result<usize, i64> {
    let ptr = buf.as_ptr() as u64;
    let len = buf.len() as u64;
    ret(unsafe { syscall_3(1, fd as u64, ptr, len) }).map(|v| v as usize)
}

pub fn sys_yield() {
    unsafe { syscall_0(2); }
}

pub fn sys_getpid() -> u32 {
    unsafe { syscall_0(3) as u32 }
}

pub fn sys_read(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr() as u64;
    let len = buf.len() as u64;
    ret(unsafe { syscall_3(4, fd as u64, ptr, len) }).map(|v| v as usize)
}

pub fn sys_open(path: &str) -> Result<u64, i64> {
    let ptr = path.as_ptr() as u64;
    ret(unsafe { syscall_2(10, ptr, 0) })
}

pub fn sys_readfile(handle: u64, buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr() as u64;
    let len = buf.len() as u64;
    ret(unsafe { syscall_3(11, handle, ptr, len) }).map(|v| v as usize)
}

pub fn sys_writefile(handle: u64, buf: &[u8]) -> Result<usize, i64> {
    let ptr = buf.as_ptr() as u64;
    let len = buf.len() as u64;
    ret(unsafe { syscall_3(12, handle, ptr, len) }).map(|v| v as usize)
}

pub fn sys_close(fd: u8) -> Result<(), i64> {
    ret_unit(unsafe { syscall_1(13, fd as u64) })
}

pub fn sys_brk(new_break: u64) -> Result<u64, i64> {
    ret(unsafe { syscall_1(18, new_break) })
}

pub fn sys_mmap(hint: u64, len: u64, prot: u16, flags: u16, file_handle: u64) -> Result<u64, i64> {
    ret(unsafe { syscall_5(19, hint, len, prot as u64, flags as u64, file_handle) })
}

pub fn sys_munmap(addr: u64, len: u64) -> Result<(), i64> {
    ret_unit(unsafe { syscall_2(20, addr, len) })
}
