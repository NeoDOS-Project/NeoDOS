# NeoDOS Architecture Source of Truth v1.0

> **Governance document.** This file defines architectural invariants that must never be violated.
> If code contradicts this document, the code is wrong — not the document.
> Every rule uses MUST / MUST NOT / ONLY IF language. Violations are architectural regressions.

---

## 1. System Overview

NeoDOS is a monolithic x86_64 kernel with a driver isolation layer, preemptive priority scheduler,
IRP-based async I/O, and a user-mode process model (Ring 3). It runs on UEFI (OVMF) with
bootloader → kernel → NeoInit (PID 1) startup sequence.

- **Kernel image**: ELF64, loaded at `0x200000`, identity-mapped.
- **User processes**: loads at `0x400000`, heap at `0x10000000`, mmap at `0x20000000`.
- **Drivers**: NEM format (`.nem`), loaded from NeoFS at boot or on demand.
- **NeoInit (PID 1)**: userland supervisor, started by kernel after boot.
- **Interrupts**: PIC → APIC, INT 0x80 for syscalls.
- **Command execution**: all interactive commands MUST run in Ring 3 as `.NXE`/`.BAT` user binaries. Ring 0 may only perform bootstrap, validation, loading, and kernel-internal maintenance; it MUST NOT host user-facing shell commands.

---

## 2. Core Invariants (HARD RULES)

**INV-1. NO CIRCULAR DEPENDENCIES BETWEEN SUBSYSTEMS.**
The dependency graph MUST be a DAG. The following couplings are hard-forbidden:

| Consumer | May NOT depend on |
|----------|-------------------|
| Scheduler | VFS, block drivers, AHCI/ATA, filesystems, event bus dispatch |
| IRQ handler | `schedule()`, VFS, heap allocation, pipe operations |
| Block driver | Scheduler, filesystems, other block drivers |
| BlockDevice trait | Scheduler, VFS, filesystems |
| Console | Scheduler, filesystems, drivers |
| Frame allocator | Scheduler, filesystems, drivers, VFS |
| Memory/paging | Scheduler, filesystems, drivers, VFS |
| HAL | Any kernel subsystem (HAL is the bottom layer) |

**INV-2. NO DYNAMIC ALLOCATION IN IRQ CONTEXT.**
IRQ handlers MUST NOT call `alloc`, `Box::new`, `Vec::push`, or any heap-allocating function.
Ring buffers and lock-free queues used in IRQ context MUST be pre-allocated at boot.

**INV-3. NO BLOCKING IN IRQ CONTEXT.**
IRQ handlers MUST NOT acquire spinlocks that can contend with non-IRQ context (use
`spin::Mutex` with IRQ-safe guard or `without_interrupts`). IRQ handlers MUST NOT call `schedule()`.

**INV-4. NO SCHEDULER INVOCATION FROM RING 0 (KERNEL) EXCEPT EXPLICIT SITES.**
The scheduler is invoked at exactly these points:
- Syscall return (when `NEED_RESCHED` is set)
- Timer tick preempting Ring 3 (CS=0x1B)
- `sys_yield` from Ring 3
- `sys_exit` (final reschedule)
The shell (Ring 0) MUST NOT invoke `schedule()`.

**INV-5. EVERY PHYSICAL FRAME HAS EXACTLY ONE OWNER AT ALL TIMES.**
Frame allocator hands out frames. A frame MUST NOT be mapped in two different page tables with
different identities unless explicitly coordinated (e.g., identity-mapped DMA buffer shared with
device). `free_frame()` MUST only be called by the owner.

**INV-6. EVERY PROCESS SLOT IS EITHER FREE OR VALID.**
No half-initialized slots. The scheduler recycles a slot only after `cleanup_terminated_process`
completes all resource freeing. After recycle, the slot's PID generation counter is incremented.

**INV-7. NO INTERRUPT-STACK EXECUTION OF SCHEDULER CODE.**
The scheduler, frame allocator, and slab allocator must never execute on the interrupt stack.
Only interrupt dispatch and IRQ handler bodies run on the interrupt stack.

**INV-8. KERNEL HEAP (0x01000000..0x02000000) MUST NOT BE IDENTITY-MAPPED AS USER-ACCESSIBLE.**
User processes must never be able to read or write kernel heap pages.

**INV-9. THE SYSCALL HANDLER IS THE ONLY GATE FROM RING 3 TO RING 0.**
No other interrupt or exception vector may transfer control from Ring 3 to Ring 0 with user-controlled
register state. The INT 0x80 handler is the sole entry point.

**INV-10. NeoInit (PID 1) MUST NEVER BE KILLED.**
`sched::kill_pid(1)` panics. `sys_exit` from PID 1 is equivalent to kernel panic.

