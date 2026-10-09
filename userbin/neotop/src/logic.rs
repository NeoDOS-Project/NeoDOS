//! Pure, dependency-free helpers for neotop.
//!
//! No syscalls and no ABI access, so these can be unit-tested on the host with
//! `rustc --test src/logic.rs` (the neotop binary itself is no_std and its
//! Cargo test target is disabled).
//!
//! ## CPU utilization model (Phase 15-A.1)
//!
//! CPU time is an authoritative **monotonic** kernel counter (timer intervals).
//! `neotop` observes it twice and divides the process delta by the *actual*
//! elapsed wall time:
//!
//! ```text
//! CPU% = Δ(cpu_time) / Δ(wall_time) × 100
//! ```
//!
//! A process with several threads on several CPUs may exceed 100% (up to
//! N×100% on N CPUs); the value is **never clamped**. The first observation has
//! no predecessor, so no percentage is fabricated — it is reported as N/A.

/// CPU percentage with one decimal place, scaled by 10 (e.g. `1374` == 137.4%).
///
/// Returns `None` when a percentage is not meaningful:
/// - no previous sample (`prev_cpu_time == None`),
/// - a non-positive wall delta (`wall_delta_units == 0`),
/// - a counter that appears to move backwards (`cur < prev`, an accounting
///   inconsistency — never displayed as a negative percentage).
pub fn cpu_percent_x10(
    prev_cpu_time: Option<u64>,
    cur_cpu_time: u64,
    wall_delta_units: u64,
) -> Option<u64> {
    let prev = prev_cpu_time?;
    if wall_delta_units == 0 {
        return None;
    }
    if cur_cpu_time < prev {
        // Monotonicity violation: report "unknown", not a negative value.
        return None;
    }
    let delta = cur_cpu_time - prev;
    // ×1000 to keep one decimal: (delta / wall) * 100 * 10
    Some(delta.saturating_mul(1000) / wall_delta_units)
}

/// Validate a `ProcSnapshotHeader` against the ABI this tool was built with.
/// Guards against kernel/libneodos layout drift instead of misparsing records.
pub fn proc_header_matches(
    version: u32,
    process_entry_size: u32,
    thread_entry_size: u32,
    expected_version: u32,
    expected_process_entry_size: u32,
    expected_thread_entry_size: u32,
) -> bool {
    version == expected_version
        && process_entry_size == expected_process_entry_size
        && thread_entry_size == expected_thread_entry_size
}

/// True when the process/thread snapshot is truncated: the explicit flag is set,
/// or either section returned fewer records than were available.
pub fn proc_truncated(
    flags: u32,
    process_returned: u32,
    process_total: u32,
    thread_returned: u32,
    thread_total: u32,
) -> bool {
    flags & 1 != 0 || process_returned < process_total || thread_returned < thread_total
}

/// Number of spaces required to pad a field of `len` bytes up to `width`.
///
/// Saturating by construction: a value wider than its column never underflows.
pub fn pad_width(len: usize, width: usize) -> usize {
    width.saturating_sub(len)
}

/// MEM-PROC (#274): render a byte count as a short human-readable string with a
/// fixed-ish width, using binary units. Zero renders as `0K`, `< 1 KiB` as
/// `0.5K`, and values are one decimal in KiB/MiB/GiB. Writes into `out` and
/// returns the slice actually used (no allocation, no formatting machinery).
///
/// `out` must be at least 8 bytes. The result is deterministic so the column
/// width stays stable across frames.
pub fn format_bytes(out: &mut [u8], bytes: u64) -> &[u8] {
    // Unit scaling and decimal formatting come from the shared service in
    // `math.nxl` (`libmath`); column padding stays here (UI concern).
    let mut tmp = [0u8; 12];
    let n = libmath::format_size_compact(bytes, &mut tmp);
    // Right-align "whole.frac<unit>" into a fixed inner width of 6.
    let inner = 6usize;
    let mut total = 0usize;
    let pad = inner.saturating_sub(n);
    for _ in 0..pad {
        if total < out.len() { out[total] = b' '; total += 1; }
    }
    for i in 0..n {
        if total < out.len() { out[total] = tmp[i]; total += 1; }
    }
    &out[..total]
}

