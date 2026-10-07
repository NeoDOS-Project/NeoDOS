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
use libneodos::syscall::{
    self, CpuStatsEntry, ProcessInfoRaw, ProcSnapshotHeader, SmpStats, StatsHeader, ThreadInfoRaw,
    PROC_SNAPSHOT_VERSION, SMP_STATS_VERSION,
};
use libneodos::tr_id;

const APP_NAME: &str = "neotop";

// ── Locale IDs (keep in sync with data/locale/*/neotop.toml) ──
const IDS_USAGE: u32 = 1001;
const IDS_USAGE_LINE2: u32 = 1002;
const IDS_USAGE_LINE3: u32 = 1003;
const IDS_USAGE_LINE4: u32 = 1004;
const IDS_TITLE: u32 = 1005;
const IDS_COL_PID: u32 = 1006;
const IDS_THREADS: u32 = 1013;
const IDS_NA: u32 = 1023;
const IDS_TRUNCATED: u32 = 1024;
const IDS_ERR_STATS: u32 = 1025;
const IDS_COL_PROCESS: u32 = 1026;
const IDS_COL_THREAD: u32 = 1027;
const IDS_COL_STATE: u32 = 1028;
const IDS_COL_CPU: u32 = 1029;
const IDS_COL_CPUPCT: u32 = 1030;
const IDS_COL_CUR: u32 = 1031;
const IDS_COL_IDLE: u32 = 1032;
const IDS_HELP_QUIT: u32 = 1033;
const IDS_HELP_REFRESH: u32 = 1034;
const IDS_COL_WSS: u32 = 1035;
const IDS_COL_COMMIT: u32 = 1036;
const IDS_STEAL: u32 = 1037;

/// Must be >= the kernel's `MAX_SNAPSHOT_PROCESSES`.
const PROC_SNAP_MAX: usize = 64;
/// Must be >= the kernel's `MAX_SNAPSHOT_THREADS`.
const THREAD_SNAP_MAX: usize = 128;

const PROC_BUF_LEN: usize = size_of::<ProcSnapshotHeader>()
    + size_of::<ProcessInfoRaw>() * PROC_SNAP_MAX
    + size_of::<ThreadInfoRaw>() * THREAD_SNAP_MAX;

/// Room for the stats header plus one CPU entry. Entries are emitted in CPU
/// order, so a single-entry buffer yields CPU0 (the BSP) — the wall clock.
const CPU_BUF_LEN: usize = size_of::<StatsHeader>() + size_of::<CpuStatsEntry>();

/// Buffer for one `SmpStats` (global work-stealing counters).
const SMP_BUF_LEN: usize = size_of::<SmpStats>();

/// Output accumulator: sized for title + process/thread table (~128 rows).
const OUT_CAP: usize = 16384;
static mut OUT: [u8; OUT_CAP] = [0u8; OUT_CAP];
static mut OUT_LEN: usize = 0;

/// Approximate refresh interval, in timer ticks (1 tick = 1 ms). This only
/// paces the refresh; the CPU percentage uses the *measured* wall interval (see
/// `read_wall_tick`), never this constant.
const REFRESH_TICKS: u64 = 1000;

fn write_str(s: &[u8]) {
    unsafe {
        let len = OUT_LEN;
        let space = OUT_CAP - len;
        if space == 0 {
            return;
        }
        let n = s.len().min(space);
        core::ptr::copy_nonoverlapping(
            s.as_ptr(),
            core::ptr::addr_of_mut!(OUT[0]).add(len),
            n,
        );
        OUT_LEN = len + n;
    }
}