**INV-11. NO USER-FACING COMMANDS IN RING 0.**
The legacy Ring 0 shell exists only as bootstrap glue. It MUST NOT expose or execute user-facing commands.
All commands intended for operator interaction MUST be implemented as Ring 3 `.NXE`/`.BAT` binaries under `userbin/`
and launched via NeoInit / neoshell.

---

## 3. Kernel Contract

### 3.1 Boot Sequence

The kernel boot sequence is a strict linear pipeline of phases:

```
PHASE 1     Serial init, GDT, IDT (early exceptions)
PHASE 2     PIC → APIC, PIT/HPET timer
PHASE 2.5   Heap init (buddy → slab → global allocator)
PHASE 3     Boot storage (A0 scan: NVMe > AHCI > ATA PIO), GPT parse
PHASE 3.5   Block cache init, NeoFS mount
PHASE 3.7   RamDisk init
PHASE 3.8   VFS init, working directory
PHASE 3.80  Driver isolation region init (0x30000000)
PHASE 3.85  Boot driver loader (from C:\System\Drivers, dependency-sorted)
PHASE 4     NeoInit loader: `cmd_run` starts PID 1 from `C:\Programs\NeoInit.nxe`
```

**Rule 3.1.1**: Phases MUST execute in order. No phase may run before its predecessor completes.
**Rule 3.1.2**: If Phase 3 (storage) fails, the system MUST panic (no fallback).
**Rule 3.1.3**: If NeoInit fails to load, the system MUST panic.
**Rule 3.1.4**: Driver load failures in Phase 3.85 are non-fatal — the driver is marked `Faulted`
and boot continues.

### 3.2 Kernel Address Space Layout

| Region | Base | Size | Owner |
|--------|------|------|-------|
| Kernel image | 0x200000 | ~1 MB | Kernel (read-only exec) |
| Kernel .rodata | 0x00100000 | ~1 MB | Kernel (read-only) |
| Kernel heap | 0x01000000 | 16 MB | Slab allocator (global) |
| User window | 0x400000 | 4 MB | User processes (code+stack) |
| User heap | 0x10000000 | 32 MB | Per-process (demand paged) |
| NXL region | 0x1E000000 | 2 MB | Shared libraries |
| mmap region | 0x20000000 | 32 MB | Per-process mmap |
| Driver isolation | 0x30000000 | 16 MB | NEM drivers (per-slot) |

**Rule 3.2.1**: All regions MUST be registered in `MemoryLayout` at boot. Overlap detection
MUST panic.
**Rule 3.2.2**: The identity map covers 0..4 GiB. Any region added outside 4 GiB requires separate
page table management.
**Rule 3.2.3**: User-accessible PTEs must only be set in user heap, user window, NXL, mmap, and
driver isolation regions.

### 3.3 Syscall Handler Integrity

**Rule 3.3.1**: `syscall_handler_asm` MUST save all callee-saved registers (RBX, R12–R15, RBP)
before dispatching to Rust code.
**Rule 3.3.2**: On `sys_exit` (RAX=0), the handler MUST NOT return through the normal dispatch —
it MUST jump to `exit_to_kernel` which restores `EXIT_RSP`/`EXIT_RIP` and callee-saved regs.
**Rule 3.3.3**: `validate_abi()` MUST be called at boot and panic if any syscall number is
unhandled or if error encoding is incorrect.
**Rule 3.3.4**: Return convention: `≥ 0` success, `< 0` error. This MUST NOT change.

---

## 4. Process Model Contract

### 4.1 Process States

```
Ready ──→ Running ──→ Ready        (preemption / yield)
Running ──→ Blocked                (pipe read, IRP wait, waitpid)
Blocked ──→ Ready                  (wake event)
Running ──→ Terminated             (sys_exit / kill)
Terminated ──→ (recycled)          (waitpid or cleanup_terminated_process)
```

**Rule 4.1.1**: Only `Running` processes may be on a CPU.
**Rule 4.1.2**: `Terminated` processes hold no resources except their scheduler slot
and kernel stack. Those are freed in `cleanup_terminated_process`.
**Rule 4.1.3**: `Blocked` MUST store `waiting_for: u64` to identify the wake source.

### 4.2 Handle Table

**Rule 4.2.1**: fds 0 (stdin), 1 (stdout), 2 (stderr) are reserved and always present.
**Rule 4.2.2**: `HandleEntry::Closed` slots can be reused. `HandleTable` grows dynamically.
**Rule 4.2.3**: Pipe handles carry reference counts. `sys_close` on a pipe fd decrements refcount.
Pipe is freed when refcount reaches 0.
**Rule 4.2.4**: File handles carry `offset: u64` for independent per-fd read/write positioning.
**Rule 4.2.5**: `sys_exit` MUST iterate the handle table and release all resource types (pipes
decrement refcount, files closed, devices detached).

