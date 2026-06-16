use crate::export;

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

fn ret(val: i64) -> Result<u64, i64> {
    if val < 0 { Err(val) } else { Ok(val as u64) }
}

fn ret_unit(val: i64) -> Result<(), i64> {
    if val < 0 { Err(val) } else { Ok(()) }
}

pub fn sys_exit(code: u32) -> ! {
    (export::get_table().sys_exit)(code)
}

pub fn sys_write(fd: u8, buf: &[u8]) -> Result<usize, i64> {
    let ptr = buf.as_ptr();
    let len = buf.len();
    ret((export::get_table().sys_write)(fd, ptr, len)).map(|v| v as usize)
}

pub fn sys_yield() {
    (export::get_table().sys_yield)()
}

pub fn sys_getpid() -> u32 {
    (export::get_table().sys_getpid)()
}

pub fn sys_read(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr();
    let len = buf.len();
    ret((export::get_table().sys_read)(fd, ptr, len)).map(|v| v as usize)
}

fn path_to_null_terminated(path: &str) -> Result<[u8; 256], i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 {
        return Err(EINVAL);
    }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    Ok(buf)
}

pub fn sys_open(path: &str) -> Result<u8, i64> {
    let buf = path_to_null_terminated(path)?;
    let ptr = buf.as_ptr();
    ret((export::get_table().sys_open)(ptr)).map(|v| v as u8)
}

pub fn sys_readfile(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr();
    let len = buf.len();
    ret((export::get_table().sys_readfile)(fd, ptr, len)).map(|v| v as usize)
}

pub fn sys_writefile(fd: u8, buf: &[u8]) -> Result<usize, i64> {
    let ptr = buf.as_ptr();
    let len = buf.len();
    ret((export::get_table().sys_writefile)(fd, ptr, len)).map(|v| v as usize)
}

pub fn sys_close(fd: u8) -> Result<(), i64> {
    ret_unit((export::get_table().sys_close)(fd))
}

pub fn sys_chdir(path: &str) -> Result<(), i64> {
    let buf = path_to_null_terminated(path)?;
    let ptr = buf.as_ptr();
    ret_unit((export::get_table().sys_chdir)(ptr))
}

pub fn sys_getcwd(buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr();
    let len = buf.len();
    ret((export::get_table().sys_getcwd)(ptr, len)).map(|v| v as usize)
}

pub fn sys_brk(new_break: u64) -> Result<u64, i64> {
    ret((export::get_table().sys_brk)(new_break))
}

pub fn sys_mmap(hint: u64, len: u64, prot: u16, flags: u16, file_handle: u64) -> Result<u64, i64> {
    ret((export::get_table().sys_mmap)(hint, len, prot, flags, file_handle))
}

pub fn sys_munmap(addr: u64, len: u64) -> Result<(), i64> {
    ret_unit((export::get_table().sys_munmap)(addr, len))
}

pub fn sys_loadlib(path: &str) -> Result<u64, i64> {
    let buf = path_to_null_terminated(path)?;
    let ptr = buf.as_ptr();
    ret((export::get_table().sys_loadlib)(ptr))
}

// ── CpuInfoFull (mirrors kernel cpu::CpuInfoFull) ──

#[repr(C)]
pub struct CpuInfoFull {
    // Identity
    pub vendor_id: [u8; 12],
    pub brand: [u8; 48],
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub cpu_type: u32,
    // Feature flags
    pub features_edx: u32,
    pub features_ecx: u32,
    pub ext_features_edx: u32,
    pub ext_features_ecx: u32,
    pub features_ebx_leaf7: u32,
    // Addressing
    pub phys_addr_bits: u8,
    pub virt_addr_bits: u8,
    // SMP / Topology
    pub cpu_count: u32,
    pub apic_id: u32,
    pub cpu_id: u32,
    pub is_bsp: bool,
    // Timer / Frequency
    pub tsc_khz: u64,
    pub timer_source: u8,
    pub tick_rate_hz: u64,
}

