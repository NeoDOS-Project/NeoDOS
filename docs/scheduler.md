# NeoDOS Scheduler

Source: `src/scheduler/mod.rs`, `src/arch/x64/cpu_local.rs`. Priority scheduler
(A2) with 4 levels, dynamic time-slicing, preemption from Ring 3, work stealing,
and aging.

## Priority Levels

| Level | Constant               | Time Slice | Description                      |
|-------|------------------------|------------|----------------------------------|
| 0     | PRIORITY_HIGH          | 400 ticks  | Critical system processes        |
| 1     | PRIORITY_ABOVE_NORMAL  | 200 ticks  | Important user processes         |
| 2     | PRIORITY_NORMAL        | 100 ticks  | Default (new processes)          |
| 3     | PRIORITY_IDLE          | 50 ticks   | Background, runs when idle       |

```rust
pub const PRIORITY_COUNT: u8 = 4;
pub const TIME_SLICES: [u16; 4] = [400, 200, 100, 50];
```

## Algorithm

### schedule()

1. Try local `CpuRunQueue` (per-CPU 64-entry ring buffer)
2. If empty, call `try_work_steal()` to steal from another CPU's IDLE queue
3. If still empty, fall back to global thread table scan
4. Scan priority levels HIGH -> IDLE
5. Within each level, round-robin across Ready threads

Returns a `*mut Kthread` to switch to (or the current thread if no better candidate).

### Timer Tick

`on_timer_tick()` at each timer interrupt:

1. Increment `timer_ticks`
2. Decrement `time_slice_remaining` of current thread
3. If expired: mark thread Ready, set `cpu_local.need_resched = true`
4. Every `AGING_INTERVAL_TICKS` (500): run aging check

### sys_yield

Transitions Running -> Ready, resets time slice to full value, forces reschedule
on next return to userspace.

### Preemption from Ring 3

Timer interrupt handler checks CS selector. If CS == 0x1B (user mode):

1. Save current RSP into the KTHREAD
2. Call `schedule()` to select next thread
3. Update TSS.RSP0 to the new thread's kernel stack top
4. Return new thread's RSP to assembly stub

### Aging

Every 500 ticks, scan all Ready threads. If a thread has been Ready for >= 5000
ticks (`MAX_STARVATION_TICKS`) since it last ran, boost its priority by one level
(up to HIGH). This prevents starvation of low-priority threads under load.

### Work Stealing

`try_work_steal()` iterates other CPUs and examines their CpuRunQueue. It steals
the oldest thread from the IDLE priority queue of the most-loaded remote CPU.
Stolen threads are enqueued on the local CPU.

## Per-CPU Run Queues

Each CPU has a `CpuRunQueue` embedded in its KPRCB (accessed via GS segment):

```rust
pub struct CpuRunQueue {
    pub entries: [u32; 64],  // TIDs in ring buffer
    pub head_idx: u16,       // dequeue position
    pub tail_idx: u16,       // enqueue position
    pub count: u16,          // number of entries
}
```

- `push(tid)` -- enqueue at tail, panic if full
- `pop() -> Option<u32>` -- dequeue from head
- `is_empty()`, `len()`, `peek()`

When a thread is enqueued on a remote CPU, an `IPI_RESCHEDULE` (vector 0xF0) is
sent to that CPU to trigger a reschedule check.

## Process and Thread Model

### KTHREAD

Per-thread structure holding CPU context and scheduling state:

```rust
pub struct Kthread {
    // Saved register context
    pub rax: u64,  rbx: u64,  rcx: u64,  rdx: u64,
    pub rsi: u64,  rdi: u64,  r8: u64,   r9: u64,
    pub r10: u64,  r11: u64,  r12: u64,  r13: u64,
    pub r14: u64,  r15: u64,  rbp: u64,
    pub rsp: u64,  pub rip: u64,  pub rflags: u64,
    // Identity
    pub tid: u32,
    pub pid: u32,
    // Scheduling
    pub state: ThreadState,
    pub priority: u8,
    pub time_slice_remaining: u16,
    pub ticks_since_scheduled: u64,
    pub cpu: u32,
    // Kernel stack
    pub kernel_stack_top: u64,
    pub teb_base: u64,
    // APC queues
    pub kernel_apc_queue: VecDeque<ApcEntry>,
    pub user_apc_queue: VecDeque<ApcEntry>,
    pub apc_pending: bool,
}
```

### EPROCESS

Per-process structure for shared resources:

```rust
pub struct Eprocess {
    pub pid: u32,
    pub parent_pid: u32,
    pub handle_table: HandleTable,
    pub cwd_drive: u8,
    pub cwd_path: String,
    pub heap_base: u64,
    pub heap_break: u64,
    pub user_slot: Option<u8>,
    pub mmap_regions: Vec<MmapRegion>,
    pub thread_count: u32,
    pub exit_code: i64,
    pub address_space: AddressSpace,
    pub token: Token,
    pub vt_num: u8,
}
```

### ThreadState

```rust
pub enum ThreadState {
    Ready,
    Running,
    Blocked { waiting_for: u32 },
    Terminated,
}
```

Blocked state includes the resource ID (pipe_id, irp_id, etc.) for unblock-on-event.

Only user-mode (Ring 3) threads participate in scheduling. The Ring 0 shell
uses the idle bootstrap path.

## SMP Integration

### Per-CPU Data (KPRCB)

Each CPU has a KPRCB structure accessible via GS segment base, containing:

| Offset | Size | Field          |
|--------|------|----------------|
| 0x000  | 8    | current_thread |
| 0x008  | 8    | idle_thread    |
| 0x010  | 1    | need_resched   |
| 0x018  | 264  | run_queue      |
| 0x120  | 8    | exit_rsp       |
| 0x128  | 8    | exit_rip       |
| ...    | ...  | slab hot caches|

### CPU Boot (SMP)

BSP sends INIT-SIPI-SIPI sequence to APs. Each AP initializes its KPRCB,
creates its idle thread, and enters the scheduler's idle loop.

### IPI Vectors

| Vector | Purpose            |
|--------|--------------------|
| 0xF0   | IPI_RESCHEDULE     |
| 0xF1   | IPI_TLB_SHOOTDOWN  |
| 0xF2   | IPI_CALL_FUNCTION  |

## Context Switch

### Timer ISR Assembly Stub

```
timer_stub:
    push all GPRs
    mov rdi, rsp         // pass current RSP as argument
    call timer_handler_inner  // returns new RSP in RAX
    mov rsp, rax         // switch stacks
    pop all GPRs
    iretq
```

### timer_handler_inner (Rust)

1. `on_timer_tick()` -- decrement time slice, check need_resched
2. If reschedule needed: save old thread RIP/RSP, call `schedule()`
3. Update TSS.RSP0 to new thread's `kernel_stack_top`
4. Return new thread's RSP (or old RSP if no switch)

### Exit Trampoline

On `sys_exit`, the per-CPU `exit_rsp`/`exit_rip` in KPRCB point to a trampoline
that returns to the kernel's idle loop after the final `iretq`.

## API

| Function                          | Purpose                        |
|-----------------------------------|--------------------------------|
| `sched_set_process_priority(pid, priority)` | Change priority at runtime   |
| `sys_yield(RAX=2)`                | Voluntary yield                 |
| `PRI <pid> <level>`               | Shell: set priority             |
| `PS`                              | Shell: list processes + H/AN/N/I|

## Tests

7 kernel tests cover:

- Priority scheduling (HIGH runs before IDLE)
- Round-robin within same priority
- Time-slice expiration and reschedule
- Aging boost after starvation
- sys_yield forces reschedule
- Blocked thread does not run
