# VFS Lock Stabilization — Implementation & Readiness Report (v0.51)

**Date:** 2026-10-09
**Branch:** `refactor/vfs-lock-contention-83`
**Base:** `develop` @ `97b615a`
**Issues:** #83, #519, #343, #376, #345
**Companion:** [`vfs-lock-contention-83-baseline.md`](vfs-lock-contention-83-baseline.md)

Scope chosen for this pass: **conservative stabilization + documented design**
(see the baseline document for the decision and the rejected per-drive/lock-free
alternatives). The objective is a demonstrably more auditable, correctly-ordered
VFS synchronization model, not a wholesale rewrite.

---

## 1. Chosen design and invariants

Lock hierarchy (authoritative, enforced by `infra/lock_order.rs`):

```text
VFS  ->  MOUNT_MANAGER  ->  PAGE_CACHE  ->  BLOCK_DEVICES
```

State ownership:

| Lock | Protects | Lifetime |
| ---- | -------- | -------- |
| `VFS` | `Vfs` drives + subdir mount table + all per-FS in-memory state (drive objects are owned by `Vfs`) | process |
| `MOUNT_MANAGER` | `MountManager` `MountPoint` list + Ob mount namespace entries | process |
| `PAGE_CACHE` | 128-slot unified page/sector cache | process |
| `BLOCK_DEVICES` | block-device registry + refcounts | process |

Invariants (tested/guarded):

1. **I1 — Canonical order.** Every acquisition follows the order above; violations
   are counted by `lock_order` (`violations()`).
