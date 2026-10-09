---
name: ipc
description: Modify pipes, handle table, IRP system, work queue, or event bus
---

# IPC Subsystem

## When to use

Modifying pipes, the per-process handle table, the IRP system, the deferred work
queue, or the event bus.

## Goal

Implement IPC primitives correctly with reference counting, blocking semantics,
lock-free queues, and IRQ-safe patterns.

## References

- `docs/kernel/ipc.md` — subsystem documentation
- `src/object/pipe.rs` — `PipeManager`, pipe buffer, blocking reads
- `src/infra/handle.rs` — per-process `HandleTable` / `HandleEntry`
- `src/irp/mod.rs` — IRP alloc/free, `IrpQueue`, `BlockDevice` trait, completion
- `src/infra/work_queue.rs` — high/low SPSC queues, DPC dispatch
- `src/eventbus/mod.rs` — event types, filters, push/dispatch

## Architecture

### Pipes (`src/object/pipe.rs`)

`PipeManager { pipes: Mutex<Vec<Option<Mutex<PipeInner>>>>, kobj_ids: ... }`.
`MAX_PIPES = 16`; pipe IDs are `u8`. Each pipe has a boxed `[u8; 4096]` ring
buffer, read/write cursors, and reference counts. Freed when all reader/writer
fds close.

```rust
pub fn pipe_peek_read_ready(pipe_id: u8) -> Option<bool>;
pub fn block_current_for_pipe(pipe_id: u8);
```

- `write` → `Err(())` if the read end is closed or the buffer is full.
- `read` → `Ok(0)` (EOF) if the write end is closed and empty; `Err(())` if no
  data is available.

`sys_pipe` was removed; pipes are created with `ob_create(Pipe)` (RAX 41) which
returns a read/write fd pair.

**Blocking reads**: the reader sets
`ThreadState::Blocked { waiting_for: 0xFFFF_0000 | pipe_id }` and
`NEED_RESCHED`; the writer calls `wake_pipe_readers(pipe_id)` to unblock it.

### Handle table (`src/infra/handle.rs`)

Per-`EPROCESS` `HandleTable` (`Vec<HandleEntry>`), growing dynamically;
`alloc_handle()` returns `None` past 255 entries (fds are `u8`).

```rust
pub struct HandleEntry { pub object_id: ObId, pub offset: u64 }
```

Sentinels: `HANDLE_CLOSED=0`, `HANDLE_STDIN=ObId::MAX`,
`HANDLE_STDOUT=ObId::MAX-1`, `HANDLE_STDERR=ObId::MAX-2`. FDs 0/1/2 are
stdin/stdout/stderr; 3+ are allocated. File handles carry a per-open `offset`.
`sys_exit` walks the table: closes pipe ends (decrementing refcount) and calls
`ob_close` on Ob handles.

### IRP system (`src/irp/mod.rs`)

64-slot global pool (sequential `AtomicU32` IDs); per-device FIFO `IrpQueue` (32
entries).

`IrpOp` = `Read(0)`, `Write(1)`, `Flush(2)`, `Discard(3)`, `Ioctl(4)`;
`IrpStatus` = `Pending(0)`, `Completed(1)`, `Error(2)`.

```rust
pub trait BlockDevice {
    fn submit_irp(&self, irp_id: IrpId);
    fn poll_irp(&self, irp_id: IrpId) -> Option<IrpStatus>;
}
```

Implementors: `RamDisk`, `BootAta`, `AhciDriver`, `NvmeDriver`, `NemBlockDevice`.
`irp_complete()` sets status, wakes the `waiting_pid` thread, and pushes the
callback onto the high-priority work queue.

### Work queue (`src/infra/work_queue.rs`)

Two-level lock-free SPSC ring buffers (64 slots each): **high** processed on
syscall return (`clear_need_resched()`), **low** in the idle loop before `HLT`.

```rust
pub fn push_high(&self, func: WorkFn, data: *mut u8) -> Result<(), ()>;
pub fn push_low(&self, func: WorkFn, data: *mut u8) -> Result<(), ()>;
pub fn process_high(&self) -> usize;
pub fn process_low(&self) -> usize;
```

`push_*` returns `Err(())` on a full queue (backpressure).

### Event bus v2 (`src/eventbus/mod.rs`)

