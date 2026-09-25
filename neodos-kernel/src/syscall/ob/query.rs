//! Ob query — extracted from ob.rs
use alloc::string::{String, ToString};
use crate::scheduler::{self, ThreadState};
use crate::object::types::{ObInfoClass, ObSetInfoClass};
use crate::log::LogSubsys;
use crate::syscall::ob::types::{ObBasicInfo, ObFileInfo, ObProcessInfo, ObPipeInfo, ObThreadInfo, ObDeviceInfo, SysDateTime, DriveInfoRaw, DriverInfoRaw, ObPipeFds, StatsHeader, CpuStatsEntry, ThreadStatsEntry, STATS_VERSION};
use crate::syscall::{current_handle_entry, copy_handle_entry_for_child, resolve_chdir_target, err_to_u64, ob_err_to_syscall, SyscallError};
use crate::syscall::util::{is_user_ptr_valid, copy_user_string};

// ═══════════════════════════════════════════════════════════════════════
// SMP observability — CpuStats (24) / ThreadStats (25)
//
// Design notes:
//  * CpuStats reads each CPU's KPRCB by kernel virtual address (KPRCB_PAGES),
//    the same mechanism `is_pid_running_on_any_cpu` already uses. The reads are
//    lock-free and each field is an aligned scalar, so no torn word is
//    observable, but the *set* of fields is NOT a single atomic instant: a
//    timer/interrupt on that CPU may update one counter between two of our
//    reads. This is documented best-effort snapshot semantics.
//  * ThreadStats takes the global scheduler lock only to copy a bounded batch
//    of Kthread fields into a static staging buffer, then releases it before
//    touching user memory. No Kthread pointer escapes to user space.
// ═══════════════════════════════════════════════════════════════════════

/// Maximum threads captured in a single ThreadStats snapshot. If the live
/// count exceeds this, the header reports `total > returned` (no silent loss).
const THREAD_STATS_MAX: usize = 128;

/// Serialises the shared staging buffer across CPUs. `try_lock` only: a
/// contended query returns `-Again` and the caller may retry; we never sleep
/// while holding the scheduler lock.
static THREAD_STATS_LOCK: spin::Mutex<()> = spin::Mutex::new(());

/// Staging area for one ThreadStats snapshot (never exposed to user space).
static mut THREAD_STATS_STAGING: [ThreadStatsEntry; THREAD_STATS_MAX] = [
    ThreadStatsEntry {
        tid: 0, pid: 0, cpu_id: 0, priority: 0, state: 0, _pad: [0; 2], cpu_ticks: 0,
    };
    THREAD_STATS_MAX
];

/// Read one CPU's KPRCB counters by kernel virtual address.
///
/// `online` is supplied by the caller (derived from `cpu_local::cpu_count()`),
/// so this function never invents an online state.
fn read_cpu_stats(cpu: u32, online: bool, total: u32) -> CpuStatsEntry {
    use crate::arch::x64::cpu_local::{
        KPRCB_PAGES, OFFSET_CPU_ID, OFFSET_APIC_ID, OFFSET_INTERRUPT_COUNT,
        OFFSET_CONTEXT_SWITCH_COUNT, OFFSET_TIMER_TICK_COUNT,
    };

    let addr = unsafe {
        if (cpu as usize) < KPRCB_PAGES.len() {
            core::ptr::read_volatile(core::ptr::addr_of!(KPRCB_PAGES[cpu as usize]))
        } else {
            0
        }
    };

    // A zero KPRCB means the slot was never allocated (should not happen after
    // boot); report identity but zero counters rather than dereferencing null.
    if addr == 0 {
        return CpuStatsEntry {
            interrupt_count: 0,
            context_switch_count: 0,
            timer_tick_count: 0,
            cpu_id: cpu,
            apic_id: 0,
            online: (online && cpu < total) as u8,
            _pad: [0; 7],
        };
    }

    unsafe {
        CpuStatsEntry {
            interrupt_count: core::ptr::read_volatile(
                (addr + OFFSET_INTERRUPT_COUNT as u64) as *const u64,
            ),
            context_switch_count: core::ptr::read_volatile(
                (addr + OFFSET_CONTEXT_SWITCH_COUNT as u64) as *const u64,
            ),
            timer_tick_count: core::ptr::read_volatile(
                (addr + OFFSET_TIMER_TICK_COUNT as u64) as *const u64,
            ),
            // Identity comes from the KPRCB itself; we never assume
            // index == cpu_id or cpu_id == apic_id.
            cpu_id: core::ptr::read_volatile((addr + OFFSET_CPU_ID as u64) as *const u32),
            apic_id: core::ptr::read_volatile((addr + OFFSET_APIC_ID as u64) as *const u32),
            online: (online && cpu < total) as u8,
            _pad: [0; 7],
        }
    }
}

/// Build a `[StatsHeader][CpuStatsEntry; returned]` snapshot into `buf_ptr`.
fn snapshot_cpu_stats(buf_ptr: u64, buf_size: usize) -> u64 {
    let hdr_sz = core::mem::size_of::<StatsHeader>();
    let entry_sz = core::mem::size_of::<CpuStatsEntry>();
    if buf_size < hdr_sz {
        return err_to_u64(SyscallError::Inval);
    }

    // NeoDOS brings CPUs online sequentially and `cpu_count()` returns the
    // highest online CPU id + 1, so `0..total` is exactly the online set.
    let total = crate::arch::x64::cpu_local::cpu_count()
        .clamp(1, crate::arch::x64::cpu_local::MAX_CPUS as u32);
    let cap = (buf_size - hdr_sz) / entry_sz;
    let returned = core::cmp::min(cap, total as usize);

    let hdr = StatsHeader {
        version: STATS_VERSION,
        total,
        returned: returned as u32,
        entry_size: entry_sz as u32,
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            &hdr as *const StatsHeader as *const u8,
            buf_ptr as *mut u8,
            hdr_sz,
        );
        let mut out = (buf_ptr as *mut u8).add(hdr_sz) as *mut CpuStatsEntry;
        for cpu in 0..returned {
            let entry = read_cpu_stats(cpu as u32, true, total);
            out.write(entry);
            out = out.add(1);
        }
    }
    (hdr_sz + returned * entry_sz) as u64
}

