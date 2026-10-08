# Large NXE Binaries / User Stack — Design Document

> **Version:** v0.1-draft
> **Status:** Design
> **Target Release:** v0.52+
> **Issue:** [#470](https://github.com/NeoDOS-Project/NeoDOS/issues/470)
> **Related:** [#397](https://github.com/NeoDOS-Project/NeoDOS/issues/397),
> [#396](https://github.com/NeoDOS-Project/NeoDOS/issues/396)
> **ABI Impact:** None (pool sizes are non-breaking; see source-of-truth
> Rule 16.2.4)

---

## 1. Problem Analysis

### 1.1 Current Limitation

The userland loader imposes a hard per-binary cap and a fixed small stack:

```rust
// neodos-kernel/src/arch/x64/paging.rs
pub const USER_BASE:  u64 = 0x0040_0000;   // 4 MB
pub const USER_LIMIT: u64 = 0x0240_0000;   // 36 MB (32 MB window)
pub const MAX_BIN_SIZE: u64 = 192 * 1024;  // 192 KB per user binary
pub const USER_STACK_SIZE: u64 = 64 * 1024; // 64 KB
pub const USER_SLOT_SIZE: u64 = MAX_BIN_SIZE + USER_STACK_SIZE; // 256 KB
pub const USER_SLOT_COUNT: u64 = (USER_LIMIT - USER_BASE) / USER_SLOT_SIZE; // 128
```

`usermode::MAX_PROCESS_BIN` mirrors the cap. Any `.NXE` larger than 192 KB is
rejected. The per-process heap is also small (`PROCESS_HEAP_SIZE = 2 MB`,
`MAX_HEAP_SLOTS = 16`), with the mmap region (32 MB) as the overflow path.

### 1.2 Why It Matters

A Doom port exceeds 192 KB almost certainly (the game logic + renderer alone
are hundreds of KB, before data). The cap blocks "run Doom" even after
graphics/input/time exist. It also limits every substantial userland program.

### 1.3 Constraints

- `USER_BASE..USER_LIMIT` is only 36 MB total for **all** process slots.
- Source-of-truth Rule 4.3.1 states the max binary is the entire user window.
  Moving `USER_BASE`/`USER_LIMIT` is a **breaking** memory-layout change
  (Rule 16.2.1 / table in §15.2) and is avoided.
- The loader runs at boot Phase 4 from the filesystem (`C:\Programs\...`), so
  VFS is already available.

---

## 2. Design

### 2.1 Phase 1 — Raise the Cap, Separate the Stack

Low-risk, unblocks large binaries immediately:

- Raise `MAX_BIN_SIZE` to **2 MB**.
- Introduce `USER_STACK_DEFAULT = 256 KB` and allow a per-binary override
  (see 2.3).
- Decouple stack from the code slot: a slot holds code only; the stack lives in
  its own allocation. `USER_SLOT_SIZE` then tracks `MAX_BIN_SIZE` granularity,
  and `USER_SLOT_COUNT` shrinks accordingly (2 MB → 15 slots in a 32 MB
  window). Fewer simultaneous processes, far larger binaries.
- Update `usermode::MAX_PROCESS_BIN` to match.

Trade-off: raising the cap to 2 MB with slot granularity wastes space for small
binaries (a 20 KB tool still occupies a 2 MB slot). See 2.2 to avoid that.

### 2.2 Phase 1b — Variable-Size Slots

Instead of fixed slots, track a **bump/allocator over the user window**:

- Each process reserves `align_up(bin_size, 2 MB) + stack_size` starting at a
  free base.
- A small free-list (reuse of `Slab`-style logic) hands out and reclaims
  ranges on process exit.

This preserves many concurrent processes while allowing large binaries. It is
more code than fixed slots but avoids the 2 MB minimum footprint.

### 2.3 Phase 2 — ELF Segments + File-Backed Demand Paging

The real long-term fix:

- Load ELF `PT_LOAD` segments at their `p_vaddr` (already planned in
  `elf.rs`; `ELF_ERR_MMAP_COLLISION` guard exists for `0x2000_0000`).
- Map text read-only **file-backed**, demand-paged from the NXE in NeoFS
  (reuse the mmap demand-paging machinery). Only touched pages consume frames,
  so a binary's resident set is proportional to use, not file size.
- Zero-fill `.bss` on fault.
- No 192 KB (or 2 MB) cap; bounded only by the user window and free frames.

This removes the copy-into-slot model entirely, but requires file-backed VMA
support in the mmap region (the display design, `#465`, also needs to map
frames into mmap, so the paging work is shared).

### 2.4 User Stack

- Default 256 KB, guard page below (unmapped → clean fault on overflow).
- Optional per-binary stack size encoded in the NXE ELF note metadata
  (`docs/userland/nxe-format.md`); kernel reads it at load time.
- Stack memory is lazily demand-paged like the heap; only touched pages are
  backed.

### 2.5 User Heap

- Keep `PROCESS_HEAP_SIZE` configurable; raise to 4 MB or keep 2 MB and steer
  large allocations to mmap (already 32 MB).
- Add a per-process `MmUsage` query class (tracked separately by `#274`) so
  clients can size themselves; not required for v1.

### 2.6 Migration

1. **v1:** Phase 1 (2 MB cap + 256 KB stack + guard). Unblocks Doom.
2. **v1.x:** Phase 1b (variable slots) to restore process concurrency.
3. **v2:** Phase 2 (file-backed segments), removing the cap for good.

---

## 3. Alternatives Considered

- **Split the window / move `USER_BASE` to 1 GB**: breaking layout change
  (Rule 16.2.1) and touches paging/memory-layout tests. Avoided.
- **Raise the cap to 36 MB with one slot**: kills concurrency. Rejected.
- **Compress/decompress the binary at load**: still needs the whole image
  resident; does not solve the cap, only the file size. Rejected.
- **Load only `.text` and fault in the rest**: this is Phase 2.

---

## 4. Affected Components

| Subsystem | Change | Impact |
|-----------|--------|--------|
| `arch/x64/paging.rs` | `MAX_BIN_SIZE`, stack constants, variable slots | Moderate |
| `usermode` | `MAX_PROCESS_BIN`, slot allocator/reclaim | Moderate |
| `elf.rs` | Per-segment `p_vaddr` load (Phase 2) | High |
| `mmap` / page fault | File-backed VMAs (Phase 2) | High |
| `docs/architecture/source-of-truth.md` | Note non-breaking size increase | Low |
| `docs/userland/nxe-format.md` | Stack-size metadata (2.4) | Low |
| NeoDev | Optionally emit stack metadata | Low |

---

## 5. API / Contracts

| Contract | Value |
|----------|-------|
| Max binary (v1) | 2 MB (raised from 192 KB) |
| Default stack (v1) | 256 KB + guard page |
| Slot model | Fixed 2 MB (v1) → variable (v1b) → segments (v2) |
| Failure | Binary > cap → spawn fails with `-NoMem`; no partial load (INV-6) |

---

## 6. Test Plan

| Test | Expected |
|------|----------|
| `userbin_192k_still_loads` | Existing binaries unaffected |
| `userbin_2mb_loads` | A 2 MB `.NXE` loads and runs |
| `userbin_over_cap_rejected` | > cap → clean spawn failure, no half-init slot |
| `userbin_stack_256k` | Deep recursion uses the larger stack |
| `userbin_stack_guard_fault` | Overflow hits the guard page, process dies cleanly |
| `userbin_many_small_slots` | Concurrency restored by variable slots (1b) |
| `userbin_file_backed_text` | Touching one page faults in only that page (2) |
| `userbin_bss_zeroed` | `.bss` is zero on first touch (2) |
| `userbin_reclaim_on_exit` | Slot/range reclaimed and reusable |

---

## 7. Files and Modules

### Modified

| Path | Change |
|------|--------|
| `neodos-kernel/src/arch/x64/paging.rs` | Cap/stack/slot constants + allocator |
| `neodos-kernel/src/infra/usermode.rs` | `MAX_PROCESS_BIN`, load path, reclaim |
| `neodos-kernel/src/infra/elf.rs` | Segment loading (Phase 2) |
| `neodos-kernel/src/scheduler/*` | Stack allocation per process |
| `docs/userland/libneodos.md`, `nxe-format.md` | Document limits/metadata |

---

## 8. Implementation Plan

1. Raise `MAX_BIN_SIZE` to 2 MB; decouple stack; 256 KB default + guard.
2. Update `MAX_PROCESS_BIN` and the slot allocator; add tests.
3. Implement variable slots (1b) and reclaim on exit.
4. Add NXE stack-size metadata (optional).
5. Phase 2: `p_vaddr` segment loading + file-backed demand paging.
6. Docs + `neodev test` + markdownlint.

---

## 9. Open Questions

1. Cap value: 2 MB (proposed) vs 4 MB? Affects slot count.
2. Do we need variable slots, or is a smaller fixed slot (e.g. 512 KB) with a
   separate large-binary path enough for v1?
3. Should the stack live in the mmap region (demand-paged) rather than the user
   window? (Proposed: mmap region for lazy backing.)
4. Is per-binary stack metadata worth the NXE format bump, or is a global
   default enough?

---

## 10. Dependencies

- Paging / user window (`arch/x64/paging.rs`).
- Userland loader (`usermode.rs`, `elf.rs`).
- VFS (Phase 2 read of NXE segments).
- Shared with the display design's mmap frame-mapping work (`#465`).
