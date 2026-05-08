# NeoDOS Phase 2: Interrupts, Timer, and Scheduling

Phase 2 introduces the CPU/interrupt foundations needed for preemption and (eventually) multitasking.

## Architecture Overview

### 1. Global Descriptor Table (GDT)
We implemented a custom GDT replacing the one provided by UEFI.
- **Selectors**:
  - `0x08`: Kernel Code (64-bit)
  - `0x10`: Kernel Data
  - `0x18`: User Code
  - `0x20`: User Data
  - `0x28`: Task State Segment (TSS)
- **TSS**: Provides a dedicated stack for double fault exceptions (IST0), preventing system crashes when a stack overflow occurs.

### 2. Interrupt Descriptor Table (IDT)
A complete IDT was implemented with handlers for all 32 CPU exceptions and hardware IRQs.
- **Exception Handlers**: All exceptions (Divide Error, Page Fault, etc.) now result in a clean panic with debug information.
- **Hardware Interrupts**:
  - `IRQ0 (INT 32)`: Timer interrupt, triggers the context switch.
  - `IRQ1 (INT 33)`: Keyboard interrupt, currently a stub.

### 3. PIC Initialization
The legacy 8259A PICs were remapped to start at interrupt vector 32 to avoid conflicts with CPU exceptions.

### 4. Scheduler & Context Switching
A round-robin scheduler manages 4 concurrent kernel processes.
- **Context Switch Flow**:
  1. **Interrupt Entry**: Timer fires, hardware pushes `SS, RSP, RFLAGS, CS, RIP`.
  2. **Register Save**: `timer_handler_asm` pushes all general-purpose registers (`RAX` to `RBP`).
  3. **Schedule**: `timer_handler_inner` saves the current `RSP` into the process table and calls `scheduler.schedule()` to pick the next `Ready` process.
  4. **Register Restore**: `timer_handler_asm` switches to the new process's `RSP` and pops the saved registers.
  5. **Return**: `iretq` restores the hardware frame and jumps to the new process's `RIP`.

## Notes

- Kernel link/entry address starts at `0x200000` (see `neodos-kernel/kernel.ld`).
- The idle task exists so the timer can fire before other processes are created without panicking.

## Verifying

If/when test processes are enabled, you should observe process output alternating under timer preemption.
