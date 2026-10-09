//! Object Manager (Ob) syscall wrappers, ABI enums and query helpers.

use super::{ret, ret_unit, EINVAL, sys_close};

// ═══════════════════════════════════════════════════════════════════════
// Object Manager (Ob) — RAX 60–77
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
    CpuStats = 24,
    ThreadStats = 25,
    /// Phase 15-A: coherent process+thread inspection snapshot (read-only).
    ProcessSnapshot = 26,
    /// Work-stealing / SMP balancing counters (global, read-only). Fase 3 M3.2.
    SmpStats = 27,
    /// Per-interface network counters (read-only). #373: one 40-byte entry
    /// per `NicInfo` enumeration slot (physical NICs, then loopback).
    NetStats = 28,
    ServiceState = 29,
    ServiceConfig = 30,
    ServiceStatus = 31,
    FsckStatus = 33,
    ProcessId = 34,
    KeyboardInfo = 35,
    KeyboardCaps = 36,
    KeyboardLayouts = 37,
    Hostname = 38,
    ProcessArgs = 39,
    /// Timezone configuration (standard offset + DST window) — #357.
    TimeZone = 40,
    /// RTC time converted to local time by the kernel — #357.
    LocalDateTime = 41,
    /// #358: whether a graceful shutdown has been requested for the caller's
    /// service (1 byte: 0 = normal, 1 = shutdown requested).
    ProcessShutdownState = 42,
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
    SetNicGateway = 28,
    SocketBindNic = 29,
    ServiceStart = 33,
    ServiceStop = 34,
    ServiceRestart = 35,
    ServiceSetConfig = 36,
    PowerShutdown = 37,
    PowerReboot = 38,
    FsckRepair = 39,
    KeyboardSetLayout = 43,
    KeyboardSetRepeatDelay = 44,
    KeyboardSetRepeatRate = 45,
    KeyboardSetLeds = 46,
    KeyboardSetModifier = 47,
    SetHostname = 49,
    /// Set the system (RTC) clock from a `DateTime` payload. Admin-only.
    DateTime = 50,
    /// Mark the target process object as the foreground process of the caller's
    /// VT (the Ctrl+C target). Used by the interactive shell.
    SetForegroundProcess = 51,
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
    pub const SET_FOREGROUND: ObSetInfoClass = ObSetInfoClass::SetForegroundProcess;
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
    pub const SET_NIC_GATEWAY: ObSetInfoClass = ObSetInfoClass::SetNicGateway;
    pub const SERVICE_START: ObSetInfoClass = ObSetInfoClass::ServiceStart;
    pub const SERVICE_STOP: ObSetInfoClass = ObSetInfoClass::ServiceStop;
    pub const SERVICE_RESTART: ObSetInfoClass = ObSetInfoClass::ServiceRestart;
    pub const SERVICE_SET_CONFIG: ObSetInfoClass = ObSetInfoClass::ServiceSetConfig;
    pub const POWER_SHUTDOWN: ObSetInfoClass = ObSetInfoClass::PowerShutdown;
    pub const POWER_REBOOT: ObSetInfoClass = ObSetInfoClass::PowerReboot;
    pub const KEYBOARD_SET_LAYOUT: ObSetInfoClass = ObSetInfoClass::KeyboardSetLayout;
    pub const KEYBOARD_SET_REPEAT_DELAY: ObSetInfoClass = ObSetInfoClass::KeyboardSetRepeatDelay;
    pub const KEYBOARD_SET_REPEAT_RATE: ObSetInfoClass = ObSetInfoClass::KeyboardSetRepeatRate;
    pub const KEYBOARD_SET_LEDS: ObSetInfoClass = ObSetInfoClass::KeyboardSetLeds;
    pub const KEYBOARD_SET_MODIFIER: ObSetInfoClass = ObSetInfoClass::KeyboardSetModifier;
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

/// Query per-CPU stats via an fd opened on `\Global\Info\CpuInfo`.
pub fn sys_ob_query_cpu_stats(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    sys_ob_query_info(fd, ObInfoClass::CpuStats, buf)
}

