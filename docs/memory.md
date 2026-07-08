# Memory Architecture

## Buddy Allocator

Source: `src/memory/buddy.rs`. Physical page allocator managing system RAM via the
binary buddy algorithm. Supports 11 power-of-2 orders:

| Order | Size    | Pages per block |
|-------|---------|-----------------|
| 0     | 4 KB    | 1               |
| 1     | 8 KB    | 2               |
| 2     | 16 KB   | 4               |
| 3     | 32 KB   | 8               |
| 4     | 64 KB   | 16              |
| 5     | 128 KB  | 32              |
| 6     | 256 KB  | 64              |
| 7     | 512 KB  | 128             |
| 8     | 1 MB    | 256             |
| 9     | 2 MB    | 512             |
| 10    | 4 MB    | 1024            |

Each order maintains a free list stored as `free_lots[[u64; MAX_FREE_SLOTS]; 11]`
with `free_counts[11]` tracking the number of entries. MAX_FREE_SLOTS = 512 per order.

A bitmap tracks the used/free status of every 4 KB frame, providing O(1) buddy
lookup. The legacy fallback uses 16384 u64 words covering 1,048,576 frames (4 GB).
In practice the bitmap is dynamically sized from UEFI memory descriptors.

```rust
// Round a physical address to the containing 2 MB-aligned buddy block
let block_start = addr & !(HUGE_PAGE_SIZE - 1);
let order = 9; // 2 MB
```

API:

- `alloc_frames(order)` -- allocate a block of 2^order pages, returns physical address
- `free_frames(addr, order)` -- return a block to the appropriate free list, coalesce
- `allocate_frame()` / `free_frame()` -- convenience wrappers for order 0

Global instance:

```rust
lazy_static! {
    pub static ref ALLOCATOR: Mutex<BuddyAllocator> = Mutex::new(BuddyAllocator::new());
}
```

## Dynamic Physical Memory

Source: `src/memory/mod.rs`. `PHYS_MEM_END` is determined dynamically from the UEFI
memory map, not hard-coded. Initialization sequence:

1. `init_bitmap(ptr, words)` -- sets all bits to 1 (allocated)
2. `init_from_regions(regions, phys_max)` -- clears bits for usable UEFI regions:
   - Conventional memory (type 7)
   - Boot services code/data (types 3, 4)
3. Reserves low 1 MiB, kernel image, framebuffer, and page table structures
4. Supports physical RAM above 4 GiB natively

## Memory Layout Manager

Source: `src/memory/layout.rs`. Tracks reservations via `MemoryRegion`:

```rust
pub struct MemoryRegion {
    pub base: u64,
    pub size: u64,
    pub name: [u8; 24],
    pub flags: u32,
}
```

`MemoryLayout` stores up to 32 regions (`MAX_REGIONS`). `reserve_region()` panics
on overlap detection. `init_default()` defines the canonical layout:

| Name         | Base      | Size     | Purpose                         |
|--------------|-----------|----------|---------------------------------|
| kernel_image | 0x4000000 | ~1.2 MB  | Kernel .text/.rodata/.data/.bss |
| user_window  | 0x400000  | 32 MB    | Ring 3 code + stack slots       |
| kernel_heap  | 0x2400000 | 16 MB    | Kernel linked-list heap         |
| crash_dump   | 0xF000000 | 16 MB    | Panic-time crash dump region    |
| user_heap    | 0x10000000| 32 MB    | Per-process heap (2 MB x16)     |
| nxl_region   | 0x1E000000| 2 MB     | NXL user libraries              |
| mmap_region  | 0x20000000| 32 MB    | Anonymous + file-backed mmap    |
| driver_iso   | 0x30000000| 16 MB    | Isolated NEM driver slots       |

`validate_layout_consistency()` asserts boot-time invariants: no overlap between
any two reserved regions, all regions fit within detected physical memory, and
critical regions (kernel_image, kernel_heap) are non-overlapping.

## Kernel Slab Allocator

Source: `src/slab.rs`, `src/slab_container.rs`. 9 power-of-2 size classes:

| Class | Size   |
|-------|--------|
| 0     | 8 B    |
| 1     | 16 B   |
| 2     | 32 B   |
| 3     | 64 B   |
| 4     | 128 B  |
| 5     | 256 B  |
| 6     | 512 B  |
| 7     | 1024 B |
| 8     | 2048 B |