/// Single write of everything buffered so far.
fn flush_output() {
    unsafe {
        let len = OUT_LEN;
        if len > 0 {
            let buf = core::slice::from_raw_parts(core::ptr::addr_of!(OUT[0]), len);
            let _ = syscall::sys_write(1, buf);
            OUT_LEN = 0;
        }
    }
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

/// Write a `×10` percentage (e.g. `1374` → `"137.4"`), or the localized N/A.
fn write_percent_x10(pct: Option<u64>) {
    match pct {
        None => write_field_right(tr_id!(IDS_NA).as_bytes(), 8),
        Some(v) => {
            let whole = v / 10;
            let frac = v % 10;
            let mut buf = [0u8; 20];
            let wb = u64_bytes(whole, &mut buf);
            write_pad(logic::pad_width(wb.len() + 2, 8));
            write_str(wb);
            write_str(b".");
            let d = [b'0' + frac as u8];
            write_str(&d);
        }
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
    write_str(tr_id!(IDS_HELP_QUIT).as_bytes());
    write_str(b"\r\n");
    write_str(tr_id!(IDS_HELP_REFRESH).as_bytes());
    write_str(b"\r\n\r\n");
}

fn print_unavailable() {
    write_str(tr_id!(IDS_ERR_STATS).as_bytes());
    write_str(b"\r\n");
}

/// Copy a fixed 32-byte kernel name field into a `&str` (stops at NUL).
fn bytes_to_str(n: &[u8]) -> &str {
    let end = n.iter().position(|&b| b == 0).unwrap_or(n.len());
    core::str::from_utf8(&n[..end]).unwrap_or("")
}

struct SnapshotView<'a> {
    hdr: ProcSnapshotHeader,
    buf: &'a [u8],
    pbase: usize,
    tbase: usize,
}

impl<'a> SnapshotView<'a> {
    fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < size_of::<ProcSnapshotHeader>() {
            return None;
        }
        let hdr = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const ProcSnapshotHeader) };
        if !logic::proc_header_matches(
            hdr.version,
            hdr.process_entry_size,
            hdr.thread_entry_size,
            PROC_SNAPSHOT_VERSION,
            size_of::<ProcessInfoRaw>() as u32,
            size_of::<ThreadInfoRaw>() as u32,
        ) {
            return None;
        }
        let pbase = size_of::<ProcSnapshotHeader>();
        let tbase = pbase + hdr.process_returned as usize * size_of::<ProcessInfoRaw>();
        Some(SnapshotView { hdr, buf, pbase, tbase })
    }

    fn process(&self, index: usize) -> Option<ProcessInfoRaw> {
        if index >= self.hdr.process_returned as usize {
            return None;
        }
        let off = self.pbase + index * size_of::<ProcessInfoRaw>();
        if off + size_of::<ProcessInfoRaw>() > self.buf.len() {
            return None;
        }
        Some(unsafe {
            core::ptr::read_unaligned(self.buf.as_ptr().add(off) as *const ProcessInfoRaw)
        })
    }

    fn thread(&self, index: usize) -> Option<ThreadInfoRaw> {
        if index >= self.hdr.thread_returned as usize {
            return None;
        }
        let off = self.tbase + index * size_of::<ThreadInfoRaw>();
        if off + size_of::<ThreadInfoRaw>() > self.buf.len() {
            return None;
        }
        Some(unsafe {
            core::ptr::read_unaligned(self.buf.as_ptr().add(off) as *const ThreadInfoRaw)
        })
    }

    fn find_process_name(&self, pid: u32) -> [u8; syscall::PROC_NAME_MAX] {
        for i in 0..self.hdr.process_returned as usize {
            if let Some(p) = self.process(i) {
                if p.pid == pid {
                    return p.name;
                }
            }
        }
        [0u8; syscall::PROC_NAME_MAX]
    }

    fn find_process_cpu_time(&self, pid: u32) -> u64 {
        for i in 0..self.hdr.process_returned as usize {
            if let Some(p) = self.process(i) {
                if p.pid == pid {
                    return p.cpu_time;
                }
            }
        }
        0
    }

    /// MEM-PROC (#274): process committed bytes (heap + mmap).
    fn find_process_commit(&self, pid: u32) -> u64 {
        for i in 0..self.hdr.process_returned as usize {
            if let Some(p) = self.process(i) {
                if p.pid == pid {
                    return p.committed_bytes;
                }
            }
        }
        0
    }

    /// MEM-PROC (#274): process working-set bytes (resident heap pages).
    fn find_process_ws(&self, pid: u32) -> u64 {
        for i in 0..self.hdr.process_returned as usize {
            if let Some(p) = self.process(i) {
                if p.pid == pid {
                    return p.working_set_bytes;
                }
            }
        }
        0
    }
}