/// Build a `[StatsHeader][ThreadStatsEntry; returned]` snapshot into `buf_ptr`.
///
/// The scheduler lock is held only while copying Kthread fields into the
/// staging buffer; user memory is written after it is released. Callers see a
/// best-effort snapshot: threads created/terminated concurrently may or may
/// not appear, and `Kthread.cpu` reflects the CPU the thread is enqueued or
/// running on at copy time.
fn snapshot_thread_stats(buf_ptr: u64, buf_size: usize) -> u64 {
    let hdr_sz = core::mem::size_of::<StatsHeader>();
    let entry_sz = core::mem::size_of::<ThreadStatsEntry>();
    if buf_size < hdr_sz {
        return err_to_u64(SyscallError::Inval);
    }

    // One snapshot in flight; contended calls are retryable, never blocking.
    let _guard = match THREAD_STATS_LOCK.try_lock() {
        Some(g) => g,
        None => return err_to_u64(SyscallError::Again),
    };

    let mut total = 0usize;
    let mut staged = 0usize;
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        for k in lock.kthreads.iter().flatten() {
            total += 1;
            if staged < THREAD_STATS_MAX {
                let entry = ThreadStatsEntry {
                    tid: k.tid,
                    pid: k.pid,
                    cpu_id: k.cpu,
                    priority: k.priority,
                    state: k.state.to_u8(),
                    _pad: [0; 2],
                    cpu_ticks: k.cpu_ticks,
                };
                unsafe {
                    (core::ptr::addr_of_mut!(THREAD_STATS_STAGING) as *mut ThreadStatsEntry)
                        .add(staged)
                        .write(entry);
                }
                staged += 1;
            }
        }
    });

    let cap = (buf_size - hdr_sz) / entry_sz;
    let copied = core::cmp::min(staged, cap);
    let hdr = StatsHeader {
        version: STATS_VERSION,
        total: total as u32,
        returned: copied as u32,
        entry_size: entry_sz as u32,
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            &hdr as *const StatsHeader as *const u8,
            buf_ptr as *mut u8,
            hdr_sz,
        );
        if copied > 0 {
            core::ptr::copy_nonoverlapping(
                core::ptr::addr_of!(THREAD_STATS_STAGING) as *const u8,
                (buf_ptr as *mut u8).add(hdr_sz),
                copied * entry_sz,
            );
        }
    }
    (hdr_sz + copied * entry_sz) as u64
}

/// Aggregate a process's thread states into the documented `ObProcessInfo`
/// state encoding (0 Ready, 1 Running, 2 Blocked, 3 Terminated):
///   1 Running    — at least one thread Running on some CPU
///   0 Ready      — no Running thread, at least one Ready
///   2 Blocked    — live threads exist, all Blocked/Suspended/Terminated
///   3 Terminated — no live thread exists
fn process_state_aggregate(any_live: bool, any_running: bool, any_ready: bool) -> u8 {
    if !any_live {
        3
    } else if any_running {
        1
    } else if any_ready {
        0
    } else {
        2
    }
}

