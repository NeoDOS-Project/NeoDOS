# VFS Concurrency Baseline — v0.51 Final Stabilization (#83 / #519 / #343 / #376)

**Date:** 2026-10-09
**Branch:** `refactor/vfs-lock-contention-83`
**Base:** `develop` @ `97b615a`
**Issues:** #83 (VFS global lock), #519 (global-lock contention / O(n) lookups), #343 (SMP boot lock-order inversion, closed), #376 (SMP boot `VFS` lock wait, open), #345 (SMP1 heap panic, closed)
**Environment:** QEMU 10.2.2 TCG, `q35`, `neodev test` (single-CPU test harness),
`neodev build --quick --image`, kernel v0.51.4.

> This document is the **reconnaissance and baseline** written before the main
> changes. It describes the pre-change state. The implementation and validation
> results are recorded in
> [`vfs-lock-contention-83-report.md`](vfs-lock-contention-83-report.md).

---

## 1. Current lock topology and ownership

The filesystem stack is protected by three process-wide spinlocks declared in
`neodos-kernel/src/infra/globals.rs`:

| Lock | Type | Protects | Owner |
| ---- | ---- | -------- | ----- |
| `VFS` | `spin::Mutex<crate::fs::vfs::Vfs>` | The whole `Vfs`: the 26 `drives` (`Box<dyn FileSystem>`), the subdir mount table, and — through the drive objects — all per-filesystem in-memory state (NeoFS `inode_cache`, `freelist`, `snapshot_table`, `cow_garbage`) | any VFS caller |
| `PAGE_CACHE` | `spin::Mutex<PageCache>` | 128 × 4 KB unified page/sector cache: hash table, LRU list, dirty state, read-ahead | any VFS/block caller |
| `BLOCK_DEVICES` | `lazy_static Mutex<BlockDeviceManager>` | Registry of up to 8 `Box<dyn BlockDevice>` + refcounts | any storage caller |

There is also a fourth, **unranked** lock:

| Lock | Type | Protects | Owner |
| ---- | ---- | -------- | ----- |
| `MOUNT_MANAGER` | `lazy_static Mutex<MountManager>` (`fs/vfs/mount.rs:124`) | `MountPoint` list + Ob `MountPoint`/`\DosDevices` namespace entries | mount/unmount |

The `FileSystem` trait (`fs/vfs/mod.rs:55`) is `&mut self`, and `Vfs` owns the
drive objects directly, so **the `VFS` lock is the single serialization point for
the entire filesystem**, including drive-local state and block I/O performed by
the drive methods.

### Preemption / IRQ discipline

`with_vfs`, `with_page_cache` and `with_block_devices` (`globals.rs:48-150`)
bracket their closures with `scheduler::preempt_disable()/enable()`. The
justification (`scheduler/mod.rs:30-40`) is #376: a thread holding one of these
spinlocks must not be descheduled by the timer, or every waiter spins forever.
`spin::Mutex` does **not** disable interrupts by itself.

### Diagnostics

- `scheduler/diag/vfs_owner.rs` records the current `VFS` owner/waiter (lock-free
  atomics) and counts waits (`VFS_WAIT_COUNT`, `VFS_WAIT_MAX_TICKS`).
- `infra/lock_order.rs` records per-CPU held ranks and counts inversions
  (`VIOLATIONS`). It is diagnostic only (no semantics).
- `LOCK_DIAG` (`globals.rs:8`) enables `[LOCK_WAIT]`/`[LOCK_ACQUIRE]` serial
  tracing; enabled at boot before NeoInit (`boot/mod.rs:647`).

---

## 2. Lock-order graph

Documented and enforced order (`lock_order.rs:24-26`):

```text
VFS  ->  PAGE_CACHE  ->  BLOCK_DEVICES
```