Each slab page is 4 KB with a 32-byte header at offset 0:

```rust
pub struct SlabHeader {
    magic: u32,        // "SLAB" (0x534C_4142)
    slot_size: u16,
    capacity: u16,
    allocated: u16,
    free_head: u16,
    // ... padding to 32 bytes
}
```

Free slots form an intrusive linked list via inline u16 indices. Slots store the
next-free index at offset 0, giving O(1) alloc and free within a slab page.

Per-CPU hot cache (KPRCB, accessed via GS segment): 32-object free list per size
class. Lock-free fast path for alloc and free:

```rust
// Fast path — no locks, no atomics
let obj = this_cpu_slab_cache_alloc(size);
// Slow path — acquire global Mutex, move batch of 32 objects
```

Objects larger than 2048 bytes fall through to `linked_list_allocator::LockedHeap`.

`SlabAllocator` implements `GlobalAlloc` so all `#[global_allocator]` requests
use slabs for small allocations and the linked-list heap for large ones.

14 tests: per-size alloc/free, multi-page slab expansion, mixed-size stress,
large-object fallback, and a 100k-iteration stress test.

## Demand Paging

Source: `arch/x64/paging.rs`. The kernel identity-maps the first 4 GiB with 2 MB
huge pages at boot. Regions that need per-page tracking (user heap, mmap) split
huge pages into 4 KB page tables.

### split_2mb_page

Allocates one physical 4 KB frame for a new page table, fills 512 PTEs pointing at
consecutive physical pages, and updates the Page Directory entry to point at the new
table instead of a huge page.

### walk_ptes_4k

Traverses the 4-level page table hierarchy (PML4 -> PDPT -> PD -> PT) and returns
a mutable reference to the final PTE, or None if any intermediate entry is not
present.

### Heap Page Management

Every Ring 3 process gets a 2 MB heap at `PROCESS_HEAP_BASE + slot_idx *
PROCESS_HEAP_SIZE`. The heap region's huge pages are split at process creation.
On first write to any 4 KB-aligned heap address, the page fault handler allocates
a physical frame and maps it as USER_ACCESSIBLE.

```rust
pub fn heap_alloc_page(virt: u64) -> bool;
pub fn heap_free_page(virt: u64);
pub fn heap_free_range(start: u64, end: u64);
```

### Heap Growth Flow

```
sys_brk(new_break)
  -> write to pages in [old_break, new_break)
  -> page fault (if page not mapped)
  -> page_fault_handler -> handle_heap_page_fault
  -> heap_alloc_page -> allocate_frame() + map USER_ACCESSIBLE
  -> instruction re-executed (write succeeds)
```

### Heap Destruction Flow

```
sys_exit -> heap_free_range(heap_base, heap_base + PROCESS_HEAP_SIZE)
  -> for each present PTE with phys != virt:
       free_frame(phys)
       mark PTE not-present
  -> shootdown_range TLB
  -> mmap_free_range for each MmapRegion
```

## mmap (Lazy)

Region: `0x20000000..0x22000000` (32 MB), initially split into 4 KB page table
entries by `init_mmap_demand_paging()`.

```rust
pub struct MmapRegion {
    pub base: u64,
    pub len: u64,
    pub prot: u16,    // 1=R, 2=W
    pub flags: u16,   // bit0=1 anonymous, 0=file-backed
    pub drive: u8,
    pub inode: u32,
    pub file_size: u32,
}
```

- `sys_mmap(RAX=19)` -- registers the VMA only; no physical pages allocated
- `sys_munmap(RAX=20)` -- frees pages, removes VMA
- Anonymous faults: `allocate_frame()` + zero-fill + map USER_ACCESSIBLE
- File-backed faults: read from VFS (checking PageCache first) into a freshly
  allocated physical frame, then map USER_ACCESSIBLE

`is_user_ptr_valid()` extends to cover both the user window (0x400000..0x2400000)
and all registered mmap regions. Process exit frees every mmap region.

Cross-CPU TLB shootdown: `shootdown_single_page()` / `shootdown_range()` builds a
CPU bitmask of all active CPUs and sends IPI vector 0xF1.