pub fn handler_ob_query_info(regs: crate::syscall::Registers) -> u64 {
    let fd = regs.rbx as u8;
    let info_class = regs.rcx as u32;
    let buf_ptr = regs.rdx;
    let buf_size = regs.r8 as usize;

    if buf_ptr == 0 || buf_size == 0 {
        return err_to_u64(SyscallError::Inval);
    }
    if !is_user_ptr_valid(buf_ptr, buf_size as u64) {
        return err_to_u64(SyscallError::Fault);
    }

    let entry = current_handle_entry(fd);
    if !entry.is_open() {
        return err_to_u64(SyscallError::BadF);
    }

    match info_class {
        _ if info_class == ObInfoClass::Basic as u32 => {
            if entry.object_id == 0 {
                let basic = ObBasicInfo {
                    obj_type: entry.obj_type().map(|t| t as u32).unwrap_or(0),
                    refcount: 1,
                    name: {
                        let mut n = [0u8; 32];
                        let src: &[u8] = if entry.is_stdin() {
                            b"STDIN"
                        } else if entry.is_stdout() {
                            b"STDOUT"
                        } else if entry.is_stderr() {
                            b"STDERR"
                        } else {
                            b"HANDLE"
                        };
                        let len = src.len().min(31);
                        n[..len].copy_from_slice(&src[..len]);
                        n
                    },
                };
                let sz = core::mem::size_of::<ObBasicInfo>();
                if buf_size < sz { return err_to_u64(SyscallError::Inval); }
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &basic as *const ObBasicInfo as *const u8,
                        buf_ptr as *mut u8, sz,
                    );
                }
                return sz as u64;
            }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                let mut name = [0u8; 32];
                let src = obj.name;
                let len = src.iter().position(|&b| b == 0).unwrap_or(32).min(31);
                name[..len].copy_from_slice(&src[..len]);
                let basic = ObBasicInfo {
                    obj_type: obj.obj_type as u32,
                    refcount: obj.refcount,
                    name,
                };
                let sz = core::mem::size_of::<ObBasicInfo>();
                if buf_size < sz { return err_to_u64(SyscallError::Inval); }
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &basic as *const ObBasicInfo as *const u8,
                        buf_ptr as *mut u8, sz,
                    );
                }
                sz as u64
            } else {
                err_to_u64(SyscallError::BadF)
            }
        }
        _ if info_class == ObInfoClass::Name as u32 => {
            if entry.object_id == 0 {
                return 0u64;
            }
            if let Some(obj) = crate::object::ob_lookup(entry.object_id) {
                let name_str = obj.name_str();
                let bytes = name_str.as_bytes();
                let copy_len = bytes.len().min(buf_size - 1).min(255);
                unsafe {
                    core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, copy_len);
                    (buf_ptr as *mut u8).add(copy_len).write(0u8);
                }
                copy_len as u64
            } else {
                err_to_u64(SyscallError::BadF)
            }
        }
        _ if info_class == ObInfoClass::File as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive = entry.drive().unwrap_or(0);
            let inode = entry.native_id().unwrap_or(0) as u32;
            let size = crate::globals::with_vfs(|vfs| {
                vfs.stat(drive as usize, inode).map(|n| n.size).unwrap_or(0)
            });
            let fi = ObFileInfo {
                size: size as u64,
                drive,
                inode,
                padding: [0u8; 3],
            };
            let sz = core::mem::size_of::<ObFileInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &fi as *const ObFileInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Process as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            let pi = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let lock = s.lock();
                if let Some(ep) = lock.find_eprocess(pid) {
                    // Aggregate the process's threads into one documented state
                    // (layout/semantics compatible with ObProcessInfo::state):
                    //   1 Running  — at least one thread Running on some CPU
                    //   0 Ready    — no Running thread, at least one Ready
                    //   2 Blocked  — all live threads Blocked/Suspended
                    //   3 Terminated — no live thread exists
                    // This replaces the old `if thread_count == 0 { 1 } else { 0 }`
                    // which reported every live process as "Ready".
                    let mut prio = 2u8;
                    let mut found_thread = false;
                    let mut any_running = false;
                    let mut any_ready = false;
                    for k in lock.kthreads.iter().flatten() {
                        if k.pid != pid {
                            continue;
                        }
                        if !found_thread {
                            prio = k.priority;
                            found_thread = true;
                        }
                        match k.state {
                            ThreadState::Running => any_running = true,
                            ThreadState::Ready => any_ready = true,
                            _ => {}
                        }
                    }
                    let state = process_state_aggregate(found_thread, any_running, any_ready);
                    ObProcessInfo {
                        pid,
                        parent_pid: ep.parent_pid,
                        priority: prio,
                        thread_count: ep.thread_count,
                        state,
                        padding: [0u8; 2],
                    }
                } else {
                    ObProcessInfo {
                        pid, parent_pid: 0, priority: 0,
                        thread_count: 0, state: 0, padding: [0u8; 2],
                    }
                }
            });
            let sz = core::mem::size_of::<ObProcessInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &pi as *const ObProcessInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Thread as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Process {
                return err_to_u64(SyscallError::Inval);
            }
            let pid = obj.native_id as u32;
            let ti = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler();
                let lock = s.lock();
                let mut found = ObThreadInfo {
                    tid: 0, pid, state: 0, priority: 0, padding: [0u8; 2],
                };
                    for k in lock.kthreads.iter().flatten() {
                        if k.pid == pid {
                            found.tid = k.tid;
                            found.state = k.state.to_u8();
                            found.priority = k.priority;
                            break;
                        }
                    }
                found
            });
            let sz = core::mem::size_of::<ObThreadInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &ti as *const ObThreadInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Pipe as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Pipe) {
                return err_to_u64(SyscallError::Inval);
            }
            let pipe_id = entry.native_id().unwrap_or(0) as u8;
            let capacity = crate::object::pipe::PIPE_BUF_SIZE;
            let read_refs = crate::object::pipe::pipe_peek_read_ready(pipe_id)
                .map(|_| 1u32).unwrap_or(0);
            let info = ObPipeInfo {
                capacity: capacity as u32,
                read_refs,
                write_refs: 0,
            };
            let sz = core::mem::size_of::<ObPipeInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &info as *const ObPipeInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Device as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            let di = ObDeviceInfo {
                device_id: obj.native_id as u32,
                reserved: 0,
            };
            let sz = core::mem::size_of::<ObDeviceInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &di as *const ObDeviceInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::CpuInfo as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 3 {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<crate::cpu::CpuInfoFull>();
            if buf_size < (sz as usize) { return err_to_u64(SyscallError::Inval); }
            let info = crate::cpu::get_cpu_info_full();
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &info as *const crate::cpu::CpuInfoFull as *const u8,
                    buf_ptr as *mut u8, sz as usize,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Version as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 4 {
                return err_to_u64(SyscallError::Inval);
            }
            let ver = crate::KERNEL_VERSION.as_bytes();
            let copy_len = ver.len().min(buf_size);
            unsafe {
                core::ptr::copy_nonoverlapping(ver.as_ptr(), buf_ptr as *mut u8, copy_len);
            }
            ver.len() as u64
        }
        _ if info_class == ObInfoClass::DateTime as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 5 {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<SysDateTime>() as usize;
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let dt = crate::drivers::rtc_bridge::request_datetime();
            let sysdt = match dt {
                Some(d) => SysDateTime {
                    second: d.second,
                    minute: d.minute,
                    hour: d.hour,
                    day: d.day,
                    month: d.month,
                    year: d.year,
                    valid: 1,
                },
                None => SysDateTime {
                    second: 0, minute: 0, hour: 0,
                    day: 0, month: 0, year: 0, valid: 0,
                },
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &sysdt as *const SysDateTime as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Memory as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 1 {
                return err_to_u64(SyscallError::Inval);
            }
            const OLD_SZ: usize = 48;
            let full_sz = core::mem::size_of::<crate::memory::MemoryStats>();
            if buf_size < OLD_SZ { return err_to_u64(SyscallError::Inval); }
            let copy_sz = core::cmp::min(buf_size, full_sz);
            let stats = crate::memory::stats();
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &stats as *const crate::memory::MemoryStats as *const u8,
                    buf_ptr as *mut u8, copy_sz,
                );
            }
            copy_sz as u64
        }
        _ if info_class == ObInfoClass::Drives as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 6 {
                return err_to_u64(SyscallError::Inval);
            }
            let entry_size = core::mem::size_of::<DriveInfoRaw>();
            let max_entries = buf_size / entry_size;
            if max_entries == 0 { return 0u64; }
            let written = crate::globals::with_vfs(|vfs| {
                let mut count = 0usize;
                for i in 0..26 {
                    if count >= max_entries { break; }
                    if vfs.drives[i].is_some() {
                        let letter = (b'A' + i as u8) as char;
                        let label = vfs.volume_label(letter).unwrap_or_default();
                        let (fs_type_str, total_sectors) = {
                            let fs = vfs.drives[i].as_ref().unwrap();
                            (fs.fs_type(), fs.total_sectors())
                        };
                        let mut fs_type_bytes = [0u8; 16];
                        let fst = fs_type_str.as_bytes();
                        let copy_len = fst.len().min(15);
                        fs_type_bytes[..copy_len].copy_from_slice(&fst[..copy_len]);
                        let mut label_bytes = [0u8; 32];
                        let lbl = label.as_bytes();
                        let lbl_len = lbl.len().min(31);
                        label_bytes[..lbl_len].copy_from_slice(&lbl[..lbl_len]);
                        let raw = DriveInfoRaw {
                            letter: i as u8 + b'A',
                            present: 1,
                            fs_type: fs_type_bytes,
                            label: label_bytes,
                            total_sectors,
                        };
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                &raw as *const DriveInfoRaw as *const u8,
                                (buf_ptr as *mut u8).add(count * entry_size),
                                entry_size,
                            );
                        }
                        count += 1;
                    }
                }
                (count * entry_size) as u64
            });
            written
        }
        _ if info_class == ObInfoClass::Drivers as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };

            if obj.obj_type == crate::object::ObType::Driver {
                let driver_id = obj.native_id as u32;
                let entry_size = core::mem::size_of::<DriverInfoRaw>();
                if buf_size < entry_size { return 0u64; }
                if let Some(d) = crate::drivers::driver_runtime::get_driver(driver_id) {
                    let raw = DriverInfoRaw {
                        id: d.id, state: d.state as u8, category: d.category as u8,
                        driver_type: d.driver_type as u8, api_version: d.api_version,
                        abi_min: d.abi_min, abi_target: d.abi_target, abi_max: d.abi_max,
                        last_error: d.last_error, caps: d.caps, isolation_mode: d.isolation_mode,
                        events_received: d.events_received, tick_count: d.tick_count,
                        registered_at_tick: d.registered_at_tick, name: d.name,
                    };
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            &raw as *const DriverInfoRaw as *const u8,
                            buf_ptr as *mut u8,
                            entry_size,
                        );
                    }
                    return entry_size as u64;
                }
                return err_to_u64(SyscallError::NoEnt);
            }

            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 7 {
                return err_to_u64(SyscallError::Inval);
            }
            let entry_size_bulk = core::mem::size_of::<DriverInfoRaw>();
            let max_entries = buf_size / entry_size_bulk;
            if max_entries == 0 { return 0u64; }
            let runtime = crate::drivers::driver_runtime::DRIVER_RUNTIME.lock();
            let ids = runtime.driver_ids();
            let count = ids.len().min(max_entries);
            for (i, &id) in ids.iter().enumerate().take(count) {
                if let Some(d) = crate::drivers::driver_runtime::get_driver(id) {
                    let raw = DriverInfoRaw {
                        id: d.id, state: d.state as u8, category: d.category as u8,
                        driver_type: d.driver_type as u8, api_version: d.api_version,
                        abi_min: d.abi_min, abi_target: d.abi_target, abi_max: d.abi_max,
                        last_error: d.last_error, caps: d.caps, isolation_mode: d.isolation_mode,
                        events_received: d.events_received, tick_count: d.tick_count,
                        registered_at_tick: d.registered_at_tick, name: d.name,
                    };
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            &raw as *const DriverInfoRaw as *const u8,
                            (buf_ptr as *mut u8).add(i * entry_size_bulk),
                            entry_size_bulk,
                        );
                    }
                }
            }
    drop(runtime);
    (count * entry_size_bulk) as u64
        }
        _ if info_class == ObInfoClass::Cwd as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 8 {
                return err_to_u64(SyscallError::Inval);
            }
            let (drive, path) = crate::scheduler::get_current_cwd();
            let full = alloc::format!("{}:{}", (b'A' + drive) as char, path);
            let bytes = full.as_bytes();
            let copy_len = bytes.len().min(buf_size.saturating_sub(1));
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, copy_len);
                (buf_ptr as *mut u8).add(copy_len).write(0);
            }
            copy_len as u64
        }
        _ if info_class == ObInfoClass::KeyboardLayout as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type == crate::object::ObType::KeyboardDevice {
                if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
                let kbd = crate::kbd::KBD.lock();
                unsafe { core::ptr::write_volatile(buf_ptr as *mut u8, kbd.state.active_layout_index as u8); }
                return 1u64;
            }
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 9 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 1 { return err_to_u64(SyscallError::Inval); }
            let kbd = crate::kbd::KBD.lock();
            unsafe { core::ptr::write_volatile(buf_ptr as *mut u8, kbd.state.active_layout_index as u8); }
            1u64
        }
        _ if info_class == ObInfoClass::KeyboardInfo as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<crate::kbd::KbdState>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let kbd = crate::kbd::KBD.lock();
            let state = crate::kbd::KbdState {
                modifiers: kbd.state.modifiers,
                leds: kbd.state.leds,
                active_layout_index: kbd.state.active_layout_index,
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &state as *const crate::kbd::KbdState as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::KeyboardCaps as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            let sz = core::mem::size_of::<crate::kbd::KbdCaps>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            let kbd = crate::kbd::KBD.lock();
            let caps = crate::kbd::KbdCaps {
                max_layouts: 64,
                supports_repeat_config: true,
                supports_led_control: true,
                supports_hotkeys: true,
                num_layouts: kbd.layouts.len() as u32,
                _pad: [0u8; 3],
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &caps as *const crate::kbd::KbdCaps as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::KeyboardLayouts as u32 => {
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::KeyboardDevice {
                return err_to_u64(SyscallError::Inval);
            }
            let kbd = crate::kbd::KBD.lock();
            let entry_sz = core::mem::size_of::<crate::kbd::KbdLayoutInfo>();
            let max_entries = buf_size / entry_sz;
            let count = kbd.layouts.len().min(max_entries);
            for i in 0..count {
                let info = kbd.layouts[i].to_info(i as u32);
                let offset = i * entry_sz;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &info as *const crate::kbd::KbdLayoutInfo as *const u8,
                        (buf_ptr + offset as u64) as *mut u8, entry_sz,
                    );
                }
            }
            (count * entry_sz) as u64
        }
        _ if info_class == ObInfoClass::ReadContent as u32 => {
            let (drive_idx, inode_num, handle_offset) = crate::hal::without_interrupts(|| {
                let s = scheduler::current_scheduler();
                let mut lock = s.lock();
                if let Some(ep) = lock.current_eprocess_mut() {
                    let e = ep.handle_table[fd as usize];
                    if e.has_ob_object() {
                        if let Some(obj) = crate::object::ob_lookup(e.object_id) {
                            if obj.obj_type == crate::object::ObType::Filesystem {
                                return (obj.flags as usize, obj.native_id as u32, e.offset);
                            }
                        }
                    }
                    if let Some(ot) = e.obj_type() {
                        if ot == crate::object::ObType::Filesystem {
                            return (e.drive().unwrap_or(0) as usize, e.native_id().unwrap_or(0) as u32, e.offset);
                        }
                    }
                }
                (usize::MAX, 0, 0)
            });
            if drive_idx == usize::MAX {
                return err_to_u64(SyscallError::Inval);
            }
            let mut temp_buf = alloc::vec![0u8; buf_size];
            let result = crate::globals::with_vfs(|vfs| {
                vfs.read(drive_idx, inode_num, handle_offset, &mut temp_buf)
            });
            match result {
                Ok(bytes_read) => {
                    unsafe {
                        core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr as *mut u8, bytes_read);
                    }
                    crate::hal::without_interrupts(|| {
                        let s = scheduler::current_scheduler();
                        let mut lock = s.lock();
                        if let Some(ep) = lock.current_eprocess_mut() {
                            ep.handle_table[fd as usize].offset += bytes_read as u64;
                        }
                    });
                    bytes_read as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObInfoClass::VolumeLabel as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_byte = entry.drive().unwrap_or(0xFF);
            if drive_byte == 0xFF {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_char = (b'A' + drive_byte) as char;
            let result = crate::globals::with_vfs(|vfs| {
                vfs.volume_label(drive_char)
            });
            match result {
                Ok(label) => {
                    let bytes = label.as_bytes();
                    let copy_len = bytes.len().min(buf_size.saturating_sub(1));
                    unsafe {
                        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, copy_len);
                        (buf_ptr as *mut u8).add(copy_len).write(0);
                    }
                    copy_len as u64
                }
                Err(_) => err_to_u64(SyscallError::Io),
            }
        }
        _ if info_class == ObInfoClass::FsckStatus as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Filesystem) {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_byte = entry.drive().unwrap_or(0xFF);
            if drive_byte == 0xFF {
                return err_to_u64(SyscallError::Inval);
            }
            let drive_char = (b'A' + drive_byte) as char;
            let stat = crate::globals::with_vfs(|vfs| {
                let mut result = crate::fs::fsck::FsckStatsRaw {
                    total_blocks: 0, used_blocks: 0, free_blocks: 0,
                    total_nodes: 0, total_dirs: 0, total_files: 0,
                    errors: 0, warnings: 0, repaired: 0,
                };
                let drive_idx = match crate::fs::vfs::Vfs::drive_index(drive_char) {
                    Some(idx) => idx,
                    None => return result,
                };
                if let Some(fs) = vfs.drives[drive_idx].as_mut() {
                    let _ = fs.fsck(false, false, &mut result);
                }
                result
            });
            let sz = core::mem::size_of::<crate::fs::fsck::FsckStatsRaw>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &stat as *const _ as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::ProcessId as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 12 {
                return err_to_u64(SyscallError::Inval);
            }
            if buf_size < 4 { return err_to_u64(SyscallError::Inval); }
            let pid = crate::hal::without_interrupts(|| {
                crate::scheduler::current_scheduler().lock().current_pid()
            });
            let bytes = (pid as u32).to_le_bytes();
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, 4);
            }
            4u64
        }
        _ if info_class == ObInfoClass::SocketInfo as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let mgr = crate::net::socket::SOCKET_MANAGER.lock();
            let socket = match mgr.get_socket(socket_id) {
                Some(s) => s,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            #[repr(C)]
            struct NetSocketInfo {
                socket_type: u32,
                direction: u32,
                local_ip: [u8; 4],
                local_port: u16,
                remote_ip: [u8; 4],
                remote_port: u16,
            }
            let info = NetSocketInfo {
                socket_type: socket.socket_type as u32,
                direction: socket.direction as u32,
                local_ip: socket.local.ip.0,
                local_port: socket.local.port.to_be(),
                remote_ip: socket.remote.ip.0,
                remote_port: socket.remote.port.to_be(),
            };
            drop(mgr);
            let sz = core::mem::size_of::<NetSocketInfo>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &info as *const NetSocketInfo as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::SocketAddr as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let mgr = crate::net::socket::SOCKET_MANAGER.lock();
            let socket = match mgr.get_socket(socket_id) {
                Some(s) => s,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            #[repr(C)]
            struct NetSocketAddr {
                local_ip: [u8; 4],
                local_port: u16,
                remote_ip: [u8; 4],
                remote_port: u16,
            }
            let addr = NetSocketAddr {
                local_ip: socket.local.ip.0,
                local_port: socket.local.port.to_be(),
                remote_ip: socket.remote.ip.0,
                remote_port: socket.remote.port.to_be(),
            };
            drop(mgr);
            let sz = core::mem::size_of::<NetSocketAddr>();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &addr as *const NetSocketAddr as *const u8,
                    buf_ptr as *mut u8, sz,
                );
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::TcpStatus as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            let mgr = crate::net::socket::SOCKET_MANAGER.lock();
            let socket = match mgr.get_socket(socket_id) {
                Some(s) => s,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            let tcp_state = if socket.socket_type == crate::net::types::SocketType::Tcp {
                if let Some(tcp_id) = socket.tcp_conn_id {
                    crate::net::tcp::tcp_get_state(tcp_id).map(|s| s as u32).unwrap_or(0)
                } else { 0 }
            } else { 0 };
            if buf_size < 4 { return err_to_u64(SyscallError::Inval); }
            unsafe { core::ptr::write_volatile(buf_ptr as *mut u32, tcp_state); }
            4u64
        }
        // ── SocketRecv (23): read data from socket receive buffer ──
        _ if info_class == ObInfoClass::SocketRecv as u32 => {
            if entry.object_id == 0 { return err_to_u64(SyscallError::BadF); }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Socket {
                return err_to_u64(SyscallError::Inval);
            }
            let socket_id = obj.native_id as u32;
            if buf_size == 0 || buf_ptr == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let user_buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_size) };
            match crate::net::socket::socket_recv(socket_id, user_buf) {
                Ok(n) => n as u64,
                Err(_) => err_to_u64(SyscallError::Again),
            }
        }
        _ if info_class == ObInfoClass::NicInfo as u32 => {
            #[repr(C)]
            struct NicInfoRaw {
                nic_id: u32,
                mac: [u8; 6],
                ip: [u8; 4],
                link_up: u8,
                vendor_id: u16,
                device_id: u16,
                name: [u8; 16],
                description: [u8; 48],
            }
            let entry_size = core::mem::size_of::<NicInfoRaw>();
            let max_entries = buf_size / entry_size;
            if max_entries == 0 { return 0u64; }
            let count = crate::net::nic::nic_count().min(max_entries);
            for i in 0..count {
                let nic_id = i as u32;
                let mac = crate::net::nic::NIC_REGISTRY.lock().get(nic_id).map(|n| n.mac_address().0).unwrap_or([0; 6]);
                let ip = crate::net::nic::nic_get_ip(nic_id).unwrap_or(crate::net::types::Ipv4Addr::unspecified());
                let vendor_id = crate::net::nic::nic_get_vendor_id(nic_id).unwrap_or(0);
                let device_id = crate::net::nic::nic_get_device_id(nic_id).unwrap_or(0);
                let name = crate::net::nic::nic_get_name(nic_id).unwrap_or([0u8; 16]);
                let description = crate::net::nic::nic_get_description(nic_id).unwrap_or([0u8; 48]);
                let raw = NicInfoRaw {
                    nic_id,
                    mac,
                    ip: ip.0,
                    link_up: 1,
                    vendor_id,
                    device_id,
                    name,
                    description,
                };
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &raw as *const NicInfoRaw as *const u8,
                        (buf_ptr as *mut u8).add(i * entry_size),
                        entry_size,
                    );
                }
            }
            (count * entry_size) as u64
        }
        // ── RegistryKey (21): query key metadata (subkey count, value count) ──
        _ if info_class == ObInfoClass::RegistryKey as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            // Decode hive and cell, query key info via cm
            let (hive_idx, cell_idx) = crate::cm::decode_cell(native_id);
            let cm_lock = crate::cm::CM_MANAGER.lock();
            if (hive_idx as usize) >= cm_lock.hives.len() {
                return err_to_u64(SyscallError::NoEnt);
            }
            let hm = &cm_lock.hives[hive_idx as usize];
            let subkey_count = hm.hive.key_count(cell_idx) as u32;
            let value_count = hm.hive.value_count(cell_idx) as u32;
            drop(cm_lock);
            // Write [subkey_count: u32, value_count: u32] = 8 bytes
            let header = [
                (subkey_count & 0xFF) as u8, ((subkey_count >> 8) & 0xFF) as u8,
                ((subkey_count >> 16) & 0xFF) as u8, ((subkey_count >> 24) & 0xFF) as u8,
                (value_count & 0xFF) as u8, ((value_count >> 8) & 0xFF) as u8,
                ((value_count >> 16) & 0xFF) as u8, ((value_count >> 24) & 0xFF) as u8,
            ];
            let sz = 8;
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(header.as_ptr(), buf_ptr as *mut u8, sz);
            }
            sz as u64
        }
        // ── RegistryValue (22): query a value by name (name in buf, output overwrites) ──
        _ if info_class == ObInfoClass::RegistryValue as u32 => {
            if entry.obj_type() != Some(crate::object::ObType::Key) {
                return err_to_u64(SyscallError::Inval);
            }
            // Read value name from buf (null-terminated)
            let base = buf_ptr as *const u8;
            let name = {
                let mut s = alloc::string::String::new();
                for i in 0..buf_size {
                    let c = unsafe { core::ptr::read_volatile(base.add(i)) };
                    if c == 0 { break; }
                    s.push(c as char);
                }
                s
            };
            if name.is_empty() {
                return err_to_u64(SyscallError::Inval);
            }
            let native_id = match entry.native_id() {
                Some(id) => id,
                None => return err_to_u64(SyscallError::BadF),
            };
            match crate::cm::cm_query_value(native_id, &name) {
                Ok(val) => {
                    let data = &val.data;
                    let total_size = 8 + data.len();
                    // Write [value_type: u32 LE, data_len: u32 LE, data...]
                    let header = [
                        (val.value_type & 0xFF) as u8, ((val.value_type >> 8) & 0xFF) as u8,
                        ((val.value_type >> 16) & 0xFF) as u8, ((val.value_type >> 24) & 0xFF) as u8,
                        (data.len() & 0xFF) as u8, ((data.len() >> 8) & 0xFF) as u8,
                        ((data.len() >> 16) & 0xFF) as u8, ((data.len() >> 24) & 0xFF) as u8,
                    ];
                    let copy_len = if buf_size >= total_size { total_size } else { buf_size };
                    unsafe {
                        core::ptr::copy_nonoverlapping(header.as_ptr(), buf_ptr as *mut u8, 8);
                        if copy_len > 8 {
                            let data_copy = &data[..core::cmp::min(data.len(), buf_size - 8)];
                            core::ptr::copy_nonoverlapping(
                                data_copy.as_ptr(), (buf_ptr + 8) as *mut u8, data_copy.len(),
                            );
                        }
                    }
                    total_size as u64
                }
                Err(()) => err_to_u64(SyscallError::NoEnt),
            }
        }
        _ if info_class == ObInfoClass::ServiceState as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Service {
                return err_to_u64(SyscallError::Inval);
            }
            let sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            let svc = &sm.services[idx];
            let state_bytes = [svc.state as u8];
            let pid_bytes = svc.pid.to_le_bytes();
            let tick_bytes = svc.start_tick.to_le_bytes();
            let out: [u8; 13] = [
                state_bytes[0],
                pid_bytes[0], pid_bytes[1], pid_bytes[2], pid_bytes[3],
                tick_bytes[0], tick_bytes[1], tick_bytes[2], tick_bytes[3],
                tick_bytes[4], tick_bytes[5], tick_bytes[6], tick_bytes[7],
            ];
            let sz = out.len();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(out.as_ptr(), buf_ptr as *mut u8, sz);
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::ServiceConfig as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Service {
                return err_to_u64(SyscallError::Inval);
            }
            let sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            let svc = &sm.services[idx];
            let st = svc.start_type as u8;
            let rp = svc.restart_policy as u8;
            let mf = svc.max_failures.to_le_bytes();
            let mut display = [0u8; 128];
            let dn_bytes = svc.display_name.as_bytes();
            let dn_len = dn_bytes.len().min(127);
            display[..dn_len].copy_from_slice(&dn_bytes[..dn_len]);
            let mut binpath = [0u8; 256];
            let bp_bytes = svc.binary_path.as_bytes();
            let bp_len = bp_bytes.len().min(255);
            binpath[..bp_len].copy_from_slice(&bp_bytes[..bp_len]);
            let mut out = alloc::vec::Vec::with_capacity(394);
            out.push(st);
            out.push(rp);
            out.extend_from_slice(&mf);
            out.extend_from_slice(&display);
            out.extend_from_slice(&binpath);
            let sz = out.len();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(out.as_ptr(), buf_ptr as *mut u8, sz);
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::ServiceStatus as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            if obj.obj_type != crate::object::ObType::Service {
                return err_to_u64(SyscallError::Inval);
            }
            let sm = crate::services::SERVICE_MANAGER.lock();
            let idx = match sm.find_by_obj_id(entry.object_id) {
                Some(i) => i,
                None => return err_to_u64(SyscallError::NoEnt),
            };
            let svc = &sm.services[idx];
            let state = [svc.state as u8];
            let pid = svc.pid.to_le_bytes();
            let ecnt = svc.exit_count.to_le_bytes();
            let lec = svc.last_exit_code.to_le_bytes();
            let fc = svc.failure_count.to_le_bytes();
            let tick = svc.start_tick.to_le_bytes();
            let mut out = [0u8; 29];
            out[0] = state[0];
            out[1..5].copy_from_slice(&pid);
            out[5..9].copy_from_slice(&ecnt);
            out[9..17].copy_from_slice(&lec);
            out[17..21].copy_from_slice(&fc);
            out[21..29].copy_from_slice(&tick);
            let sz = out.len();
            if buf_size < sz { return err_to_u64(SyscallError::Inval); }
            unsafe {
                core::ptr::copy_nonoverlapping(out.as_ptr(), buf_ptr as *mut u8, sz);
            }
            sz as u64
        }
        _ if info_class == ObInfoClass::Hostname as u32 => {
            let root_native = crate::cm::encode_cell(0, 0);
            let key_native = match crate::cm::cm_open_key(root_native, "CurrentControlSet\\Control\\ComputerName") {
                Ok(nid) => nid,
                Err(_) => {
                    let default = b"NeoDOS-PC";
                    if buf_size > 0 {
                        let len = default.len().min(buf_size - 1);
                        unsafe {
                            core::ptr::copy_nonoverlapping(default.as_ptr(), buf_ptr as *mut u8, len);
                            core::ptr::write((buf_ptr + len as u64) as *mut u8, 0u8);
                        }
                        return (len + 1) as u64;
                    }
                    return 0;
                }
            };
            match crate::cm::cm_query_value(key_native, "ComputerName") {
                Ok(vc) => {
                    let data = &vc.data;
                    let len = data.len().min(buf_size);
                    let copy_len = if len > 0 && data[len - 1] == 0 { len - 1 } else { len };
                    if buf_size > 0 {
                        unsafe {
                            core::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr as *mut u8, copy_len.min(buf_size - 1));
                            core::ptr::write((buf_ptr + copy_len.min(buf_size - 1) as u64) as *mut u8, 0u8);
                        }
                        (copy_len.min(buf_size - 1) + 1) as u64
                    } else {
                        0
                    }
                }
                Err(_) => {
                    let default = b"NeoDOS-PC";
                    if buf_size > 0 {
                        let len = default.len().min(buf_size - 1);
                        unsafe {
                            core::ptr::copy_nonoverlapping(default.as_ptr(), buf_ptr as *mut u8, len);
                            core::ptr::write((buf_ptr + len as u64) as *mut u8, 0u8);
                        }
                        (len + 1) as u64
                    } else {
                        0
                    }
                }
            }
        }
        _ if info_class == ObInfoClass::ProcessArgs as u32 => {
            // Per-process args buffer: return current process's args (fix 1.2)
            // Usable via any valid handle, or via \Global\Info\Process handle.
            // This isolates concurrent pipeline args — kernel copies from 0x41F000
            // at spawn time into the child's Eprocess.args.
            let args = crate::hal::without_interrupts(|| {
                let s = crate::scheduler::current_scheduler().lock();
                if let Some(ep) = s.current_eprocess() {
                    ep.args
                } else {
                    [0u8; 256]
                }
            });
            let arg_len = args.iter().position(|&b| b == 0).unwrap_or(256);
            let copy_len = core::cmp::min(arg_len, buf_size.saturating_sub(1));
            unsafe {
                if copy_len > 0 {
                    core::ptr::copy_nonoverlapping(args.as_ptr(), buf_ptr as *mut u8, copy_len);
                }
                if buf_size > 0 {
                    (buf_ptr as *mut u8).add(copy_len).write(0u8);
                }
            }
            return arg_len as u64;
        }
        _ if info_class == ObInfoClass::CpuStats as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            // Global CPU stats hang off \Global\Info\CpuInfo (native_id 3).
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 3 {
                return err_to_u64(SyscallError::Inval);
            }
            snapshot_cpu_stats(buf_ptr, buf_size)
        }
        _ if info_class == ObInfoClass::ThreadStats as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            // Global thread stats hang off \Global\Info\Threads (native_id 13).
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 13 {
                return err_to_u64(SyscallError::Inval);
            }
            snapshot_thread_stats(buf_ptr, buf_size)
        }
        _ => err_to_u64(SyscallError::Inval),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SMP observability tests
