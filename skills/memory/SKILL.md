# Memory

## When to use
Modifying the frame allocator, page table management, heap allocation, mmap region, slab allocator, or demand paging.

## Goal
Change memory subsystem correctly without breaking allocation invariants or introducing leaks/corruption.

## Steps

1. **Identify subsystem**
   - **Frame allocator**: `src/memory/buddy.rs` — physical page allocation (buddy system, 4KB frames)
   - **Memory regions**: `src/memory/layout.rs` — memory map, region definitions
   - **General init**: `src/memory/mod.rs` — memory subsystem initialization sequence
   - **Slab allocator**: `src/slab.rs` — 9 size classes (8B–2KB), per-CPU hot cache
   - **Page tables**: `src/arch/x64/paging.rs` — page table walk, TLB management, demand paging
   - **Heap**: Fixed at `0x10000000..0x12000000`
   - **Mmap**: Fixed at `0x20000000..0x22000000`

2. **Read `docs/memory.md`**
   Understand the buddy allocator invariants, slab hot cache refill policy, demand paging fault handling.

3. **Buddy allocator changes** (`src/memory/buddy.rs`)
   - Maintain 10 free lists for orders 0–9 (4KB – 2MB).
   - Allocation: split larger blocks. Deallocation: coalesce buddies.
   - Update `BUDDY_MAX_ORDER` if changing max block size.
   - Never allocate during interrupt context above IRQL DISPATCH_LEVEL.

4. **Slab allocator changes** (`src/slab.rs`)
   - Size classes: index 0=8, 1=16, 2=32, 3=64, 4=128, 5=256, 6=512, 7=1024, 8=2048.
   - Per-CPU hot cache holds a small batch of pre-filled slabs (refill from central pool).
   - Adding a new size class: update `SLAB_CLASSES` array and rebuild.

5. **Demand paging / page fault handler** (`src/arch/x64/paging.rs`)
   - Fault types: page-not-present, protection violation, write to read-only.
   - For mmap'd regions: allocate a physical page on first access.
   - Handle copy-on-write for forked process pages.
   - Update `PageTableEntry` flags (Present, Writable, User, etc.).

6. **Heap/mmap region changes** (`src/memory/layout.rs`)
   - If changing heap bounds: update `HEAP_START`, `HEAP_END`, `MMAP_START`, `MMAP_END`.
   - Ensure no overlap with kernel image, stack, or MMIO regions.
   - Update `MemoryRegion` enum and the region descriptor table.

7. **Write tests**
   Add tests in `src/testing.rs` for:
   - Buddy: allocate and free all orders, verify coalescing
   - Slab: allocate from each class, verify alignment and recycling
   - Demand paging: mmap a page, access it, verify page fault is handled
   - Heap: repeated alloc/free patterns, ensure no corruption

## Best practices
- Always zero frames before handing to userspace (security).
- Slab allocations from a single class must be 8-byte aligned minimum.
- Use `allocate_frames()` / `free_frames()` for physical pages — never touch the buddy metadata directly.
- Page table changes must flush TLB via `invlpg` or full `cr3` reload.
- Validate mmap region bounds against `MMAP_END` before mapping.

## Common mistakes
- Coalescing buddies that are not actually buddies (wrong order or not adjacent).
- Leaking slab objects when hot cache is discarded without returning to central pool.
- Forgetting TLB flush after modifying page table entries.
- Allocating in interrupt context above DISPATCH_LEVEL (can't block for refill).
- Overlapping heap and mmap regions after adjusting bounds.

## Final checklist
- [ ] Buddy allocator maintains coalescing invariant (no memory leak)
- [ ] Slab hot cache refill/return policy correct
- [ ] TLB flush after any page table modification
- [ ] Heap and mmap regions non-overlapping, within valid address space
- [ ] Demand paging path tested (mmap → page fault → map → access)
- [ ] Kernel tests added and pass
- [ ] `docs/memory.md` updated if bounds or algorithm changed