/// Query the global work-stealing counters via an fd opened on
/// `\Global\Info\CpuInfo` (same key as `CpuStats`). Writes one [`SmpStats`].
pub fn sys_ob_query_smp_stats(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    sys_ob_query_info(fd, ObInfoClass::SmpStats, buf)
}

/// Query the global thread snapshot via an fd opened on `\Global\Info\Threads`.
pub fn sys_ob_query_thread_stats(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    sys_ob_query_info(fd, ObInfoClass::ThreadStats, buf)
}

/// Query the coherent process/thread snapshot via an fd opened on
/// `\Global\Info\Processes`.
pub fn sys_ob_query_process_snapshot(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    sys_ob_query_info(fd, ObInfoClass::ProcessSnapshot, buf)
}

/// Open `\Global\Info\Processes` ready for a `ProcessSnapshot` query.
pub fn ob_open_processes() -> Result<u8, i64> {
    sys_ob_open("\\Global\\Info\\Processes", ob_access::READ)
}

/// Open `\Global\Info\CpuInfo` ready for a `CpuStats` query.
pub fn ob_open_cpu_info() -> Result<u8, i64> {
    sys_ob_open("\\Global\\Info\\CpuInfo", ob_access::READ)
}

/// Open `\Global\Info\Threads` ready for a `ThreadStats` query.
pub fn ob_open_threads() -> Result<u8, i64> {
    sys_ob_open("\\Global\\Info\\Threads", ob_access::READ)
}

/// sys_ob_open (RAX=40): open an Ob namespace object.
pub fn sys_ob_open(path: &str, access_mask: u32) -> Result<u8, i64> {
    let bytes = path.as_bytes();
    if bytes.len() >= 255 { return Err(EINVAL); }
    let mut buf = [0u8; 256];
    buf[..bytes.len()].copy_from_slice(bytes);
    let ptr = buf.as_ptr() as u64;
    let r = unsafe { ob_syscall_2!(40, ptr, access_mask as u64) };
    ret(r).map(|v| v as u8)
}

/// sys_ob_create (RAX=41): create an object.
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
    let r = unsafe { ob_syscall_4!(41, ptr, obj_type as u64, fds_ptr, attrs) };
    ret(r).map(|v| v as u8)
}

/// sys_ob_query_info (RAX=62): query metadata for an object by fd.
pub fn sys_ob_query_info(fd: u8, info_class: ObInfoClass, buf: &mut [u8]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr() as u64;
    let len = buf.len() as u64;
    let r = unsafe { ob_syscall_4!(42, fd as u64, info_class as u64, ptr, len) };
    ret(r).map(|v| v as usize)
}

/// sys_ob_set_info (RAX=63): set metadata for an object by fd.
pub fn sys_ob_set_info(fd: u8, info_class: ObSetInfoClass, buf: &[u8]) -> Result<(), i64> {
    let ptr = buf.as_ptr() as u64;
    let len = buf.len() as u64;
    let r = unsafe { ob_syscall_4!(43, fd as u64, info_class as u32 as u64, ptr, len) };
    if r < 0 { Err(r) } else { Ok(()) }
}

/// sys_ob_enum (RAX=64): enumerate objects in a namespace directory by fd.
pub fn sys_ob_enum(dir_fd: u8, entries: &mut [ObEnumEntry]) -> Result<usize, i64> {
    let ptr = entries.as_mut_ptr() as *mut u8 as u64;
    let max = entries.len() as u64;
    let r = unsafe { ob_syscall_3!(44, dir_fd as u64, ptr, max) };
    ret(r).map(|v| v as usize)
}

