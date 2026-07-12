# Test Framework

## Overview

In-kernel test harness. No external test runner required. 656 tests across 50+ suites compiled directly into the kernel image. Tests execute in kernel mode and can exercise all subsystems including privileged operations.

Two execution paths:

1. **Boot-time**: `test` shell command (built-in) calls `testing::run_all()` — prints PASS/FAIL per test, returns summary count.
2. **Automated**: `python3 scripts/auto_test.py` boots QEMU headless with serial output, waits for test completion line, parses PASS/FAIL counts from serial console.

## Registration

Global test list in `src/testing.rs`. Each subsystem exports a `register_*_tests()` function (e.g., `register_neofs_tests()`). The central `testing::register_tests()` function calls all module registration functions to populate the static test array.

## Macro API

```rust
test_case!("my_test", {
    // test body
    test_eq!(a, b);
    test_ne!(a, b);
    test_true!(cond);
    test_fail!("message");
});
```

- `test_case!(name, { body })`: Registers a named test. Panic in body = FAIL. Clean exit = PASS.
- `test_eq!(a, b)`: Asserts a == b, panics with values on mismatch.
- `test_ne!(a, b)`: Asserts a != b, panics on equality.
- `test_true!(cond)`: Asserts condition is true.
- `test_fail!(msg)`: Always panics with formatted message (for testing error paths).

## Execution

`testing::run_all()` iterates the global test array, runs each test in sequence, captures panics (tests run with `catch_unwind`), prints `PASS: <name>` or `FAIL: <name> [reason]` to serial. Returns `(total, passed, failed)`. The `test` built-in shell command prints a summary line: `TESTS: X total, Y passed, Z failed`.

`auto_test.py` waits for a regex match on the final summary line, then exits with code 0 if 0 failures, code 1 otherwise.

## Full Suite Table

