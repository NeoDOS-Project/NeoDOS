# Performance for NeoDOS Kernel

## Kernel Performance Priorities

1. **IRQL correctness** — Wrong IRQL causes deadlocks, not just slowness
2. **Allocator efficiency** — Buddy allocator for page-sized, slab for small objects
3. **Syscall dispatch** — Direct jump table, no megamorphic dispatch
4. **Cache locality** — Keep hot data on same cache line
5. **Lock contention** — Minimize shared mutable state, use RCU where possible

## Memory Allocation Strategy

- **Page allocator** (buddy): for 4KB+ allocations
- **Slab allocator**: for small frequently-allocated types (ObHandles, tokens)
- **Stack**: fixed-size per-thread, no dynamic stack allocation
- **No heap**: no global allocator in kernel

## Syscall Optimization

- Jump table indexed by RAX (O(1) dispatch)
- Validate arguments early, fail fast
- Minimize TLB flushes (use large pages where possible)
- Batch handle operations (ObClose multiple handles at once)

## Locking Guidelines

- IRQL-based synchronization preferred over spinlocks
- Use `IrqlGuard` for raising/lowering IRQL
- Spinlocks only at or below DISPATCH_LEVEL
- Minimize critical section length
- No nested spinlocks (deadlock risk)
