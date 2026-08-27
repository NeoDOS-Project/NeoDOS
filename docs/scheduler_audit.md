# Phase 0 — Scheduler Audit Results

## Context

- 678 kernel tests pass on clean `develop` branch
- The system works via O(n) global priority scan fallback in `schedule()` (step 3)
- The per-CPU runqueue fast path (steps 1-2) is effectively dead code for Running→Ready transitions
- The earlier PID 5 triple-fault investigation showed RSP = `0xDEADBEEFCAFEBABE` (stack canary value) — not directly caused by these scheduler bugs, but the inconsistency window in P0-1/P0-4 could mask the real crash cause

---

## Invariant Violations Found

### P0-1: Running → Ready without enqueue

| Location | Code | Violation |
|----------|------|-----------|
| `handlers.rs:192-211` (`handler_yield`) | Sets `state = Ready`, calls `set_need_resched()` | **FIXED** — now uses `make_thread_ready()` |
| `handlers.rs:347-360` (`handler_waitpid` wildcard branch) | Sets `state = Ready` if Running | **FIXED** — now uses `make_thread_ready()` |
| `handlers.rs:593-614` (`handler_sleep_ex`) | Sets `state = Ready` if Running | **FIXED** — now uses `make_thread_ready()` |
| `scheduler/mod.rs:1544-1567` (`yield_current_thread`) | Sets `state = Ready` if Running | **FIXED** — now uses `make_thread_ready()` |
| `syscall/mod.rs:364-371` (`syscall_try_resched`) | Sets `state = Ready` if Running | **FIXED** — now uses `make_thread_ready()` |
| `scheduler/mod.rs:1368-1373` (`on_timer_tick`) | Sets `state = Ready` on timeslice expiry | **EXEMPT** — called mid-timer-handler before RSP save; timer handler handles context switch |

**Root cause**: 5 of 6 `Running → Ready` transitions set state but don't enqueue to the per-CPU runqueue. The scheduler compensates with a O(n) global priority scan fallback (step 3 in `schedule()`), but this bypasses the runqueue fast path entirely, making it dead code. Any thread that yields is invisible to the runqueue and can only be found by the O(n) global scan. The 6th path (`on_timer_tick`) is exempt because it runs mid-timer-handler before RSP is saved.

**Impact**: Performance degradation (O(N) per schedule instead of O(1)) and a correctness gap — the thread is `state == Ready` but not in any runqueue, which violates the invariant.

### P0-2: Suspended → Ready without enqueue

| Location | Code | Status |
|----------|------|--------|
| `scheduler/mod.rs:1111-1125` (`wake_waiters`) | Sets Ready + `enqueue_to_cpu_run_queue()` | ✅ Correct |
| `scheduler/mod.rs:1127-1136` (`wake_blocked_on_magic`) | Sets Ready + `enqueue_to_cpu_run_queue()` | ✅ Correct |
| `kwait/mod.rs:120-134` (`kwait_wake`) | Sets Ready + `enqueue_to_cpu_run_queue()` | ✅ Correct |
| `handlers.rs:85-92` (`handler_exit` ThreadJoin wake) | Sets Ready + `enqueue_to_cpu_run_queue()` | ✅ Correct |
| `handlers.rs:96-103` (`handler_exit` ChildExit wake) | Sets Ready + `enqueue_to_cpu_run_queue()` | ✅ Correct |
| `apc/mod.rs:110-117` (`queue_user_apc` alertable wake) | Sets Ready + `enqueue_to_cpu_run_queue()` | ✅ Correct |
| `ob.rs:3247` (ObWait activates Suspended child) | Sets `state = Ready` | **FIXED** — now uses `make_thread_ready()` (P0-2) |

The Blocked→Ready paths are all correct (they enqueue). The only violation is the Suspended→Ready transition in `handler_ob_wait` which skips enqueue.

### P0-3: Runqueue stale/duplicate entries

| Concern | Analysis |
|---------|----------|
| **Duplicate enqueue** | `enqueue_to_cpu_run_queue()` does `run_queue.push(k.tid)` unconditionally — no dedup check. `CpuRunQueue` is a bare ring buffer (64-entry FIFO). If `wake_blocked_on_magic(0xFFFFFFFF)` fires twice before the thread runs, TID appears twice. |
| **Stale entries** | Ready → Blocked after enqueue leaves TID in runqueue. `schedule()` skips it via `state == Ready` check, but wastes a dequeue slot. |
| **Work stealing** | `steal_from_cpu_run_queue` copies TIDs blindly. Stale TIDs are skipped but not cleaned up. |

### P0-4: current_tid / Running consistency

| Concern | Analysis |
|---------|----------|
| `schedule()` sets `current_tid = X` and `X.state = Running` atomically (under lock) | ✅ Correct |
| `syscall_try_resched` sets `current_tid` and `next.state = Running` | ✅ Correct |
| Timer handler sets per-CPU pointers via `this_cpu_set_current_thread` | ✅ Correct |
| **Gap**: `handler_yield` sets `state = Ready` but does NOT update `current_tid` | Thread is still `current_tid` but state is `Ready` — inconsistency window |

### P0-5: Syscall return after reschedule

