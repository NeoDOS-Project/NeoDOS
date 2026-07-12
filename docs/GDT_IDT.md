# NeoDOS GDT & IDT

This document summarizes the segmentation/interrupt setup as implemented under `neodos-kernel/src/arch/x64/`.

## Global Descriptor Table (GDT)

NeoDOS implements a 64-bit GDT with support for Ring 0 (Kernel) and Ring 3 (User) segments.

| Index | Selector | Type | DPL |
| ------- | ---------- | ------ | ----- |
| 0 | `0x00` | Null | - |
| 1 | `0x08` | Kernel Code | 0 |
| 2 | `0x10` | Kernel Data | 0 |
| 3 | `0x18` | User Code | 3 |
| 4 | `0x20` | User Data | 3 |
| 5 | `0x28` | TSS | 0 |

### Task State Segment (TSS)

The TSS is used to provide a "clean" stack for critical exceptions like Double Faults. This prevents a system crash if the kernel stack is corrupted.

## Interrupt Descriptor Table (IDT)

The IDT maps 256 interrupt vectors to their respective handlers.

### Exception Vectors (0-31)

| Vector | Name | Type | Handler |
| -------- | ------ | ------ | --------- |
| 0 | Divide Error | Fault | Panic |
| 6 | Invalid Opcode | Fault | Panic |
| 8 | Double Fault | Abort | Panic (IST0) |
| 13 | GPF | Fault | Panic |
| 14 | Page Fault | Fault | Panic |

### Hardware IRQs (32-47)

The PIC is remapped to avoid collisions with exceptions.

- **32 (IRQ0)**: System Timer (Context Switch)
- **33 (IRQ1)**: PS/2 Keyboard
