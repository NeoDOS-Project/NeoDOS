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

pub fn sys_chdir_parent(path: &str) -> Result<(), i64> {
    let buf = path_to_null_terminated(path)?;
    let ptr = buf.as_ptr();
    ret_unit((export::get_table().sys_chdir_parent)(ptr))
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
    pub vendor_id: [u8; 12],
    pub brand: [u8; 48],
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub cpu_type: u32,
    pub features_edx: u32,
    pub features_ecx: u32,
    pub ext_features_edx: u32,
    pub ext_features_ecx: u32,
    pub features_ebx_leaf7: u32,
    pub phys_addr_bits: u8,
    pub virt_addr_bits: u8,
    pub cpu_count: u32,
    pub apic_id: u32,
    pub cpu_id: u32,
    pub is_bsp: bool,
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

/// MemInfo — matches kernel's MemInfo (RAX=45).
#[repr(C)]
pub struct MemInfo {
    pub phys_max: u64,
    pub total_kib: u64,
    pub usable_kib: u64,
    pub free_kib: u64,
    pub used_kib: u64,
    pub reserved_kib: u64,
}

/// sys_get_meminfo (RAX=45): fill a MemInfo struct from the kernel.
pub fn sys_get_meminfo(info: &mut MemInfo) -> Result<(), i64> {
    ret_unit((export::get_table().sys_get_meminfo)(info as *mut MemInfo as *mut u8))
}

/// sys_open_with_flags (RAX=10): open a file with creation flags.
/// flags & 1 = O_CREAT (create file if it doesn't exist).
/// Uses raw int 0x80 to pass the flags parameter.
pub fn sys_open_with_flags(path: &str, flags: u64) -> Result<u8, i64> {
    let bytes = path.as_bytes();
    let mut buf = [0u8; 256];
    if bytes.len() >= 255 { return Err(EINVAL); }
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr();
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 10",
            "mov rbx, {ptr}",
            "mov rcx, {flags}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            ptr = in(reg) ptr as u64,
            flags = in(reg) flags,
            out("rax") r,
            options(nostack),
        );
    }
    ret(r).map(|v| v as u8)
}

/// sys_pipe (RAX=5): create a pipe.
/// fds is a 2-element u64 array: [read_fd, write_fd].
pub fn sys_pipe(fds: &mut [u64; 2]) -> Result<(), i64> {
    let ptr = fds.as_mut_ptr();
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov rax, 5",
            "mov rbx, {ptr}",
            "int 0x80",
            "pop rbx",
            ptr = in(reg) ptr as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_dup2 (RAX=6): duplicate a file descriptor.
pub fn sys_dup2(old_fd: u8, new_fd: u8) -> Result<u8, i64> {
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 6",
            "mov rbx, {old}",
            "mov rcx, {new}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            old = in(reg) old_fd as u64,
            new = in(reg) new_fd as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(r as u8) }
}

/// sys_get_volume_label (RAX=46): get the volume label for a drive.
/// drive = ASCII drive letter (e.g. b'C'). Returns label string in buf (null-terminated).
pub fn sys_get_volume_label(drive: u8, buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr();
    let len = buf.len();
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "push rdx",
            "mov rax, 46",
            "mov rbx, {drive}",
            "mov rcx, {ptr}",
            "mov rdx, {len}",
            "int 0x80",
            "pop rdx",
            "pop rcx",
            "pop rbx",
            drive = in(reg) drive as u64,
            ptr = in(reg) ptr as u64,
            len = in(reg) len as u64,
            out("rax") r,
            options(nostack),
        );
    }
    ret(r).map(|v| v as usize)
}

/// DriveInfo — matches kernel's DriveInfoRaw (RAX=33).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DriveInfo {
    pub letter: u8,
    pub present: u8,
    pub fs_type: [u8; 16],
    pub label: [u8; 32],
    pub total_sectors: u64,
}