impl CpuInfoFull {
    pub fn vendor_str(&self) -> &str {
        core::str::from_utf8(&self.vendor_id).unwrap_or("Unknown")
    }

    pub fn brand_str(&self) -> &str {
        let mut end = self.brand.len();
        while end > 0 && (self.brand[end - 1] == 0 || self.brand[end - 1] == b' ') {
            end -= 1;
        }
        core::str::from_utf8(&self.brand[..end]).unwrap_or("Unknown")
    }

    pub fn cpu_type_str(&self) -> &'static str {
        match self.cpu_type {
            0 => "Reserved (overclocked)",
            1 => "Other",
            2 => "Unknown",
            3 => "Normal desktop/mobile",
            _ => "Unknown",
        }
    }

    pub fn has_sse(&self) -> bool { (self.features_edx >> 25) & 1 == 1 }
    pub fn has_sse2(&self) -> bool { (self.features_edx >> 26) & 1 == 1 }
    pub fn has_sse3(&self) -> bool { (self.features_ecx >> 0) & 1 == 1 }
    pub fn has_ssse3(&self) -> bool { (self.features_ecx >> 9) & 1 == 1 }
    pub fn has_sse41(&self) -> bool { (self.features_ecx >> 19) & 1 == 1 }
    pub fn has_sse42(&self) -> bool { (self.features_ecx >> 20) & 1 == 1 }
    pub fn has_avx(&self) -> bool { (self.features_ecx >> 28) & 1 == 1 }
    pub fn has_avx2(&self) -> bool { (self.features_ebx_leaf7 >> 5) & 1 == 1 }
    pub fn has_aes(&self) -> bool { (self.features_ecx >> 25) & 1 == 1 }
    pub fn has_fma(&self) -> bool { (self.features_ecx >> 12) & 1 == 1 }
    pub fn has_f16c(&self) -> bool { (self.features_ecx >> 29) & 1 == 1 }
    pub fn has_popcnt(&self) -> bool { (self.features_ecx >> 23) & 1 == 1 }
    pub fn has_xsave(&self) -> bool { (self.features_ecx >> 26) & 1 == 1 }
    pub fn has_osxsave(&self) -> bool { (self.features_ecx >> 27) & 1 == 1 }
    pub fn has_rdrand(&self) -> bool { (self.features_ecx >> 30) & 1 == 1 }
    pub fn has_pclmulqdq(&self) -> bool { (self.features_ecx >> 1) & 1 == 1 }
    pub fn has_fsgsbase(&self) -> bool { (self.features_ebx_leaf7 >> 0) & 1 == 1 }
    pub fn has_bmi1(&self) -> bool { (self.features_ebx_leaf7 >> 3) & 1 == 1 }
    pub fn has_bmi2(&self) -> bool { (self.features_ebx_leaf7 >> 8) & 1 == 1 }
    pub fn has_hle(&self) -> bool { (self.features_ebx_leaf7 >> 4) & 1 == 1 }
    pub fn has_rtm(&self) -> bool { (self.features_ebx_leaf7 >> 11) & 1 == 1 }
    pub fn has_smep(&self) -> bool { (self.features_ebx_leaf7 >> 7) & 1 == 1 }
    pub fn has_erms(&self) -> bool { (self.features_ebx_leaf7 >> 9) & 1 == 1 }
    pub fn has_invcpcid(&self) -> bool { (self.features_ebx_leaf7 >> 10) & 1 == 1 }
    pub fn has_x2apic(&self) -> bool { (self.features_ecx >> 21) & 1 == 1 }
    pub fn has_htt(&self) -> bool { (self.features_edx >> 28) & 1 == 1 }
    pub fn has_nx(&self) -> bool { (self.ext_features_edx >> 20) & 1 == 1 }
    pub fn has_long_mode(&self) -> bool { (self.ext_features_edx >> 29) & 1 == 1 }
    pub fn has_syscall(&self) -> bool { (self.ext_features_edx >> 11) & 1 == 1 }
    pub fn has_mmx(&self) -> bool { (self.features_edx >> 23) & 1 == 1 }
    pub fn has_fxsr(&self) -> bool { (self.features_edx >> 24) & 1 == 1 }
}