/// Map `Kthread::state` (`ThreadState::to_u8`) to a display string.
pub fn thread_state_str(state: u8) -> &'static str {
    match state {
        0 => "Ready",
        1 => "Running",
        2 => "Blocked",
        3 => "Suspended",
        4 => "Terminated",
        _ => "?",
    }
}

/// Incremental CPU-time history, keyed by PID, used to compute CPU% from two
/// consecutive snapshots. Bounded (fixed capacity) so it never allocates.
pub struct CpuHistory {
    entries: [(u32, u64); MAX_HISTORY],
    len: usize,
}

pub const MAX_HISTORY: usize = 64;

impl CpuHistory {
    pub const fn new() -> Self {
        CpuHistory { entries: [(0, 0); MAX_HISTORY], len: 0 }
    }

    /// Record a snapshot's per-process CPU counters.
    pub fn store(&mut self, procs: &[(u32, u64)]) {
        self.len = 0;
        for &(pid, cpu) in procs {
            if self.len >= MAX_HISTORY { break; }
            self.entries[self.len] = (pid, cpu);
            self.len += 1;
        }
    }

    /// Previous CPU counter for `pid` in the stored snapshot, if present.
    pub fn prev(&self, pid: u32) -> Option<u64> {
        self.entries[..self.len]
            .iter()
            .find(|&&(p, _)| p == pid)
            .map(|&(_, c)| c)
    }
}