### 4.3 User Process Memory

**Rule 4.3.1**: Code/stack is loaded at `0x400000` (flat binary) or at ELF-specified `p_vaddr`
(ELF binary). Max size: 4 MB (entire user window).
**Rule 4.3.2**: Heap grows from `PROCESS_HEAP_BASE` via demand paging. `sys_brk` adjusts the
break but pages are allocated on first access (page fault).
**Rule 4.3.3**: `heap_free_range` MUST be called on `sys_exit` to free all heap frames.
**Rule 4.3.4**: `mmap_free_range` MUST be called on `sys_exit` to free all mmap frames.

---

## 5. Memory Ownership Rules

**Rule 5.1**: The frame allocator (`buddy.rs`) owns the free frame metadata (free lists + bitmap).
Allocated frames are owned by the allocator client until `free_frame` is called.

**Rule 5.2**: A frame allocated for a user process heap page is owned by that process. It is
freed ONLY by `heap_free_page` or `heap_free_range` during process exit.

**Rule 5.3**: A frame allocated for an mmap page is owned by the process's VMA list entry.
It is freed ONLY by `mmap_free_range` or `sys_munmap`.

**Rule 5.4**: Slab-allocated objects (`Box`, `Vec`, `String`) are owned by the allocating
subsystem. Cross-subsystem ownership transfer requires explicit documentation.

**Rule 5.5**: IRP structures are owned by the allocator (global pool `irp_alloc`/`irp_free`).
An IRP is live from `irp_alloc` through `irp_complete` and callback dispatch. After callback,
the IRP is freed and MUST NOT be accessed.

**Rule 5.6**: The kernel heap (`0x01000000..0x02000000`) is exclusively managed by
`SlabAllocator` (small objects) and `linked_list_allocator::LockedHeap` (large objects).
No other code may allocate from this region.

**Rule 5.7**: Driver isolation slots (`0x30000000` per slot) are owned by the isolation layer.
Each slot's memory is freed ONLY by `free_driver_slot` / `free_isolated_range`.

---

## 6. Scheduler & Execution Rules

### 6.1 Priority Model

| Level | Constant | Time Slice | Purpose |
|-------|----------|-----------|---------|
| 0 | `PRIORITY_HIGH` | 400 ticks | System-critical processes |
| 1 | `PRIORITY_ABOVE_NORMAL` | 200 ticks | Important user processes |
| 2 | `PRIORITY_NORMAL` | 100 ticks | Default |
| 3 | `PRIORITY_IDLE` | 50 ticks | Background only |

**Rule 6.1.1**: `schedule()` scans HIGH → IDLE, round-robin within the same level.
**Rule 6.1.2**: A RUNNING process at a higher priority starves all lower levels.
**Rule 6.1.3**: Aging MUST boost priority of any Ready process not scheduled in ≥ 1000 ticks.

### 6.2 Preemption

**Rule 6.2.1**: Only Ring 3 (user mode) is preempted. The kernel (Ring 0) runs to completion
except at explicit reschedule points (syscall return, yield).
**Rule 6.2.2**: `timer_handler_inner` reads CS from the interrupt stack frame. If CS=0x1B
(user mode), it saves RSP, calls `schedule()`, and updates TSS.RSP0. If CS=0x08 (kernel mode),
it returns without scheduling.

### 6.3 Process Slot Management

**Rule 6.3.1**: The scheduler has a fixed maximum number of process slots (`MAX_PROCESSES`).
**Rule 6.3.2**: Each slot has a `pid_gen: u32` counter incremented on recycle. This prevents
use-after-free of stale PIDs.
**Rule 6.3.3**: `cleanup_terminated_process` is called exactly once per process lifecycle:
after `sys_exit` and before slot recycle.

---

## 7. IRP / I/O System Contract

### 7.1 IRP Lifecycle

```
irp_alloc() → Pending
  → submit to device queue
  → device processes → irp_complete() → Completed / Error
  → callback dispatched via work queue (sync path) or APC (async alertable path)
  → irp_free() (automatic after callback)
```

**Rule 7.1.1**: An IRP MUST NOT be accessed after `irp_complete` dispatches its callback.
**Rule 7.1.2**: Chaining (`chain_next`) MUST be set before the parent IRP is submitted.
**Rule 7.1.3**: The device driver calls `irp_complete` exactly once per IRP.
**Rule 7.1.4**: `irp_sync_read` / `irp_sync_write` are synchronous wrappers that block the
calling process. They MUST NOT be called from IRQ context.
**Rule 7.1.5**: `irp_complete_with_apc` delivers the IRP completion callback via the
target thread's user APC queue (DIRQL → DPC → APC flow). The callback runs at PASSIVE_LEVEL
in user context before the next user-mode instruction.

