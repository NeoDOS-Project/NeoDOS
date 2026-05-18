# NeoDOS Kernel Validation & Regression Framework

## Philosophy

NeoDOS must evolve toward a kernel that can continuously prove its own stability.
Every architectural invariant must be checked at runtime in debug-validation builds.
Every panic must be classified and recorded.
Every regression must be caught automatically.

> A feature without validation is incomplete.
> An invariant without enforcement is unreliable.
> A panic without forensic information is a tooling failure.

---

## Invariants Being Tested

### 1. Scheduler Invariants

| Invariant | Check | Failure |
|-----------|-------|---------|
| No two processes share the same PID | `add_ring3_process` scans all PIDs | `SCHED_PID_COLLISION` |
| Idle (PID 0) is never picked when non-idle processes exist | `schedule()` scans all PIDs > 0 first | `SCHED_PICKED_IDLE` |
| Running state is unique | Before scheduling, assert current is Running | `SCHED_MULTIPLE_RUNNING` |
| Terminated processes never get rescheduled | `schedule()` skips Terminated | `SCHED_RESCHED_TERMINATED` |
| Kernel stack never exceeds allocated bounds | Stack guard pages | `SCHED_STACK_OVERFLOW` |
| Process state transitions are legal | `Ready→Running→Ready/Blocked→Terminated` | `SCHED_ILLEGAL_TRANSITION` |

### 2. IRQ Invariants

| Invariant | Check | Failure |
|-----------|-------|---------|
| No nested interrupt reentrancy (single-core) | Atomic nesting counter in IRQ entry | `IRQ_NESTED_ENTRY` |
| Timer handler never context-switches | `timer_handler_inner` must not set `NEED_RESCHED` when in Ring 0 | `IRQ_TIMER_CONTEXT_SWITCH` |
| Interrupts are disabled during critical sections | Assert `IF=0` in scheduler lock regions | `IRQ_IF_LEAKED` |
| Double fault handler always has a valid stack | IST-index verified at GDT setup | `IRQ_DF_NO_STACK` |

### 3. Syscall ABI Invariants

| Invariant | Check | Failure |
|-----------|-------|---------|
| Arguments are valid user pointers | `is_user_ptr_valid` on all buffer args | `SYSCALL_INVALID_PTR` |
| Syscall number is within dispatch table | `syscall_dispatch` validates RAX < 20 | `SYSCALL_INVALID_NUMBER` |
| Return value encoding is consistent | All syscalls return `u64` | `SYSCALL_ABI_DRIFT` |
| `EXIT_RSP`/`EXIT_RIP` are never clobbered | Atomic guard on usermode entry | `SYSCALL_CLOBBERED_STATE` |

### 4. Memory Invariants

| Invariant | Check | Failure |
|-----------|-------|---------|
| Heap allocator is not double-locked | Mutex poisoning detection | `MEM_ALLOC_DEADLOCK` |
| Page table mappings are identity-mapped in kernel range | Assert on page walk | `MEM_PT_MISMATCH` |
| User slot boundaries never overlap | `alloc_user_slot` asserts unique | `MEM_SLOT_OVERLAP` |
| Framebuffer memory is never allocated from heap | Separate framebuffer region | `MEM_FB_LEAK` |

### 5. BlockDevice Invariants

| Invariant | Check | Failure |
|-----------|-------|---------|
| `base_lba + read_lba` does not overflow partition bounds | ATA driver validates against GPT partition | `BLK_LBA_OVERFLOW` |
| Block cache is flushed within `FLUSH_INTERVAL_TICKS` | Atomic timestamp check in timer | `BLK_CACHE_STALE` |

---

## Expected Scheduler Behavior

1. **Rescheduling** only happens at two points:
   - `syscall_handler_asm` epilogue (after `clear_need_resched()` returns true)
   - `timer_handler_asm` (when current PID = 0/idle)
2. The timer handler marks `Running` → `Ready` every 100 ticks.
3. `schedule()` always scans `current_pid + 1` through `MAX_PROCESSES - 1`, wrapping to 1 before falling back to PID 0.
4. A process that calls `sys_exit` is immediately `Terminated` and is never rescheduled.
5. Context switch in `syscall_try_resched` saves RSP and returns the next process's RSP.

## IRQ/Syscall Interaction Guarantees

1. IRQs are disabled during `syscall_handler_asm` (via `disable_interrupts(true)`).
2. `syscall_try_resched` re-enables interrupts after context switching.
3. The timer handler never calls `schedule()` — it only sets `NEED_RESCHED`.
4. Nested IRQs are not possible on single-core (IF cleared by hardware on IRQ entry).
5. Double fault is always on IST stack — the regular kernel stack is preserved for debugging.

---

## Panic Classification System

### Classification Categories

