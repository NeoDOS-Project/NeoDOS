# IPC Subsystem

## Pipes (src/pipe.rs)

### Architecture

Dynamic pipe storage using `Vec<Option<Mutex<PipeInner>>>` (since v0.41). Each `PipeInner` contains a boxed ring buffer (`Box<[u8; 4096]>`), read/write cursors, and reference counts.

Reference-counted: auto-freed when all reader/writer file descriptors are closed. `pipe_close()` decrements refcount; the buffer is freed when it reaches 0.

### System Calls

- **sys_pipe** (RAX 5, **REMOVED**): Was pipe creation. Replaced by `ob_create` + pipe fd.
- **sys_close** (RAX 13) on a pipe fd: Removes the handle entry, decrements refcount. Calls `pipe_free()` if refcount reaches 0.
- **sys_dup2** (RAX 6): Copies an fd with refcount increment. Both old and new fd share the same pipe buffer.

### Blocking Reads

When a pipe buffer is empty: reader sets `ThreadState::Blocked { waiting_for: 0xFFFF_0000 | pipe_id }`, sets `NEED_RESCHED` flag, and returns `-EAGAIN`. The scheduler will not schedule the thread again until the blocking condition is cleared.

After a writer writes data, it calls `wake_pipe_readers(pipe_id)` which scans the scheduler's thread list for any thread blocked on that pipe and sets them back to `Ready` state.

### Pipe Manager API

```rust
pub fn pipe_alloc() -> Option<PipeId>;
pub fn pipe_write(pipe_id: PipeId, buf: &[u8]) -> Result<usize, i64>;
pub fn pipe_read(pipe_id: PipeId, buf: &mut [u8]) -> Result<usize, i64>;
pub fn pipe_close(pipe_id: PipeId, is_writer: bool);
```

- `pipe_write` returns `-EPIPE` (-32) if no readers remain.
- `pipe_read` returns `0` (EOF) if no writers remain and buffer empty.
- Both return number of bytes transferred on success.

## Handle Table (src/handle.rs)

### Structure

Per-EPROCESS `HandleTable` stores a `Vec<HandleEntry>`. Grows dynamically; no maximum fd limit beyond memory.

### HandleEntry Variants

| Variant | Payload |
| --------- | --------- |
| `Closed` | None |
| `Stdin` | None |
| `Stdout` | None |
| `Stderr` | None |
| `PipeReader(id)` | PipeId |
| `PipeWriter(id)` | PipeId |
| `ObObject(ob_id, access_mask, offset)` | ObId, AccessMask for security, u64 file offset |

### FD Layout

- FD 0: stdin (Stdin)
- FD 1: stdout (Stdout)
- FD 2: stderr (Stderr)
- FD 3+: allocated by `alloc_handle()` via O(n) scan for first `Closed` slot

### File Handle Behavior

File handles carry a per-open `offset` field (u64) allowing independent read/write positioning for each fd referencing the same ObObject. `sys_read`/`sys_write` advance the offset; `sys_ob_set_info(Seek)` or `ob_query_info` can reposition.

### Cleanup on Exit

`sys_exit` iterates all HandleEntry values and:

- Closes pipe readers/writers (decrements refcount)
- Calls `ob_close` on ObObject handles

## IRP System (src/irp/mod.rs)

### Data Structures

**IrpOp** (u8 enum):

- `Read(0)`, `Write(1)`, `Flush(2)`, `Discard(3)`, `Ioctl(4)`

**IrpStatus** (u8 enum):

- `Pending(0)`, `Completed(1)`, `Error(2)`

**Irp** (`#[repr(C)]` struct):

- `op: IrpOp` — operation type
- `buffer: *mut u8` — data buffer
- `length: usize` — buffer length
- `lba: u64` — logical block address (for block devices)
- `count: u64` — sector count (for block devices)
- `status: IrpStatus` — completion status
- `callback: Option<fn(ctx: *mut u8, status: IrpStatus)>` — completion callback
- `ctx: *mut u8` — opaque callback context
- `chain_next: Option<IrpId>` — chained IRP to dispatch on completion
- `waiting_pid: u64` — PID waiting on this IRP, or 0

### Global Pool