### 7.2 BlockDevice Trait

```rust
trait BlockDevice {
    fn submit_irp(&mut self, irp_id: IrpId) -> Result<(), DeviceError>;
    fn poll_irp(&mut self, irp_id: IrpId) -> Option<IrpStatus>;
}
```

**Rule 7.2.1**: `submit_irp` MUST NOT block. It enqueues and returns immediately.
**Rule 7.2.2**: `poll_irp` MUST return `None` if the IRP is still pending.
**Rule 7.2.3**: Every implementor (RamDisk, BootAta, AhciDriver, NvmeDriver, NemBlockDevice)
MUST call `irp_get_params` before processing and `irp_complete_result` on completion.
**Rule 7.2.4**: No block device implementation may call `schedule()`, allocate memory, or
access filesystems.

### 7.3 IRP Pool

**Rule 7.3.1**: The global pool has 64 slots. `irp_alloc` returns `None` if exhausted.
**Rule 7.3.2**: IDs are sequential via `AtomicU32` — wrap-around MUST NOT produce IDs
that collide with live IRPs. (Pool size 64 vs 32-bit ID space makes this safe.)
**Rule 7.3.3**: `irp_get_params` returns a snapshot struct to avoid double-lock deadlock
— callers MUST NOT hold the pool lock when calling this.

---

## 8. NEM Driver ABI Contract

### 8.1 Header Format (v3)

| Offset | Size | Field | Valid Range |
|--------|------|-------|-------------|
| 0 | 4 | magic | `b"NEM\0"` |
| 4 | 4 | version | 3 |
| 8 | 2 | header_size | 48 |
| 10 | 2 | driver_type | [0, 3] |
| 12 | 4 | entry_offset | < code_size |
| 16 | 4 | code_size | ≥ 1, ≤ MAX_DRIVER_SIZE |
| 20 | 2 | compat_flags | any |
| 22 | 2 | abi_min | 1..=ABI_MAX_VALID |
| 24 | 2 | abi_target | ABI_MIN_VALID..=ABI_MAX_VALID |
| 26 | 2 | abi_max | ABI_MIN_VALID..=ABI_MAX_VALID |
| 28 | 1 | category | 0 (Boot), 1 (System), 2 (Demand) |
| 29 | 3 | reserved | zero |
| 32 | 16 | name | ASCII, null-terminated |

### 8.2 Lifecycle States (W2 Hot Reload compatible)

```
Loaded ──→ Initialized ──→ Registered ──→ Bound ──→ Active
Active ──→ Unloading ──→ Unloaded ──→ Loaded (reload loop)
Any ──→ Faulted
Any ──→ Unloaded
```

**Rule 8.2.1**: Only the transitions listed above are valid. All others produce
`TransitionError`.
**Rule 8.2.2**: A driver is `Active` ONLY IF:
- State == `Bound` (all prior transitions passed)
- `last_error == ERR_NONE`
- Not `Faulted`
**Rule 8.2.3**: `certify_and_activate` runs the full check. If any condition fails,
state stays unchanged and `last_error = ERR_CERTIFICATION_FAILED`.

### 8.3 ABI Negotiation

**Rule 8.3.1**: A driver is compatible iff ALL of:
- `driver.abi_min ≥ 1`
- `driver.abi_max ≥ ABI_MIN_VALID`
- `driver.abi_min ≤ ABI_MAX_VALID`
- `ABI_MIN_VALID ≤ driver.abi_target ≤ ABI_MAX_VALID`

**Rule 8.3.2**: If driver is compatible but `driver.abi_max < ABI_TARGET`, the kernel
issues `CompatibleWithWarnings("Driver ABI predates kernel target...")`.
**Rule 8.3.3**: If incompatible, the driver MUST NOT be loaded.

### 8.4 Capability System

**Rule 8.4.1**: Every `hst_*` export function MUST call `check_cap(required_cap)` before
executing. If denied, return error sentinel (0, -1, or no-op).
**Rule 8.4.2**: Category defaults:
- BOOT → `CAP_ALL` (all 11 flags)
- SYSTEM → `CAP_PORTIO | CAP_IRQ | CAP_MMIO | CAP_DMA | CAP_EVENT_BUS | CAP_INPUT | CAP_LOG | CAP_TIMING`
- DEMAND → `CAP_EVENT_BUS | CAP_LOG | CAP_TIMING`