/// DirEntry — matches kernel's DirEntryRaw (RAX=8).
#[repr(C)]
pub struct DirEntry {
    pub inode: u32,
    pub mode: u16,
    pub size: u32,
    pub name: [u8; 260],
}

impl DirEntry {
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(0);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

/// sys_spawn: spawn a process (RAX=7).
/// stdin_fd/stdout_fd/stderr_fd = 0xFF means inherit default.
pub fn sys_spawn(path: &str, stdin_fd: u8, stdout_fd: u8, stderr_fd: u8) -> Result<u32, i64> {
    let buf = path_to_null_terminated(path)?;
    let ptr = buf.as_ptr();
    ret((export::get_table().sys_spawn)(ptr, stdin_fd, stdout_fd, stderr_fd)).map(|v| v as u32)
}

/// sys_readdir: read a directory entry (RAX=8).
/// Returns 1 if an entry was written, 0 at end, negative on error.
pub fn sys_readdir(fd: u8, entry: &mut DirEntry) -> Result<usize, i64> {
    let ptr = entry as *mut DirEntry as *mut u8;
    ret((export::get_table().sys_readdir)(fd, ptr)).map(|v| v as usize)
}

/// sys_mkdir: create a directory (RAX=25).
pub fn sys_mkdir(path: &str) -> Result<(), i64> {
    let buf = path_to_null_terminated(path)?;
    let ptr = buf.as_ptr();
    ret_unit((export::get_table().sys_mkdir)(ptr))
}

/// sys_unlink: delete a file (RAX=26).
pub fn sys_unlink(path: &str) -> Result<(), i64> {
    let buf = path_to_null_terminated(path)?;
    let ptr = buf.as_ptr();
    ret_unit((export::get_table().sys_unlink)(ptr))
}

/// sys_rmdir: remove an empty directory (RAX=27).
pub fn sys_rmdir(path: &str) -> Result<(), i64> {
    let buf = path_to_null_terminated(path)?;
    let ptr = buf.as_ptr();
    ret_unit((export::get_table().sys_rmdir)(ptr))
}

/// sys_rename: rename a file/directory (RAX=28).
pub fn sys_rename(old_path: &str, new_path: &str) -> Result<(), i64> {
    let old_buf = path_to_null_terminated(old_path)?;
    let new_buf = path_to_null_terminated(new_path)?;
    ret_unit((export::get_table().sys_rename)(old_buf.as_ptr(), new_buf.as_ptr()))
}

/// sys_waitpid: wait for a child process to exit (RAX=9).
pub fn sys_waitpid(pid: u32) -> Result<(), i64> {
    ret_unit((export::get_table().sys_waitpid)(pid))
}

/// sys_poweroff: power off the machine (RAX=42).
pub fn sys_poweroff() -> ! {
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("rax") 42u64,
            options(noreturn)
        );
    }
}

/// sys_getcpuinfo: fill a CpuInfoFull buffer from the kernel.
pub fn sys_getcpuinfo(buf: &mut CpuInfoFull) -> Result<(), i64> {
    ret_unit((export::get_table().sys_getcpuinfo)(
        buf as *mut CpuInfoFull as *mut u8,
        core::mem::size_of::<CpuInfoFull>(),
    ))
}

/// DateTime — matches kernel's SysDateTime (RAX=44).
#[repr(C)]
pub struct DateTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    pub valid: u8,
}

/// sys_get_version (RAX=43): copy kernel version string to buf.
/// Returns the full string length (may be larger than buf).
pub fn sys_get_version(buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr();
    let len = buf.len();
    ret((export::get_table().sys_get_version)(ptr, len)).map(|v| v as usize)
}

/// sys_get_datetime (RAX=44): fill a DateTime struct from the kernel RTC.
pub fn sys_get_datetime(dt: &mut DateTime) -> Result<(), i64> {
    ret_unit((export::get_table().sys_get_datetime)(dt as *mut DateTime as *mut u8))
}
