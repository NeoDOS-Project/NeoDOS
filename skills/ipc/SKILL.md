---
name: ipc
description: Modify pipes, handle table, IRP system, work queue, or event bus
---

# IPC Subsystem

## When to use

Modifying pipes, the per-process handle table, the IRP (I/O Request Packet) system, the deferred work queue, or the event bus dispatch.

## Goal

Correctly implement IPC primitives with proper reference counting, blocking semantics, lock-free queues, and IRQ-safe patterns.

## References

- `docs/kernel/ipc.md` — subsystem documentation
- `src/object/pipe.rs` — pipe buffer, alloc/write/read/close, blocking reads
- `src/handle.rs` — per-process HandleTable, HandleEntry, FD management
- `src/irp/mod.rs` — IRP alloc/free, IrpQueue, BlockDevice trait, completion flow
- `src/work_queue.rs` — high/low priority SPSC queues, DPC dispatch
- `src/eventbus/mod.rs` — event types, subscription filters, push/dispatch

## Subsystem Architecture

### Pipes (`src/object/pipe.rs`)

Global `Vec<Option<Mutex<PipeInner>>>` (max 16 pipes, v0.41+). Each pipe has a `Box<[u8; 4096]>` ring buffer, read/write cursors, and reference counts.

API:
```rust
pub fn pipe_alloc() -> Option<PipeId>;                 // allocate new pipe
pub fn pipe_write(pipe_id: PipeId, buf: &[u8]) -> Result<usize, i64>;  // returns -EPIPE if no readers
pub fn pipe_read(pipe_id: PipeId, buf: &mut [u8]) -> Result<usize, i64>; // returns 0 (EOF) if no writers
pub fn pipe_close(pipe_id: PipeId, is_writer: bool);   // decrement refcount, free at 0
pub fn pipe_register_tests();                          // 5+ unit tests
```

**Blocking reads**: reader sets `ThreadState::Blocked { waiting_for: 0xFFFF_0000 | pipe_id }`, writer calls `wake_pipe_readers()` to unblock.

### Handle Table (`src/handle.rs`)

Per-EPROCESS `HandleTable` (Vec\<HandleEntry\>). FD layout:

| FD | Type |
| -- | ---- |
| 0 | Stdin |
| 1 | Stdout |
| 2 | Stderr |
| 3+ | Allocated via `alloc_handle()` |

```rust
pub struct HandleEntry { object_id: ObId, offset: u64 }
```

Sentinel values: `HANDLE_CLOSED(0)`, `HANDLE_STDIN(ObId::MAX)`, `HANDLE_STDOUT(ObId::MAX-1)`, `HANDLE_STDERR(ObId::MAX-2)`.

Cleanup on `sys_exit`: closes pipes (decrements refcount), calls `ob_close` on Ob handles.

### IRP System (`src/irp/mod.rs`)

64-slot global pool with sequential IDs. Per-device FIFO `IrpQueue` (32 entries).

```rust
pub struct Irp { op: IrpOp, buffer: *mut u8, length: usize, lba: u64, count: u64, 
                 status: IrpStatus, callback: Option<fn(ctx: *mut u8, status: IrpStatus)>,
                 ctx: *mut u8, chain_next: Option<IrpId>, waiting_pid: u64 }
```

`IrpOp`: `Read(0)`, `Write(1)`, `Flush(2)`, `Discard(3)`, `Ioctl(4)`.

`BlockDevice` trait:
```rust
pub trait BlockDevice { fn submit_irp(&self, irp_id: IrpId); fn poll_irp(&self, irp_id: IrpId) -> Option<IrpStatus>; }
```

Implementors: `RamDisk`, `BootAta`, `AhciDriver`, `NvmeDriver`, `NemBlockDevice`.

### Work Queue (`src/work_queue.rs`)

Two-level lock-free SPSC ring buffer (64 slots each):

- **High priority**: processed on syscall return (`clear_need_resched()`)
- **Low priority**: processed in idle loop (before HLT)

```rust
pub fn push_high(&self, func: WorkFn, data: *mut u8) -> Result<(), ()>;
pub fn push_low(&self, func: WorkFn, data: *mut u8) -> Result<(), ()>;
pub fn process_high(&self) -> usize;
pub fn process_low(&self) -> usize;
```

### Event Bus v2 (`src/eventbus/mod.rs`)

56-byte `#[repr(C)]` event struct with ABI-stable layout. Two priority queues (high 16 slots, normal 64 slots).

```rust
pub struct Event { event_id: u64, event_type: u32, source: u32, timestamp: u64,
                   device_id: u32, driver_target: u32, data0: u64, data1: u64, flags: u32 }
```

32+ event types (0=TimerTick, 1=KeyboardInput, ..., 31=KeyRepeat, 0x1000+=user).

Event sources: `SOURCE_HAL(0)`, `SOURCE_DRIVER(1)`, `SOURCE_KERNEL(2)`, `SOURCE_USERLAND(3)`.