/// sys_ob_wait (RAX=65): wait on an Ob object (process, thread).
/// Waits for the object to be signaled (e.g. process exit).
/// Returns 0 on success, negative on error.
pub fn sys_ob_wait(fd: u8) -> Result<(), i64> {
    let handles = [fd as u64];
    let fd_ptr = handles.as_ptr() as u64;
    let r = unsafe { ob_syscall_3!(45, 1u64, fd_ptr, 0u64) };
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
    let r = unsafe { ob_syscall_2!(46, fd as u64, 0u64) };
    if r < 0 { Err(r) } else { Ok(()) }
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

// Service Manager syscall (RAX=77)
pub const SERVICE_CONTROL_START: u32 = 0;
pub const SERVICE_CONTROL_STOP: u32 = 1;
pub const SERVICE_CONTROL_RESTART: u32 = 2;
pub const SERVICE_CONTROL_QUERY_STATUS: u32 = 3;
pub const SERVICE_CONTROL_SET_CONFIG: u32 = 4;

pub fn sys_ob_service(fd: u8, control: u32, buf: &mut [u8]) -> Result<usize, i64> {
    let buf_ptr = buf.as_mut_ptr() as u64;
    let buf_len = buf.len() as u64;
    let r = unsafe { ob_syscall_4!(47, fd as u64, control as u64, buf_ptr, buf_len) };
    if r < 0 { Err(r as i64) } else { Ok(r as usize) }
}

// ── Snapshots (RAX=48) ──────────────────────────────────────────────

pub mod snapshot_op {
    pub const CREATE: u32 = 0;
    pub const RESTORE: u32 = 1;
    pub const LIST: u32 = 2;
    pub const PURGE: u32 = 3;
    pub const DELETE: u32 = 4;
    pub const EXTRACT: u32 = 5;
}

/// ABI-stable snapshot entry returned by `LIST` (24 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SnapshotEntry {
    pub id: u64,
    pub root_lba: u64,
    pub timestamp: u64,
}

pub fn sys_ob_snapshot_create(fd: u8) -> Result<u64, i64> {
    let r = unsafe { ob_syscall_4!(48, fd as u64, snapshot_op::CREATE as u64, 0u64, 0u64) };
    ret(r)
}

pub fn sys_ob_snapshot_purge(fd: u8) -> Result<(), i64> {
    let r = unsafe { ob_syscall_4!(48, fd as u64, snapshot_op::PURGE as u64, 0u64, 0u64) };
    ret_unit(r)
}

pub fn sys_ob_snapshot_list(fd: u8, buf: &mut [u8]) -> Result<usize, i64> {
    let r = unsafe { ob_syscall_4!(48, fd as u64, snapshot_op::LIST as u64, buf.as_mut_ptr() as u64, buf.len() as u64) };
    ret(r).map(|n| n as usize)
}

pub fn sys_ob_snapshot_restore(fd: u8, id: u64) -> Result<(), i64> {
    let r = unsafe { ob_syscall_4!(48, fd as u64, snapshot_op::RESTORE as u64, &id as *const u64 as u64, 8u64) };
    ret_unit(r)
}

pub fn sys_ob_snapshot_delete(fd: u8, id: u64) -> Result<(), i64> {
    let r = unsafe { ob_syscall_4!(48, fd as u64, snapshot_op::DELETE as u64, &id as *const u64 as u64, 8u64) };
    ret_unit(r)
}

/// Copiar `src` (tal como estaba en el snapshot `id`) a `dst` (árbol actual).
pub fn sys_ob_snapshot_extract(fd: u8, id: u64, src: &str, dst: &str) -> Result<u64, i64> {
    let sb = src.as_bytes();
    let db = dst.as_bytes();
    if sb.len() > 255 || db.len() > 255 { return Err(EINVAL); }
    let mut buf = [0u8; 16 + 255 + 255];
    buf[0..8].copy_from_slice(&id.to_le_bytes());
    buf[8..12].copy_from_slice(&(sb.len() as u32).to_le_bytes());
    buf[12..16].copy_from_slice(&(db.len() as u32).to_le_bytes());
    buf[16..16 + sb.len()].copy_from_slice(sb);
    buf[16 + sb.len()..16 + sb.len() + db.len()].copy_from_slice(db);
    let total = 16 + sb.len() + db.len();
    let r = unsafe { ob_syscall_4!(48, fd as u64, snapshot_op::EXTRACT as u64, buf.as_ptr() as u64, total as u64) };
    ret(r)
}