impl DriveInfo {
    pub fn fs_type_str(&self) -> &str {
        let end = self.fs_type.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.fs_type[..end]).unwrap_or("Unknown")
    }

    pub fn label_str(&self) -> &str {
        let end = self.label.iter().position(|&b| b == 0).unwrap_or(32);
        if end == 0 { return "(no label)"; }
        core::str::from_utf8(&self.label[..end]).unwrap_or("")
    }
}

/// sys_get_drives (RAX=33): enumerate mounted drives.
/// Returns number of drives written.
pub fn sys_get_drives(buf: &mut [DriveInfo]) -> Result<usize, i64> {
    let buf_ptr = buf.as_mut_ptr() as *mut u8;
    let max = buf.len() as u64;
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 33",
            "mov rbx, {ptr}",
            "mov rcx, {max}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            ptr = in(reg) buf_ptr as u64,
            max = in(reg) max,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(r as usize) }
}

/// KObjEntryRaw — matches kernel's KObjEntryRaw (RAX=48).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KObjEntryRaw {
    pub id: u64,
    pub obj_type: u32,
    pub padding: u32,
    pub name: [u8; 24],
    pub refcount: u32,
    pub native_id: u64,
}

impl KObjEntryRaw {
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(24);
        core::str::from_utf8(&self.name[..end]).unwrap_or("<?>")
    }

    pub fn type_str(&self) -> &'static str {
        match self.obj_type {
            0 => "UNKNOWN",
            1 => "PROCESS",
            2 => "DRIVER",
            3 => "DEVICE",
            4 => "PIPE",
            5 => "EVENTBUS",
            6 => "BLOCKDEV",
            7 => "FILESYSTEM",
            8 => "MEMREGION",
            9 => "SYMLINK",
            10 => "MOUNTPOINT",
            11 => "DIRECTORY",
            _ => "?",
        }
    }
}

/// sys_set_keyboard_layout (RAX=49): change keyboard layout.
/// layout = 0 (US) or 1 (SP).
pub fn sys_set_keyboard_layout(layout: u8) -> Result<(), i64> {
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov rax, 49",
            "mov rbx, {layout}",
            "int 0x80",
            "pop rbx",
            layout = in(reg) layout as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_set_priority (RAX=51): set process scheduling priority (admin).
pub fn sys_set_priority(pid: u32, priority: u8) -> Result<(), i64> {
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 51",
            "mov rbx, {pid}",
            "mov rcx, {priority}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            pid = in(reg) pid as u64,
            priority = in(reg) priority as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_kill (RAX=52): terminate a process by PID (admin).
pub fn sys_kill(pid: u32) -> Result<(), i64> {
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov rax, 52",
            "mov rbx, {pid}",
            "int 0x80",
            "pop rbx",
            pid = in(reg) pid as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_set_volume_label (RAX=54): set the volume label for a drive.
pub fn sys_set_volume_label(drive: u8, label: &[u8]) -> Result<(), i64> {
    if label.len() > 11 {
        return Err(EINVAL);
    }
    let mut buf = [0u8; 12];
    buf[..label.len()].copy_from_slice(label);
    let ptr = buf.as_ptr();
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 54",
            "mov rbx, {drive}",
            "mov rcx, {ptr}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            drive = in(reg) drive as u64,
            ptr = in(reg) ptr as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(()) }
}

/// FsckStats — mirrors kernel's FsckStatsRaw (RAX=55).
#[repr(C)]
pub struct FsckStats {
    pub total_inodes: u32,
    pub used_inodes: u32,
    pub valid_inodes: u32,
    pub corrupted_inodes: u32,
    pub cross_linked_blocks: u32,
    pub orphan_inodes: u32,
    pub dangling_entries: u32,
    pub dir_errors: u32,
    pub superblock_errors: u32,
    pub repairs_applied: u32,
}

/// sys_fsck (RAX=55): Run filesystem integrity check.
/// drive = ASCII drive letter (e.g. b'C'), repair = true to repair errors.
pub fn sys_fsck(drive: u8, repair: bool) -> Result<FsckStats, i64> {
    let mut stats = FsckStats {
        total_inodes: 0, used_inodes: 0, valid_inodes: 0,
        corrupted_inodes: 0, cross_linked_blocks: 0,
        orphan_inodes: 0, dangling_entries: 0, dir_errors: 0,
        superblock_errors: 0, repairs_applied: 0,
    };
    let ptr = &mut stats as *mut FsckStats as *mut u8;
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "push rdx",
            "mov rax, 55",
            "mov rbx, {ptr}",
            "mov rcx, {drive}",
            "mov rdx, {repair}",
            "int 0x80",
            "pop rdx",
            "pop rcx",
            "pop rbx",
            ptr = in(reg) ptr as u64,
            drive = in(reg) drive as u64,
            repair = in(reg) repair as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(stats) }
}

/// DriverInfo — mirrors kernel's DriverInfoRaw (RAX=56).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DriverInfo {
    pub id: u32,
    pub state: u8,
    pub category: u8,
    pub driver_type: u8,
    pub api_version: u16,
    pub abi_min: u16,
    pub abi_target: u16,
    pub abi_max: u16,
    pub last_error: u32,
    pub caps: u64,
    pub isolation_mode: u8,
    pub events_received: u64,
    pub tick_count: u64,
    pub registered_at_tick: u64,
    pub name: [u8; 8],
}

