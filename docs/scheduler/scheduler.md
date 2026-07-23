# NeoDOS Scheduler

Source: `src/scheduler/mod.rs`, `src/arch/x64/cpu_local.rs`. Priority-based
preemptive scheduler with 4 levels, per-CPU run queues, global priority scan,
work stealing, aging, and round-robin within each level.

---

## Architecture Overview

The scheduler uses a **two-tier** decision mechanism:

| Tier | Component | Purpose |
|------|-----------|---------|
| 1    | **Per-CPU Run Queue** | Async notification: new threads, remote wakeups, SMP IPI |
| 2    | **Global Priority Scan** | Fair round-robin selection across all Ready threads |
| 3    | **Idle fallback** | Select TID 1 (PRIORITY_IDLE) when no Ready thread exists |

```
Thread wakes / created / unblocked
        |
        v
  [Run Queue]  ← only for notification/IPC (not for time-slice re-enqueue)
        |
        v
  schedule() called (timer IRQ, explicit yield, wakeup)
        |
        v
  try_dequeue_local()  → fast path if notification pending
        |
        v  (empty)
  try_work_steal()     → remote CPU queue
        |
        v  (empty)
  GLOBAL PRIORITY SCAN → fairness by priority + round-robin
        |
        v  (no Ready threads)
   Idle fallback → TID 1 (idle thread, PRIORITY_IDLE)
```

### Key invariant: the run queue is NOT a scheduling cache

The per-CPU run queue is used **only** for:
- Notification of new threads (`add_ring3_process`, `add_thread_to_process`)
- Remote wakeups (`wake_waiters`, `wake_blocked_on_magic` → IPI to target CPU)
- SMP work stealing (`try_work_steal` → `steal_from_cpu_run_queue`)

It is **not** used for re-enqueuing threads after time-slice expiry. After a
thread transitions `Running → Ready` in `on_timer_tick()`, it is found by the
global priority scan, not by the run queue. This prevents the run queue from
bypassing the fairness guarantees of the scan.

---

## Thread Lifecycle

### State transitions

```
                 ┌─────────────────┐
                 │     READY       │
                 └────────┬────────┘
                          │ schedule() picks thread
                          ▼
                 ┌─────────────────┐
    timer tick → │    RUNNING      │ → syscall/kwait → BLOCKED
    (expiry)     │                 │
                 └────────┬────────┘
                          │ thread exit
                          ▼
                 ┌─────────────────┐
                 │   TERMINATED    │
                 └─────────────────┘

    BLOCKED → READY: wake_waiters() / wake_blocked_on_magic() + enqueue
```

### Thread creation flow

```
spawn_kthread()          add_ring3_process()
    │                         │
    ├─ alloc_kthread_slot()   ├─ alloc_eprocess_slot()
    ├─ Box<AlignedKStack>()   ├─ alloc_kthread_slot()
    ├─ init_ring0_frame()     ├─ Kthread::new_ring3()
    ├─ Kthread{state:Ready}   ├─ init_ring3_frame()
    ├─ Eprocess::new_kernel() ├─ enqueue_to_cpu_run_queue()
    └─ (no run queue enqueue) └─ return PID
       Thread found by
       global priority scan
```

**Kernel threads** (`spawn_kthread`) do NOT enqueue themselves. They enter the
scheduling pool via the global priority scan, avoiding starvation of TID 0
(the idle/boot thread) during early boot.

**Ring 3 threads** (`add_ring3_process`) DO enqueue themselves. This sends an
IPI to the target CPU on SMP systems, notifying it of new work.

---

## Priority Levels

| Level | Constant               | Time Slice | Description                      |
|-------|------------------------|------------|----------------------------------|
| 0     | PRIORITY_HIGH          | 400 ticks  | Critical system processes        |
| 1     | PRIORITY_ABOVE_NORMAL  | 200 ticks  | Important user processes         |
| 2     | PRIORITY_NORMAL        | 100 ticks  | Default (new processes, boot)    |
| 3     | PRIORITY_IDLE          | 50 ticks   | Background, runs when idle       |

```rust
pub const PRIORITY_COUNT: u8 = 4;
pub const TIME_SLICES: [u16; 4] = [400, 200, 100, 50];
pub const IDLE_TIME_SLICE: u16 = 10;   // idle thread: brief CPU then yield
```

---

## Global Priority Scan

### `schedule()`