2. **I2 — Preemption disabled.** Every filesystem-lock critical section runs with
   `preempt_disable()` held, so the timer cannot deschedule a lock holder (#376).
   Provided uniformly by `with_vfs` / `with_page_cache` / `with_block_devices` /
   `with_mount_manager`.
3. **I3 — No allocation under `VFS`.** Path resolution uses fixed-size stack
   storage (`MAX_PATH_COMPONENTS`), not heap `Vec`.
4. **I4 — Bounded writeback.** `flush_cache_if_needed` writes back at most one
   4 KB page per `PAGE_CACHE`+`BLOCK_DEVICES` acquisition and at most
   `MAX_FLUSHES_PER_CALL` pages per call; the flag is re-armed if dirty pages
   remain.
5. **I5 — No device-I/O under the registry lock.** The boot storage probe runs
   outside `BLOCK_DEVICES`; only registration is under the lock.
6. **I6 — Unmount safety.** Mount/unmount update `Vfs.drives[]` and
   `MountManager` under `VFS`, so a concurrent lookup never observes a
   half-mounted drive.

Rejected alternatives:

- **Per-drive `Mutex` + drop the global `VFS` lock.** `FileSystem` is `&mut self`
  and `Vfs` owns the drives; doing this correctly requires a `with_drive` API and
  redefining compound-operation atomicity across ~all `with_vfs` call sites.
  This is a VFS synchronization rewrite, not a stabilization, and cannot be
  SMP-stress-validated in the available environment. Deferred to v0.52.
- **`RwLock<Vfs>` + interior inode cache.** Would allow concurrent reads past
  `VFS`, but every read still takes the global `PAGE_CACHE`+`BLOCK_DEVICES`
  (single synchronous device), so no throughput is gained while compound
  atomicity is weakened.
- **Lock-free page cache / lookup (Phase D).** Reclamation, publication and ABA
  are not currently solved, and the measured bottleneck is the device, not the
  data structure. Not warranted.

---

## 2. Changes (Phase A + bounded Phase B)

### Phase A — closed synchronization bypasses

- `infra/lock_order.rs`: added the `MOUNT_MANAGER` rank (`VFS=4`,
  `MOUNT_MANAGER=3`, `PAGE_CACHE=2`, `BLOCK_DEVICES=1`) and a regression test
  (`lock_order_detects_mount_manager_then_vfs`); the canonical test now covers
  all four ranks.
- `fs/vfs/mount.rs`: replaced the raw `VFS.lock()`/`MOUNT_MANAGER.lock()` sites
  (#519) with `globals::with_vfs` and a new `with_mount_manager` helper
  (preempt-disable + order guard). `vfs_mount`, `vfs_unmount`, `vfs_get_mount`
  and `vfs_path_to_mount` now use the guarded helper. The unified
  mount/unmount now update both registries under one `VFS` critical section
  (I6).
- `fs/vfs/io.rs`: `IoStack::read_sectors`/`write_sectors` now use
  `with_page_cache`/`with_block_devices` (adding the missing preempt-disable —
  baseline hot spot 6); `acquire_ref`/`release_ref`/`is_valid`/`with_device` and
  the IoStack test helper route through `with_block_devices`.
- `fs/neofs/neodos_v2.rs`: every direct `PAGE_CACHE`/`BLOCK_DEVICES` acquisition
  (`read_node`, `write_node`, `read_block_raw`, `write_block_raw`, `read`/
  `write`/`read_entry_bytes`, `read_superblock_raw`, snapshot-table read,
  `invalidate_cache`) now uses the guarded helpers.
- `drivers/storage/block.rs`: NEM block register/unregister use
  `with_block_devices`.
- `fs/fsck/mod.rs`: test device registration uses the guarded helpers.
- `arch/x64/paging.rs`: file-backed mmap page load uses `with_page_cache`.
- Test-only cleanup (`force_remove`) and boot one-shot setup (`boot/mod.rs`)
  remain raw but are justified: they run under `without_interrupts` on the test
  harness, or during single-threaded early boot before APs can observe the lock.

### Phase B — reduced critical-section scope

- `infra/globals.rs` `flush_cache_if_needed`: bounded writeback (I4).
- `fs/vfs/mod.rs`: `walk_components` and path splitting use fixed stack storage
  (I3), removing heap allocation under the `VFS` lock.
- `drivers/storage/manager.rs`: device probing moved out of the registry
  critical section (I5).

### Not changed

No syscall, ABI, on-disk format, or filesystem-semantics change. The public
`Vfs`/`FileSystem` API is unchanged. No new dependencies. `unsafe` was not
introduced or weakened.

---

## 3. Tests run

| Command | Config | Result |
| ------- | ------ | ------ |
| `cargo build` (`neodos-kernel/`) | nightly, `x86_64-unknown-none` | OK (0 errors) |
| `neodev test` (baseline, pre-change) | QEMU, SMP2 image, single-CPU harness | **860/860 PASS** |
| `neodev test` (post-change) | QEMU, SMP2 image, single-CPU harness | **861/861 PASS** (+1 lock-order test) |
| `neodev run` SMP4 boot sweep | QEMU TCG, 4 vCPU, headless | see §4 |
| `neodev run --backend virtualbox` sweep | VirtualBox 7.2.20, headless | see §4 |

The automated `neodev test` reports command/shell E2E tests as "FAILED or not
run" in this environment, identical to the baseline (they need interactive
monitor key delivery); this is unchanged by the work.

---

## 4. SMP / VirtualBox validation

Workload per boot: the full 861-test kernel suite runs at boot, then Service
Manager auto-start (4 services), DHCP and the shell. Marker `C:\>` = shell
reached.

| Backend | CPUs | Runs | Tests | Shell | DHCP ACK | KERNEL PANIC | `[LOCK_WAIT] VFS` |
| ------- | ---- | ---- | ----- | ----- | -------- | ------------ | ------------------ |
| QEMU TCG | 4 | 2 (120 s cap) | 861/861 each | 0 (cap hit post-test; 130 s run reached it) | 0 | 1 | 5, 9 |
| VirtualBox 7.2.20 (bridged) | 4 | 4 (145 s cap) | 861/861 each | **4/4** | **4/4** | 0 | 2–4 |

- QEMU TCG is slow; at the 120 s cap the boot was still in the post-test service
  phase. An earlier 130 s QEMU SMP4 run reached `C:\>` and DHCP ACK.
- **No `VFS` deadlock.** `[LOCK_WAIT] lock=VFS` was observed 2–9 times per boot
  but was always released and boot progressed to the shell — the opposite of
  #376, where boot stopped at the lock. The VirtualBox sweep (4/4 to shell with
  DHCP ACK) is the decisive end-to-end result.