| Suite | Count | Description |
| ------- | ------- | ------------- |
| NeoFS | - | NeoFS v1 removed. NeoFS v2 tests via btree, freelist, snapshot, neodos_dir |
| NEM | 23 | v1/v2/v3 parsing, header validation, type decoding, ABI field extraction, category parsing, v3 relocations/symbols/sections |
| ELF | 20 | Header validation, segment loading, edge cases (empty, truncated, corrupted), PIE offset/relocation, address space validation |
| Event Bus | 17 | v2 queue ops, subscription filters, dynamic payload lifecycle, backpressure, priority dispatch, handler registration/unregistration |
| Driver State | 21 | 7-state lifecycle (Unloaded→Loaded→Started→Ready→Running→Stopped→Unloaded), transition matrix validation, certification, error tracking |
| Object | 14 | ObObjectTable create/lookup/destroy, refcount management, close-auto-destroy, namespace path resolution |
| Page Cache | 13 | Hash map O(1) lookups, LRU doubly-linked list, create/peek/dirty/invalidate, capacity enforcement, stats, hit_rate, pending_writes |
| Syscall | 13 | SSDT dispatch routing, permission table enforcement, handler_close for file+pipe resources, A4.6 spawn/readdir integration |
| Input | 13 | Ring buffer operations, VT switching between consoles, independent queue isolation, framebuffer swap, input routing to active VT |
| Pipe | 13 | Alloc/free (all 16 slots), write/read data integrity, EOF detection, EPIPE on reader-drop, blocking semantics, fd table integration |
| Security | 12 | SID format validation, Token inheritance chain, ACL allow/deny evaluation, SeAccessCheck path coverage, admin bypass, SAM binary serialization |
| Isolation | 12 | X4 isolation: constants, bounds checking, alloc/free cycles, driver_id lookup, pointer validation, mode string formatting |
| Stress | 14 | Scheduler stress (1000 context switches), syscall storm, memory pressure, buddy allocator fragmentation, handle table exhaustion |
| URN | 11 | Parse all URI schemes (file://, device://, registry://, kobj://), resolve each scheme to Ob path, Ob frontend integration |
| Hot Reload | 11 | Resource tracking across reloads, registry persistence, state transition (Unloaded→Loaded→Unloaded), error code propagation |
| PS/2 Kbd Ref | 10 | Entrypoint validation, lifecycle (init→read→cleanup), key event generation, error handling for invalid scancodes |
| IRP | 11 | IRP alloc/free cycling, status transitions Pending→Completed/Error, error code propagation, queue FIFO/wraparound, callback dispatch ordering |
| ABI | 10 | Version negotiation (v6/v7), compatibility matrix, edge cases (min>max), warning generation |
| Slab | 9 | Per-size alloc/free (16B to 2KB), multi-page slab expansion, realloc fallback, memory reuse after free |
| Boot Loader | 8 | NEM driver scan, load, init, activate, unload lifecycle, category ordering (Boot→System→FileSystem→Input→Network) |
| Framebuffer Ref | 8 | Entrypoint validation, lifecycle (init→clear→pixel→scroll→cleanup), error handling for null params |
| Scheduler | 7 | Priority levels, time-slice accounting, round-robin fairness, aging boost after starvation threshold |
| Mmap | 6 | MmapRegion struct layout, flag combinations, address bounds validation, VMA add/remove from process address space |
| FSCK | 2 | NE2 B-tree walk + CRC32 node verification, freelist coherency, superblock checksum, corrupt node detection, repair mode |
| UTF-8 | 6 | Validation: valid 1-4 byte sequences, overlong rejection, surrogate rejection, continuation byte errors |
| Work Queue | 6 | Push/pop ordering, FIFO semantics, empty queue behavior, overflow returns Err, high/low queue isolation, pending flag atomicity |
| IPI | 5 | Constants verification, TLB shootdown struct layout, call function struct layout, local-only mask, no-targets edge case |
| Per-CPU Slab | 5 | Alloc/free on AP CPUs, refill/drain batching threshold, scalability with CPU count |
| IRQL | 5 | Raise/lower transition, page fault invariant (IRQL <= APC_LEVEL), spinlock implicit raise to DISPATCH_LEVEL, nesting depth, preemption disabled at high IRQL |
| DPC | 5 | Enqueue/dispatch cycle, IRQ→DPC transition, nesting depth, callback ordering FIFO, stress test (100 enqueue) |
| KPRCB | 5 | Struct size assertion, slab cache count (0-3), run queue operations, init sequence, offset sanity (CPU field at correct offset) |
| IoStack | 5 | Partition offset passthrough, cache level routing, device read passthrough, offset accumulation across layers |
| ANSI | 3 | Color foreground/background escape sequences, cursor position (row/col), clear screen (erase display) |
| SMP | 3 | Constants (MAX_CPUS=8), trampoline size < 4 KB, BSP is CPU 0 invariant |
| PCI Enum | 3 | Bus 0 device scan, bus 1 empty (no secondary bus), bridge detection config space |
| Process/Thread | 4 | Kthread struct layout, ThreadState enum values, Eprocess constructor field initialization |
| Allocator | 8 | Box, Vec, String allocation/deallocation, capacity growth, zero-size allocation, aligned allocation |
| Sync | 4 | Atomic flags: NEED_RESCHED bit test/set/clear, cross-CPU visibility |
| Capability | 11 | Capability flag bit values, CapabilitySet construction, category defaults (kernel vs user), check/enforce logic, escalation rules |
| Keyboard | 5 | UTF-8 encoding from scancode, compose key sequences (e.g., dead keys), modifier combinations |
| IoStack | 5 | Partition offset passthrough, cache layer routing, device-level read, offset accumulation |
| Capability | 11 | Flag values, set operations, category defaults, check/enforce path, escalation detection |
| Allocator | 8 | Box/ Vec/String allocation, drop, resize, zero-size edge cases |

## Adding a New Test

1. Write `test_case!("my_test", { ... })` in the relevant module.
2. Create a `pub fn register_my_tests()` function in that module that invokes `test_case!` (macro registers at call site).
3. Add a call to `register_my_tests()` inside `testing::register_tests()` in `src/testing.rs`.
4. Build: `cargo build` in `neodos-kernel/`.
5. Run: `python3 scripts/auto_test.py`.
6. Verify the new test appears in the PASS/FAIL output.

The test name appears in the serial log and `auto_test.py` output, making it easy to identify failures in CI or manual testing.