**Rule 8.4.3**: SYSTEM may request additional caps via `EVENT_CAP_ESCALATION` (type `0x2000`).
DEMAND MUST NOT escalate.
**Rule 8.4.4**: `current_driver_id()` tracks the active driver. It is set before
`driver_init`/activate/event callbacks and cleared after return.

### 8.5 Isolation Layer

**Rule 8.5.1**: `DRIVER_ISO_BASE` = `0x30000000`, size = `0x1000000` (16 MB).
**Rule 8.5.2**: `MAX_ISOLATED_DRIVERS` = 16, `DRIVER_SLOT_SIZE` = `0x100000` (1 MB).
**Rule 8.5.3**: `validate_driver_ptr` accepts only:
- Driver's own isolated slot
- Kernel heap (`0x01000000..0x02000000`)
- Kernel .rodata/.text (`0x00100000..0x01000000`)
- User heap (`0x10000000..0x12000000`)
- mmap region (`0x20000000..0x22000000`)
- User code (`0x400000..0x800000`)
- Kernel image (`0x200000..PHYS_MEM_END`)

All other addresses are rejected.
**Rule 8.5.4**: Isolation mode `Sandbox` (DEMAND drivers) marks the driver `FAULTED` on any
page fault within its slot. `None` and `Basic` ignore the fault.

---

## 9. Event Bus Rules

### 9.1 Event Structure

```rust
#[repr(C)]
struct Event {
    event_id: u64,
    event_type: u32,
    source: u8,        // SOURCE_HAL, SOURCE_DRIVER, SOURCE_KERNEL, SOURCE_USERLAND
    timestamp: u32,
    device_id: u32,
    data0: u64,
    data1: u64,
    flags: u16,
};
```

### 9.2 Queue Architecture

**Rule 9.2.1**: Two priority queues:
- **High** (16 slots) — timers, IRQ completions
- **Normal** (64 slots) — system events, keyboard, disk

**Rule 9.2.2**: High queue is drained first. Normal queue is drained only when high is empty.
**Rule 9.2.3**: Both queues are lock-free SPSC ring buffers. Push from IRQ context is safe.
**Rule 9.2.4**: If a queue is full, `push_event` returns `Err(())` (backpressure). The
producer MUST handle this.

### 9.3 Dispatch

**Rule 9.3.1**: `dispatch_pending()` is called from:
1. `clear_need_resched()` on every syscall return
2. The idle loop (before HLT)
3. The shell input loop

**Rule 9.3.2**: No driver callback is executed in IRQ context. All callbacks run in
kernel thread context (syscall return or idle loop).
**Rule 9.3.3**: Recursive dispatch is forbidden. If `dispatch_pending` is called while
dispatch is already in progress, it MUST return immediately.

### 9.4 Subscription

**Rule 9.4.1**: Maximum 64 handlers. New subscription past 64 returns error.
**Rule 9.4.2**: `EventFilter` supports filtering by `event_type`, `source_mask` (bitfield),
and `device_id`.
**Rule 9.4.3**: Unsubscription is by callback pointer (`unregister_handler`) or by name
(`unregister_handler_by_name`).

---

## 10. NeoInit (PID 1) Authority Model

### 10.1 Startup Contract

**Rule 10.1.1**: NeoInit is loaded from `C:\Programs\NeoInit.nxe` at Phase 4.
**Rule 10.1.2**: NeoInit is the only process that starts at boot. All other user processes
are descendants of NeoInit.
**Rule 10.1.3**: NeoInit receives argv `["/Programs/NeoInit.nxe"]` and inherits fds 0/1/2
pointing to the kernel console.

### 10.2 Privileges

**Rule 10.2.1**: NeoInit MAY:
- Create pipes (`sys_pipe`)
- Spawn child processes via `cmd_run` or equivalent
- Redirect child fds via `sys_dup2` before spawn
- Wait for any process (`sys_waitpid`)
- Receive SIGCHLD equivalent when children exit

**Rule 10.2.2**: NeoInit MUST NOT:
- Call `sys_exit` (this panics the kernel — INV-10)
- Be killed by `kill_pid(1)` (this panics)
- Have its priority reduced below `PRIORITY_HIGH` by user command

### 10.3 Respawning Policy

**Rule 10.3.1**: If NeoInit crashes (page fault, GPF), the kernel panics. There is no
respawn for PID 1.
**Rule 10.3.2**: NeoInit SHOULD respawn its direct children if they exit (unless the exit
was intentional).

---

## 11. TTY / Terminal Model Contract

### 11.1 TTY Lifecycle

```
Unclaimed ──→ Claimed (driver binds via EventBus)
Claimed ──→ Active (input/output flow enabled)
Active ──→ Drained (device disconnect / driver unload)
Drained ──→ Unclaimed (resources cleaned)
```

