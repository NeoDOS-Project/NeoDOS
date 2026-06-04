# NeoDOS Scheduler

NeoDOS includes a **priority scheduler** (A2) with 4 levels, dynamic time-slicing, preemption from Ring 3, and aging.

## Priority Levels

| Level | Constant | Time Slice | Description |
|-------|----------|-----------|-------------|
| 0 | `PRIORITY_HIGH` | 400 ticks | Critical system processes |
| 1 | `PRIORITY_ABOVE_NORMAL` | 200 ticks | Important user processes |
| 2 | `PRIORITY_NORMAL` | 100 ticks | Default priority (new processes) |
| 3 | `PRIORITY_IDLE` | 50 ticks | Background, runs only when nothing else is ready |

## Algorithm

- **schedule()**: scans by priority level (HIGH→IDLE), round-robin within the same level
- **on_timer_tick()**: decrements `time_slice_remaining` each tick; at expiry marks Ready + `NEED_RESCHED`
- **sys_yield**: Running→Ready + reset time slice + force reschedule
- **Preemption from Ring 3**: timer handler detects CS=0x1B (user mode), saves RSP, calls schedule(), changes TSS.RSP0
- **Aging** (every 100 ticks): boosts priority if a Ready process hasn't run in >= 1000 ticks

## Process Model

- The scheduler stores up to 16 processes in a static table (`MAX_PROCESSES`).
- PID `0` is reserved for the idle process, which is always present.
- Process states: `Ready`, `Running`, `Blocked`, `Terminated`.

## Timer Integration

The timer interrupt handler (`timer_handler_inner`) performs:
1. Decrement time slice (`on_timer_tick`)
2. If Ring 3: save current process stack pointer, select next runnable process (`schedule`), update TSS.RSP0
3. Return next process RSP to assembly stub (or old RSP if no reschedule)

## Context Switch Stack Layout

The timer ISR uses an assembly stub that:
- pushes general-purpose registers
- calls into Rust (`timer_handler_inner(rsp)`)
- switches `rsp` to the value returned by Rust
- pops registers
- returns with `iretq`

## API

- `sched_set_process_priority(pid, priority)` — change priority at runtime
- `PRI <pid> <level>` shell command (0=HIGH, 1=ABOVE_NORMAL, 2=NORMAL, 3=IDLE)
- `PS` shell command shows priority column (H/AN/N/I)

## Related

- `sys_yield` (RAX=2) — voluntary yield
- `Blocked` state via `irp_block_current()` or pipe reads — scheduler integration for async I/O
- See `docs/KERNEL.md` for boot sequence