/// Render one frame. `history` holds the previous snapshot's per-process CPU
/// counters (empty on the first frame), and `wall_delta` is the measured
/// elapsed interval in timer units (0 on the first frame).
fn render_frame(view: &SnapshotView, history: &logic::CpuHistory, wall_delta: u64, clear: bool, smp: Option<SmpStats>) {
    if clear {
        // ANSI home + clear, mirroring `corecls` (no new console subsystem).
        write_str(b"\x1b[2J\x1b[H");
    }

    write_str(tr_id!(IDS_TITLE).as_bytes());
    write_str(b"\r\n\r\n");

    write_str(b"processes=");
    write_u32_field(view.hdr.process_total, 1);
    write_str(b"  ");
    write_str(tr_id!(IDS_THREADS).as_bytes());
    write_str(b"=");
    write_u32_field(view.hdr.thread_total, 1);
    if logic::proc_truncated(
        view.hdr.flags,
        view.hdr.process_returned,
        view.hdr.process_total,
        view.hdr.thread_returned,
        view.hdr.thread_total,
    ) {
        write_str(b"  ");
        write_str(tr_id!(IDS_TRUNCATED).as_bytes());
    }
    write_str(b"\r\n");
    if let Some(s) = smp {
        write_str(tr_id!(IDS_STEAL).as_bytes());
        write_str(b": ");
        write_u64_field(s.steal_success, 1);
        write_str(b"/");
        write_u64_field(s.steal_attempts, 1);
        write_str(b"\r\n");
    }
    write_str(b"\r\n");

    write_field_right(tr_id!(IDS_COL_PID).as_bytes(), 5);
    write_str(b"  ");
    write_field_left(tr_id!(IDS_COL_PROCESS).as_bytes(), 14);
    write_str(b"  ");
    write_field_right(b"TID", 5);
    write_str(b"  ");
    write_field_left(tr_id!(IDS_COL_THREAD).as_bytes(), 14);
    write_str(b"  ");
    write_field_left(tr_id!(IDS_COL_STATE).as_bytes(), 10);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_CPUPCT).as_bytes(), 8);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_CPU).as_bytes(), 3);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_WSS).as_bytes(), 6);
    write_str(b"  ");
    write_field_right(tr_id!(IDS_COL_COMMIT).as_bytes(), 6);
    write_str(b"  ");
    write_field_left(tr_id!(IDS_COL_CUR).as_bytes(), 1);
    write_str(b" ");
    write_field_left(tr_id!(IDS_COL_IDLE).as_bytes(), 1);
    write_str(b"\r\n");
    write_str(b"-----  --------------  -----  --------------  ----------  --------  ---  ------  ------  - -\r\n");

    for i in 0..view.hdr.thread_returned as usize {
        let t = match view.thread(i) {
            Some(t) => t,
            None => break,
        };
        let pname = view.find_process_name(t.pid);
        // Process CPU% (not per-thread): derived from the owning process's
        // aggregated monotonic CPU time over the measured wall interval.
        let pct = logic::cpu_percent_x10(
            history.prev(t.pid),
            view.find_process_cpu_time(t.pid),
            wall_delta,
        );
        write_u32_field(t.pid, 5);
        write_str(b"  ");
        write_field_left(bytes_to_str(&pname).as_bytes(), 14);
        write_str(b"  ");
        write_u32_field(t.tid, 5);
        write_str(b"  ");
        write_field_left(t.name_str().as_bytes(), 14);
        write_str(b"  ");
        write_field_left(logic::thread_state_str(t.state).as_bytes(), 10);
        write_str(b"  ");
        write_percent_x10(pct);
        write_str(b"  ");
        write_u32_field(t.cpu, 3);
        write_str(b"  ");
        {
            let mut b = [0u8; 8];
            write_str(logic::format_bytes(&mut b, view.find_process_ws(t.pid)));
        }
        write_str(b"  ");
        {
            let mut b = [0u8; 8];
            write_str(logic::format_bytes(&mut b, view.find_process_commit(t.pid)));
        }
        write_str(b"  ");
        write_str(if t.is_current_thread() { b"*" } else { b"-" });
        write_str(b" ");
        write_str(if t.is_idle() { b"*" } else { b"-" });
        write_str(b"\r\n");
    }

    write_str(b"\r\n");
    write_str(tr_id!(IDS_HELP_QUIT).as_bytes());
    write_str(b"   ");
    write_str(tr_id!(IDS_HELP_REFRESH).as_bytes());
    write_str(b"\r\n");

    flush_output();
}