**Rule 11.1.1**: A TTY is identified by `device_id` matching the keyboard/graphics driver.
**Rule 11.1.2**: Input flow: PS/2 IRQ1 → lock-free ring buffer (1024 bytes) → event bus
(`KEYBOARD_INPUT` type) → TTY subscriber → shell input loop.
**Rule 11.1.3**: Output flow: shell write → `hst_write(device_id, buf)` → framebuffer/serial.
**Rule 11.1.4**: There is exactly one active TTY at any time. Switching requires event
(`KEYB_LAYOUT` type 9) or explicit command.

### 11.2 Keyboard Layout

**Rule 11.2.1**: The PS/2 keyboard driver stores scan code → ASCII tables generated at
build time from `.klc` files. Two layouts: US (index 0), SP (index 1, default).
**Rule 11.2.2**: Layout switching is via Event Bus (`EVENT_KEYB_LAYOUT` type 9) sent by
the `KEYB` shell command.
**Rule 11.2.3**: The input system produces sentinel bytes `0x01` (up arrow) and `0x02`
(down arrow) for history navigation.

---

## 12. Syscall ABI Contract

### 12.1 Calling Convention

```
RAX = syscall number
RBX = arg0
RCX = arg1
RDX = arg2
R8  = arg3
R9  = arg4
Return: RAX (≥ 0 success, < 0 error)
```

**Rule 12.1.1**: This convention is frozen. New syscalls MUST use this convention.
**Rule 12.1.2**: Error is encoded as negative `u64`. `err_to_u64(SyscallError::NoEnt)`
produces `0xFFFF_FFFF_FFFF_FFFE`. User code checks `cmp rax, -1`.
**Rule 12.1.3**: `validate_abi()` at boot confirms every SSDT entry (0..=MAX_SYSCALL) has a handler
and error encoding is correct.

### 12.1.1 SSDT Architecture

The syscall dispatch uses a centralized indexed table (SSDT) instead of a monolithic match:

```rust
pub static SYSCALL_TABLE: [Option<SyscallFn>; 256]  // handler dispatch
pub static SYSCALL_PERMISSIONS: [SyscallPermission; 256]  // parallel permissions
```

**Rule 12.1.1.1**: All syscalls 0..=MAX_SYSCALL MUST have entries in both tables.
**Rule 12.1.1.2**: Adding a new syscall requires: 1 handler function + 1 SSDT entry + 1 permission entry.
**Rule 12.1.1.3**: Unknown syscalls (None in SSDT) return ENOSYS without panic.

### 12.2 Syscall Table

| RAX | Name | Signature | Stability |
|-----|------|-----------|-----------|
| 0 | `exit` | `(code)` | STABLE |
| 1 | `write` | `(fd, buf, len)` | STABLE |
| 2 | `yield` | `()` | STABLE |
| 3 | `getpid` | `()` | STABLE |
| 4 | `read` | `(fd, buf, count)` | STABLE |
| 5 | `pipe` | `(fds)` | STABLE |
| 6 | `dup2` | `(old, new)` | STABLE |
| 7 | | (reserved) | |
| 8 | | (reserved) | |
| 9 | `waitpid` | `(pid)` | STABLE |
| 10 | `open` | `(path, flags)` | STABLE |
| 11 | `readfile` | `(fd, buf, count)` | STABLE |
| 12 | `writefile` | `(fd, buf, count)` | STABLE |
| 13 | `close` | `(fd)` | STABLE |
| 14 | | (reserved) | |
| 15 | | (reserved) | |
| 16 | `chdir` | `(path)` | STABLE |
| 17 | `getcwd` | `(buf, len)` | STABLE |
| 18 | `brk` | `(new_break)` | STABLE |
| 19 | `mmap` | `(hint, len, prot, flags, fd)` | STABLE |
| 20 | `munmap` | `(addr, len)` | STABLE |
| 21 | `loadlib` | `(path)` | STABLE |
| 22 | `thread_create` | `(entry, stack)` | STABLE |
| 23 | `thread_join` | `(tid)` | STABLE |
| 50 | `ndreg` | `()` | ADMIN-ONLY |

**Rule 12.2.1**: Reserved slots (7, 8) MUST NOT be assigned without a breaking
change version bump.
**Rule 12.2.2**: Adding a new syscall at the next available RAX is NOT a breaking change.
**Rule 12.2.3**: Changing the signature, return convention, or semantics of a STABLE syscall
IS a breaking change.

---

## 13. Failure & Recovery Rules

### 13.1 Process Crash

**Rule 13.1.1**: If a user process (Ring 3) triggers a page fault, general protection fault,
or any other exception:

