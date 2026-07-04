# Scheduler

## When to use
Modifying scheduling policy, priority management, SMP load balancing, thread/process state transitions, or timeslice allocation.

## Goal
Make correct scheduler changes without breaking preemption, fairness, or SMP invariants.

## Steps

1. **Read `docs/scheduler.md`**
   Understand priorities, aging, time slices, SMP work stealing, and per-CPU run queues.

2. **Locate the right source file**
   - `src/scheduler/mod.rs` — main scheduler logic, priority, aging, timeslice allocation
   - `src/scheduler/runnable_queue.rs` — per-CPU run queues, work stealing
   - `src/scheduler/priority.rs` — priority levels and boost logic
   - `src/process/mod.rs` — EPROCESS, KTHREAD structures and thread state machine
   - `src/scheduler/smp.rs` — IPI, per-CPU idle threads

3. **Priority levels** (`src/scheduler/mod.rs`)
   - `HIGH(0)`: 400 tick timeslice
   - `ABOVE_NORMAL(1)`: 200 tick timeslice
   - `NORMAL(2)`: 100 tick timeslice
   - `IDLE(3)`: 50 tick timeslice
   When changing, update the `PRIORITY_TIMESLICES` array.

4. **Thread states** (`src/process/mod.rs` — `ThreadState` enum)
   Valid transitions: Ready→Running, Running→Ready (preempt), Running→Blocked, Blocked→Ready.
   Never transition directly between Blocked and Running.

5. **SMP work stealing** (`src/scheduler/runnable_queue.rs`)
   When a CPU's run queue is empty, it steals from the busiest sibling CPU.
   The `steal_work()` function iterates CPUs and transfers a batch of threads.
   After stealing, send IPI via `send_ipi()` in `src/scheduler/smp.rs` to trigger reschedule on the victim CPU.

6. **Add new scheduling policy** (if needed)
   - Implement the policy algorithm in a new file `src/scheduler/my_policy.rs`.
   - Add a policy enum variant in the scheduler's main types.
   - Wire it into the `schedule()` function in `src/scheduler/mod.rs`.
   - Ensure it respects the preemption model (voluntary + preemptive).

7. **Write tests**
   Add tests in `src/testing.rs`:
   - Thread creation and state transitions
   - Priority inheritance/boost (if applicable)
   - SMP steal under load
   - Timeslice exhaustion triggers reschedule

8. **Build and test**
   ```bash
   cargo build && python3 scripts/auto_test.py
   ```

## Best practices
- Keep the scheduler lock-free where possible — use atomic operations on thread states.
- Always yield the current timeslice before blocking (voluntary preemption).
- Work stealing must be O(1) on the steal target — don't scan all threads.
- Priority boost on I/O completion prevents starvation.
- Use `SCHED_DEBUG` or trace points for scheduler diagnostics, not printk.

## Common mistakes
- Introducing deadlocks by acquiring scheduler lock while holding another spinlock.
- Breaking the invariant: a thread in Running state must be on exactly one CPU's run queue.
- Forgetting to send IPI after moving threads between CPU run queues.
- Starving HIGH priority threads by allowing NORMAL threads to run without preemption.
- Not updating `ThreadState` atomically — leading to inconsistent scheduler views.

## Final checklist
- [ ] Thread state machine transitions correct (no invalid transitions)
- [ ] Timeslice values aligned with priority levels
- [ ] SMP work stealing tested under concurrent load
- [ ] IPI sent after thread migration
- [ ] No new spinlock inversions introduced
- [ ] Kernel tests added and pass
- [ ] `docs/scheduler.md` updated if behavior changed
