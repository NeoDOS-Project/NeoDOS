# Investigation — #343 SMP>1 Service Manager boot gap: PAGE_CACHE / BLOCK_DEVICES lock-order inversion

**Date:** 2026-09-29
**Branch:** `investigation/343-smp-service-lock-order`
**Base commit:** `15f9c11` (`develop`, includes #344 / #331 signature-2 fix)
**Environment:** QEMU 10.2.2, TCG, `q35`, 2/4 vCPU, user-mode NIC,
`TICK_INTERVAL_US = 1000`, kernel `RUSTUP_TOOLCHAIN=nightly`.

---

## 1. Baseline

- `develop` HEAD `15f9c11`; working tree clean.
- `neodev test`: **749/749** kernel tests pass.
- Reproducer: `neodev run --headless --net user --config <smpN.toml>`, serial to
  a file, per-CPU state captured with HMP `info registers -a`, and the global
  lock state bytes read directly with HMP `x /1xb` (kernel data is
  identity-mapped, so physical == virtual).

---

## 2. Reproduction

Boot with the kernel/Service Manager auto-start, no shell interaction needed:

```text
neodev run --headless --net user --config <smp4.toml>   # cpus = 4
```

Serial always stops at the same point, while starting `Dhcpc`:

```text
[SPAWN] pid=3 heap_base=0x10200000 slot=... OK
[SM] started: Netcfg
[SM] Started: Netcfg
[SM] start service idx=0            <- Dhcpc: no further output, no shell
```

---

## 3. Frequency (before the fix)

Same image/harness, boot reaches a shell?

```text
develop @ 15f9c11   SMP2: 7/8    SMP4: 1/5
```

---

## 4. Multicore evidence (HMP at the wedge, SMP4)

Three `info registers -a` samples; the two key CPUs are frozen at stable RIPs:

```text
CPU#0  RIP=0x00000000040fd0c2  RFL=0x202 (IF=1)  CPL=0
CPU#1  RIP=0x00000000040b70c2  RFL=0x002 (IF=0)  CPL=0
CPU#2  RIP=0x000000000414d542  RFL=0x202 (IF=1)  CPL=0
CPU#3  RIP=0x00000000040b70c2  RFL=0x002 (IF=0)  CPL=0
```

Symbolication / disassembly of the frozen RIPs:

- CPU0 `0x40fd0c2` → `NeoDosFsV2 as FileSystem>::read`, the **PAGE_CACHE
  acquire spin** (`lock cmpxchg` on `PAGE_CACHE` at `0x41b3fc8`).
- CPU2 `0x414d542` → `globals::flush_cache_if_needed`, the **BLOCK_DEVICES
  acquire spin** (`lock cmpxchg` on `BLOCK_DEVICES` at `0x42fb6a0`; it already
  acquired `PAGE_CACHE` at `0x414d504`).
- CPU1/CPU3 `0x40b70c2` → `arch::x64::idt::timer_handler_inner`, the
  **SCHEDULER acquire spin** (secondary effect).

Lock state bytes read directly at the wedge (all held):

```text
0x42ce8e8 SCHEDULER     = 0x01
0x42fb330 VFS           = 0x01
0x41b3fc8 PAGE_CACHE    = 0x01
0x42fb6a0 BLOCK_DEVICES = 0x01
```

---

## 5. Lock graph (cycle)

```text
CPU0  NeoDosFsV2::read            holds VFS, holds BLOCK_DEVICES
                                     waits PAGE_CACHE         ─┐
                                                                │ circular wait
CPU2  flush_cache_if_needed       holds PAGE_CACHE             │
                                     waits BLOCK_DEVICES      ─┘

CPU1/CPU3  timer_handler_inner    spin on SCHEDULER (blocked behind the above)
```

The mutual dependency is between **`BLOCK_DEVICES` and `PAGE_CACHE`**, in
opposite acquisition orders.

---

## 6. First divergence

The host kernel has two contradictory orders for the same pair:

```text
PAGE_CACHE  ->  BLOCK_DEVICES      (canonical)
  globals::flush_cache_if_needed         (globals.rs:117)
  IoStack::read_sectors / write_sectors  (vfs/io.rs:92-93, 117-118)

BLOCK_DEVICES  ->  PAGE_CACHE      (inverse)
  NeoDosFsV2::read                        (fs/neodos_v2.rs:188-190)
  NeoDosFsV2::write                       (fs/neodos_v2.rs:199-201)
  NeoDosFsV2::write_node                  (fs/neodos_v2.rs:66,79)
```

The first divergence is the moment the Service Manager spawn path
(`sm_start_auto_services` → `start_service` → `spawn_process` → `with_vfs` →
`Vfs::read` → `NeoDosFsV2::read`) takes `BLOCK_DEVICES` **before**
`PAGE_CACHE` while another CPU (`clear_need_resched` → `flush_cache_if_needed`)
takes `PAGE_CACHE` **before** `BLOCK_DEVICES`.

---

## 7. Root cause (proven)

A classic AB-BA lock-order inversion:

```text
trigger  SMP>1 boot runs a filesystem read (Dhcpc binary) concurrently with a
         syscall-return cache flush
   ↓
CPU0 NeoDosFsV2::read:   lock BLOCK_DEVICES   → lock PAGE_CACHE (blocks)
CPU2 flush_cache_if_needed: lock PAGE_CACHE   → lock BLOCK_DEVICES (blocks)
   ↓
neither can proceed; both spins are unbounded (plain non-reentrant spin::Mutex)
   ↓
the boot thread never finishes starting Dhcpc; the timer on the other CPUs
ends up blocked on SCHEDULER; no shell
```

Evidence: at the wedge both `PAGE_CACHE` and `BLOCK_DEVICES` read `0x01`
(held), CPU0 is at the `PAGE_CACHE` acquire spin inside
`NeoDosFsV2::read`, and CPU2 is at the `BLOCK_DEVICES` acquire spin inside
`flush_cache_if_needed`. This is a genuine circular wait, not a timeout or a
starvation.

---

## 8. Fix

Establish a single acquisition order for the pair: **`PAGE_CACHE` before
`BLOCK_DEVICES`** (the order already used by `IoStack` and
`flush_cache_if_needed`). In `fs/neodos_v2.rs`, reorder the acquisition in
`read`, `write` and `write_node`:

```rust
// before
let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
let dev = bdevs.get(self.io_stack.device_id)...;
let mut pc = crate::globals::PAGE_CACHE.lock();

// after
let mut pc = crate::globals::PAGE_CACHE.lock();
let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
let dev = bdevs.get(self.io_stack.device_id)...;
```

No sleeps, retries, timeouts or scheduling changes are introduced; the
canonical order is simply made consistent. This is the minimal fix (preference
1: "corregir el orden de adquisición").

---

## 9. Regression

<!-- REGRESSION -->

A bounded, lock-free lock-order checker (`neodos-kernel/src/lock_order.rs`) was
added and wired into the acquisition sites of the three filesystem locks
(`with_vfs`, `with_page_cache`, `with_block_devices`, `flush_cache_if_needed`,
`IoStack::read_sectors`/`write_sectors`, `NeoDosFsV2::read`/`write`/`write_node`).
It records per-CPU held ranks and counts inversions in an atomic counter; it
does not change lock semantics.

Tests (`lock_order::register_tests`):

- `lock_order_canonical_is_clean` — `VFS → PAGE_CACHE → BLOCK_DEVICES` counts 0.
- `lock_order_detects_block_then_page_cache` — the exact #343 inverse
  (`BLOCK_DEVICES` held → `PAGE_CACHE`) is detected (counter ≥ 1); the
  canonical order afterwards is clean. This test fails on the pre-fix order.
- `lock_order_detects_page_then_vfs` — a second inverse is detected.

Because the boot test harness is single-CPU, a true AB-BA cannot be executed as
a unit test; the checker encodes the invariant and the real regression is the
SMP>1 boot validation below.

---

## 10. SMP validation

<!-- SMP_VALIDATION -->

Kernel suite: `neodev test` → **752/752** (was 749; +3 lock-order tests).

The decisive measurement is the wedge **lock state**: the #343 inversion
occupies `PAGE_CACHE` *and* `BLOCK_DEVICES` simultaneously.

### Before the fix

`develop @ 15f9c11`, SMP4, boot with auto-start services:

```text
4 / 5 boots wedge at [SM] start service idx=0
wedge lock bytes: SCHEDULER=0x01  VFS=0x01  PAGE_CACHE=0x01  BLOCK_DEVICES=0x01
CPU0 = NeoDosFsV2::read      (PAGE_CACHE acquire spin)
CPU2 = flush_cache_if_needed (BLOCK_DEVICES acquire spin)
```

### After the fix

SMP4, 6 boots: **3 pass / 2 boot-gap / 1 command hang**. Every captured
boot-gap shows `PAGE_CACHE=0x00` — the cycle is gone:

- run 4: all four locks `0x00` (a different, lock-free boot gap);
- run 6: `VFS=0x01`, `BLOCK_DEVICES=0x01`, `PAGE_CACHE=0x00` (no inversion; a
  thread holding VFS/BD without progress — #338 class).

### Full matrix (checker build) and cause classification

```text
SMP4: pass 3 / boot-gap 1 / command-hang 2   (6 runs)
SMP2: pass 1 / boot-gap 3 / command-hang 0   (4 runs)
SMP1: pass 0 / boot-gap 0 / command-hang 2   (2 runs)
```

Each residual failure was classified by RIP; **none is the #343 cycle**:

| Where | Symptom | Class |
| ----- | ------- | ----- |
| SMP2 | user `#PF` at NeoInit entry `0x9c01c0` (terminated) | user page-mapping |
| SMP2 | stall after `[LOCK_WAIT] lock=VFS tid=5`, CPUs in `ob_open_path`/`netd_entry` | held-lock/no-progress |
| SMP2 | stall after `[READY_GUARD_STATS]`, CPU spinning on `SCHEDULER` in `timer_handler_inner` | scheduler frame class |
| SMP1 | `tree` hangs mid-output in user mode | #338 frame dispatch |
| SMP4 | boot gap with all locks free; idle-frame `SS=0x15`/canary | #338/#346 |

Conclusion: the specific #343 defect — the `PAGE_CACHE` ↔ `BLOCK_DEVICES`
acquisition-order inversion during Service Manager auto-start — is eliminated
(the inversion no longer appears in any captured wedge). SMP>1 boot and
command reliability still need the separate frame/scheduler fixes tracked in
issues #338, #346 and the user-`#PF` follow-up; those are not caused by #343
and are not fixed here.

---

## 11. Related issues

- #331 — SMP>1 hang. Signature 2 (child-exit TLB-shootdown self-deadlock) was
  fixed by #344. This issue is signature 1 (boot kernel→user gap).
- #338 — Ring-0 `Ready` frame dispatch (separate; not touched here).
- #345 — SMP1 kernel-heap allocation panic in NeoInit's shell-spawn loop
  (separate).
- #346 — SMP>1 idle-thread frame corruption (`SS=0x15` / stack canary),
  observed after the #343 cycle was removed (separate).
- #83 — remove the global VFS lock (longer-term design; this fix only makes the
  existing order consistent).

## 12. Residual failures after the fix

The #343 cycle is gone (post-fix stall captures show
`VFS/PAGE_CACHE/BLOCK_DEVICES` all `0x00`), but SMP>1 boot is not yet 100 %
reliable: separate frame/scheduler defects (#338, #346) still surface
intermittently. They are out of scope here and tracked separately.
