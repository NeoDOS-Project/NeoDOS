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
    // Use Ob API: open \Global\Info\Process, query ProcessId, close
    let fd = match sys_ob_open("\\Global\\Info\\Process", ob_access::READ) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut pid = [0u8; 4];
    let r = sys_ob_query_info(fd, ObInfoClass::ProcessId, &mut pid);
    let _ = sys_close(fd);
    match r {
        Ok(_) => u32::from_le_bytes(pid),
        Err(_) => 0,
    }
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

// NOTE: sys_open and sys_readfile removed — use ob_open/ob_query_info(ReadContent) instead.

pub fn sys_close(fd: u8) -> Result<(), i64> {
    ret_unit((export::get_table().sys_close)(fd))
}

pub fn sys_getcwd(buf: &mut [u8]) -> Result<usize, i64> {
    let fd = sys_ob_open("\\Global\\Info\\Cwd", ob_access::READ)?;
    let n = sys_ob_query_info(fd, ObInfoClass::Cwd, buf)?;
    let _ = sys_close(fd);
    Ok(n)
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

/// Open the PowerManager object and perform a shutdown.
pub fn ob_power_shutdown() -> ! {
    match sys_ob_open("\\System\\PowerManager", ob_access::WRITE) {
        Ok(fd) => {
            let _ = sys_ob_set_info(fd, ObSetInfoClass::PowerShutdown, &[]);
            let _ = sys_close(fd);
        }
        Err(_) => {}
    }
    loop {}
}

/// Open the PowerManager object and perform a reboot.
pub fn ob_power_reboot() -> ! {
    match sys_ob_open("\\System\\PowerManager", ob_access::WRITE) {
        Ok(fd) => {
            let _ = sys_ob_set_info(fd, ObSetInfoClass::PowerReboot, &[]);
            let _ = sys_close(fd);
        }
        Err(_) => {}
    }
    loop {}
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

/// MemInfo — matches kernel's extended MemoryStats (NeoMem v0.1).
/// Backward compatible: first 6 fields (48 bytes) match the v0.44 layout.
#[repr(C)]
pub struct MemInfo {
    // Physical memory (6 fields, backward compatible)
    pub phys_max: u64,
    pub total_kib: u64,
    pub usable_kib: u64,
    pub free_kib: u64,
    pub used_kib: u64,
    pub reserved_kib: u64,

    // Kernel heap (added in v0.46 / NeoMem v0.1)
    pub kernel_heap_total_kib: u64,
    pub kernel_heap_used_kib: u64,
    pub kernel_heap_free_kib: u64,

    // User memory pools
    pub user_memory_total_kib: u64,
    pub user_memory_used_kib: u64,
    pub user_memory_free_kib: u64,

    // Paging
    pub total_pages: u64,
    pub free_pages: u64,
    pub used_pages: u64,
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
/// Must match kernel's `ObInfoClass` in `src/object/types.rs`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObInfoClass {
    Basic = 0,
    Name = 1,
    File = 2,
    Process = 3,
    Thread = 4,
    Pipe = 5,
    Device = 6,
    CpuInfo = 7,
    Version = 8,
    DateTime = 9,
    Memory = 10,
    Drives = 11,
    Drivers = 12,
    Cwd = 13,
    KeyboardLayout = 14,
    ReadContent = 15,
    VolumeLabel = 16,
    SocketInfo = 17,
    SocketAddr = 18,
    TcpStatus = 19,
    NicInfo = 20,
    RegistryKey = 21,
    RegistryValue = 22,
    SocketRecv = 23,
    ServiceState = 29,
    ServiceConfig = 30,
    ServiceStatus = 31,
    FsckStatus = 33,
    ProcessId = 34,
}

pub mod ob_type {
    pub const PROCESS: u32 = 1;
    pub const DRIVER: u32 = 2;
    pub const PIPE: u32 = 4;
    pub const DIRECTORY: u32 = 11;
    pub const EVENT: u32 = 13;
    pub const THREAD: u32 = 16;
    pub const SOCKET: u32 = 18;
    pub const SERVICE: u32 = 20;
}

/// ObSetInfoClass — info classes for sys_ob_set_info (RAX=63).
/// Must match kernel's `ObSetInfoClass` in `src/object/types.rs`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObSetInfoClass {
    ProcessPriority = 0,
    ThreadPriority = 1,
    ObjectName = 2,
    Security = 3,
    ProcessTerminate = 4,
    KeyboardLayout = 5,
    VfsRename = 6,
    WriteContent = 7,
    SetCwd = 8,
    SetVolumeLabel = 9,
    TimerStart = 10,
    TimerCancel = 11,
    SemaphoreRelease = 12,
    SectionMapView = 13,
    SectionUnmapView = 14,
    FileCreate = 15,
    FileDelete = 16,
    SetProcessVt = 17,
    SocketConnect = 18,
    SocketBind = 19,
    SocketListen = 20,
    SocketSend = 21,
    SocketClose = 22,
    RegistryCreateKey = 23,
    RegistryDeleteKey = 24,
    RegistrySetValue = 25,
    RegistryDeleteValue = 26,
    SetNicIp = 27,
    ServiceStart = 33,
    ServiceStop = 34,
    ServiceRestart = 35,
    ServiceSetConfig = 36,
    PowerShutdown = 37,
    PowerReboot = 38,
    FsckRepair = 39,
}

/// Backward-compatible constants for `ObSetInfoClass`.
pub mod ob_set_info_class {
    use super::ObSetInfoClass;
    pub const PROCESS_PRIORITY: ObSetInfoClass = ObSetInfoClass::ProcessPriority;
    pub const THREAD_PRIORITY: ObSetInfoClass = ObSetInfoClass::ThreadPriority;
    pub const OBJECT_NAME: ObSetInfoClass = ObSetInfoClass::ObjectName;
    pub const SECURITY: ObSetInfoClass = ObSetInfoClass::Security;
    pub const PROCESS_TERMINATE: ObSetInfoClass = ObSetInfoClass::ProcessTerminate;
    pub const KEYBOARD_LAYOUT: ObSetInfoClass = ObSetInfoClass::KeyboardLayout;
    pub const VFS_RENAME: ObSetInfoClass = ObSetInfoClass::VfsRename;
    pub const WRITE_CONTENT: ObSetInfoClass = ObSetInfoClass::WriteContent;
    pub const SET_CWD: ObSetInfoClass = ObSetInfoClass::SetCwd;
    pub const SET_VOLUME_LABEL: ObSetInfoClass = ObSetInfoClass::SetVolumeLabel;
    pub const TIMER_START: ObSetInfoClass = ObSetInfoClass::TimerStart;
    pub const TIMER_CANCEL: ObSetInfoClass = ObSetInfoClass::TimerCancel;
    pub const SEMAPHORE_RELEASE: ObSetInfoClass = ObSetInfoClass::SemaphoreRelease;
    pub const SECTION_MAP_VIEW: ObSetInfoClass = ObSetInfoClass::SectionMapView;
    pub const SECTION_UNMAP_VIEW: ObSetInfoClass = ObSetInfoClass::SectionUnmapView;
    pub const FILE_CREATE: ObSetInfoClass = ObSetInfoClass::FileCreate;
    pub const FILE_DELETE: ObSetInfoClass = ObSetInfoClass::FileDelete;
    pub const SET_PROCESS_VT: ObSetInfoClass = ObSetInfoClass::SetProcessVt;
    pub const SOCKET_CONNECT: ObSetInfoClass = ObSetInfoClass::SocketConnect;
    pub const SOCKET_BIND: ObSetInfoClass = ObSetInfoClass::SocketBind;
    pub const SOCKET_LISTEN: ObSetInfoClass = ObSetInfoClass::SocketListen;
    pub const SOCKET_SEND: ObSetInfoClass = ObSetInfoClass::SocketSend;
    pub const SOCKET_CLOSE: ObSetInfoClass = ObSetInfoClass::SocketClose;
    pub const REGISTRY_CREATE_KEY: ObSetInfoClass = ObSetInfoClass::RegistryCreateKey;
    pub const REGISTRY_DELETE_KEY: ObSetInfoClass = ObSetInfoClass::RegistryDeleteKey;
    pub const REGISTRY_SET_VALUE: ObSetInfoClass = ObSetInfoClass::RegistrySetValue;
    pub const REGISTRY_DELETE_VALUE: ObSetInfoClass = ObSetInfoClass::RegistryDeleteValue;
    pub const SET_NIC_IP: ObSetInfoClass = ObSetInfoClass::SetNicIp;
    pub const SERVICE_START: ObSetInfoClass = ObSetInfoClass::ServiceStart;
    pub const SERVICE_STOP: ObSetInfoClass = ObSetInfoClass::ServiceStop;
    pub const SERVICE_RESTART: ObSetInfoClass = ObSetInfoClass::ServiceRestart;
    pub const SERVICE_SET_CONFIG: ObSetInfoClass = ObSetInfoClass::ServiceSetConfig;
    pub const POWER_SHUTDOWN: ObSetInfoClass = ObSetInfoClass::PowerShutdown;
    pub const POWER_REBOOT: ObSetInfoClass = ObSetInfoClass::PowerReboot;
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
pub fn sys_ob_create(path: &str, obj_type: u32, fds_out: Option<&mut [u64; 2]>, attrs: u64) -> Result<u8, i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr() as u64;
    let fds_ptr = match fds_out {
        Some(f) => f.as_mut_ptr() as u64,
        None => 0u64,
    };
    let r = unsafe { ob_syscall_4!(61, ptr, obj_type as u64, fds_ptr, attrs) };
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
pub fn sys_ob_set_info(fd: u8, info_class: ObSetInfoClass, buf: &[u8]) -> Result<(), i64> {
    let ptr = buf.as_ptr() as u64;
    let len = buf.len() as u64;
    let r = unsafe { ob_syscall_4!(63, fd as u64, info_class as u32 as u64, ptr, len) };
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

/// ob_thread_create: create a thread in the current process via ob_create(Thread).
/// `path` = Ob namespace path (e.g. "\\WorkerThread")
/// `entry` = entry point address for the new thread
/// Returns fd on success.
pub fn ob_thread_create(path: &str, entry: u64) -> Result<u8, i64> {
    sys_ob_create(path, ob_type::THREAD, None, entry)
}

/// ob_thread_join: wait for a thread to exit via ob_wait(Thread).
/// `thread_fd` = fd from ob_thread_create.
pub fn ob_thread_join(thread_fd: u8) -> Result<(), i64> {
    sys_ob_wait(thread_fd)
}

/// ob_set_thread_priority: set scheduling priority for a thread via ob_set_info.
/// `thread_fd` = fd from ob_thread_create (or ob_open on a Thread object).
/// `priority` = 0 (HIGH) .. 3 (IDLE).
pub fn ob_set_thread_priority(thread_fd: u8, priority: u8) -> Result<(), i64> {
    if priority > 3 { return Err(EINVAL); }
    let p = [priority as u32];
    let buf = unsafe { core::slice::from_raw_parts(p.as_ptr() as *const u8, 4) };
    sys_ob_set_info(thread_fd, ObSetInfoClass::ThreadPriority, buf)
}

/// sys_ob_destroy (RAX=66): destroy/delete an object by fd.
/// Removes namespace objects (directories, pipes, etc.) from Ob namespace.
/// For files, use ob_file_delete() instead.
pub fn sys_ob_destroy(fd: u8) -> Result<(), i64> {
    let r = unsafe { ob_syscall_2!(66, fd as u64, 0u64) };
    if r < 0 { Err(r) } else { Ok(()) }
}

/// ob_file_create: create a file via ob_set_info(FileCreate).
/// Calls sys_ob_set_info on the process context with FileCreate class.
/// Returns the new file fd on success.
pub fn ob_file_create(path: &str) -> Result<u8, i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr() as u64;
    let len = bytes.len() as u64;
    let r = unsafe { ob_syscall_4!(63, 1u64, ObSetInfoClass::FileCreate as u32 as u64, ptr, len) };
    if r < 0 { Err(r) } else { Ok(r as u8) }
}

/// ob_file_delete: delete a file by fd via ob_set_info(FileDelete).
/// Calls sys_ob_set_info(fd, FileDelete, null, 0).
pub fn ob_file_delete(fd: u8) -> Result<(), i64> {
    let dummy: u64 = 0;
    let r = unsafe { ob_syscall_4!(63, fd as u64, ObSetInfoClass::FileDelete as u32 as u64, &dummy as *const u64 as u64, 8u64) };
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_poll: poll file descriptors for readiness (RAX=59).
/// `fds` — array of PollFd entries.
/// `timeout_ms` — 0 = non-blocking, u64::MAX = infinite.
/// Returns number of ready fds.
#[repr(C)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

pub const POLLIN: i16 = 1;
pub const POLLOUT: i16 = 2;
pub const POLLERR: i16 = 4;
pub const POLLHUP: i16 = 8;

pub fn sys_poll(fds: &mut [PollFd], timeout_ms: u64) -> Result<usize, i64> {
    let pfds_ptr = fds.as_ptr() as u64;
    let nfds = fds.len() as u64;
    let r = unsafe { ob_syscall_3!(59, pfds_ptr, nfds, timeout_ms) };
    if r < 0 { Err(r) } else { Ok(r as usize) }
}

/// sys_sleep_ex: yield alertable — cede CPU, chequea APCs pendientes (RAX=41).
pub fn sys_sleep_ex() -> Result<(), i64> {
    let r = unsafe { ob_syscall_2!(41, 0u64, 0u64) };
    if r < 0 { Err(r) } else { Ok(()) }
}

// ── Socket wrappers ──

/// SocketAddrV4 — ABI-compatible address for socket operations.
/// IP is network-byte-order (big-endian), port is host-byte-order.
#[repr(C)]
pub struct SocketAddrV4 {
    pub ip: [u8; 4],
    pub port: u16,
}

impl SocketAddrV4 {
    pub fn new(ip: [u8; 4], port: u16) -> Self {
        SocketAddrV4 { ip, port }
    }
}

/// ob_socket_create: create a socket via ob_create(Socket).
/// `sock_type` = 1 (TCP), 2 (UDP), 3 (Raw). `port` = local port hint (0 = ephemeral).
pub fn ob_socket_create(path: &str, sock_type: u32, port: u16) -> Result<u8, i64> {
    let attrs = (sock_type & 0xFF) as u64 | ((port as u64) << 8);
    sys_ob_create(path, ob_type::SOCKET, None, attrs)
}

/// ob_socket_connect: connect to a remote address via ob_set_info(SocketConnect).
pub fn ob_socket_connect(fd: u8, ip: [u8; 4], port: u16) -> Result<(), i64> {
    let mut buf = [0u8; 6];
    buf[..4].copy_from_slice(&ip);
    buf[4..6].copy_from_slice(&port.to_be_bytes());
    sys_ob_set_info(fd, ObSetInfoClass::SocketConnect, &buf)
}

/// ob_socket_bind: bind to a local address via ob_set_info(SocketBind).
pub fn ob_socket_bind(fd: u8, ip: [u8; 4], port: u16) -> Result<(), i64> {
    let mut buf = [0u8; 6];
    buf[..4].copy_from_slice(&ip);
    buf[4..6].copy_from_slice(&port.to_be_bytes());
    sys_ob_set_info(fd, ObSetInfoClass::SocketBind, &buf)
}

/// ob_socket_listen: start listening via ob_set_info(SocketListen).
pub fn ob_socket_listen(fd: u8) -> Result<(), i64> {
    sys_ob_set_info(fd, ObSetInfoClass::SocketListen, &[])
}

/// ob_socket_send: send data via ob_set_info(SocketSend).
/// Returns number of bytes sent on success.
pub fn ob_socket_send(fd: u8, data: &[u8]) -> Result<usize, i64> {
    let r = unsafe { ob_syscall_4!(63, fd as u64, ObSetInfoClass::SocketSend as u32 as u64, data.as_ptr() as u64, data.len() as u64) };
    if r < 0 { Err(r) } else { Ok(r as usize) }
}

/// ob_socket_recv: receive data via ob_query_info(SocketRecv).
pub fn ob_socket_recv(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    sys_ob_query_info(fd, ObInfoClass::SocketRecv, buf)
}

/// ob_socket_close: close a socket via ob_set_info(SocketClose).
pub fn ob_socket_close(fd: u8) -> Result<(), i64> {
    sys_ob_set_info(fd, ObSetInfoClass::SocketClose, &[])
}

/// FsckStats — mirrors kernel's FsckStatsRaw.
#[repr(C)]
pub struct FsckStats {
    pub total_blocks: u64,
    pub used_blocks: u64,
    pub free_blocks: u64,
    pub total_nodes: u64,
    pub total_dirs: u64,
    pub total_files: u64,
    pub errors: u32,
    pub warnings: u32,
    pub repaired: u32,
}

/// ob_fsck_status: run read-only fsck check via ob_query_info(FsckStatus).
/// `drive_fd` = fd from ob_open on a file in the target filesystem.
pub fn ob_fsck_status(drive_fd: u8) -> Result<FsckStats, i64> {
    let mut stats = FsckStats {
        total_blocks: 0, used_blocks: 0, free_blocks: 0,
        total_nodes: 0, total_dirs: 0, total_files: 0,
        errors: 0, warnings: 0, repaired: 0,
    };
    let buf = unsafe {
        core::slice::from_raw_parts_mut(
            &mut stats as *mut FsckStats as *mut u8,
            core::mem::size_of::<FsckStats>(),
        )
    };
    sys_ob_query_info(drive_fd, ObInfoClass::FsckStatus, buf).map(|_| stats)
}

/// ob_fsck_repair: run fsck with repair via ob_set_info(FsckRepair).
/// `drive_fd` = fd from ob_open on a file in the target filesystem.
/// `repair` = true to attempt fixes, false for read-only check.
pub fn ob_fsck_repair(drive_fd: u8, repair: bool) -> Result<(), i64> {
    let flag = [if repair { 1u8 } else { 0u8 }];
    sys_ob_set_info(drive_fd, ObSetInfoClass::FsckRepair, &flag)
}

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
    let r = unsafe { ob_syscall_2!(67, ptr, 0u64) };
    ret(r).map(|v| v as u8)
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
    let r = unsafe { ob_syscall_4!(69, fd as u64, name_ptr, buf_ptr, buf_len) };
    ret(r).map(|v| v as usize)
}

macro_rules! ob_syscall_5 {
    ($rax:literal, $rbx:expr, $rcx:expr, $rdx:expr, $r8:expr, $r9:expr) => {{
        let r: i64;
        core::arch::asm!(
            "push rbx", "push rcx", "push rdx", "push r8", "push r9",
            "mov r10, {a0}", "mov r11, {a1}", "mov r12, {a2}", "mov r13, {a3}", "mov r14, {a4}",
            "mov rbx, r10", "mov rcx, r11", "mov rdx, r12", "mov r8, r13", "mov r9, r14",
            "mov rax, {n}",
            "int 0x80",
            "pop r9", "pop r8", "pop rdx", "pop rcx", "pop rbx",
            a0 = in(reg) $rbx, a1 = in(reg) $rcx, a2 = in(reg) $rdx, a3 = in(reg) $r8, a4 = in(reg) $r9,
            n = const $rax,
            out("rax") r,
            out("r10") _, out("r11") _, out("r12") _, out("r13") _, out("r14") _,
            options(nostack),
        );
        r
    }}
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
    let r = unsafe { ob_syscall_5!(70, fd as u64, name_ptr, value_type as u64, data_ptr, data_len) };
    if r < 0 { Err(r) } else { Ok(()) }
}

// Service Manager syscall (RAX=77)
pub const SERVICE_CONTROL_START: u32 = 0;
pub const SERVICE_CONTROL_STOP: u32 = 1;
pub const SERVICE_CONTROL_RESTART: u32 = 2;
pub const SERVICE_CONTROL_QUERY_STATUS: u32 = 3;
pub const SERVICE_CONTROL_SET_CONFIG: u32 = 4;

pub fn sys_ob_service(fd: u8, control: u32, buf: &mut [u8]) -> Result<usize, i64> {
    let buf_ptr = buf.as_mut_ptr() as u64;
    let buf_len = buf.len() as u64;
    let r = unsafe { ob_syscall_4!(77, fd as u64, control as u64, buf_ptr, buf_len) };
    if r < 0 { Err(r as i64) } else { Ok(r as usize) }
}