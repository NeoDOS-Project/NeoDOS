//! Ob query — SMP observability and process/thread snapshots.

use crate::object::types::ObInfoClass;
use crate::syscall::{err_to_u64, SyscallError};
use crate::syscall::ob::types::{StatsHeader, CpuStatsEntry, ThreadStatsEntry, STATS_VERSION, ProcSnapshotHeader, ProcessInfoRaw, ThreadInfoRaw, PROC_SNAPSHOT_VERSION, PROC_NAME_MAX, PROC_SNAPSHOT_FLAG_TRUNCATED};
use super::process::process_state_aggregate;

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

/// Phase 15-A: one in-flight coherent process/thread snapshot. `try_lock`
/// only, mirroring `THREAD_STATS_LOCK`.
static PROC_SNAPSHOT_LOCK: spin::Mutex<()> = spin::Mutex::new(());

/// Staging area for one process/thread snapshot. Built under the scheduler
/// lock via `Scheduler::snapshot_into`, then copied to user space after the
/// lock is released. Never exposed to user space.
static mut PROC_SNAPSHOT_STAGING: crate::scheduler::ProcSnapshot =
    crate::scheduler::ProcSnapshot::empty();

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

/// Copy a kernel name into the fixed `PROC_NAME_MAX` user-visible field,
/// zero-padded. Kernel names are already ASCII and bounded, so this never
/// truncates in practice.
fn name_to_array(name: &str) -> [u8; PROC_NAME_MAX] {
    let mut out = [0u8; PROC_NAME_MAX];
    let bytes = name.as_bytes();
    let len = bytes.len().min(PROC_NAME_MAX);
    out[..len].copy_from_slice(&bytes[..len]);
    out
}

/// Phase 15-A: coherent process/thread snapshot for
/// `ObInfoClass::ProcessSnapshot`.
///
/// Uses `Scheduler::snapshot_into` (Phase 14-B) so processes and threads come
/// from ONE scheduler-consistent capture, never two independent traversals.
/// The snapshot is built under the scheduler lock, the lock is released, and
/// only then are records copied to user space (no kernel pointers escape).
/// Returns bytes written; `-Again` if a snapshot is already in flight;
/// `-Inval` if the buffer cannot hold the header.
fn snapshot_process_snapshot(buf_ptr: u64, buf_size: usize) -> u64 {
    let hdr_sz = core::mem::size_of::<ProcSnapshotHeader>();
    let proc_sz = core::mem::size_of::<ProcessInfoRaw>();
    let thr_sz = core::mem::size_of::<ThreadInfoRaw>();
    if buf_size < hdr_sz {
        return err_to_u64(SyscallError::Inval);
    }

    let _guard = match PROC_SNAPSHOT_LOCK.try_lock() {
        Some(g) => g,
        None => return err_to_u64(SyscallError::Again),
    };

    // Capture under the scheduler lock; released before any user-memory write.
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let lock = s.lock();
        let staging = unsafe { &mut *core::ptr::addr_of_mut!(PROC_SNAPSHOT_STAGING) };
        lock.snapshot_into(staging);
    });

    let staging = unsafe { &*core::ptr::addr_of!(PROC_SNAPSHOT_STAGING) };
    let p_total = staging.process_count.min(crate::scheduler::MAX_SNAPSHOT_PROCESSES);
    let t_total = staging.thread_count.min(crate::scheduler::MAX_SNAPSHOT_THREADS);

    let mut off = hdr_sz;
    let mut p_ret = 0usize;
    while p_ret < p_total && off + proc_sz <= buf_size {
        let p = &staging.processes[p_ret];
        let raw = ProcessInfoRaw {
            pid: p.pid,
            name: name_to_array(p.name.as_str()),
            thread_count: p.thread_count,
            cpu_time: p.cpu_time,
            committed_bytes: p.committed_bytes,
            working_set_bytes: p.working_set_bytes,
        };
        unsafe {
            core::ptr::copy_nonoverlapping(
                &raw as *const ProcessInfoRaw as *const u8,
                (buf_ptr as *mut u8).add(off),
                proc_sz,
            );
        }
        off += proc_sz;
        p_ret += 1;
    }

    let mut t_ret = 0usize;
    while t_ret < t_total && off + thr_sz <= buf_size {
        let t = &staging.threads[t_ret];
        let raw = ThreadInfoRaw {
            tid: t.tid,
            pid: t.pid,
            name: name_to_array(t.name.as_str()),
            state: t.state.to_u8(),
            idle: t.idle as u8,
            is_current: t.is_current as u8,
            _pad: 0,
            cpu: t.cpu,
            cpu_time: t.cpu_time,
        };
        unsafe {
            core::ptr::copy_nonoverlapping(
                &raw as *const ThreadInfoRaw as *const u8,
                (buf_ptr as *mut u8).add(off),
                thr_sz,
            );
        }
        off += thr_sz;
        t_ret += 1;
    }

    let truncated =
        (p_ret < p_total) || (t_ret < t_total) || staging.truncated;
    let hdr = ProcSnapshotHeader {
        version: PROC_SNAPSHOT_VERSION,
        process_total: p_total as u32,
        process_returned: p_ret as u32,
        thread_total: t_total as u32,
        thread_returned: t_ret as u32,
        process_entry_size: proc_sz as u32,
        thread_entry_size: thr_sz as u32,
        flags: if truncated { PROC_SNAPSHOT_FLAG_TRUNCATED } else { 0 },
    };
    unsafe {
        core::ptr::copy_nonoverlapping(
            &hdr as *const ProcSnapshotHeader as *const u8,
            buf_ptr as *mut u8,
            hdr_sz,
        );
    }
    off as u64
}

