# Investigation — #476 allocator/resource ownership audit (`FREE_BAD`)

**Date:** 2026-10-04
**Branch:** `investigation/476-allocator-free-bad` (from `e8972b9`, the #476 H1 branch)
**Related:** #383 (free-list corruption, closed not-reproducible), #384 (user `#PF`), #476 (`#UD`).

---

## 1. Verdict

```text
ALLOCATOR CORRUPTION (global heap)      = NOT OBSERVED (FREE_BAD = 0)
RESOURCE-SLOT OWNERSHIP (paging slots)  = VIOLATION FOUND (unsynchronized tables)
```

Global kernel-heap ownership is clean across the tested workloads (no `[FREE_BAD]`
and the detector data path is unit-tested). A **separate, concrete ownership
violation** was found while auditing the process-resource allocators:
`SLOT_USED` / `HEAP_SLOT_USED` in `arch/x64/paging.rs` are mutated with
non-atomic load/store from `alloc_user_slot` / `alloc_heap_slot`, with **no lock**
at the call sites (`usermode.rs::create_process_from_ob_path` /
`spawn_usermode` run the allocation *outside* the scheduler critical section).
Two CPUs spawning concurrently can therefore both claim the same user slot / heap
slot (double allocation), which later surfaces as a double free.

---

## 2. Ownership model (real)

### 2.1 Global allocator (`#[global_allocator]`, `slab.rs`)

```text
alloc(layout):
  size <= 2048 && align <= 16 && cache_index(size).is_some()
      -> per-CPU slab hot cache (KPRCB free_list[32])
      -> refill from global slab pages (buddy pages, outside the fallback heap)
  else -> fallback LockedHeap (linked_list_allocator)

dealloc(ptr, layout):
  ptr in [HEAP_START, HEAP_START+HEAP_SIZE)  -> fallback LockedHeap
  else                                        -> slab cache by cache_index(size)
  (routing is exact: `memory::reserve_range` keeps slab pages out of the fallback
   heap range; #383 replaced the old payload-magic heuristic with this range)
```

- **Slab ownership metadata**: `SlabPage` header (magic/slot_size/capacity/
  free_head) at the page base. `SlabPage::free` validates offset/slot and
  double-free (walks the page free list). `SlabCache::free` validates
  magic + slot_size.
- **Gap**: the per-CPU hot path (`this_cpu_slab_free_local`) pushes pointers with
  **no validation**; the page free-list double-free check is skipped there.
- **Fallback ownership metadata**: none. `linked_list_allocator` writes the freed
  block into its free list; a double free / foreign free corrupts it — the #383
  kernel `#PF` class.

### 2.2 Process-resource slots (`paging.rs`)

| Resource | Alloc | Free | Metadata | Sync |
|----------|-------|------|----------|------|
| user slot (code+stack, 256 KB ×128) | `alloc_user_slot` | `free_user_slot` | `SLOT_USED[128]` | **none** |
| heap slot (2 MB ×16) | `alloc_heap_slot` | `free_heap_slot` | `HEAP_SLOT_USED[16]` | **none** |
| heap pages | `heap_alloc_page` | `heap_free_range` | page tables | per-CPU? |

`create_process_from_ob_path` → `alloc_user_slot()` (no lock) → `load_elf` →
`spawn_usermode` → `alloc_heap_slot()` (no lock) → transactional scheduler
critical section. The lock is taken **after** both slot allocations, so the slot
tables are read/modified concurrently by parallel spawns.

---

## 3. `FREE_BAD` detector (diagnostic, not a fix)

### 3.1 Global allocator (`slab.rs::audit_free`)

Before every `dealloc`:

```text
ptr null                     -> ok
ptr in fallback heap range   -> alignment check
ptr outside fallback range   -> must be a slab page:
   page base == 0            -> OUT_OF_RANGE        (no deref)
   addr < 1 MB / > 3.5 GB    -> OUT_OF_RANGE        (no deref)
   page magic != SLAB_MAGIC  -> NOT_OWNED
   page.slot_size != cache   -> OWNER_MISMATCH
   offset misaligned         -> MISALIGNED
   slot in per-CPU free list -> ALREADY_FREE
   slot in page free list    -> ALREADY_FREE
   else                      -> ok
```

- Fallback double-free / not-owned is detected with a fixed open-addressing
  **allocation shadow** (`FB_SHADOW`, 8192 slots, tombstones): inserted on every
  fallback alloc, removed on free; a missing entry on free → `ALREADY_FREE`.
- On `Err`, the detector **logs `[FREE_BAD]` and leaks the allocation** instead
  of feeding an invalid pointer to a free list (documented diagnostic
  mitigation; it prevents the corruption from destroying the evidence).
- Counters + a 64-entry ring; dumped on panic (`[FREE_BAD_RING]`).

### 3.2 Process-resource slots (`paging.rs`)

`SLOT_USED`/`HEAP_SLOT_USED` are now `AtomicBool` and the free paths use
`swap(false)`; a free of an already-free slot logs
`[FREE_BAD] kind=ALREADY_FREE allocator=paging_{user,heap}_slot id=<idx> owner=<pid>`.
The **allocation test-and-set is deliberately left non-atomic** so the double
allocation still occurs and its double-free consequence is observable. Each slot
carries an owner pid tag for the log.

---

## 4. Unit test

`memory/mod.rs::register_free_bad_tests` → `n476_free_bad_ownership_detector`
(covers `None`, `Misaligned`, `OwnerMismatch`, `OutOfRange`, `NotOwned`,
`AlreadyFree` via `slab::audit_free_probe`). `neodev test`: **822/822 PASS**, no
`FREE_BAD` during the suite.

---

## 5. Findings

### F1 — Global heap ownership: no violation observed

`[FREE_BAD]` = 0 in all campaigns below; the detector was unit-validated and
active (it runs on every `dealloc`).

### F2 — Paging slot tables are unsynchronized (ownership violation)

`alloc_user_slot` / `alloc_heap_slot` use a plain `if !used { used = true }`
test-and-set on a shared table with **no lock** and are called outside the
scheduler critical section. Parallel spawns can claim the same slot. Consequences:
two processes sharing a user code/stack region (ELF overwrite → user `#PF`, #384)
or a 2 MB heap; the shared slot is then freed twice (detected as `[FREE_BAD]
paging_*_slot ALREADY_FREE`). This is a distinct resource-lifetime bug from the
global allocator free-list issue of #383.

Evidence: see campaign table (§6) and source: `paging.rs::alloc_user_slot`
(`SLOT_USED[i].load/.store`), `alloc_heap_slot`, call sites
`usermode.rs:187,257`.

---

## 6. Campaign table

| Environment | allocator audited | runs | `FREE_BAD` (global) | slot-race | panics (frame family) |
|-------------|-------------------|-----:|--------------------:|----------:|----------------------:|
| `neodev test` (QEMU SMP2) | global + paging | 1 | 0 (822 tests PASS) | 0 | 0 |
| VBox SMP2 normal ×12 (H1 branch, no churn) | — | 12 | 0 | — | 0 |
| VBox SMP2 churn 1×200 (global detector) | global | 1 | 0 | — | 0 |
| VBox SMP2 churn 2×32 (`fb2c`, pre-fix detector) | global + paging | 6 | 0 real (3× test false positive, fixed) | 0 | 1 GPF (`syscall_handler_asm` iretq) |
| VBox SMP2 churn 2×32 (`fb2d`) | global + paging (+`*_race`) | 6 | **0** | 0 | 1 `#PF` (`rip=0x24b12bf`) |
| VBox SMP2 churn 2×32 (`fb2e`, direct race detector) | global + paging (+`*_race`) | 6 | **0** | 0 | 0 |

The `#PF`/GPF under churn do **not** carry any `FREE_BAD`: the allocator free
lists are intact at the moment of the crash.

## 7. Correlation

- `FREE_BAD` (global) was **never** observed in any campaign, and the detector is
  unit-validated — `#383`'s residual free-list corruption is **not reproduced**
  by these workloads.
- The `#476`/`#384` barrier is **not accompanied by an allocator or slot
  ownership violation**: in `fb2d_5` the `#PF` (`rip=0x24b12bf`, a kernel-stack
  address, `write=true`) happened during service startup with `FREE_BAD = 0`.
  In `fb2c_4` the GPF was in the `iretq` of `syscall_handler_asm`
  (`err=0xf820`, garbage selector) after repeated `[K355] resched handoff -> idle`.
- F2 (paging slot race) is a real unsynchronized ownership violation but its
  double-allocation/double-free consequence was **not observed** in the
  campaigns (the window is a few instructions; the churn only has 2 concurrent
  spawners).

## 8. Verdict

```text
GLOBAL ALLOCATOR HEAP CORRUPTION          = NOT OBSERVED (FREE_BAD = 0)
RESOURCE-SLOT OWNERSHIP (paging tables)   = VIOLATION (code-level, not observed)
#476 FRAME/CONTEXT CORRUPTION             = NOT the allocator
```

No fix applied (no demonstrated allocator cause). F2 is documented and should be
tracked as its own issue (recommend an atomic `compare_exchange` claim in
`alloc_user_slot`/`alloc_heap_slot`). The remaining `#476` family points at the
context-switch/`iretq` path (e.g. the `[K355]` idle hand-off), not at allocation.

## 9. Next hypothesis (highest diagnostic value)

The corruption manifests as a wild control transfer into a kernel stack
(`rip=0x24b12bf`) or a bad `iretq` selector, with no allocator `FREE_BAD` and no
KStack reclaim conflict (H1 refuted). The most valuable next experiment is to
instrument the **`iretq`/switch-out frame** itself across the K355 idle
hand-off and the syscall/timer return paths: verify the frame a CPU is about to
`iretq` into (RIP/CS/RFLAGS/RSP/SS) belongs to the selected thread's
`kernel_stack_top`, and flag any mismatch (`[IRETQ_BAD_FRAME]`) before the fault.