1. The exception handler in `idt.rs` calls `syscall_dispatch` with RAX=0 (forced exit).
2. Resources (heap, mmap, handles, pipes) are freed.
3. `waitpid` unblocks the parent (if any) with the exit code.
4. The scheduler slot is recycled.

**Rule 13.1.2**: An exception in Ring 0 is always a kernel panic.
**Rule 13.1.3**: The only exception to 13.1.2 is a page fault in the driver isolation region
with `Sandbox` mode — the driver is marked `Faulted` but the kernel continues.

### 13.2 Driver Failure

**Rule 13.2.1**: If a driver crashes (page fault, GPF) in its isolation slot:
- `Sandbox` mode: driver → `Faulted`, `handle_isolated_page_fault` returns true.
- `Basic` or `None` mode: kernel panics.

**Rule 13.2.2**: A `Faulted` driver can only transition to `Unloaded`.
**Rule 13.2.3**: `NDREG UNLOAD` on a Faulted driver cleans up isolation slot, unregisters
from event bus, and marks `Unloaded`.

### 13.3 OOM Policy

**Rule 13.3.1**: If the frame allocator cannot satisfy a request, the kernel panics.
No OOM killer.
**Rule 13.3.2**: If `irp_alloc` returns `None` (pool exhausted), the caller receives
`Err(ERR_OUT_OF_MEMORY)` propagated to user as `-ENOMEM`.
**Rule 13.3.3**: If the slab allocator cannot grow (heap frame allocation fails), the
kernel panics.

### 13.4 Blocking Pipe Read Timeout

**Rule 13.4.1**: A process reading from an empty pipe with the write end still open
transitions to `Blocked { waiting_for: 0xFFFF_0000 | pipe_id }`.
**Rule 13.4.2**: On pipe write, `wake_pipe_readers()` scans scheduler processes and
transitions Blocked→Ready.
**Rule 13.4.3**: There is no timeout on pipe reads. If the writer never writes, the
reader blocks indefinitely.

---

## 14. Forbidden Behaviors (ANTI-PATTERNS)

**AP-1**: ATA or AHCI driver calling `schedule()` or any VFS function. (Block driver
MUST NOT depend on scheduler or filesystem.)

**AP-2**: Timer IRQ handler calling `schedule()` when interrupted code is Ring 0.
(Only Ring 3 may be preempted.)

**AP-3**: An IRQ handler allocating memory (`Box::new`, `Vec::push`, `String::push`, etc.).
(No heap allocation in IRQ context — INV-2.)

**AP-4**: A kernel subsystem calling `schedule()` from a spinlock-protected region.
(Can deadlock if scheduler needs the same lock.)

**AP-5**: A NEM driver accessing memory outside its validated region.
(`validate_driver_ptr` MUST catch this.)

**AP-6**: The shell invoking `schedule()` directly. (Shell runs in Ring 0; scheduling
happens only on syscall return or timer tick while in Ring 3.)

**AP-7**: Two subsystems writing to the same physical frame without coordination.
(Every frame has exactly one owner — INV-5.)

**AP-8**: A pipe reader busy-looping on `-EAGAIN` in user space without yielding.
(Userspace MUST call `sys_yield` or accept the `-EAGAIN` retry in the `read` syscall.)

**AP-9**: Calling `sys_exit(pid)` with PID != current process. (There is no kill-other
syscall; use `KILL` command or `kill_pid` internal.)

**AP-10**: Registering a new `BlockDevice` implementor that depends on filesystem types.
(`BlockDevice` trait must not pull in `NeoDosFs`, `Fat32`, or any filesystem.)

---

## 15. Versioning & Breaking Change Policy

### 15.1 Version Format

The kernel version follows `MAJOR.MINOR.PATCH`:
- **MAJOR**: ABI-breaking changes (syscall semantics change, NEM ABI change, IRP format change).
- **MINOR**: Feature additions without breaking ABI (new syscall at next RAX, new driver,
  new shell command).
- **PATCH**: Bug fixes, refactoring, documentation, test additions.

### 15.2 What Counts as Breaking

The following changes are ALWAYS breaking (MAJOR bump):

| Component | Breaking Change |
|-----------|----------------|
| Syscall ABI | Change any syscall number, signature, or return convention |
| Syscall ABI | Change error encoding (negative u64) |
| NEM header | Change field offset, size, or interpretation |
| NEM ABI | Change ABI_MIN_VALID / ABI_MAX_VALID |
| IRP format | Change `#[repr(C)]` struct layout |
| Event struct | Change `#[repr(C)]` struct layout |
| Capability flags | Change `CAP_*` bit positions |
| Driver lifecycle | Add/remove states or transition rules |
| Memory layout | Change base address of a registered region |
| Process states | Add/remove states or transition rules |
| Boot phases | Reorder, remove, or add mandatory phases between existing ones |