| Code | Category | Trigger |
|------|----------|---------|
| `STACK_CORRUPTION` | Stack overflow or misaligned stack pointer | GPF with RSP in guard page range |
| `INVALID_IRETQ` | IRETQ with wrong number of stack items | GPF at `iretq` instruction |
| `IRQ_REENTRANCY` | IRQ handler entered while another IRQ active | Nesting counter > 0 at IRQ entry |
| `ABI_MISMATCH` | Incompatible module or bootloader version | Header parse failure |
| `PAGE_TABLE_CORRUPTION` | Invalid page table entry or mapping | Page fault with protection violation |
| `DOUBLE_FAULT` | Exception during exception handling | Double fault entry |
| `INVALID_CONTEXT_SWITCH` | Context switch from illegal state | Schedule from non-Running process |
| `SCHED_PANIC` | Scheduler state machine violation | Illegal state transition detected |
| `MEMORY_CORRUPTION` | Heap allocator metadata corruption | Allocator panic |
| `UNKNOWN_CPU_EXCEPTION` | Exception without specific handler | Reserved vector |

### Classification Algorithm

```
On panic:
  1. Read RIP from panic location (or exception frame)
  2. Match RIP against known addresses:
     - IRETQ instruction → INVALID_IRETQ
     - scheduler code → SCHED_PANIC
     - allocator code → MEMORY_CORRUPTION
     - usermode entry/exit → INVALID_CONTEXT_SWITCH
  3. If in exception handler:
     - double_fault_handler → DOUBLE_FAULT
     - page_fault_handler + protection violation → PAGE_TABLE_CORRUPTION
     - gpf_handler → check error code + RIP
  4. Fallback: UNSPECIFIED_PANIC
```

## Regression Policy

1. **Zero-tolerance for panic regressions**: Any new panic cause must be classified and added to the validation suite.
2. **Intermittent failures are bugs**: A test that fails 1 in 100 iterations is a test failure. The failing iteration must be investigated.
3. **Validation is mandatory**: Every commit must pass:
   - `cargo build` (dev + release)
   - `python3 scripts/regression_runner.py` (100 iterations minimum)
   - `python3 scripts/check_deps.py`
4. **Trace before panic**: The trace ring buffer is dumped on every panic (debug-validation builds).
5. **ABI compatibility**: All `.ndm` and `.bin` binaries must pass ABI validation before execution.

---

## Validation Build Profiles

| Profile | Cargo flags | Assertions | Trace | Stress tests |
|---------|------------|------------|-------|-------------|
| `release` | default | Core invariants only | Off | Off |
| `debug-validation` | `--features validation` | All invariants | Full ring buffer | Available |
| `stress-validation` | `--features stress` | All invariants + extra | Full ring buffer | Active during boot |

Enable with:
```bash
cargo build --features validation
# or in config.toml:
# [build]
# features = ["validation"]
```

---

## Trace Buffer Specification

- Ring buffer of 1024 entries, lock-free, in a dedicated `.traces` section (no heap).
- Each entry: tick (u64), event kind (u8), arg0-3 (u64 each) = 40 bytes total.
- Events: CONTEXT_SWITCH (0x01), SYSCALL_ENTER (0x02), SYSCALL_EXIT (0x03),
  IRQ_ENTER (0x04), IRQ_EXIT (0x05), SCHED_DECISION (0x06), PANIC (0xFF).
- Dumped automatically on panic.
- Readable post-mortem from serial log.

---

## File Structure

```
neodos-kernel/src/
├── trace.rs                  # Ring-buffer trace logging
├── invariants.rs             # Runtime invariant validation
├── panic_classification.rs   # Panic signature classification
├── main.rs                   # Enhanced panic handler
├── testing.rs                # + stress test suites
├── scheduler.rs              # + trace points + invariant checks
├── syscall.rs                # + ABI validation + trace points
├── arch/x64/idt.rs           # + enhanced exception handlers
└── module_abi.rs             # + ABI regression checks

scripts/
├── regression_runner.py      # 100+ iteration test runner
├── auto_test.py              # Existing single-run test runner
└── check_deps.py             # Existing dependency validator
```

---

## Forensic Dump Format (on panic)

```
!!! KERNEL PANIC (CLASS: {classification}) !!!
Location: {file}:{line}
Message: {message}
RIP: {rip}  RSP: {rsp}  RFLAGS: {rflags}
CR2: {cr2}  (if page fault)
Error code: {error_code}  (if exception with error code)

Trace buffer (last 16 entries):
  [{tick}] {event} arg0={:#x} arg1={:#x} arg2={:#x} arg3={:#x}
  ...

Scheduler state:
  Current PID: {pid}  State: {state}
  Ready: [{pids}]  Blocked: [{pids}]
  Timer ticks: {ticks}

Stack dump:
  {16 words above RSP}
```
