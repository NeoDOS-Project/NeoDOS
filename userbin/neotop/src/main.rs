#![no_std]
#![no_main]
#![cfg_attr(test, feature(custom_test_frameworks))]
#![cfg_attr(test, test_runner(noop_test_runner))]
#![cfg_attr(test, reexport_test_harness_main = "test_main")]

#[cfg(test)]
fn noop_test_runner(_tests: &[&dyn Fn()]) {
    loop {}
}

mod logic;

use core::mem::size_of;
use libneodos::i18n;
use libneodos::syscall::{self, CpuStatsEntry, StatsHeader, ThreadStatsEntry};
use libneodos::tr_id;

const APP_NAME: &str = "neotop";

// ── Locale IDs (keep in sync with data/locale/*/neotop.toml) ──
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_USAGE_LINE3: u32 = 1003;
const IDS_USAGE_LINE4: u32 = 1004;
const IDS_TITLE: u32 = 1005;
const IDS_COL_PID: u32 = 1006;
const IDS_COL_STATE: u32 = 1008;
const IDS_COL_PRI: u32 = 1009;
const IDS_CPUS: u32 = 1012;
const IDS_THREADS: u32 = 1013;
const IDS_COL_CPU: u32 = 1014;
const IDS_COL_APIC: u32 = 1015;
const IDS_COL_ONLINE: u32 = 1016;
const IDS_COL_TICKS: u32 = 1017;
const IDS_COL_CTXSW: u32 = 1018;
const IDS_COL_IRQ: u32 = 1019;
const IDS_COL_TID: u32 = 1020;
const IDS_YES: u32 = 1021;
const IDS_NO: u32 = 1022;
const IDS_NA: u32 = 1023;
const IDS_TRUNCATED: u32 = 1024;
const IDS_ERR_STATS: u32 = 1025;

/// Must be >= the kernel's `cpu_local::MAX_CPUS`.
const CPU_MAX: usize = 16;
/// Must be >= the kernel's `THREAD_STATS_MAX`.
const THREAD_MAX: usize = 128;

const CPU_BUF_LEN: usize = size_of::<StatsHeader>() + size_of::<CpuStatsEntry>() * CPU_MAX;
const THREAD_BUF_LEN: usize = size_of::<StatsHeader>() + size_of::<ThreadStatsEntry>() * THREAD_MAX;

const EMPTY_THREAD: ThreadStatsEntry = ThreadStatsEntry {
    tid: 0,
    pid: 0,
    cpu_id: 0,
    priority: 0,
    state: 0,
    _pad: [0; 2],
    cpu_ticks: 0,
};

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_pad(n: usize) {
    for _ in 0..n {
        write_str(b" ");
    }
}

fn write_field_left(s: &[u8], width: usize) {
    write_str(s);
    write_pad(logic::pad_width(s.len(), width));
}

fn write_field_right(s: &[u8], width: usize) {
    write_pad(logic::pad_width(s.len(), width));
    write_str(s);
}

fn u64_bytes(mut v: u64, buf: &mut [u8; 20]) -> &[u8] {
    if v == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let mut i = 20;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    &buf[i..]
}

fn write_u32_field(v: u32, width: usize) {
    let mut buf = [0u8; 20];
    let bytes = u64_bytes(v as u64, &mut buf);
    write_field_right(bytes, width);
}

fn write_u64_field(v: u64, width: usize) {
    let mut buf = [0u8; 20];
    let bytes = u64_bytes(v, &mut buf);
    write_field_right(bytes, width);
}

fn read_header(buf: &[u8]) -> Option<StatsHeader> {
    if buf.len() < size_of::<StatsHeader>() {
        return None;
    }
    Some(unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const StatsHeader) })
}

/// Validate the snapshot header against the struct this binary was built with.
/// Guards against a kernel/libneodos layout drift instead of misparsing.
fn header_matches<T>(hdr: &StatsHeader) -> bool {
    hdr.version == syscall::STATS_VERSION && hdr.entry_size as usize == size_of::<T>()
}

fn read_entry<T: Copy>(buf: &[u8], index: usize) -> Option<T> {
    let off = size_of::<StatsHeader>() + index * size_of::<T>();
    if off + size_of::<T>() <= buf.len() {
        Some(unsafe { core::ptr::read_unaligned(buf.as_ptr().add(off) as *const T) })
    } else {
        None
    }
}

fn print_help() {
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE2).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE3).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_USAGE_LINE4).as_bytes());
    write_str(b"\r\n\r\n");
}

fn print_unavailable() {
    write_str(tr_id!(IDS_ERR_STATS).as_bytes());
    write_str(b"\r\n");
}