**Rule 15.2.1**: Breaking changes MUST be documented in `CHANGELOG.md` with a `### Breaking`
header.
**Rule 15.2.2**: New syscalls at the next available RAX are NOT breaking.
**Rule 15.2.3**: New event types are NOT breaking.
**Rule 15.2.4**: Increasing `MAX_PROCESSES`, pool sizes, or queue depths are NOT breaking.

---

## 16. Testable Invariants (MANDATORY TESTS)

Every invariant below MUST have a corresponding automated test in `testing.rs`.
The test suite MUST be run before every release.

| # | Invariant | Test type | What to assert |
|---|-----------|-----------|----------------|
| T1 | INV-1: No circular dep | Static analysis | `scripts/check_deps.py` exits 0 |
| T2 | INV-2: No alloc in IRQ | Code review + test | IRQ handlers never call heap alloc. Test IRQ handler list. |
| T3 | INV-4: Scheduler not invoked from Ring 0 shell | Functional | Shell process priority stays unchanged across timer ticks |
| T4 | INV-5: Frame has one owner | Unit | Allocate frame, read bitmap, free, confirm bitmap cleared |
| T5 | INV-6: Process slots valid | Unit | Create process, read slot state, terminate, confirm recycled |
| T6 | INV-8: Kernel heap not user-accessible | Functional | Try reading kernel heap from user mode → page fault |
| T7 | INV-10: Kill PID 1 panics | Unit | Call `kill_pid(1)` → expect panic |
| T8 | Scheduler aging | Unit | Ready process with 1000+ ticks unscheduled → priority boosted |
| T9 | Scheduler priority | Unit | Higher-priority process always scheduled before lower |
| T10 | IRP lifecycle | Unit | Alloc → complete → callback → freed. Double-complete fails. |
| T11 | IRP pool exhaustion | Unit | Alloc 65 IRPs → 65th returns None |
| T12 | NEM ABI negotiation | Unit | Valid, invalid, warning scenarios (10 cases) |
| T13 | Driver lifecycle transitions | Unit | All valid transitions pass; all invalid produce TransitionError |
| T14 | Capability enforcement | Unit | Check denied hst call returns error sentinel |
| T15 | Isolation pointer validation | Unit | Valid pointers accepted, invalid rejected (12 test cases) |
| T16 | Event bus backpressure | Unit | Fill queue → 65th push returns Err |
| T17 | Event bus dispatch order | Unit | High queue events dispatched before normal |
| T18 | Process handle cleanup on exit | Unit | Open pipes/files → exit → refcounts decremented, handles recycled |
| T19 | Demand paging | Unit | Allocate page → read/write succeeds without pre-allocation |
| T20 | mmap lazy allocation | Unit | Register VMA → page fault on access → frame allocated |
| T21 | Pipe blocking/wake | Unit | Read empty pipe → blocked → writer writes → reader unblocked |
| T22 | No preempt in Ring 0 | Functional | Long kernel loop → no reschedule until syscall return |
| T23 | Syscall ABI validation at boot | Unit | `validate_abi()` panics on missing handler |
| T24 | NeoFS permission enforcement | Unit | Read-only file → write → denied |
| T25 | Boot phase ordering | Unit | Phase N+1 cannot execute before Phase N completes |
| T26 | Slab allocator per-size stress | Unit | Alloc/free all sizes, mix sizes, large fallback, reuse |
| T27 | Buddy allocator merge | Unit | Alloc adjacent blocks, free both, alloc larger order succeeds |
| T28 | KOBJ unregister edge cases | Unit | Double unregister, lookup after unregister, refcount zero |
| T29 | Mmap region bounds | Unit | Out-of-bounds addresses rejected by VMA add |
| T30 | Hot reload driver lifecycle | Unit | Unload → reload → Active again, resources tracked |

---

## Appendix A: Architecture Compliance Checklist

Before any commit, verify:

- [ ] `scripts/check_deps.py` passes (T1)
- [ ] `python3 scripts/auto_test.py` passes (all 320+ kernel tests + user-mode tests)
- [ ] `cargo build` in `neodos-kernel/` compiles without warnings
- [ ] No new `use` statements that create forbidden dependencies (INV-1)
- [ ] No new heap allocation in IRQ handlers (INV-2)
- [ ] No new `schedule()` call outside allowed sites (INV-4)
- [ ] Any new syscall added at the next available RAX (Section 12)
- [ ] Any new driver passes ABI negotiation (Section 8.3)
- [ ] `CHANGELOG.md` updated if this commit changes user-visible behavior
- [ ] `AGENTS.md` updated if adding syscalls, commands, or driver categories