impl DriverInfo {
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(8);
        core::str::from_utf8(&self.name[..end]).unwrap_or("<?>")
    }

    pub fn state_str(&self) -> &'static str {
        match self.state {
            0 => "Loaded",
            1 => "Initialized",
            2 => "Registered",
            3 => "Bound",
            4 => "Active",
            5 => "Faulted",
            6 => "Unloaded",
            7 => "Unloading",
            _ => "Unknown",
        }
    }

    pub fn category_str(&self) -> &'static str {
        match self.category {
            0 => "Boot",
            1 => "System",
            2 => "Demand",
            _ => "Unknown",
        }
    }
}

/// sys_driver_enum (RAX=56): enumerate registered drivers by index.
/// index = 0-based driver index. Returns Some(info) if entry exists, None at end.
pub fn sys_driver_enum(index: usize) -> Result<Option<DriverInfo>, i64> {
    let mut info = DriverInfo {
        id: 0, state: 0, category: 0, driver_type: 0,
        api_version: 0, abi_min: 0, abi_target: 0, abi_max: 0,
        last_error: 0, caps: 0, isolation_mode: 0,
        events_received: 0, tick_count: 0, registered_at_tick: 0,
        name: [0; 8],
    };
    let ptr = &mut info as *mut DriverInfo as *mut u8;
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 56",
            "mov rbx, {idx}",
            "mov rcx, {ptr}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            idx = in(reg) index as u64,
            ptr = in(reg) ptr as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 {
        Err(r)
    } else if r == 0 {
        Ok(None)
    } else {
        Ok(Some(info))
    }
}

/// sys_driver_load (RAX=57): load a NEM driver from a filesystem path (admin).
pub fn sys_driver_load(path: &str) -> Result<u32, i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr();
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov rax, 57",
            "mov rbx, {ptr}",
            "int 0x80",
            "pop rbx",
            ptr = in(reg) ptr as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(r as u32) }
}

/// sys_driver_unload (RAX=58): unload a NEM driver by name (admin).
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
            "mov rax, 58",
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

/// sys_cursor_blink (RAX=53): enable/disable automatic cursor blinking.
pub fn sys_cursor_blink(enabled: bool) -> Result<(), i64> {
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov rax, 53",
            "mov rbx, {enable}",
            "int 0x80",
            "pop rbx",
            enable = in(reg) enabled as u64,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_kobj_enum (RAX=48): enumerate kernel objects.
/// buf must be large enough for max_entries entries (each 48 bytes).
/// Returns number of entries written.
pub fn sys_kobj_enum(buf: &mut [KObjEntryRaw]) -> Result<usize, i64> {
    let buf_ptr = buf.as_mut_ptr() as *mut u8;
    let max = buf.len() as u64;
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 48",
            "mov rbx, {ptr}",
            "mov rcx, {max}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            ptr = in(reg) buf_ptr as u64,
            max = in(reg) max,
            out("rax") r,
            options(nostack),
        );
    }
    ret(r).map(|v| v as usize)
}