64 IRP slots with sequential IDs via `AtomicU32`. `irp_alloc()` returns `IrpId` (1-indexed u32). `irp_free(id)` returns to pool.

### IrpQueue

Per-device FIFO ring buffer of 32 entries. Used by block device drivers to queue operations. `irp_queue_push(queue, id)` enqueues; `irp_queue_pop(queue)` dequeues for processing.

### Completion Flow

1. Device driver calls `irp_complete(id, status)`.
2. Sets `Irp::status` to `Completed` or `Error`.
3. Wakes waiting thread: `waiting_pid` matched against scheduler; thread state restored from `Blocked` to `Ready`.
4. Dispatches callback via `WORK_QUEUE.push_high(callback_fn, ctx)` if callback exists.

### Scheduler Integration

`irp_complete` calls `irp_wake_waiter(id)` to find and unblock the thread waiting on the IRP.

### BlockDevice Trait

```rust
pub trait BlockDevice {
    fn submit_irp(&self, irp_id: IrpId);
    fn poll_irp(&self, irp_id: IrpId) -> Option<IrpStatus>;
}
```

5 implementors:

- `RamDisk` — synthetic in-memory disk
- `BootAta` — legacy ATA PIO driver
- `AhciDriver` — AHCI SATA driver
- `NvmeDriver` — NVMe driver
- `NemBlockDevice` — NEM module wrapping a block device

### Tests

11 unit tests covering alloc/free, status transitions, error codes, queue FIFO ordering, queue wraparound, and callback dispatch ordering.

## Deferred Work Queue (src/work_queue.rs)

### Architecture

Two-level priority queue:

- **High-priority**: Processed on syscall return (in `clear_need_resched()`), before returning to user mode.
- **Low-priority**: Processed in idle loop, before executing `HLT` instruction.

### Implementation

Lock-free SPSC (Single Producer, Single Consumer) ring buffer with 64 slots per level.

**WorkEntry**: `(fn(*mut u8), *mut u8)` — function pointer and opaque data argument.

`pending: AtomicBool` flag avoids unnecessary processing when queue is empty.

### API

```rust
impl WorkQueue {
    pub fn push_high(&self, func: WorkFn, data: *mut u8) -> Result<(), ()>;
    pub fn push_low(&self, func: WorkFn, data: *mut u8) -> Result<(), ()>;
    pub fn process_high(&self) -> usize;   // returns count processed
    pub fn process_low(&self) -> usize;    // returns count processed
}
```

`push_*` returns `Err(())` if queue is full (backpressure handling).

### Integration Points

- **Syscall return**: `clear_need_resched()` → `WORK_QUEUE.process_high()
- **Idle loop**: Before HLT → `WORK_QUEUE.process_low(); WORK_QUEUE.process_high()
- **Timer interrupt**: Periodic low-priority drain

Used for DPC dispatch, event bus delivery, background I/O completion callbacks.

### Tests

6 tests: push/pop ordering, FIFO semantics, empty queue, overflow behavior, high/low isolation, pending flag correctness.

## Event Bus v2 (src/eventbus/mod.rs)

### Event Structure

`#[repr(C)]` 56-byte struct with ABI-stable layout for NEM driver compatibility:

| Field | Type | Description |
| ------- | ------ | ------------- |
| `event_id` | u64 | Unique event ID |
| `event_type` | u32 | Event type constant |
| `source` | u32 | Event source |
| `timestamp` | u64 | TSC timestamp |
| `device_id` | u32 | Device identifier |
| `driver_target` | u32 | Targeted driver ID |
| `data0` | u64 | Payload word 0 |
| `data1` | u64 | Payload word 1 |
| `flags` | u32 | Event flags |

### Event Type Constants