1. Increment `schedule_count` (for idle boost backstop)
2. **Run queue**: `try_dequeue_local()` → if a TID is pending notification
3. **Work stealing**: `try_work_steal()` → steal from remote CPU queues
4. **Global scan**: iterate priority levels HIGH → IDLE, round-robin within level
5. **Fallback**: find TID 1 (idle, PRIORITY_IDLE) if nothing else is Ready → panic if idle terminated

The global scan iterates **all** TIDs (including boot TID 0), checking `state == Ready`
at each priority level. The idle thread (TID 1) has PRIORITY_IDLE and is naturally
skipped while any higher-priority thread is Ready.

### Round-robin

Within a priority level, the scan starts from `(current_tid + 1) % next_tid`
and wraps around. This gives natural round-robin fair scheduling across all
Ready threads at the same priority.

---

## Timer Tick

`on_timer_tick()` at each timer interrupt:

1. Increment `timer_ticks`
2. If current thread is Running:
   - Decrement `time_slice_remaining`
   - On expiry: `state = Ready`, set `needs_resched`
3. Every `AGING_INTERVAL_TICKS` (500): run aging check

The expired thread transitions to Ready but is **not** re-enqueued in the run
queue. The next `schedule()` call finds it via the global priority scan.

---

## Per-CPU Run Queues

```rust
pub struct CpuRunQueue {
    pub entries: [u32; 64],  // TIDs in ring buffer
    pub head_idx: u16,       // dequeue position
    pub tail_idx: u16,       // enqueue position
    pub count: u16,          // number of entries
}
```

- `push(tid)` → enqueue at tail, returns false if full (64 entries max)
- `pop() -> Option<u32>` → dequeue from head

When a thread is enqueued on a remote CPU, `IPI_RESCHEDULE` (vector 0xF0) is sent.

### When threads enter the run queue

| Operation | Enqueues? | Reason |
|-----------|-----------|--------|
| `add_ring3_process` | Yes | New Ring 3 thread, needs discoverability + IPI |
| `add_thread_to_process` | Yes | Additional Ring 3 thread |
| `wake_waiters` | Yes | Unblocked thread, may be on remote CPU |
| `wake_blocked_on_magic` | Yes | KWait unblock, may be on remote CPU |
| `spawn_kthread` | **No** | Kernel threads found by global scan |
| `on_timer_tick` (expiry) | **No** | Found by global scan, not a notification |

---

## Context Switch

### Timer ISR Assembly (`timer_handler_asm`)

```text
timer_handler_asm:
    push all 15 GPRs (rbp, r15, ..., rax)
    mov rdi, rsp                  // current_rsp = RSP after GPR pushes
    call timer_handler_inner      // returns new thread's RSP in RAX
    mov rsp, rax                  // switch to new thread's saved stack
    pop all 15 GPRs (reverse order)
    iretq
```

### `timer_handler_inner` (Rust)

1. Acquire `SCHEDULER` lock
2. `on_timer_tick()` → decrement time slice, check expiry
3. Check `should_preempt`:
   - TID > 0: preempt if current thread is Ready AND its TID matches
   - TID == 0 (idle): preempt if idle is Ready AND non-idle threads exist
4. If preempting: save `k.rsp = current_rsp`, call `schedule()`, update TSS.RSP0
5. Update per-CPU current thread/PID/context switch count
6. Return new thread's `rsp` (or `current_rsp` if no switch)
7. Release lock

### Ring 0 vs Ring 3 context switch

| Thread type | Frame init function | iretq frame | Transition |
|-------------|---------------------|-------------|------------|
| Kernel (Ring 0) | `init_ring0_frame()` | 3 entries: RIP, CS=0x08, RFLAGS=0x202 | Ring 0→Ring 0 |
| User (Ring 3) | `init_ring3_frame()` | 5 entries: RIP, CS=0x1B, RFLAGS, RSP, SS=0x23 | Ring 0→Ring 3 |

---

## Kernel Threads (`spawn_kthread`)

Kernel threads are created with:
- A **heap-allocated stack** (`Box<AlignedKStack>` of 16 KB, 16-byte aligned)
- An EPROCESS marked as kernel (`Eprocess::new_kernel`)
- State `Ready`, enqueued in `kthreads` table but **not** in the per-CPU run queue
- Priority `PRIORITY_NORMAL` by default, overridable

The kernel stack is freed when the Kthread is dropped (via `kill_pid` or
`recycle_thread`), which drops the `Box<AlignedKStack>`.

### `netd` — network kernel thread

```rust
pub fn spawn_net_kthread(entry: u64) -> Option<u32> {
    crate::hal::without_interrupts(|| {
        current_scheduler().lock().spawn_kthread(entry, PRIORITY_NORMAL)
    })
}
```