/// Render the `CpuStats` section. Only fields the kernel actually exposes are
/// printed; no CPU% is computed (see docs/kernel/objects.md).
fn render_cpus(buf: &[u8]) {
    let hdr = match read_header(buf) {
        Some(h) if header_matches::<CpuStatsEntry>(&h) => h,
        _ => return print_unavailable(),
    };

    write_str(tr_id!(IDS_CPUS).as_bytes());
    write_str(b": ");
    write_u32_field(hdr.total, 1);
    write_str(b"\r\n");

    write_field_right(tr_id!(IDS_COL_CPU).as_bytes(), 3);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_APIC).as_bytes(), 4);
    write_str(b"  ");
    write_field_left(tr_id!(IDS_COL_ONLINE).as_bytes(), 6);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_TICKS).as_bytes(), 12);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_CTXSW).as_bytes(), 11);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_IRQ).as_bytes(), 11);
    write_str(b"\r\n");
    write_str(b"---  ----  ------  ------------  -----------  -----------\r\n");

    for i in 0..hdr.returned as usize {
        if let Some(e) = read_entry::<CpuStatsEntry>(buf, i) {
            write_u32_field(e.cpu_id, 3);
            write_str(b"  ");
            write_u32_field(e.apic_id, 4);
            write_str(b"  ");
            write_field_left(
                if e.is_online() {
                    tr_id!(IDS_YES).as_bytes()
                } else {
                    tr_id!(IDS_NO).as_bytes()
                },
                6,
            );
            write_str(b"  ");
            write_u64_field(e.timer_tick_count, 12);
            write_str(b"  ");
            write_u64_field(e.context_switch_count, 11);
            write_str(b"  ");
            write_u64_field(e.interrupt_count, 11);
            write_str(b"\r\n");
        }
    }

    if logic::is_truncated(hdr.returned, hdr.total) {
        write_str(tr_id!(IDS_TRUNCATED).as_bytes());
        write_str(b"\r\n");
    }
}

/// Render the `ThreadStats` section, grouped by (PID, TID) so a process's
/// threads are adjacent. Thread→CPU comes straight from `Kthread.cpu`.
fn render_threads(buf: &[u8]) {
    let hdr = match read_header(buf) {
        Some(h) if header_matches::<ThreadStatsEntry>(&h) => h,
        _ => return print_unavailable(),
    };

    write_str(tr_id!(IDS_THREADS).as_bytes());
    write_str(b": ");
    write_u32_field(hdr.total, 1);
    write_str(b"\r\n");

    write_field_right(tr_id!(IDS_COL_TID).as_bytes(), 5);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_PID).as_bytes(), 5);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_CPU).as_bytes(), 4);
    write_str(b"  ");
    write_field_left(tr_id!(IDS_COL_STATE).as_bytes(), 10);
    write_str(b"  ");
    write_field_left(tr_id!(IDS_COL_PRI).as_bytes(), 12);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_TICKS).as_bytes(), 12);
    write_str(b"\r\n");
    write_str(b"-----  -----  ----  ----------  ------------  ------------\r\n");

    // Copy into a local array so the rows can be sorted by (PID, TID).
    let mut rows = [EMPTY_THREAD; THREAD_MAX];
    let n = core::cmp::min(hdr.returned as usize, THREAD_MAX);
    for (i, row) in rows.iter_mut().enumerate().take(n) {
        if let Some(e) = read_entry::<ThreadStatsEntry>(buf, i) {
            *row = e;
        }
    }
    for i in 1..n {
        let key = rows[i];
        let mut j = i;
        while j > 0 && (rows[j - 1].pid, rows[j - 1].tid) > (key.pid, key.tid) {
            rows[j] = rows[j - 1];
            j -= 1;
        }
        rows[j] = key;
    }

    for row in rows.iter().take(n) {
        write_u32_field(row.tid, 5);
        write_str(b"  ");
        write_u32_field(row.pid, 5);
        write_str(b"  ");
        write_u32_field(row.cpu_id, 4);
        write_str(b"  ");
        write_field_left(logic::thread_state_str(row.state).as_bytes(), 10);
        write_str(b"  ");
        write_field_left(row.priority_str().as_bytes(), 12);
        write_str(b"  ");
        write_u64_field(row.cpu_ticks, 12);
        write_str(b"\r\n");
    }

    if logic::is_truncated(hdr.returned, hdr.total) {
        write_str(tr_id!(IDS_TRUNCATED).as_bytes());
        write_str(b"\r\n");
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        syscall::sys_exit(0);
    }

    write_str(b"\r\n");
    write_str(tr_id!(IDS_TITLE).as_bytes());
    write_str(b"\r\n\r\n");

    let mut cpu_buf = [0u8; CPU_BUF_LEN];
    let mut thread_buf = [0u8; THREAD_BUF_LEN];

    match syscall::ob_open_cpu_info() {
        Ok(fd) => {
            match syscall::sys_ob_query_cpu_stats(fd, &mut cpu_buf) {
                Ok(n) if n >= size_of::<StatsHeader>() => render_cpus(&cpu_buf[..n]),
                _ => print_unavailable(),
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => print_unavailable(),
    }
    write_str(b"\r\n");

    match syscall::ob_open_threads() {
        Ok(fd) => {
            match syscall::sys_ob_query_thread_stats(fd, &mut thread_buf) {
                Ok(n) if n >= size_of::<StatsHeader>() => render_threads(&thread_buf[..n]),
                _ => print_unavailable(),
            }
            let _ = syscall::sys_close(fd);
        }
        Err(_) => print_unavailable(),
    }

    write_str(b"\r\n");
    syscall::sys_exit(0)
}