| Constant | Value | Description |
| ---------- | ------- | ------------- |
| `TIMER_TICK` | 0 | Periodic PIT/HPET timer |
| `KEYBOARD_INPUT` | 1 | PS/2 or USB key event |
| `SERIAL_DATA` | 2 | Serial port data available |
| `DISK_IO_COMPLETE` | 3 | Disk IRP completion |
| `PROCESS_EXIT` | 4 | Process terminated |
| `DRIVER_LOADED` | 5 | NEM driver loaded |
| `DRIVER_CRASH` | 6 | NEM driver fault |
| `POLICY_VIOLATION` | 7 | Security policy check failed |
| `FS_MOUNTED` | 8 | Filesystem mounted |
| `KEYB_LAYOUT` | 9 | Keyboard layout changed |
| `EVENT_RTC_READ` | 10 | RTC read request |
| `EVENT_RTC_DATA` | 11 | RTC data ready |
| `EVENT_SHUTDOWN` | 12 | System shutdown in progress |
| `EVENT_DRIVER_UNLOAD` | 13 | Driver unload request |
| `EVENT_DRIVER_UNLOAD_ACK` | 14 | Driver unload acknowledgment |
| `EVENT_NMI_WATCHDOG` | 15 | NMI watchdog timeout |
| `EVENT_MOUSE_INPUT` | 16 | PS/2 mouse raw bytes |
| `EVENT_NETWORK_PACKET` | 17 | NIC received a packet |
| `EVENT_PS2_SHUTDOWN` | 18 | PS/2 shutdown request |
| `EVENT_PM_SUSPEND` | 19 | Power management suspend |
| `EVENT_PM_RESUME` | 20 | Power management resume |
| `EVENT_PM_BATTERY` | 21 | Battery status change |
| `EVENT_PM_THERMAL` | 22 | Thermal event |
| `EVENT_PM_DISPLAY` | 23 | Display power state |
| `EVENT_PM_IDLE` | 24 | System idle notification |
| `EVENT_PM_POWER_BUTTON` | 25 | Power button pressed |
| `EVENT_PM_SLEEP_BUTTON` | 26 | Sleep button pressed |
| `EVENT_KEYDOWN` | 27 | Key pressed (scancode in data0) |
| `EVENT_KEYUP` | 28 | Key released (scancode in data0) |
| `EVENT_KEY_CHAR` | 29 | Character typed (Unicode codepoint in data0) |
| `EVENT_KBD_MODIFIER` | 30 | Modifier state change (new mods byte in data0) |
| `EVENT_KBD_REPEAT` | 31 | Key repeat event |
| `USER` | 0x1000+ | User-defined event types |

### Event Sources

- `SOURCE_HAL(0)`
- `SOURCE_DRIVER(1)`
- `SOURCE_KERNEL(2)`
- `SOURCE_USERLAND(3)`

### Delivery Queues

Two lock-free SPSC ring buffers:

- **High priority**: 16 slots (for time-sensitive events like keyboard input)
- **Normal priority**: 64 slots (for general events like disk completion)

### Subscription Filters

`register_handler_v2(filter, callback, name)` with `EventFilter` struct:

```rust
pub struct EventFilter {
    pub event_type: u16,       // exact match, or 0 for any
    pub source_mask: u16,      // bitmask of SOURCE_* values
    pub device_id: u32,        // exact match, or 0 for any
}
```

Max 64 handlers. Unregister by callback pointer (`unregister_handler(callback)`) or by name (`unregister_handler_by_name(name)`).

### Dynamic Payload

`push_event_with_dyn_payload()` allocates a copy of arbitrary data and stores it as `data0/data1` (padded to 16 bytes). Auto-freed after all handlers have been dispatched.

### Backpressure

If the target queue is full, `push_event()` and `push_event_with_dyn_payload()` return `Err(())`. The caller must decide retry or drop. No events are silently dropped by the event bus itself.

### Dispatch

- `dispatch_one()`: Dispatch a single event from high queue (preferred) or normal queue.
- `dispatch_pending()`: Drain all pending events (high first, then normal).

Called from:

- `clear_need_resched()` — high-priority dispatch on syscall return
- Idle loop — drain all pending before HLT
- Shell input loop — periodic dispatch during user interaction

### IRQ Integration

- `TimerTick` events pushed from PIT/HPET timer interrupt handler.
- `KeyboardInput` events pushed from PS/2 IRQ1 handler.
- Both use normal priority queue.

### Tests

17 tests covering: v2 queue operations, subscription filter matching, dynamic payload lifecycle, backpressure when queues full, priority dispatch ordering (high before normal), handler registration/unregistration, and simultaneous multi-handler delivery.
