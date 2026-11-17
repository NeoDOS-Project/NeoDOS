---
name: memory
description: Modify frame allocator, page tables, heap, slab, mmap, or demand paging
---

# Memory

## When to use

Modifying the frame allocator, page table management, heap allocation, the mmap
region, the slab allocator, or demand paging.

## Goal

Change the memory subsystem correctly without breaking allocation invariants or
introducing leaks/corruption.

## References

- `docs/memory/memory.md` — subsystem documentation
- `src/memory/buddy.rs` — physical frame allocator (buddy system)
- `src/memory/layout.rs` — `MemoryLayout`, reserved regions, `init_default()`
- `src/memory/slab.rs` — kernel slab allocator + per-CPU hot cache
- `src/memory/allocator.rs` — `#[global_allocator]` wiring
- `src/memory/mod.rs` — physical memory init (`init_from_regions`)
- `src/arch/x64/paging.rs` — page tables, demand paging, heap page mgmt

## Regions (canonical layout)

| Name | Base | Size | Purpose |
| ------ | ------ | ------ | --------- |
| `user_window` | 0x400000 | 32 MB | Ring 3 code + stack slots |
| `kernel_heap` | 0x2400000 | 16 MB | Kernel linked-list heap |
| `kernel_image` | 0x4000000 | ~1.2 MB | Kernel `.text/.rodata/.data/.bss` |
| `crash_dump` | 0xF000000 | 16 MB | Panic-time crash dump |
| `user_heap` | 0x10000000 | 32 MB | Per-process heap (16 × 2 MB) |
| `nxl_region` | 0x1E000000 | 2 MB | NXL user libraries |
| `mmap_region` | 0x20000000 | 32 MB | Anonymous + file-backed mmap |
| `driver_iso` | 0x30000000 | 16 MB | Isolated NEM driver slots |

`validate_layout_consistency()` asserts no overlaps and that regions fit in
detected physical memory.

## Steps

1. **Identify the subsystem**
   - **Frame allocator**: `src/memory/buddy.rs` (buddy system, 4 KB frames).
   - **Physical init / regions**: `src/memory/mod.rs`, `src/memory/layout.rs`.
   - **Slab**: `src/memory/slab.rs` (9 size classes, per-CPU hot cache).
   - **Page tables / demand paging**: `src/arch/x64/paging.rs`.
   - **Heap**: user heap at `0x10000000..0x12000000`.
   - **Mmap**: `0x20000000..0x22000000`.

2. **Read `docs/memory/memory.md`** — buddy invariants, slab hot-cache policy,
   demand-paging fault handling.

3. **Buddy allocator changes** (`src/memory/buddy.rs`)
   - 11 power-of-2 orders (0–10): 4 KB … 4 MB.
   - Free lists: `free_lots[[u64; MAX_FREE_SLOTS]; 11]` with
     `MAX_FREE_SLOTS = 512`; a used/free bitmap gives O(1) buddy lookup.
   - Allocation splits larger blocks; deallocation coalesces buddies.
   - Use `alloc_frames(order)` / `free_frames(addr, order)` (and
     `allocate_frame()` / `free_frame()` for order 0) — never touch metadata
     directly.

4. **Slab allocator changes** (`src/memory/slab.rs`)
   - Classes: `CACHE_SIZES = [8, 16, 32, 64, 128, 256, 512, 1024, 2048]`.
   - Each slab page is 4 KB with a 32-byte header; free slots form an intrusive
     linked list; allocation is O(1) within a page.
   - Per-CPU hot cache holds 32 objects per class (lock-free fast path); the slow
     path takes the global mutex and moves a batch of 32.
   - Objects > 2048 B fall through to `linked_list_allocator::LockedHeap`.

5. **Demand paging / fault handler** (`src/arch/x64/paging.rs`)
   - The kernel identity-maps the first 4 GiB with 2 MB huge pages; heap/mmap
     regions are split into 4 KB PTEs.
   - `heap_alloc_page(virt)` / `heap_free_page` / `heap_free_range` manage the
     per-process heap; anonymous mmap faults allocate + zero + map
     `USER_ACCESSIBLE`; file-backed faults read via the page cache first.
   - Always flush the TLB after PTE changes (`invlpg` / CR3 reload). Cross-CPU:
     `shootdown_single_page()` / `shootdown_range()` build an all-others mask and
     send IPI 0xF1 (lock-free mask builder; see
     `docs/investigation/smp331-exit-tlb-shootdown-self-deadlock.md`).

6. **Heap/mmap bound changes** (`src/memory/layout.rs`)
   Update the region descriptors and ensure no overlap with kernel image, stack,
   or MMIO. `reserve_region()` panics on overlap.

7. **Write tests** with `test_case!`: buddy alloc/free all orders + coalescing;
   slab alloc/free per class + reuse; demand-paging mmap → fault → map → access;
   heap alloc/free patterns.

## Best practices

- Zero frames before handing them to userspace.
- Allocate physical pages only through the buddy API; never rewrite bitmap
  metadata ad hoc.
- Flush the TLB after any page-table modification.
- `validate_layout_consistency()` must keep passing after region changes.
- Do not block/allocate in interrupt context above `DISPATCH_LEVEL`.

## Common mistakes

- Coalescing non-buddies (wrong order / not adjacent).
- Leaking slab objects when the hot cache is discarded without draining.
- Forgetting the TLB flush after editing PTEs.
- Overlapping heap and mmap regions after changing bounds.
- Treating `mmap` as eager — it registers a VMA only; pages are populated on
  first access.

## Final checklist

- [ ] Buddy coalescing invariant holds (no leak)
- [ ] Slab hot-cache refill/drain policy correct
- [ ] TLB flushed after PTE changes
- [ ] Heap/mmap regions non-overlapping and in valid address space
- [ ] Demand-paging path tested (mmap → fault → map → access)
- [ ] Tests added; `cargo build`, `neodev test`, `neodev check-deps` pass
- [ ] `docs/memory/memory.md` updated if bounds or algorithms changed
