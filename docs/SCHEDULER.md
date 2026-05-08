# NeoDOS Scheduler

NeoDOS includes a small round-robin scheduler intended for kernel-space processes.

## Process Model

- The scheduler stores up to a fixed number of processes (`MAX_PROCESSES`, currently 4) in a static table.
- PID `0` is reserved for the idle process, which is always present.
- Process states: `Ready`, `Running`, `Blocked`, `Terminated`.

## Timer Integration

The timer interrupt handler (`timer_handler_inner`) performs:

1. (Optional) accounting (`on_timer_tick`)
2. Saving the current process stack pointer (for non-idle processes)
3. Selecting the next runnable process (`schedule`)
4. Returning the next process `rsp` back to the assembly stub, which restores registers and `iretq`s

To avoid early boot issues, context switching is skipped until at least one non-idle process exists.

## Context Switch Stack Layout

The timer ISR uses an assembly stub that:

- pushes general-purpose registers
- calls into Rust (`timer_handler_inner(rsp)`)
- switches `rsp` to the value returned by Rust
- pops registers
- returns with `iretq`

New processes and the idle process are initialized with a prebuilt stack frame that matches this save/restore sequence so the first `iretq` lands at the process entry point.