impl Default for CpuHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Required deterministic CPU% tests (Phase 15-A.1 §24) ──

    #[test]
    fn one_cpu_full_utilization() {
        // prev=0, cur=1s, wall=1s → 100.0%
        assert_eq!(cpu_percent_x10(Some(0), 1_000_000, 1_000_000), Some(1000));
    }

    #[test]
    fn half_utilization() {
        // 500ms / 1s → 50.0%
        assert_eq!(cpu_percent_x10(Some(0), 500_000, 1_000_000), Some(500));
    }

    #[test]
    fn two_cpus_full_utilization() {
        // 2s / 1s → 200.0% (not clamped)
        assert_eq!(cpu_percent_x10(Some(0), 2_000_000, 1_000_000), Some(2000));
    }

    #[test]
    fn first_sample_is_neutral() {
        // No previous snapshot → no percentage.
        assert_eq!(cpu_percent_x10(None, 1_000_000, 1_000_000), None);
    }

    #[test]
    fn process_appears_has_no_history() {
        let mut hist = CpuHistory::new();
        hist.store(&[(1, 100), (2, 200)]);
        // PID 3 was not in the previous snapshot.
        assert_eq!(hist.prev(3), None);
        assert_eq!(cpu_percent_x10(hist.prev(3), 50, 1_000_000), None);
    }

    #[test]
    fn process_disappears_has_no_percentage() {
        // A PID present in snapshot 0 but absent in snapshot 1 simply is not
        // rendered; its history entry exists but is never looked up again.
        let mut hist = CpuHistory::new();
        hist.store(&[(7, 400)]);
        assert_eq!(hist.prev(7), Some(400));
        // Next snapshot lacks PID 7 → caller never computes anything for it.
    }

    #[test]
    fn monotonicity_violation_not_negative() {
        // cur < prev must never yield a negative percentage.
        assert_eq!(cpu_percent_x10(Some(1000), 900, 1_000_000), None);
    }

    #[test]
    fn multiple_threads_aggregate_to_100() {
        // Process CPU time is the sum over threads: 300ms + 700ms over 1s.
        let prev_process: u64 = 0;
        let cur_process: u64 = 300_000 + 700_000;
        assert_eq!(
            cpu_percent_x10(Some(prev_process), cur_process, 1_000_000),
            Some(1000)
        );
    }

    #[test]
    fn multiple_cpus_and_threads_aggregate_to_200() {
        // thread A = 1s, thread B = 1s, wall = 1s → 200.0%
        let cur_process: u64 = 1_000_000 + 1_000_000;
        assert_eq!(cpu_percent_x10(Some(0), cur_process, 1_000_000), Some(2000));
    }

    // ── Edge cases ──

    #[test]
    fn zero_wall_delta_is_neutral() {
        assert_eq!(cpu_percent_x10(Some(0), 1_000_000, 0), None);
    }

    #[test]
    fn smooth_small_values() {
        // 0.8% of a second.
        assert_eq!(cpu_percent_x10(Some(0), 8_000, 1_000_000), Some(8));
        // 2.4%
        assert_eq!(cpu_percent_x10(Some(0), 24_000, 1_000_000), Some(24));
    }

    #[test]
    fn idle_is_zero() {
        assert_eq!(cpu_percent_x10(Some(500), 500, 1_000_000), Some(0));
    }

    // ── History ──

    #[test]
    fn history_bounded_and_lookup() {
        let mut hist = CpuHistory::new();
        hist.store(&[(1, 10), (2, 20)]);
        assert_eq!(hist.prev(1), Some(10));
        assert_eq!(hist.prev(2), Some(20));
        assert_eq!(hist.prev(99), None);
        hist.store(&[(5, 50)]);
        assert_eq!(hist.prev(1), None);
        assert_eq!(hist.prev(5), Some(50));
    }

    // ── Existing helpers ──

    #[test]
    fn pad_width_never_underflows() {
        assert_eq!(pad_width(0, 4), 4);
        assert_eq!(pad_width(2, 4), 2);
        assert_eq!(pad_width(4, 4), 0);
        assert_eq!(pad_width(6, 4), 0);
        assert_eq!(pad_width(12, 4), 0);
        assert_eq!(pad_width(usize::MAX, 3), 0);
    }

    #[test]
    fn thread_states_cover_kernel_encoding() {
        assert_eq!(thread_state_str(0), "Ready");
        assert_eq!(thread_state_str(1), "Running");
        assert_eq!(thread_state_str(2), "Blocked");
        assert_eq!(thread_state_str(3), "Suspended");
        assert_eq!(thread_state_str(4), "Terminated");
        assert_eq!(thread_state_str(255), "?");
    }

    #[test]
    fn proc_header_validation() {
        assert!(proc_header_matches(3, 64, 56, 3, 64, 56));
        assert!(!proc_header_matches(2, 64, 56, 3, 64, 56));
        assert!(!proc_header_matches(1, 64, 56, 3, 64, 56));
        assert!(!proc_header_matches(3, 48, 56, 3, 64, 56));
        assert!(!proc_header_matches(3, 64, 48, 3, 64, 56));
    }

    #[test]
    fn bytes_formatting() {
        let mut b = [0u8; 8];
        assert_eq!(format_bytes(&mut b, 0), b"  0.0K");
        assert_eq!(format_bytes(&mut b, 1536), b"  1.5K");
        assert_eq!(format_bytes(&mut b, 1024 * 1024), b"  1.0M");
        assert_eq!(format_bytes(&mut b, 3 * 1024 * 1024 + 512 * 1024), b"  3.5M");
        assert_eq!(format_bytes(&mut b, 2 * 1024 * 1024 * 1024), b"  2.0G");
        // sub-KiB non-zero must not round up to a whole KiB
        assert_eq!(format_bytes(&mut b, 512), b"  0.5K");
    }

    #[test]
    fn proc_truncation_detection() {
        assert!(!proc_truncated(0, 2, 2, 6, 6));
        assert!(proc_truncated(1, 2, 2, 6, 6));
        assert!(proc_truncated(0, 1, 2, 6, 6));
        assert!(proc_truncated(0, 2, 2, 5, 6));
    }
}
