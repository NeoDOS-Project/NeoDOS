//! Phase 15-A.1: authoritative per-thread CPU execution accounting.
//!
//! This module adds a **monotonic CPU execution counter** to every schedulable
//! thread and refreshes it at the two execution boundaries the scheduler already
//! owns: timer preemption (`Scheduler::on_timer_tick`) and dispatch/switch-in
//! (`Scheduler::schedule_with`).
//!
//! Semantics
//! ---------
//! - One accounting unit == one timer tick interval. Each CPU runs its own
//!   periodic local-APIC timer at the same calibrated rate, so a thread that
//!   executes for one interval on any CPU gains exactly one unit. `neotop`
//!   converts units to time using the wall-clock delta of the same interval,
//!   which keeps the percentage independent of the unit's absolute size.
//! - `Kthread.cpu_time` only ever increases. Migration between CPUs adds the
//!   execution performed on each CPU; nothing is reset or attributed twice.
//! - Idle threads never accumulate (`is_idle` is excluded), so idle CPU is not
//!   charged to the PID-0 process.
//! - On SMP the counter is maintained per thread and *summed per process* at
//!   snapshot time, so a process on N CPUs may legitimately read close to N×100%.
//!
//! Locking / performance
//! ---------------------
//! The accounting refresh runs with the scheduler lock already held at both
//! boundaries (the caller's lock, no new lock). Work is O(1): it uses the
//! per-CPU resident counter (`KPRCB.timer_tick_count`) as the free-running time
//! base and performs one addition on the current thread — no global scan, no
//! allocation, no string work, and no extra `RDTSC` in the hot path.

use crate::scheduler::types::Kthread;

/// Sentinel meaning "this thread is not currently accumulating".
pub const CPU_TIME_UNSET: u64 = u64::MAX;

/// This CPU's execution counter value, or `None` before per-CPU data exists
/// (early boot / host-side unit tests).
///
/// The former `GsBase::read()` guard performed an MSR read on every timer tick
/// and every dispatch; a single atomic load is enough to know that per-CPU data
/// is online (the BSP marks CPU 0 online immediately after programming GS, and
/// APs set GS before they register their idle thread).
#[inline]
pub(crate) fn per_cpu_tick_base() -> Option<u64> {
    if crate::arch::x64::cpu_local::cpu_count() == 0 {
        return None;
    }
    Some(unsafe { crate::arch::x64::cpu_local::this_cpu_timer_tick_count() })
}

/// Another CPU's execution counter, read through its `KPRCB` page.
///
/// A thread's `cpu_time_base` is captured on the CPU that dispatches it, and
/// each CPU's counter has its own origin, so the in-flight interval must be
/// folded against the *owner's* counter, never the snapshotting CPU's. The
/// owning CPU is the only writer and aligned `u64` accesses are atomic on
/// x86_64, so this is a valid monotonic sample. Returns `None` when that CPU
/// has no `KPRCB` yet.
#[inline]
pub(crate) fn cpu_tick_base(cpu: u32) -> Option<u64> {
    let page = crate::arch::x64::cpu_local::kprcb_page(cpu as usize)?;
    let addr = page + crate::arch::x64::cpu_local::OFFSET_TIMER_TICK_COUNT as u64;
    Some(unsafe { core::ptr::read_volatile(addr as *const u64) })
}

/// Fold a thread's in-flight interval into its stored counter.
///
/// `stored` is the last refreshed total, `base_at_last` the counter value
/// captured when it was refreshed and `base_now` the owner's current counter.
/// Saturating so a stale/foreign base can never make the counter go backwards.
#[inline]
fn fold_in_flight(stored: u64, base_at_last: u64, base_now: u64) -> u64 {
    stored.saturating_add(base_now.saturating_sub(base_at_last))
}

/// Mark `k` as accumulating execution starting now (called at dispatch).
///
/// Arming only records the starting counter; it must **not** credit any time,
/// otherwise the absolute per-CPU counter would be folded into `cpu_time` at
/// every dispatch and inflate the total.
#[inline]
pub fn mark_dispatch(k: &mut Kthread) {
    k.cpu_time_base = match per_cpu_tick_base() {
        Some(b) => b,
        None => Kthread::CPU_TIME_UNSET,
    };
}

/// Refresh `k`'s accumulated execution and re-arm the base for the next
/// interval (called at each timer tick while `k` is the current thread).
#[inline]
pub fn account_tick(k: &mut Kthread) {
    let base = match per_cpu_tick_base() {
        Some(b) => b,
        None => return,
    };
    if k.cpu_time_base == Kthread::CPU_TIME_UNSET {
        // Thread was dispatched before per-CPU data was available.
        k.cpu_time_base = base;
        return;
    }
    let elapsed = base.saturating_sub(k.cpu_time_base);
    if elapsed == 0 {
        return;
    }
    k.cpu_time = k.cpu_time.saturating_add(elapsed);
    k.cpu_time_base = base;
}