`netd` is created during boot (in `main.rs`) after all kernel tests complete.
It runs `net_kthread_entry()` which loops:
```rust
pub fn net_kthread_entry() -> ! {
    loop {
        net_tick();                       // network_poll_all + arp_tick + dns_tick
        for _ in 0..64 { core::hint::spin_loop(); }
    }
}
```

**Scheduler lock protection**: `spawn_net_kthread` wraps the lock acquisition in
`without_interrupts`. This prevents a deadlock where the timer IRQ handler
(`timer_handler_inner`) tries to acquire the same `SCHEDULER` lock while the
boot code holds it. All other scheduler lock acquisitions in the codebase
follow this same pattern.

---

## Aging

Every 500 ticks, all Ready threads with `tid > 0` are scanned. Threads that have
been Ready for ≥ 5000 ticks get their priority boosted by one level (up to HIGH).
This prevents low-priority starved threads from being permanently ignored.

---

## Work Stealing

`try_work_steal()` iterates all remote CPUs' run queues and steals one thread
into the local run queue. This balances load across CPUs when one CPU's queue
is empty and another has Ready threads.

---

## BSS Stack Corruption (investigated v0.49)

### Symptom

When `netd` was first introduced, the static `NET_THREAD_STACK` ([u8; 16384])
caused a GPF at the `iretq` instruction (`0x400c811`) with error code `0xb17c`
(segment selector referencing the LDT). The CS value at `[sp-16]` of the initial
frame (written by `init_ring0_frame()`) was corrupted.

### Root cause hypothesis

The static array was placed in the kernel's BSS section (0x414e938−0x41f3d20)
by the linker, adjacent to mutable static variables such as `HPET_FS_PERIOD`,
`ACTIVE_TIMER`, and `WATCHDOG_*`. With LTO enabled (`lto = true`) and
`opt-level = 3`, the compiler/linker may have produced aliasing or off-by-one
placement of the stack array relative to adjacent 8-byte variables. The 34,000+
timer ticks during kernel self-tests wrote to timer and watchdog structures,
and one or more of these writes reached the stack area.

### Fix

Replaced the static array with a heap-allocated `Box<AlignedKStack>`, placing
kernel thread stacks at heap addresses (0x2400000 range) completely separate
from the BSS section.

### Recommendations for future investigation

- Build with `lto = false` temporarily and retest: if the GPF disappears, LTO
  is confirmed as the cause.
- Use `cargo careful` (cargo-careful) to detect undefined behavior in unsafe
  code that may trigger the corruption.
- Generate a linker map (`-C link-arg=-Map=kernel.map`) to visualize the exact
  layout of variables in the BSS section and detect potential overlaps.

---

## Process and Thread Model

### KTHREAD

Per-thread CPU context, kernel stack, and scheduling state.

### EPROCESS

Shared per-process resources: address space, handle table, heap, mmap, token.

### ThreadState

```rust
pub enum ThreadState { Ready, Running, Blocked { waiting_for: u32 }, Terminated }
```

---

## SMP Integration

### Per-CPU Data (KPRCB)

Each CPU has a KPRCB at a fixed address, accessed via GS segment:

| Offset | Size | Field          |
|--------|------|----------------|
| 0x000  | 8    | current_thread |
| 0x008  | 8    | idle_thread    |
| 0x010  | 1    | need_resched   |
| 0x018  | 264  | run_queue      |
| 0x120  | 8    | exit_rsp       |
| 0x128  | 8    | exit_rip       |

### IPI Vectors

| Vector | Purpose            |
|--------|--------------------|
| 0xF0   | IPI_RESCHEDULE     |
| 0xF1   | IPI_TLB_SHOOTDOWN  |
| 0xF2   | IPI_CALL_FUNCTION  |

---

## API

| Function                          | Purpose                        |
|-----------------------------------|--------------------------------|
| `sched_set_process_priority(pid, priority)` | Change priority at runtime   |
| `sys_yield(RAX=2)`                | Voluntary yield                 |
| `spawn_kthread(entry, priority)`  | Create kernel thread (Ring 0)   |
| `add_ring3_process(...)`          | Create user thread (Ring 3)     |

---

## Tests

Current kernel tests cover:

- Priority scheduling (HIGH before IDLE)
- Round-robin within same priority
- Time-slice expiration and reschedule
- Aging boost after starvation
- Thread state transitions
- New process creation and cleanup