// ═══════════════════════════════════════════════════════════════════════

pub fn register_ob_stats_tests() {
    use crate::{test_case, test_eq, test_true};

    test_case!("ob_info_class_smp_ids", {
        test_eq!(crate::object::types::ObInfoClass::CpuStats as u32, 24);
        test_eq!(crate::object::types::ObInfoClass::ThreadStats as u32, 25);
        // Existing IDs must not shift (ABI v8 frozen).
        test_eq!(crate::object::types::ObInfoClass::Process as u32, 3);
        test_eq!(crate::object::types::ObInfoClass::Thread as u32, 4);
        test_eq!(crate::object::types::ObInfoClass::CpuInfo as u32, 7);
        test_eq!(crate::object::types::ObInfoClass::ProcessArgs as u32, 39);
    });

    test_case!("smp_stats_struct_layout", {
        test_eq!(core::mem::size_of::<StatsHeader>(), 16);
        test_eq!(core::mem::size_of::<CpuStatsEntry>(), 40);
        test_eq!(core::mem::size_of::<ThreadStatsEntry>(), 24);
        test_eq!(STATS_VERSION, 1);
    });

    test_case!("process_state_aggregate_semantics", {
        test_eq!(process_state_aggregate(false, false, false), 3);
        test_eq!(process_state_aggregate(true, true, false), 1);
        test_eq!(process_state_aggregate(true, false, true), 0);
        test_eq!(process_state_aggregate(true, false, false), 2);
        // Running wins over Ready.
        test_eq!(process_state_aggregate(true, true, true), 1);
    });

    test_case!("cpu_stats_snapshot_header", {
        let mut buf = [0u8; 16 + 40 * 2];
        let n = snapshot_cpu_stats(buf.as_mut_ptr() as u64, buf.len()) as usize;
        test_true!(n >= core::mem::size_of::<StatsHeader>());
        let hdr = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const StatsHeader) };
        test_eq!(hdr.version, STATS_VERSION);
        test_eq!(hdr.entry_size as usize, core::mem::size_of::<CpuStatsEntry>());
        test_true!(hdr.total >= 1);
        test_true!(hdr.returned <= hdr.total);
        test_true!(hdr.returned as usize <= 2);
    });

    test_case!("thread_stats_snapshot_header", {
        let mut buf = [0u8; 16 + 24 * 4];
        let n = snapshot_thread_stats(buf.as_mut_ptr() as u64, buf.len()) as usize;
        test_true!(n >= core::mem::size_of::<StatsHeader>());
        let hdr = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const StatsHeader) };
        test_eq!(hdr.version, STATS_VERSION);
        test_eq!(hdr.entry_size as usize, core::mem::size_of::<ThreadStatsEntry>());
        test_true!(hdr.returned <= hdr.total);
        test_true!(hdr.returned as usize <= 4);
    });

    test_case!("thread_stats_truncation_is_reported", {
        // Room for exactly one entry: header.total must still be the true
        // count so callers can detect truncation (never silent).
        let mut buf = [0u8; 16 + 24];
        let n = snapshot_thread_stats(buf.as_mut_ptr() as u64, buf.len()) as usize;
        test_eq!(n, 16 + 24);
        let hdr = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const StatsHeader) };
        test_eq!(hdr.returned, 1);
        test_true!(hdr.total >= hdr.returned);
    });
}

// ═══════════════════════════════════════════════════════════════════════
// OB-013: ObSetInfo — RAX=63
// ═══════════════════════════════════════════════════════════════════════