`MOUNT_MANAGER` was **not** in the graph and the unified mount/unmount paths took
it with a raw `MOUNT_MANAGER.lock()` while already holding `VFS`
(`fs/vfs/mount.rs:150,158,161,166,175,184`), so any inversion involving it was
invisible (#519).

Nested acquisition paths found:

```text
(VFS) -> with_page_cache -> with_block_devices          fs path read/write
(VFS) -> PAGE_CACHE -> BLOCK_DEVICES                    fs/vfs/io.rs, neodos_v2.rs
(state, no VFS) PAGE_CACHE -> BLOCK_DEVICES             flush_cache_if_needed
(VFS) -> MOUNT_MANAGER                                  vfs_mount/unmount_filesystem
(VFS) -> BLOCK_DEVICES                                  neodos_v2.rs read/subdir paths
```

Not in the graph (unranked, undocumented order): `SCHEDULER`, `OB_TABLE`,
`OB_SECURITY`, namespace locks, `SERVICE_MANAGER`, `NXL_REGISTRY`. The
process-creation path reads the binary under `VFS` **before** taking `SCHEDULER`
(`infra/usermode.rs:164-185`, then `spawn_usermode` at 295), so no `SCHEDULER ->
VFS` chain was found on that path.

---

## 3. Contention hot spots

1. **One global `VFS` mutex for all drives and all operations.** Independent
   drives and concurrent same-file reads serialize even though they only need
   per-drive or per-inode coordination.
2. **`VFS` is held across synchronous block-device I/O.** `Vfs::read` →
   `NeoDosFsV2::read` → `read_entry_bytes` walks to `BLOCK_DEVICES` and issues
   PIO/DMA reads while all outer locks are held. Critical sections are not
   bounded by construction.
3. **`BLOCK_DEVICES` is a single global registry mutex held across the whole
   transfer** (`BlockDeviceManager::get` returns `&mut dyn BlockDevice`). With a
   single device this is inherent, but it also serializes independent devices.
4. **Unbounded writeback critical section.** `flush_cache_if_needed` held
   `PAGE_CACHE` + `BLOCK_DEVICES` while flushing up to 8 dirty pages **per
   device** in one pass.
5. **Heap allocation while holding `VFS`.** `Vfs::resolve_path` and friends built
   `Vec<&str>` component lists, and `walk_components` built a `Vec<(usize,u32)>`,
   under the lock. Allocation under a spinlock is a documented deadlock risk
   (`docs/filesystem/vfs-patterns.md:423-427`).
6. **Unpreemptible block I/O.** `IoStack::read_sectors`/`write_sectors` and the
   `neodos_v2.rs` helpers took the FS locks with `lock_order` guards but **without**
   `preempt_disable`, unlike the `with_*` helpers — the same #376 class of bug on
   any non-`with_vfs` caller (FSCK, mkfs, tests).

---

## 4. Existing synchronization guarantees and limitations

Guaranteed:

- Whole-VFS mutual exclusion: no torn drive/mount-table state.
- Canonical `PAGE_CACHE -> BLOCK_DEVICES` order at the known call sites (#343
  fix), enforced by `lock_order`.
- Per-device page-cache tags prevent cross-device aliasing (#552, #560).
- COW reclamation is gated on an empty snapshot table (#553, #563).

Limitations:

- No per-drive or per-inode parallelism.
- `MOUNT_MANAGER` was outside the order checker (#519).
- Some acquisition sites omitted `preempt_disable` (hot spot 6).
- `VFS` ↔ `PAGE_CACHE` ↔ `BLOCK_DEVICES` ordering is only as good as *every*
  call site remembering the guards; direct `.lock()` sites bypass it.
- Compound operations (resolve → read → write) rely on whole-VFS exclusivity;
  any future per-drive split must preserve that atomicity.

---

## 5. Reproducible deadlock / starvation scenarios

- **#343 (fixed):** `NeoDosFsV2::read` took `BLOCK_DEVICES` then `PAGE_CACHE`
  while `flush_cache_if_needed` took them the other way → AB-BA wedge on SMP>1
  boot during Service Manager auto-start. Documented in
  `smp343-service-lock-order-deadlock.md`. The inversion no longer appears.
- **#376 (open):** intermittent `[LOCK_WAIT] lock=VFS tid=…` stall during
  service auto-start with 3–4 concurrent starts, even with the #343 cycle gone;
  residual failures classified as held-lock/no-progress and scheduler-frame
  classes.
- No user-space or single-CPU reproducer exists; SMP>1 boot is required.

---

## 6. Existing test coverage and missing tests

Existing:

- `lock_order::register_tests` — 3 tests: canonical clean, `BLOCK_DEVICES ->
  PAGE_CACHE` detected, `PAGE_CACHE -> VFS` detected.
- `buffer::page_cache`, `vfs::io` (IoStack), `vfs::mount`, NeoFS v2 suites
  (mount/reload, freelist recovery, snapshots, COW, 60-entry dirs).
- `stress_spawn.rs` — diagnostic spawn-storm harness (compiled out:
  `ENABLED = false`).

Missing:

- A rank for `MOUNT_MANAGER` and a test for its canonical order.
- A test that the guarded helpers never record an inversion across a real
  work-sequence (mount → create → write → read → rename → unmount).
- Multi-CPU boot/stress coverage in the automated flow (the harness is single-CPU;
  `neodev run --config <smpN.toml>` is manual).
- Contention counters surviving boot (currently only `VFS_*` owner/waiter).

---

## 7. Relevant files and functions

| File | Role |
| ---- | ---- |
| `neodos-kernel/src/infra/globals.rs` | `VFS`/`PAGE_CACHE`/`BLOCK_DEVICES`, `with_vfs`, `with_page_cache`, `with_block_devices`, `flush_cache_if_needed` |
| `neodos-kernel/src/infra/lock_order.rs` | Rank constants + inversion counter/tests |
| `neodos-kernel/src/fs/vfs/mod.rs` | `Vfs`, `FileSystem`, `walk_components`, path ops |
| `neodos-kernel/src/fs/vfs/io.rs` | `IoStack`, partition/ceiling translation, cache routing |
| `neodos-kernel/src/fs/vfs/mount.rs` | `MountManager`, `MOUNT_MANAGER`, unified mount/unmount |
| `neodos-kernel/src/fs/neofs/neodos_v2.rs` | `NeoDosFsV2` `FileSystem` impl; direct lock sites |
| `neodos-kernel/src/fs/neofs/neodos_io.rs` | `file_read`/`file_write` (page cache + dev) |
| `neodos-kernel/src/fs/neofs/btree/tree.rs` | B-tree COW traversal via `BTreeIO` |
| `neodos-kernel/src/drivers/storage/block.rs` | `BlockDeviceManager`, NEM register/unregister |
| `neodos-kernel/src/drivers/storage/manager.rs` | Boot storage probe/register |
| `neodos-kernel/src/scheduler/mod.rs` | `preempt_disable/enable`, `SCHEDULER` |
| `neodos-kernel/src/scheduler/diag/vfs_owner.rs` | VFS owner/waiter instrumentation |
| `neodos-kernel/src/infra/usermode.rs` | Process creation: VFS read then `SCHEDULER` |
| `neodos-kernel/src/arch/x64/paging.rs` | File-backed mmap page load (page cache + VFS) |

---

## 8. Proposed implementation sequence (risk-ordered)

1. **Phase A (implemented).** Add `MOUNT_MANAGER` to the lock hierarchy; route
   every production direct `.lock()` of `VFS`/`PAGE_CACHE`/`BLOCK_DEVICES` through
   the guarded helpers so preemption + order cover them; extend tests.
2. **Phase B (implemented, bounded).** Bound the writeback critical section;
   move device probing out of the registry critical section; drop allocation
   from under the `VFS` lock in path resolution.
3. **Phase C (documented, not implemented).** Object-lifetime rules for a
   future per-drive split: drive/mount publication, unmount vs in-flight lookup.
4. **Phase D (documented, rejected now).** Lock-free/read-mostly conversion is
   not justified: with a single synchronous device the remaining serialization
   is `BLOCK_DEVICES`/the device itself, not `VFS`.

---

## 9. Baseline measurements

- Kernel test suite: **860/860 PASS** (`neodev test`, single-CPU, 42.6 s).
- Command/shell E2E tests: `neodev test` reports "Command tests FAILED or not
  run" / "Shell tests FAILED or not run" in this QEMU/TCG environment (unchanged
  from baseline; they require interactive monitor key delivery).
- No throughput/latency numbers are claimed: the automated harness is
  single-CPU and does not exercise contention. `VFS_WAIT_MAX_TICKS`,
  `vfs_wait_count()` and `lock_order::violations()` are the available counters
  for a subsequent SMP run; a reproducible harness is described in the
  implementation report.

Design, invariants and results: [`vfs-lock-contention-83-report.md`](vfs-lock-contention-83-report.md).