/// Read the BSP's monotonic timer-tick counter, the wall-clock reference for
/// CPU%. It advances once per timer interval on CPU0 — the same unit as every
/// thread's `cpu_time` — so `Δcpu / Δwall` needs no unit conversion. Returns
/// `None` when the stats class is unavailable or reports an unexpected layout.
fn read_wall_tick(cpu_fd: u8, buf: &mut [u8]) -> Option<u64> {
    let n = syscall::sys_ob_query_cpu_stats(cpu_fd, buf).ok()?;
    if n < CPU_BUF_LEN {
        return None;
    }
    let hdr = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const StatsHeader) };
    if hdr.version != syscall::STATS_VERSION
        || hdr.entry_size as usize != size_of::<CpuStatsEntry>()
        || hdr.returned == 0
    {
        return None;
    }
    let e = unsafe {
        core::ptr::read_unaligned(
            buf.as_ptr().add(size_of::<StatsHeader>()) as *const CpuStatsEntry,
        )
    };
    Some(e.timer_tick_count)
}

/// Read the global work-stealing counters via the CpuInfo fd. `None` when the
/// class is unavailable or reports an unexpected layout (the caller then omits
/// the line).
fn read_smp_stats(cpu_fd: u8, buf: &mut [u8]) -> Option<SmpStats> {
    let n = syscall::sys_ob_query_smp_stats(cpu_fd, buf).ok()?;
    if n < size_of::<SmpStats>() {
        return None;
    }
    let s = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const SmpStats) };
    if s.version != SMP_STATS_VERSION {
        return None;
    }
    Some(s)
}

/// Cooperative refresh wait. Polls stdin (non-blocking: the kernel reports it
/// readable only when a byte is queued) and yields the CPU between polls, so a
/// waiting neotop never busy-spins. Returns early on a key, or once the real
/// wall clock has advanced by `REFRESH_TICKS` since `frame_wall`. If no clock is
/// available it falls back to a bounded yield count rather than spinning forever.
fn wait_for_input_or_refresh(
    cpu_fd: Option<u8>,
    cpu_buf: &mut [u8],
    frame_wall: Option<u64>,
) -> WaitResult {
    let mut spins: u64 = 0;
    loop {
        if let Some(k) = poll_key() {
            return match k {
                b'q' | b'Q' => WaitResult::Quit,
                _ => WaitResult::Refresh,
            };
        }
        // Give other threads the CPU. The timer preempts us, so yield is a real
        // wait, not a busy loop.
        syscall::sys_yield();
        spins = spins.wrapping_add(1);
        // Probe the clock every few yields to keep syscall overhead low.
        if spins % 8 != 0 {
            continue;
        }
        match (frame_wall, cpu_fd.and_then(|fd| read_wall_tick(fd, cpu_buf))) {
            (Some(start), Some(now)) => {
                if now.saturating_sub(start) >= REFRESH_TICKS {
                    return WaitResult::Refresh;
                }
            }
            // No usable clock: bounded fallback.
            _ if spins >= REFRESH_TICKS.saturating_mul(8) => return WaitResult::Refresh,
            _ => {}
        }
    }
}