`#[repr(C)]` 56-byte `Event` (ABI-stable for NEM drivers). Two SPSC queues: high
16 slots, normal 64 slots. Max 64 handlers.

Event types `0..=32` are assigned; **0-15 are ABI-frozen** (v0.42) — start new
types at `33` (or use `USER = 0x1000+`). Sources: `SOURCE_HAL(0)`,
`SOURCE_DRIVER(1)`, `SOURCE_KERNEL(2)`, `SOURCE_USERLAND(3)`.

```rust
pub fn push_event(event: &Event, priority: EventPriority) -> Result<(), ()>;
pub fn push_event_with_dyn_payload(event: &Event, payload: &[u8]) -> Result<(), ()>;
pub fn dispatch_one() -> bool;
pub fn dispatch_pending();
pub fn register_handler_v2(filter: EventFilter, callback: EventCallback, name: &str);
```

`EventFilter { event_type: u16, source_mask: u16, device_id: u32 }`. Unregister by
callback (`unregister_handler`) or name (`unregister_handler_by_name`). Dynamic
payloads are auto-freed after all handlers dispatch.

## Steps

### 1. Modify pipe behavior

Check readers before writing; implement blocking on empty:

```rust
if pipe.read_refs == 0 { return Err(()); }      // no readers
if pipe.used() == 0 && pipe.write_refs > 0 {
    block_current_for_pipe(pipe_id);
    // set NEED_RESCHED and return
}
```

### 2. Add a handle type

Handles are Ob-based — add an `ObType` / `ObOperations` per the object-manager
skill rather than a new `HandleEntry` field. Make sure `sys_close`/`sys_exit`
cleanup releases it.

### 3. Allocate and submit an IRP

```rust
let irp_id = irp_alloc().expect("IRP pool exhausted");
let irp = irp_get_mut(irp_id);
irp.op = IrpOp::Read; irp.buffer = buf.as_mut_ptr(); irp.length = buf.len();
irp.lba = block_lba; irp.count = sectors; irp.waiting_pid = current_pid;
device.submit_irp(irp_id);
```

### 4. Push work

```rust
WORK_QUEUE.push_high(my_callback, data_ptr);
// IRQ handlers MUST use pre-allocated structures — no heap allocation.
```

### 5. Add an event type / handler

```rust
pub const EVENT_MY_CUSTOM: EventType = 33;   // next free after 32
eventbus::push_event(&Event::new(EVENT_MY_CUSTOM, SOURCE_KERNEL, 0, 42, 0),
                     EventPriority::Normal);

let filter = EventFilter { event_type: EVENT_MY_CUSTOM,
                           source_mask: 1 << SOURCE_KERNEL, device_id: 0 };
register_handler_v2(filter, my_handler_fn, "my_handler");
```

### 6. Tests

Register with `test_case!` via each subsystem's `register_*_tests()` (pipe, IRP,
work queue, event bus), called from `register_tests()` in `src/testing.rs`.

## Best practices

- Pair pipe `alloc` with `close` on both ends (refcounting).
- Check `irp_alloc()` for exhaustion; never touch an IRP after `irp_complete()`.
- Single producer per lock-free queue; handle full-queue `Err(())` as backpressure.
- IRQ handlers must not allocate — use pre-allocated ring buffers.
- Do not modify ABI-frozen event types (0-15) or the `#[repr(C)]` Event layout.
- Call `dispatch_pending()` from the idle loop so events do not accumulate.

## Common mistakes

- Forgetting `wake_pipe_readers()` after a write.
- Leaking one pipe end (buffer never freed).
- IRP use-after-free after completion.
- Heap allocation in IRQ context.
- Queue overflow ignored (events/work silently lost from the caller's view).
- Using the old `src/handle.rs` / `src/work_queue.rs` paths — they are
  `src/infra/handle.rs` and `src/infra/work_queue.rs`.

## Final checklist

- [ ] Pipe alloc/write/read/close refcounting correct
- [ ] Blocking reads unblock when a writer writes
- [ ] Handle table alloc/free/dup + exit cleanup works
- [ ] IRP alloc → submit → complete → free cycle tested
- [ ] IRQ path uses the work queue (no heap alloc)
- [ ] Event push → dispatch → handler works; backpressure handled
- [ ] Tests registered and passing (`neodev test`)
- [ ] `docs/kernel/ipc.md` updated for new event types/ops
- [ ] `cargo build` + `neodev check-deps` pass
