# NeoDOS Phase 2: Multitasking & Context Switching

Phase 2 successfully transitions NeoDOS from a single-threaded kernel to a multitasking environment with a round-robin scheduler.

## 🏗️ Architecture Overview

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

## 📍 Memory Layout (Updated)

| Region | Usage |
|--------|-------|
| `0x04000000` | Kernel Entry Point |
| `0x04000000 - 0x04100000` | Process 1 Stack (1MB) |
| `0x04100000 - 0x04200000` | Process 2 Stack (1MB) |
| `0x04200000 - 0x04300000` | Process 3 Stack (1MB) |
| `0x04300000 - 0x04400000` | Process 4 Stack (1MB) |

## 🚀 Verifying Multitasking
Run the kernel and observe the serial/VGA output:
```text
[+] Enabling interrupts...
AAAAAAAAAA...BBBBBBBBBB...CCCCCCCCCC...DDDDDDDDDD...
```
The alternation between letters confirms that the timer is triggering the scheduler and processes are being swapped correctly.
