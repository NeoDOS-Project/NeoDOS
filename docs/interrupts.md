# Interrupt Handling

Multi-level interrupt architecture with IRQL framework, I/O APIC routing, MSI-X, DPC engine, and IPI infrastructure.

## IRQL Framework

File: `src/hal/x64/irql.rs`. Per-CPU interrupt request level mechanism replacing raw `cli`/`sti`. Current IRQL is stored in KPRCB at `GS:0x016` (byte offset).

### IRQL Levels

| Level | Value | Description | Prevents |
|-------|-------|-------------|----------|
| `PASSIVE` | 0 | Normal kernel and user code | Nothing |
| `APC` | 1 | Asynchronous procedure call delivery | APC delivery, page faults allowed |
| `DISPATCH` | 2 | DPC and scheduler | APC + page faults (INV-14: panic on #PF) |
| `DIRQL` | 3 | Device interrupt handler | All kernel-mode interrupts at same/lower level |
| `HIGH` | 15 | NMI / machine check | All interrupts |

### API

```rust
pub fn raise_irql(new_level: u8) -> u8;     // returns old IRQL
pub fn lower_irql(old_level: u8);            // restores previous IRQL
pub fn current_irql() -> u8;                 // reads current level
pub fn at_or_above_dispatch() -> bool;       // IRQL >= DISPATCH_LEVEL(2)
pub fn at_dispatch<F, R>(f: F) -> R;         // run closure at DISPATCH_LEVEL
```

`at_dispatch()` raises to DISPATCH, executes the closure, then restores:

```rust
at_dispatch(|| {
    // scheduler-critical section, IRQL >= DISPATCH
});
```

### IrqMutex

`IrqMutex<T>` wraps a `spin::Mutex<T>` with automatic IRQL management. Locking raises to DISPATCH_LEVEL; dropping the guard restores the previous level. Used for data structures shared between PASSIVE and DISPATCH contexts (e.g., work queues, scheduler globals, pipe buffers).

### Invariant INV-14

A page fault occurring while IRQL >= DISPATCH_LEVEL triggers a `BUGCHECK(KI_EXCEPTION_ACCESS_VIOLATION)`. All pageable data must be accessed only at PASSIVE or APC level.

### Migrated Paths

- `src/scheduler/work_queue.rs` (work queue global lock)
- Scheduler global helpers (run queue operations)
- `src/fs/pipe.rs` (pipe buffer access from DISPATCH context)

## I/O APIC

File: `src/interrupts/ioapic.rs`. Detected via MADT (ACPI Multiple APIC Description Table). Legacy PIC is masked and disabled during initialization.

### ISA IRQ Routing

| ISA IRQ | Device | Vector | I/O APIC Pin |
|---------|--------|--------|-------------|
| 0 | HPET / PIT timer | 32 | 2 |
| 1 | PS/2 keyboard | 33 | 1 |
| 4 | Serial port (COM1) | 36 | 4 |
| 12 | PS/2 mouse | 44 | 12 |

### API

```rust
pub fn init();                          // Parse MADT, disable PIC, route ISA IRQs
pub fn is_active() -> bool;             // I/O APIC initialized?
pub fn mask_irq(irq: u8);              // Mask a specific IRQ
pub fn unmask_irq(irq: u8);            // Unmask a specific IRQ
pub fn ioapic_addr() -> u64;           // MMIO base address of I/O APIC
pub fn ioapic_pin_count() -> u8;       // Number of redirection entries
```

MADT parsing in HPET module: `find_ioapic()` locates the I/O APIC structure; `get_isa_overrides()` returns ISA interrupt override entries.

## MSI-X

File: `src/interrupts/msi.rs`. Per-entry MSI-X table programming for PCI devices.

### Capability Detection

| Capability ID | Type |
|--------------|------|
| 0x05 | MSI (Message Signaled Interrupts) |
| 0x11 | MSI-X (MSI with extended table) |

### API

```rust
pub fn configure_msix_entry(
    bus: u8, dev: u8, func: u8,
    entry_index: u16,
    vector: u8,
);  // Program single MSI-X table entry

pub fn configure_msix_entries(
    bus: u8, dev: u8, func: u8,
    num_entries: u16,
    handler: fn(),
);  // Batch setup with auto-vector allocation
```

Each MSI-X table entry is 16 bytes: message address (64-bit), message data (32-bit), vector control (16-bit) + reserved. `configure_msix_entry` maps the MSI-X BAR, writes the table entry, and sets the function's MSI-X enable bit in the PCI capability register.

## DPC Engine

File: `src/dpc/mod.rs`. Deferred Procedure Calls for dispatching work at DISPATCH_LEVEL.

### Per-CPU Queues

128-entry SPSC (single-producer, single-consumer) ring buffer stored in `DPC_QUEUES[16]` (16 CPUs). Not stored in KPRCB to keep it within the 4096-byte limit.

```rust
pub fn insert_queue_dpc(fn_ptr: fn(ctx: u64), ctx: u64);
    // Producer (runs at DIRQL with interrupts off): SPSC push, no locks
pub fn dpc_dispatch_pending();
    // Consumer (runs at DISPATCH_LEVEL): drain queue
```

### Nesting Protection

`MAX_DPC_DEPTH = 10` prevents infinite recursion when DPCs enqueue other DPCs. Once depth is exceeded, further DPC dispatch is deferred to the next timer tick.

### Integration Points

- **Timer ISR exit** (`src/interrupts/idt.rs`): `dpc_dispatch_pending()` called after timer interrupt handler completes
- **Syscall return** (`clear_need_resched`): flush pending DPCs before returning to user mode

### Tests

| Test | Description |
|------|-------------|
| enqueue_dispatch | Single DPC enqueue and dispatch |
| irq_transition | IRQL transitions during DPC lifecycle |
| nesting_depth | Recursive DPC nesting limit enforcement |
| fifo_order | DPCs dispatched in FIFO order |
| stress_100_irqs | 100 IRQ injections with DPC dispatch |

## IPI Infrastructure

File: `src/arch/x64/ipi.rs`. Inter-processor interrupts via Local APIC ICR.

### IPI Vectors

| Vector | Name | Protocol | Description |
|--------|------|----------|-------------|
| 0xF0 | `IPI_RESCHEDULE` | Fire-and-forget | Sets per-CPU `need_resched` flag on target |
| 0xF1 | `IPI_TLB_SHOOTDOWN` | ACK protocol | Synchronous TLB invalidation on target CPUs |
| 0xF2 | `IPI_CALL_FUNCTION` | ACK protocol | Execute arbitrary function on remote CPUs |

### TLB Shootdown

```rust
pub fn tlb_shootdown(start: u64, end: u64, target_mask: u64);
```

Uses shared `TlbShootdownPayload` with atomic ack counter. Target CPUs execute `invlpg` per page (`start` to `end` with page stride), then decrement the counter. Sender spins until counter reaches zero.

### Call Function

```rust
pub fn call_function_all(func: fn(u64), arg: u64, target_mask: u64);
```

Uses `CallFunctionPayload` with atomic func pointer and ack counter. Target CPUs execute the function, then ACK. Sender spins on completion.

### Scheduler Integration

`enqueue_to_cpu_run_queue()` sends `IPI_RESCHEDULE` to the target CPU to trigger a reschedule.

### EOI

All vectors >= 32: `ack_irq()` sends End-Of-Interrupt to the Local APIC. The IPI vectors (0xF0-0xF2) are below 32 but still go through the APIC; their ISR handlers call `ack_irq()` explicitly.

## Source Files

| File | Responsibility |
|------|---------------|
| `src/hal/x64/irql.rs` | IRQL raise/lower, current_irql, at_dispatch, IrqMutex |
| `src/interrupts/ioapic.rs` | I/O APIC init, mask/unmask, MADT parsing |
| `src/interrupts/msi.rs` | MSI-X entry programming, batch setup |
| `src/dpc/mod.rs` | Per-CPU DPC queues, dispatch engine |
| `src/arch/x64/ipi.rs` | IPI vectors, TLB shootdown, call function |
| `src/interrupts/idt.rs` | IDT setup, timer ISR, DPC dispatch hook |