// ═══════════════════════════════════════════════════════════════════════
// Object Manager (Ob) — RAX 60–64
// ═══════════════════════════════════════════════════════════════════════

/// ObAccess — access mask bits (matches kernel `ObAccess`)
pub mod ob_access {
    pub const READ: u32    = 1 << 0;
    pub const WRITE: u32   = 1 << 1;
    pub const EXECUTE: u32 = 1 << 2;
    pub const DELETE: u32  = 1 << 3;
    pub const ALL: u32     = READ | WRITE | EXECUTE | DELETE;
}

/// ObInfoClass — info classes for sys_ob_query_info (RAX=62).
#[repr(u32)]
pub enum ObInfoClass {
    Basic = 0,
    Name = 1,
    File = 2,
    Process = 3,
    Thread = 4,
    Pipe = 5,
    Device = 6,
}

/// ObBasicInfo — ABI-compatible with kernel's ObBasicInfo (RAX=62, class=0).
#[repr(C)]
pub struct ObBasicInfo {
    pub obj_type: u32,
    pub refcount: u32,
    pub name: [u8; 32],
}

impl ObBasicInfo {
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&self.name[..end]).unwrap_or("<?>")
    }
}

/// ObEnumEntry — ABI-compatible with kernel's ObEnumEntry (RAX=64).
/// Backward compatible: first 44 bytes same as v0.44, new fields mode+size at end.
#[repr(C)]
pub struct ObEnumEntry {
    pub id: u64,
    pub obj_type: u32,
    pub name: [u8; 32],
    pub mode: u16,
    pub _pad: [u8; 2],
    pub size: u32,
}

impl ObEnumEntry {
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(32);
        core::str::from_utf8(&self.name[..end]).unwrap_or("<?>")
    }
}

/// ObProcessInfo — ABI-compatible with kernel's ObProcessInfo (RAX=62, class=3).
/// Layout: u32 + u32 + u8 + 3pad + u32 + u8 + 2pad = 20 bytes.
#[repr(C)]
pub struct ObProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub priority: u8,
    _align1: [u8; 3],
    pub thread_count: u32,
    pub state: u8,
    _align2: [u8; 2],
}

impl ObProcessInfo {
    pub fn state_str(&self) -> &'static str {
        match self.state {
            0 => "Ready",
            1 => "Running",
            2 => "Blocked",
            3 => "Terminated",
            _ => "?",
        }
    }

    pub fn priority_str(&self) -> &'static str {
        match self.priority {
            0 => "HIGH",
            1 => "ABOVE_NORMAL",
            2 => "NORMAL",
            3 => "IDLE",
            _ => "?",
        }
    }
}

// ── Inline asm helpers for Ob syscalls ──
// The kernel ABI: RAX=syscall, RBX=arg0, RCX=arg1, RDX=arg2, R8=arg3.
// We use push/pop to save rbx/rcx/rdx/r8 and write syscall registers
// in an order that prevents register overlap (read ptr/tmp registers
// first, then move to syscall registers).

// Safe syscall wrappers for Ob (RAX 60-64).
// Strategy: copy all args to temp registers (r8-r10) first, then
// move to the syscall arg registers (rbx/rcx/rdx/r8). This prevents
// the situation where reading an input register overwrites another
// input register due to register allocation overlap.

macro_rules! ob_syscall_2 {
    ($rax:literal, $rbx:expr, $rcx:expr) => {{
        let r: i64;
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov r8, {a0}",
            "mov r9, {a1}",
            "mov rbx, r8",
            "mov rcx, r9",
            "mov rax, {n}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            a0 = in(reg) $rbx,
            a1 = in(reg) $rcx,
            n = const $rax,
            out("rax") r,
            out("r8") _, out("r9") _,
            options(nostack),
        );
        r
    }}
}

