# NeoDOS Scheduler

The NeoDOS scheduler is a simple Round-Robin multitasking system designed for kernel-space processes.

## 📋 Process Control Block (PCB)
Each process is represented by a `Process` struct:
- **Registers**: Full CPU context saved on every interrupt.
- **Stack**: Dedicated 1MB stack region per process.
- **State**: `Ready`, `Running`, `Blocked`, or `Terminated`.
- **PID**: Unique process identifier.

## 🔄 Scheduling Algorithm: Round-Robin
1. The scheduler maintains an array of processes.
2. Every 10ms (on timer interrupt), the current process is set to `Ready`.
3. The scheduler increments `current_pid` and searches for the next `Ready` process in the table.
4. If a process is found, it is set to `Running` and its stack pointer is returned to the context switcher.

## ⚙️ Context Switch Mechanics
The switch is performed in assembly to ensure no register state is lost:
- **Save**: `push rbp, r15, r14, r13, r12, r11, r10, r9, r8, rdi, rsi, rdx, rcx, rbx, rax`
- **Switch**: `mov rsp, rax` (where RAX is the new stack pointer from the scheduler)
- **Restore**: `pop rax, rbx, rcx, rdx, rsi, rdi, r8, r9, r10, r11, r12, r13, r14, r15, rbp`
- **Return**: `iretq`
