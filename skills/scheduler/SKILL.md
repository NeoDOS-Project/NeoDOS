---
name: scheduler
description: Modify scheduling policy, priorities, SMP, thread states, timeslices
---

# Scheduler

## When to use

Modifying scheduling policy, priority management, SMP load balancing,
thread/process state transitions, or timeslice allocation.

## Goal

Make correct scheduler changes without breaking preemption, fairness, or SMP
invariants.

## References

- `docs/scheduler/scheduler.md` — subsystem documentation
- `src/scheduler/schedule.rs` — `schedule()`, `schedule_with()`, dispatch
- `src/scheduler/mod.rs` — scheduler entry points and global state
- `src/scheduler/queue.rs` — per-CPU priority run queues (`CpuRunQueue`),
  `try_dequeue_local()`
- `src/scheduler/wake.rs` — `wake_waiters` / `wake_blocked_on_magic`
- `src/scheduler/aging.rs` — starvation boost
- `src/scheduler/smp.rs` — work stealing (`try_work_steal`), IPI
- `src/scheduler/accounting.rs` — monotonic `cpu_time` accounting (Phase 15-A.1)
- `src/scheduler/snapshot.rs` — `snapshot_into` (proc/thread inspection)
- `src/scheduler/thread.rs`, `src/scheduler/process.rs`, `src/scheduler/types.rs`
- `src/arch/x64/cpu_local.rs` — `KPRCB` per-CPU data

## Steps

1. **Read `docs/scheduler/scheduler.md`** — priority scan, run queues, work
   stealing, aging, SMP.

2. **Locate the right file** (see References). Most policy lives in
   `schedule.rs`; the O(1) dispatch path is `queue.rs`; IPI/stealing is `smp.rs`.

3. **Priority levels** (`src/scheduler/types.rs` / `mod.rs`)

   | Level | Constant | Timeslice |
   | ------- | ---------- | ----------- |
   | 0 | `PRIORITY_HIGH` | 400 ticks |
   | 1 | `PRIORITY_ABOVE_NORMAL` | 200 ticks |
   | 2 | `PRIORITY_NORMAL` | 100 ticks |
   | 3 | `PRIORITY_IDLE` | 50 ticks |

   ```rust
   pub const PRIORITY_COUNT: u8 = 4;
   pub const TIME_SLICES: [u16; 4] = [400, 200, 100, 50];
   pub const IDLE_TIME_SLICE: u16 = 10;
   ```

   When changing, update `TIME_SLICES` (the array the tick path indexes).

4. **Thread states** (`ThreadState`)

   ```rust
   pub enum ThreadState { Ready, Running, Blocked { waiting_for: u64 }, Suspended, Terminated }
   ```

   Valid transitions: `Ready→Running`, `Running→Ready` (preempt), `Running→Blocked`,
   `Blocked→Ready`, `Running→Terminated`. Never `Blocked → Running` directly.

5. **Run queue / dispatch** (`queue.rs`, `schedule.rs`)
   Each CPU owns four priority sub-queues (64 entries each) selected by an
   `active_bitmap`; `pop()` picks the highest non-empty level via `trailing_zeros`
   (O(1)). A popped candidate below the global `highest_ready_priority()` is
   returned and dispatch falls through to the global priority scan (`#382`).
   Enqueueing on a remote CPU sends `IPI_RESCHEDULE` (0xF0).

6. **Work stealing / SMP** (`smp.rs`)
   `try_work_steal()` pulls one thread from a remote CPU's queue when the local
   queue is empty. IPI vectors: `0xF0` reschedule, `0xF1` TLB shootdown,
   `0xF2` call-function.

7. **Aging** (`aging.rs`)
   Every `AGING_INTERVAL_TICKS` (500) scan Ready non-idle threads; threads Ready
   for ≥ 5000 ticks get boosted one level (up to HIGH) and re-enqueued at the new
   level.

8. **Write tests** with `test_case!` in `src/scheduler/tests/` (registered via
   the scheduler's `register_*_tests()`): priority ordering, round-robin,
   timeslice expiry, aging boost, state transitions, work stealing.

9. **Build and test**

   ```bash
   cd neodos-kernel && cargo build
   neodev build --quick --image && neodev test
   neodev check-deps
   ```

## Best practices

- Keep state transitions atomic; a Running thread must be on exactly one CPU.
- Send `IPI_RESCHEDULE` after moving a thread between CPU run queues.
- Do not acquire the scheduler lock while holding another spinlock out of order.
- Kernel threads are found by the global priority scan (they do not self-enqueue);
  Ring 3 threads enqueue and IPI the target CPU.
- Use trace points / `trace_sched_state!` for diagnostics, not `printk`.

## Common mistakes

- Introducing a lock inversion with `SCHEDULER`.
- Publishing a user thread `Ready` on a Ring-0 frame (must be Ring 3; see
  scheduler.md `#338`/`#474`).
- Forgetting the IPI after thread migration.
- Letting the run queue silently drop a thread (push degrades to a *lower* level
  only).
- Forgetting to update `TIME_SLICES` when changing priority behavior.

## Final checklist

- [ ] State machine transitions valid (no `Blocked → Running`)
- [ ] Timeslice values aligned with priority levels
- [ ] Run-queue/steal path keeps `highest_ready_priority` invariant
- [ ] IPI sent after thread migration
- [ ] No new spinlock inversions
- [ ] Tests added; `cargo build`, `neodev test`, `neodev check-deps` pass
- [ ] `docs/scheduler/scheduler.md` updated if behavior changed