/// Resolve `k`'s counter as of "now", folding in the in-flight interval without
/// mutating the thread. Used by the snapshot path (which is read-only).
///
/// `owner` must be the CPU whose `KPRCB.current_thread` is `k` (from
/// `kthread_current_cpu`), or `None` when the thread is not running. A thread
/// that has been switched out has a stale `cpu_time_base`; folding it would add
/// time the thread did not execute, so a non-running thread returns its stored
/// value. When running, the fold uses the *owner's* counter: a thread's base was
/// captured on that same CPU, while another CPU's counter has a different
/// origin.
#[inline]
pub fn cpu_time_now(k: &Kthread, owner: Option<u32>) -> u64 {
    let cpu = match owner {
        Some(c) => c,
        None => return k.cpu_time,
    };
    if k.cpu_time_base == Kthread::CPU_TIME_UNSET {
        return k.cpu_time;
    }
    match cpu_tick_base(cpu) {
        Some(b) => fold_in_flight(k.cpu_time, k.cpu_time_base, b),
        None => k.cpu_time,
    }
}

/// Register the accounting unit tests with the kernel test framework
/// (`testing::register_tests` / `scheduler::tests::register_tests`).
pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    /// Pure accounting arithmetic, independent of the scheduler/timer.
    /// Mirrors `account_tick` with an explicit time base so it is deterministic.
    fn account(k: &mut Kthread, base: u64) {
        if k.cpu_time_base == CPU_TIME_UNSET {
            k.cpu_time_base = base;
            return;
        }
        let elapsed = base.saturating_sub(k.cpu_time_base);
        if elapsed == 0 {
            return;
        }
        k.cpu_time = k.cpu_time.saturating_add(elapsed);
        k.cpu_time_base = base;
    }

    test_case!("accounting_is_monotonic_and_interval_sized", {
        let mut k = Kthread::new_idle(9, 1, 0, 0x1000);
        k.cpu_time = 0;
        k.cpu_time_base = CPU_TIME_UNSET;
        // First tick only re-arms.
        account(&mut k, 100);
        test_eq!(k.cpu_time, 0);
        test_eq!(k.cpu_time_base, 100);
        // Each subsequent tick adds the elapsed interval.
        account(&mut k, 101);
        test_eq!(k.cpu_time, 1);
        account(&mut k, 105);
        test_eq!(k.cpu_time, 5);
        test_true!(k.cpu_time >= 5); // counter never moves backwards
    });

    test_case!("accounting_sums_across_migration", {
        // CPU0: 3 units, then migrate to CPU1 (different base) for 2 units.
        let mut k = Kthread::new_idle(9, 1, 0, 0x1000);
        k.cpu_time = 0;
        k.cpu_time_base = CPU_TIME_UNSET;
        account(&mut k, 1000);
        account(&mut k, 1003);
        test_eq!(k.cpu_time, 3); // CPU0 execution
        // Migration: mark_dispatch re-arms with the new CPU's counter value.
        k.cpu_time_base = 7_000;
        account(&mut k, 7_002);
        test_eq!(k.cpu_time, 5); // CPU0 3 + CPU1 2
    });

    test_case!("accounting_idle_transition_does_not_lose_time", {
        // Running -> Ready mid-interval keeps the accumulated value; the next
        // dispatch re-arms. No double count for the partial interval.
        let mut k = Kthread::new_idle(9, 1, 0, 0x1000);
        k.cpu_time = 0;
        k.cpu_time_base = CPU_TIME_UNSET;
        account(&mut k, 10);
        account(&mut k, 14); // 4 units accumulated
        let after_running = k.cpu_time;
        test_eq!(after_running, 4);
        // Switch-out: no further accounting until re-dispatched.
        k.cpu_time_base = CPU_TIME_UNSET;
        test_eq!(k.cpu_time, after_running);
        // Re-dispatch on a different CPU.
        k.cpu_time_base = 50;
        account(&mut k, 52);
        test_eq!(k.cpu_time, 6);
    });

    test_case!("accounting_fold_resolves_in_flight_interval", {
        // Folding uses the owner's counter and is purely additive.
        test_eq!(fold_in_flight(10, 100, 105), 15);
        // No progress since the last refresh: nothing added.
        test_eq!(fold_in_flight(10, 100, 100), 10);
        // A stale/foreign base that appears in the future must not underflow.
        test_eq!(fold_in_flight(10, 200, 150), 10);
        // A thread keeps its stored total when no owner CPU can be resolved.
        let mut k = Kthread::new_idle(9, 1, 0, 0x1000);
        k.cpu_time = 10;
        k.cpu_time_base = CPU_TIME_UNSET;
        test_eq!(cpu_time_now(&k, None), 10);
        // Host tests have no KPRCB pages, so a nominal owner still yields the
        // stored value rather than dereferencing a missing counter.
        test_eq!(cpu_time_now(&k, Some(0)), 10);
    });
}