macro_rules! ob_syscall_3 {
    ($rax:literal, $rbx:expr, $rcx:expr, $rdx:expr) => {{
        let r: i64;
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "push rdx",
            "mov r8, {a0}",
            "mov r9, {a1}",
            "mov r10, {a2}",
            "mov rbx, r8",
            "mov rcx, r9",
            "mov rdx, r10",
            "mov rax, {n}",
            "int 0x80",
            "pop rdx",
            "pop rcx",
            "pop rbx",
            a0 = in(reg) $rbx,
            a1 = in(reg) $rcx,
            a2 = in(reg) $rdx,
            n = const $rax,
            out("rax") r,
            out("r8") _, out("r9") _, out("r10") _,
            options(nostack),
        );
        r
    }}
}

macro_rules! ob_syscall_4 {
    ($rax:literal, $rbx:expr, $rcx:expr, $rdx:expr, $r8:expr) => {{
        let r: i64;
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "push rdx",
            "push r8",
            "mov r9, {a0}",
            "mov r10, {a1}",
            "mov r11, {a2}",
            "mov r12, {a3}",
            "mov rbx, r9",
            "mov rcx, r10",
            "mov rdx, r11",
            "mov r8, r12",
            "mov rax, {n}",
            "int 0x80",
            "pop r8",
            "pop rdx",
            "pop rcx",
            "pop rbx",
            a0 = in(reg) $rbx,
            a1 = in(reg) $rcx,
            a2 = in(reg) $rdx,
            a3 = in(reg) $r8,
            n = const $rax,
            out("rax") r,
            out("r9") _, out("r10") _, out("r11") _, out("r12") _,
            options(nostack),
        );
        r
    }}
}

/// sys_ob_open (RAX=60): open an Ob namespace object.
pub fn sys_ob_open(path: &str, access_mask: u32) -> Result<u8, i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr() as u64;
    let r = unsafe { ob_syscall_2!(60, ptr, access_mask as u64) };
    ret(r).map(|v| v as u8)
}

/// sys_ob_create (RAX=61): create an object.
pub fn sys_ob_create(path: &str, obj_type: u32, fds_out: Option<&mut [u64; 2]>) -> Result<u8, i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr() as u64;
    let fds_ptr = match fds_out {
        Some(f) => f.as_mut_ptr() as u64,
        None => 0u64,
    };
    let r = unsafe { ob_syscall_4!(61, ptr, obj_type as u64, fds_ptr, 0u64) };
    ret(r).map(|v| v as u8)
}

/// sys_ob_query_info (RAX=62): query metadata for an object by fd.
pub fn sys_ob_query_info(fd: u8, info_class: ObInfoClass, buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr() as u64;
    let len = buf.len() as u64;
    let r = unsafe { ob_syscall_4!(62, fd as u64, info_class as u64, ptr, len) };
    ret(r).map(|v| v as usize)
}

/// sys_ob_set_info (RAX=63): set metadata for an object by fd.
pub fn sys_ob_set_info(fd: u8, info_class: u32, buf: &[u8]) -> Result<(), i64> {
    let ptr = buf.as_ptr() as u64;
    let len = buf.len() as u64;
    let r = unsafe { ob_syscall_4!(63, fd as u64, info_class as u64, ptr, len) };
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_ob_enum (RAX=64): enumerate objects in a namespace directory by fd.
pub fn sys_ob_enum(dir_fd: u8, entries: &mut [ObEnumEntry]) -> Result<usize, i64> {
    let ptr = entries.as_mut_ptr() as *mut u8 as u64;
    let max = entries.len() as u64;
    let r = unsafe { ob_syscall_3!(64, dir_fd as u64, ptr, max) };
    ret(r).map(|v| v as usize)
}

/// sys_ob_wait (RAX=65): wait on an Ob object (process, thread).
/// Waits for the object to be signaled (e.g. process exit).
/// Returns 0 on success, negative on error.
pub fn sys_ob_wait(fd: u8) -> Result<(), i64> {
    let handles = [fd as u64];
    let fd_ptr = handles.as_ptr() as u64;
    let r = unsafe { ob_syscall_3!(65, 1u64, fd_ptr, 0u64) };
    if r < 0 { Err(r) } else { Ok(()) }
}