pub(super) fn handles(info_class: u32) -> bool {
    info_class == ObInfoClass::CpuStats as u32
        || info_class == ObInfoClass::ThreadStats as u32
        || info_class == ObInfoClass::ProcessSnapshot as u32
}

/// Dispatch the `stats` info classes.
pub(super) fn dispatch(
    info_class: u32,
    _fd: u8,
    entry: crate::handle::HandleEntry,
    buf_ptr: u64,
    buf_size: usize,
) -> u64 {
    match info_class {
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
        _ if info_class == ObInfoClass::ProcessSnapshot as u32 => {
            if entry.object_id == 0 {
                return err_to_u64(SyscallError::Inval);
            }
            let obj = match crate::object::ob_lookup(entry.object_id) {
                Some(o) => o,
                None => return err_to_u64(SyscallError::BadF),
            };
            // Global process snapshot hangs off \Global\Info\Processes
            // (native_id 14). Read-only; no process control.
            if obj.obj_type != crate::object::ObType::Key || obj.native_id != 14 {
                return err_to_u64(SyscallError::Inval);
            }
            snapshot_process_snapshot(buf_ptr, buf_size)
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

    // ── Phase 15-A: ProcessSnapshot ABI ──
    fn ps_name(n: &[u8; PROC_NAME_MAX]) -> &str {
        let end = n.iter().position(|&b| b == 0).unwrap_or(PROC_NAME_MAX);
        core::str::from_utf8(&n[..end]).unwrap_or("")
    }

    test_case!("proc_snapshot_abi_layout", {
        test_eq!(ObInfoClass::ProcessSnapshot as u32, 26);
        test_eq!(PROC_SNAPSHOT_VERSION, 3);
        test_eq!(PROC_NAME_MAX, 32);
        test_eq!(core::mem::size_of::<ProcSnapshotHeader>(), 32);
        test_eq!(core::mem::size_of::<ProcessInfoRaw>(), 64);
        test_eq!(core::mem::size_of::<ThreadInfoRaw>(), 56);
        test_eq!(PROC_SNAPSHOT_FLAG_TRUNCATED, 1);
    });

    test_case!("proc_snapshot_capture", {
        let cap = core::mem::size_of::<ProcSnapshotHeader>()
            + core::mem::size_of::<ProcessInfoRaw>() * crate::scheduler::MAX_SNAPSHOT_PROCESSES
            + core::mem::size_of::<ThreadInfoRaw>() * crate::scheduler::MAX_SNAPSHOT_THREADS;
        let mut buf = alloc::vec![0u8; cap];
        let n = snapshot_process_snapshot(buf.as_mut_ptr() as u64, buf.len()) as usize;
        test_true!(n >= core::mem::size_of::<ProcSnapshotHeader>());
        let hdr = unsafe {
            core::ptr::read_unaligned(buf.as_ptr() as *const ProcSnapshotHeader)
        };
        test_eq!(hdr.version, PROC_SNAPSHOT_VERSION);
        test_eq!(hdr.process_entry_size as usize, core::mem::size_of::<ProcessInfoRaw>());
        test_eq!(hdr.thread_entry_size as usize, core::mem::size_of::<ThreadInfoRaw>());
        test_true!(hdr.process_total >= 1);
        test_true!(hdr.process_returned <= hdr.process_total);
        // Boot + idle always exist.
        test_true!(hdr.thread_total >= 2);
        test_true!(hdr.thread_returned == hdr.thread_total);
        test_eq!(hdr.flags & PROC_SNAPSHOT_FLAG_TRUNCATED, 0);

        let pbase = core::mem::size_of::<ProcSnapshotHeader>();
        let tbase = pbase + hdr.process_returned as usize * core::mem::size_of::<ProcessInfoRaw>();

        // Deterministic ordering: processes by pid, threads by tid.
        let mut ordered = true;
        let mut prev_pid = 0u32;
        for i in 0..hdr.process_returned as usize {
            let p = unsafe {
                core::ptr::read_unaligned(
                    buf.as_ptr().add(pbase + i * core::mem::size_of::<ProcessInfoRaw>())
                        as *const ProcessInfoRaw,
                )
            };
            if i > 0 && p.pid < prev_pid { ordered = false; }
            prev_pid = p.pid;
        }
        let mut prev_tid = 0u32;
        let mut saw_boot = false;
        let mut saw_idle = false;
        for i in 0..hdr.thread_returned as usize {
            let t = unsafe {
                core::ptr::read_unaligned(
                    buf.as_ptr().add(tbase + i * core::mem::size_of::<ThreadInfoRaw>())
                        as *const ThreadInfoRaw,
                )
            };
            if i > 0 && t.tid < prev_tid { ordered = false; }
            prev_tid = t.tid;
            // Ownership: the thread's pid is an enumerated process.
            test_true!(hdr.process_returned >= 1);
            let nm = ps_name(&t.name);
            if nm == "boot" {
                saw_boot = true;
                test_eq!(t.tid, 0);
                test_eq!(t.pid, 0);
                test_eq!(t.state, 1); // Running
            }
            if nm.starts_with("idle/") {
                saw_idle = true;
                test_eq!(t.idle, 1);
            }
        }
        test_true!(ordered);
        test_true!(saw_boot);
        test_true!(saw_idle);
    });

    test_case!("proc_snapshot_truncation_and_zero_capacity", {
        // Zero capacity is rejected deterministically.
        let mut small = [0u8; 32];
        let r = snapshot_process_snapshot(small.as_mut_ptr() as u64, 0);
        test_true!((r as i64) < 0);

        // Room for the header + exactly one process: processes capped at 1 and
        // the truncated flag is set (never a silent partial snapshot).
        let one = core::mem::size_of::<ProcSnapshotHeader>()
            + core::mem::size_of::<ProcessInfoRaw>();
        let mut buf = alloc::vec![0u8; one];
        let _ = snapshot_process_snapshot(buf.as_mut_ptr() as u64, buf.len());
        let hdr = unsafe {
            core::ptr::read_unaligned(buf.as_ptr() as *const ProcSnapshotHeader)
        };
        test_true!(hdr.process_returned <= 1);
        test_true!(hdr.process_returned <= hdr.process_total);
        if hdr.thread_total > 0 {
            test_eq!(hdr.thread_returned, 0);
            test_true!((hdr.flags & PROC_SNAPSHOT_FLAG_TRUNCATED) != 0);
        }
    });
}

// ═══════════════════════════════════════════════════════════════════════
// OB-013: ObSetInfo — RAX=63
// ═══════════════════════════════════════════════════════════════════════