```rust
pub fn push_event(event: &Event, priority: EventPriority) -> Result<(), ()>;
pub fn push_event_with_dyn_payload(event: &Event, payload: &[u8]) -> Result<(), ()>;
pub fn dispatch_one() -> bool;
pub fn dispatch_pending();
pub fn register_handler_v2(filter: EventFilter, callback: EventCallback, name: &str);
```

## Steps

### 1. Modify pipe behavior

```rust
// In pipe_write() — check for readers before writing
if pipe.read_refs == 0 { return Err(-32); } // -EPIPE

// In pipe_read() — implement blocking
if pipe.used() == 0 && pipe.write_refs > 0 {
    current_thread().state = ThreadState::Blocked { waiting_for: BLOCKED_PIPE | pipe_id };
    NEED_RESCHED.store(true, Ordering::SeqCst);
    return Err(-EAGAIN);
}
```

### 2. Add a new handle type

Add variant to `HandleEntry` or use `object_id` + `offset` convention. Register in `sys_close` cleanup path.

### 3. Allocate and submit an IRP

```rust
let irp_id = irp_alloc().expect("IRP pool exhausted");
let irp = irp_get_mut(irp_id);
irp.op = IrpOp::Read;
irp.buffer = buf.as_mut_ptr();
irp.length = buf.len();
irp.lba = block_lba;
irp.count = sector_count;
irp.waiting_pid = current_pid;

device.submit_irp(irp_id);
// … block until IRP completes …
```

### 4. Implement a BlockDevice

```rust
impl BlockDevice for MyDevice {
    fn submit_irp(&self, irp_id: IrpId) {
        // push to hardware queue, process in ISR or poll
        let irp = irp_get_mut(irp_id);
        // do I/O
        irp_complete(irp_id, IrpStatus::Completed);
    }

    fn poll_irp(&self, irp_id: IrpId) -> Option<IrpStatus> {
        Some(irp_get(irp_id).status)
    }
}
```

### 5. Push work to the work queue

```rust
fn my_callback(ctx: *mut u8) {
    let data = unsafe { &*(ctx as *const MyData) };
    // process…
}

WORK_QUEUE.push_high(my_callback, data_ptr);
```

### 6. Add a new event type

```rust
// Define constant (in src/eventbus/mod.rs or a driver header)
pub const EVENT_MY_CUSTOM: EventType = 32; // next available after 31

// Push event
let event = Event::new(EVENT_MY_CUSTOM, SOURCE_KERNEL, 0, 42, 0);
eventbus::push_event(&event, EventPriority::Normal);
```

### 7. Register an event handler

```rust
let filter = EventFilter { event_type: EVENT_MY_CUSTOM, source_mask: 1 << SOURCE_KERNEL, device_id: 0 };
register_handler_v2(filter, my_handler_fn, "my_handler");
```

### 8. Write tests

Tests are registered via `register_tests()` in each subsystem file and called from `src/testing.rs::register_tests()`.

## Best practices

- Pipes use reference counting — always pair `alloc` with `close` (reader and writer sides).
- IRP pool has 64 slots — check `irp_alloc()` return for exhaustion.
- Work queue and event bus use lock-free SPSC — single producer per queue.
- IRQ handlers MUST NOT allocate — use pre-allocated ring buffers.
- Event types 0–15 are ABI-frozen (v0.42). New types start at 16+.
- `irp_complete()` wakes blocked threads — it can be called from IRQ context.
- Use `push_event_with_dyn_payload()` for events with dynamic data (auto-freed after dispatch).

## Common mistakes

- Forgetting `wake_pipe_readers()` after pipe write — readers stay blocked forever.
- Pipe fd leak: not calling `close` on both read and write ends — pipe buffer never freed.
- IRP use-after-free: accessing IRP after `irp_complete()` frees it back to the pool.
- IRQ context heap allocation — IRQ handlers must use pre-allocated structures.
- Event bus queue full: `push_event` returns `Err(())` — caller must handle backpressure.
- Not calling `dispatch_pending()` in idle loop — events queue up and never get processed.
- Modifying ABI-frozen event types (0–15) or the `#[repr(C)]` Event struct layout.

## Final checklist

- [ ] Pipe alloc/write/read/close cycle works with proper refcounting
- [ ] Blocking reads unblock correctly when writer writes
- [ ] Handle table alloc/free/dup works, cleanup on exit
- [ ] IRP alloc → submit → complete → free cycle tested
- [ ] IRQ handler pushes work to work queue (no heap alloc)
- [ ] Event bus push → dispatch → handler invocation works
- [ ] Backpressure handled for full queues (event bus, work queue)
- [ ] Tests registered and pass (`neodev test`)
- [ ] `docs/kernel/ipc.md` updated for new event types, IRP ops, or handle variants
- [ ] `cargo build` succeeds, `scripts/check_deps.py` passes
