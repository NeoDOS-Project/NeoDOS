# #477 — Fix: atomic user/heap slot claim

**Branch:** `fix/477-slot-alloc-race` (from `develop` @ `a67ba09`, i.e. after the #476 merge).
**Scope:** paging resource-slot allocation only.

## Defect (demonstrated by code)

`neodos-kernel/src/arch/x64/paging.rs`:

* `alloc_heap_slot()` did `if !HEAP_SLOT_USED[i].load()` then a `compare_exchange`
  **but ignored the CAS result** and still returned the slot.
* `alloc_user_slot()` did the same for the ASLR-picked `target_idx`.

So two CPUs racing could both observe a slot free and both be handed it; the
slot is then shared (user code/stack window or a 2 MB heap) and freed twice on
teardown. The #476 diagnostics logged the lost CAS
(`paging_{user,heap}_slot_race`) but preserved the sharing so the corruption
stayed observable.

Call sites run outside the scheduler lock (`usermode.rs::create_process_from_ob_path`,
`spawn_usermode`), so there is no external serialization.

## Fix

Make the atomic claim authoritative and skip contended slots:

* New helpers `claim_first_free(used) -> Option<usize>` and
  `claim_at(used, idx) -> bool` (single `compare_exchange(false,true,AcqRel)`).
* `alloc_heap_slot()` = `claim_first_free(&HEAP_SLOT_USED)?` + owner/allocated tags.
* `alloc_user_slot()` claims the ASLR-picked slot and, on contention, falls back
  to the first free slot (or `None`). A slot that another CPU claimed is never
  returned.
* Owner tags (`*_SLOT_OWNER`) and the free-path `AlreadyFree` (`FREE_BAD`)
  detection are preserved.

## Regression

`paging_slot_claim_atomic` (registered from `testing::register_tests`): verifies
the claim helpers return distinct indices, never re-issue an already-claimed
slot, skip pre-claimed slots, and return `None` when the table is exhausted.

A deterministic single-threaded reproduction of the TOCTOU is **not possible**
(the load and the CAS see the same atomic unless another CPU runs between
them); the race itself is exercised by SMP churn and monitored by the
`paging_*_slot_race` / `paging_*_slot` FREE_BAD detectors.

## Validation

* `neodev build --quick --image`: OK.
* `neodev test`: **825/825** (824 + the new `paging_slot_claim_atomic`).
* QEMU SMP1/2/4 ×2 each: `paging_*_slot_race = 0`, `FREE_BAD = 0`, `#PF/#GP/#UD = 0`, `panic = 0`, `ALL_TESTS_COMPLETE` in every run.

The fix prevents the concurrent double-allocation; the free-path `AlreadyFree`
detector stays quiet, confirming no shared slot is double-freed.

## Verdict

```text
FIXED — user/heap slot allocation is now atomic; concurrent spawns cannot share a slot.
```