enum WaitResult {
    Quit,
    Refresh,
}

/// Non-blocking key probe: only reads when `sys_poll` reports a queued byte, so
/// the thread is never parked by an empty read.
fn poll_key() -> Option<u8> {
    let mut fds = [syscall::PollFd { fd: 0, events: syscall::POLLIN, revents: 0 }];
    match syscall::sys_poll(&mut fds, 0) {
        Ok(1) if fds[0].revents & syscall::POLLIN != 0 => {
            let mut key = [0u8; 1];
            match syscall::sys_read(0, &mut key) {
                Ok(1) => Some(key[0]),
                _ => None,
            }
        }
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    i18n::i18n_init();
    let _ = i18n::i18n_load(APP_NAME);
    let raw = libneodos::args::read_args();
    if libneodos::args::is_help_flag(&raw) {
        print_help();
        flush_output();
        syscall::sys_exit(0);
    }

    let Some(fd) = syscall::ob_open_processes().ok() else {
        print_unavailable();
        flush_output();
        syscall::sys_exit(1);
    };
    let cpu_fd = syscall::ob_open_cpu_info().ok();

    let mut cpu_buf = [0u8; CPU_BUF_LEN];
    let mut smp_buf = [0u8; SMP_BUF_LEN];
    let mut proc_buf = [0u8; PROC_BUF_LEN];
    let mut history = logic::CpuHistory::new();

    // First sample: no predecessor, so CPU% is shown as N/A.
    let mut prev_wall: Option<u64> = None;
    let mut frames: u64 = 0;

    loop {
        // Wall-clock reference for this frame, in timer intervals.
        let frame_wall = cpu_fd.and_then(|cfd| read_wall_tick(cfd, &mut cpu_buf));
        let smp = cpu_fd.and_then(|cfd| read_smp_stats(cfd, &mut smp_buf));
        let wall_delta = match (prev_wall, frame_wall) {
            (Some(p), Some(c)) if c >= p => c - p,
            _ => 0,
        };

        let mut cpu_times = [(0u32, 0u64); logic::MAX_HISTORY];
        let mut n_cpu = 0usize;

        match syscall::sys_ob_query_process_snapshot(fd, &mut proc_buf) {
            Ok(n) if n >= size_of::<ProcSnapshotHeader>() => {
                if let Some(view) = SnapshotView::parse(&proc_buf[..n]) {
                    for i in 0..view.hdr.process_returned as usize {
                        if n_cpu >= logic::MAX_HISTORY {
                            break;
                        }
                        if let Some(p) = view.process(i) {
                            cpu_times[n_cpu] = (p.pid, p.cpu_time);
                            n_cpu += 1;
                        }
                    }
                    render_frame(&view, &history, wall_delta, frames > 0, smp);
                } else {
                    clear_and_unavailable(frames > 0);
                }
            }
            _ => {
                clear_and_unavailable(frames > 0);
            }
        }

        // Store this sample for the next frame, then arm the predecessor.
        history.store(&cpu_times[..n_cpu]);
        prev_wall = frame_wall;
        frames += 1;

        // Wait for a key or the measured refresh interval, yielding the CPU.
        match wait_for_input_or_refresh(cpu_fd, &mut cpu_buf, frame_wall) {
            WaitResult::Quit => break,
            WaitResult::Refresh => {}
        }
    }

    let _ = syscall::sys_close(fd);
    if let Some(cfd) = cpu_fd {
        let _ = syscall::sys_close(cfd);
    }

    // Clean exit: leave the screen ready for the shell prompt.
    write_str(b"\x1b[2J\x1b[H");
    flush_output();
    syscall::sys_exit(0)
}

fn clear_and_unavailable(clear: bool) {
    if clear {
        write_str(b"\x1b[2J\x1b[H");
    }
    print_unavailable();
    flush_output();
}