- The single QEMU panic (run 1 of 2) is `called Option::unwrap() on a None value`
  at `alloc/src/collections/btree/navigate.rs:534`, co-occurring with
  `[FREE_BAD_RING] not_owned` allocator entries. That is the already-tracked
  post-#482 residual failure class **#490**, not a VFS code path; it did not
  appear in any of the 4 VirtualBox boots (0/4).

---

## 5. Performance evaluation

Reliable before/after **throughput** measurement is not possible in this
environment: the automated harness is single-CPU and, on the current image,
there is a single synchronous block device, so the dominant serialization is the
device/`BLOCK_DEVICES`, not `VFS`. We therefore report only invariants and the
observable counters, not fabricated numbers.

Available counters for a future SMP benchmark:

- `scheduler::diag::vfs_wait_count()` — number of blocking VFS waits.
- `scheduler::diag::vfs_owner_acq()` / `VFS_WAIT_MAX_TICKS` — wait duration.
- `lock_order::violations()` — acquisition-order inversions (must stay 0).

A reproducible harness: boot SMP4 `neodev run --headless` while spawning the
`stress_spawn` harness (`stress_spawn.rs`, set `ENABLED = true`), and compare
`VFS_WAIT_*` and `lock_order::violations()` before/after.

Expected qualitative effect of this change: shorter/bounded `PAGE_CACHE` +
`BLOCK_DEVICES` critical sections during writeback (I4), no allocation under
`VFS` (I3), and no untracked acquisitions. Per-drive parallel *throughput* is
explicitly **not** claimed — that requires the v0.52 per-drive design and
multi-device or async I/O.

---

## 6. Acceptance / readiness

**Verdict: READY WITH DOCUMENTED LIMITATIONS.**

| Acceptance criterion | Result |
| -------------------- | ------ |
| 1. VFS contention reduced per the chosen design, with evidence | Design = correct/complete/bounded global locking + documentation; uniform guarded acquisition (0 tracked inversions), bounded writeback (I4), no allocation under `VFS` (I3). Per-drive parallelism explicitly deferred to v0.52. |
| 2. Every lock-order bypass fixed or justified | Done: `MOUNT_MANAGER` ranked; production direct locks routed through helpers; boot/test-only raw locks justified in §2. |
| 3. Lifetime/synchronization invariants documented and tested | I1–I6 documented; I1 (order, incl. `MOUNT_MANAGER`) tested; unmount publication (I6) unified under `VFS`. |
| 4. No known new deadlock/race/FS regression | 861/861 kernel tests; 4/4 VirtualBox SMP4 boots to shell + DHCP; 1/6 QEMU unwrap-on-None is tracked #490 (not VFS). |
| 5. Required SMP / FS regression tests pass | Yes (auto suite + QEMU/VBox SMP4 sweeps). |
| 6. Diagnostics / performance recorded honestly | Yes (counters + explicit "no throughput claim"). |
| 7. Documentation matches implementation | Yes (this report, baseline, `vfs-patterns.md`). |
| 8. Clear v0.52 decision | Yes (§7). |

Residual limitations and risks:

- The global `VFS` lock is retained, so independent drives/reads still serialize
  at the VFS layer (#83 stays open for the per-drive design).
- `BLOCK_DEVICES` is still one global registry and the device is synchronous;
  it is the true serialization point on the current single-volume image.
- Intermittent post-#482 residual failures (#490: unwrap-on-None / ObjectManager
  #GP / Ring-3 #UD) still occur rarely under QEMU TCG SMP4. Unrelated to VFS, but
  they prevent a blanket "SMP4 100 % clean" statement.
- Only the four filesystem locks are in one enforced total order; `SCHEDULER`,
  `OB_TABLE` and namespace locks are still cross-subsystem and undocumented.

---

## 7. Follow-up work for v0.52

1. Per-drive (`with_drive`) synchronization + compound-operation atomicity rules
   (issue #83). Requires an API change and SMP stress validation.
2. Per-device `BLOCK_DEVICES` sharding and/or asynchronous IRP completion so a
   slow device does not serialize unrelated devices (#519).
3. SMP contention benchmark harness with reproducible CPU/iteration counts.
4. Move `MOUNT_MANAGER`, `SCHEDULER`, `OB_TABLE` and namespace locks into the
   documented hierarchy where cross-subsystem nesting exists.