| Concern | Analysis |
|---------|----------|
| `syscall_try_resched` saves `current_rsp`, calls `schedule()`, returns `next_rsp` | ✅ Frame swap correct |
| Ring-0 target skip (lines 395-411) restores original thread | ✅ Correct |
| Validation of RIP/RSP/CS/SS/RFLAGS on return path | ✅ Extensive checks present |
| **P0-1 interaction**: Original thread `state = Ready` without enqueue means it's not findable by fast-path dequeue | **Risk** — only found by global scan fallback |

### P0-6: sys_read Block/Wake/Retry + VT matching

| Concern | Analysis |
|---------|----------|
| `handler_read` sets `state = Blocked { waiting_for: 0xFFFFFFFF }` | ✅ Correct block |
| Returns `err_to_u64(SyscallError::Again)` = `-8` to Ring 3 | ✅ Ring 3 must retry |
| `wake_blocked_readers()` calls `wake_blocked_on_magic(0xFFFFFFFF)` which enqueues | ✅ Wake + enqueue correct |
| `kbd/mod.rs:243` calls `wake_blocked_readers()` from keyboard IRQ path | ✅ Triggered on keypress |
| **VT matching**: `wake_blocked_on_magic(0xFFFFFFFF)` wakes ALL threads blocked on `0xFFFFFFFF`, not just the VT that received input | **Bug** — threads on other VTs wake, retry, find no input, re-block. Adds unnecessary context switches. |

---

## Files Affected

- `neodos-kernel/src/scheduler/mod.rs` — lines 1181-1204, 1247-1382, 1544-1567
- `neodos-kernel/src/syscall/handlers.rs` — lines 85-103, 192-211, 327-380, 588-614
- `neodos-kernel/src/syscall/mod.rs` — lines 340-472, 778
- `neodos-kernel/src/syscall/ob.rs` — lines 3244-3267
- `neodos-kernel/src/arch/x64/idt.rs` — lines 640-1034
- `neodos-kernel/src/kwait/mod.rs` — lines 102-134
- `neodos-kernel/src/apc/mod.rs` — lines 95-128
- `neodos-kernel/src/kbd/mod.rs` — line 243

---

## Proposed Central Primitive

```rust
/// Transition a thread to Ready state and enqueue it exactly once.
/// Safe to call if thread is already Ready (no-op, avoids duplicate enqueue).
/// Must be called under scheduler lock + interrupts disabled.
pub fn make_thread_ready(k: &mut Kthread) {
    if k.state == ThreadState::Ready {
        return; // already Ready — assume already enqueued (schedule() validates)
    }
    debug_assert!(matches!(k.state, ThreadState::Running | ThreadState::Blocked { .. } | ThreadState::Suspended));
    k.state = ThreadState::Ready;
    let idx = (k.priority as usize).min(PRIORITY_COUNT as usize - 1);
    k.time_slice_remaining = TIME_SLICES[idx];
    k.ticks_since_scheduled = 0;
    Self::enqueue_to_cpu_run_queue(k);
}
```

**Note on the `Ready` early-return**: This is safe because:
1. A thread that is `state == Ready` was either (a) already enqueued by a prior `make_thread_ready` call, or (b) is a stale entry from P0-1 that was never enqueued. In case (b), the global scan fallback in `schedule()` finds it anyway.
2. After P0-1 is fixed, all transitions go through `make_thread_ready`, so case (b) disappears.
3. `schedule()` already skips non-Ready threads on dequeue, so a duplicate enqueue is harmless (just wastes one dequeue slot).

This primitive should be used in ALL Running→Ready, Blocked→Ready, and Suspended→Ready transitions.

---

## Test Plan

### Test A — yield
```
Running → sys_yield() → Ready → runqueue → scheduler
```
Verify exactly one runqueue entry after yield. Verify thread is schedulable.

### Test B — waitpid
```
Running → sys_waitpid() → [no child exited] → Ready (no child) → runqueue
```
Verify no Ready thread is left outside runqueue. No duplicate entries.

### Test C — read block
```
Running → sys_read() → no input → Blocked → scheduler picks next thread
```
Verify thread is not in runqueue while Blocked. Another thread executes.

### Test D — read wake
```
Blocked thread → keyboard IRQ → wake → Ready + enqueue → scheduler
```
Verify exactly one runqueue entry after wake.

### Test E — read return
```
After wake → scheduler → Running → sys_read retry → character → Ring 3
```
Verify syscall frame (RIP, RSP, RFLAGS, CS, SS) is correct on return.

### Test F — duplicate wake
```
Two wake-ups for same TID → one valid runqueue entry
```
Verify no duplicate in runqueue.

### Test G — stress
```
100 iterations: Running → Blocked → Ready → Running
```
No TID lost, no duplicate, no impossible state, no RSP/RIP corruption, no triple fault.

### Test H — regression
All 678 existing kernel tests must pass.

---

## Execution Order

| Order | Issue | Shared root cause |
|-------|-------|-------------------|
| 1 | P0-1: Running→Ready without enqueue | **Foundation** — `make_thread_ready()` primitive |
| 2 | P0-2: Suspended→Ready without enqueue | Uses same primitive from P0-1 |
| 3 | P0-3: Runqueue stale/duplicate | Depends on P0-1 fix for correct enqueue patterns |
| 4 | P0-4: current_tid consistency | Depends on P0-1 fix (state transitions must be atomic) |
| 5 | P0-5: Syscall return validation | Depends on P0-1/P0-4 (correct state + current_tid) |
| 6 | P0-6: sys_read + VT matching | Independent but benefits from P0-1-P0-5 fixes |
