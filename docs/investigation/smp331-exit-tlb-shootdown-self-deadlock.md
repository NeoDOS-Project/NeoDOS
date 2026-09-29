# Investigation — #331 SMP>1 hang: recursive scheduler lock in the process-exit TLB shootdown

**Date:** 2026-09-29
**Branch:** `investigation/331-smp-hang`
**Base commit:** `8ee470a` (`develop`, includes #341 / `58546478`)
**Environment:** QEMU 10.2.2, TCG, `q35`, 2/4 vCPU, user-mode NIC,
`TICK_INTERVAL_US = 1000`, kernel `RUSTUP_TOOLCHAIN=nightly`.

---

## 1. Baseline

- `git status`: clean before instrumentation.
- `develop` HEAD `8ee470a` contains #341 (`58546478`,
  `fix(net): initialize e1000 RX ring before enabling RCTL`).
- Kernel tests on `develop`: **745/745 PASS**.
- Reproducer harness: `neodev run --headless --serial <log> --net user`,
  shell driven through the QEMU monitor (`sendkey`), per-CPU state captured
  with HMP `info registers -a` at the wedge. The harness is throwaway
  (`/tmp`), not part of the commit.

---

## 2. Reproduction

Deterministic, minimal, no artificial sleeps/yields:

```text
neodev run --headless --net user --config <smpN.toml>   # cpus = N
wait for C:\> prompt
send:  tree C:
```

`tree C:` reaches the shell, prints the full tree, and then the shell **never
returns to the prompt**. `tree` is a `.NXE` child spawned by `neoshell` via
`sys_ob_create(PROCESS) + sys_ob_wait(fd)`, so this is exactly the issue's
"parent stuck in OB_WAIT after a child exits" signature.

The same behaviour was observed with the mixed sequence
`ipconfig, tree C:, cpuinfo, tree C:, drives, ...`. `ipconfig`, `cpuinfo` and
`drives` always return; `tree` reliably wedges. `tree` is special because it
allocates a large heap buffer (`Box<[Entry; MAX_ENTRIES]>`, ~12–17 KB of
`sbrk` heap) and therefore has **resident heap pages at exit** (see §7).

Other child commands that do not grow the heap (`ipconfig`, `cpuinfo`,
`drives`, `shtest`) always return.

---

## 3. Frequency (before the fix)

| Scenario | Result |
| -------- | ------ |
| SMP2, `tree C:` | 3/3 hang |
| SMP1, `tree C:` | no wedge; extremely slow to finish printing (see §10) |

When `tree` exits with resident heap pages the deadlock is deterministic; the
"30–60 % intermittent" rate in the issue is explained by whether the child
happens to own resident heap/mmap pages at exit, which depends on demand-paging
timing. Any process that faulted in heap or mmap pages at exit is affected.

---

## 4. Timeline (wedge, SMP2)

```text
SPAWN pid=10 tid=12 name=...\tree.nxe                 <- shell spawns tree
OB_WAIT entry object_type=Process native_id=10 pid_param=10
OB_WAIT pid=10 already_dead=false thread_count=1
OB_WAIT activated child pid=10 before blocking parent pid=5
... tree prints the complete directory tree ...
<child reaches sys_exit>                              <- no further kernel log
<NO wake for parent, NO prompt, NO further [READB]>
```

After the last tree line the serial log is silent: `terminate_current` never
completes, so the `ChildExit` wake for the parent is never published.

---

## 5. CPU state at the wedge (HMP `info registers -a`)

Three samples 1.5 s apart are byte-identical (all CPUs frozen, not
spin-rotating through code):

```text
CPU#0  RIP=0x00000000040f5ab2  RFL=0x00000002 (IF=0)  CPL=0  HLT=0
CPU#1  RIP=0x00000000040f28a2  RFL=0x00000002 (IF=0)  CPL=0  HLT=0
```

Symbolication of the frozen RIPs (`addr2line` on the release kernel):

- CPU0 `0x40f5ab2` → inside `without_interrupts::<…paging::build_tlb_target_mask…>`
- CPU1 `0x40f28a2` → inside `without_interrupts::<…scheduler::yield_current_thread…>`

Both functions acquire the **global `SCHEDULER` spin mutex**
(`current_scheduler().lock()`). Disassembly of CPU0's RIP confirms it is the
mutex acquire spin loop on the `SCHEDULER` lock byte:

```text
40f5aa2: lock cmpxchg %cl, <SCHEDULER lock byte>
40f5aaa: jne 40f5ab2
40f5ab0: pause
40f5ab2: movzbl <SCHEDULER lock byte>,%eax
40f5ab9: test %al,%al
40f5abb: jne 40f5ab0          <- spin while locked
```

CPU1 is also in a lock-acquire site, so **neither CPU holds the lock yet**.
The only possible holder is CPU0 itself, from an outer frame: a recursive
(self) acquisition of the non-reentrant `SCHEDULER` spin mutex.

---

## 6. OB_WAIT

- Waiter: `neoshell` PID 5 / TID 7, `Blocked { waiting_for: ChildExit{pid:10} }`.
- Object: `Process` `native_id=10` (`tree`), `thread_count = 1` when the parent
  blocked; `OB_WAIT` correctly activated the suspended child before blocking.
- Expected wake: `terminate_current` on the child's `sys_exit` sets
  `thread_count = 1 → 0` and calls `make_thread_ready` for every thread whose
  `waiting_for == ChildExit{10}`.
- Observed wake: **none**, because `terminate_current` never gets past the
  heap-free block (below). The child is the *first* divergence; `OB_WAIT` is
  only where the failure becomes visible (the task's warning "OB_WAIT may be
  where an earlier failure is observed, not the cause" applies exactly here).

---

## 7. Root cause — lock recursion in the exit page-free path

`handler_exit` calls `terminate_current` **while holding the global
`SCHEDULER` mutex** (`neodos-kernel/src/syscall/handlers.rs:25-45`):

```rust
let s = current_scheduler();
let mut scheduler = s.lock();          // SCHEDULER held
...
scheduler.terminate_current(code as i64);
```

`terminate_current` frees the process' memory in the same critical section
(`neodos-kernel/src/scheduler/lifecycle.rs:692-711`):

```rust
crate::arch::x64::paging::free_user_slot(slot);
crate::arch::x64::paging::heap_free_range(ep.heap_base, ep.heap_base + PROCESS_HEAP_SIZE);
for r in ep.mmap_regions.iter() {
    crate::arch::x64::paging::mmap_free_range(r.base, r.base + r.len);
}
```

When the process had resident pages, `heap_free_range` /
`mmap_free_range` end with `shootdown_range(...)`
(`paging.rs:495-497`, `paging.rs:785-787`), which called
`build_tlb_target_mask()`:

```rust
fn build_tlb_target_mask() -> u64 {
    crate::hal::without_interrupts(|| {
        let s = crate::scheduler::current_scheduler();
        let scheduler = s.lock();      // <-- re-acquires SCHEDULER: self-deadlock
        for k in scheduler.kthreads.iter().flatten() { ... }
    });
}
```

`struct Scheduler` is guarded by a plain non-reentrant `spin::Mutex`
(`scheduler/mod.rs:274`, `use spin::Mutex`). Re-locking a `spin::Mutex` on the
same CPU spins forever with interrupts disabled. The `terminate_current`
`ChildExit` wake, `defer_reap`, and the syscall return path are all never
reached. On SMP>1 the other CPUs then try to take the same `SCHEDULER` lock
(e.g. `yield_current_thread`, timer path) and spin with interrupts disabled,
wedging the whole machine — exactly the observed two-CPU frozen state.

`cleanup_terminated_process` (`scheduler/mod.rs:415-428`) →
`recycle_terminated` → `free_eprocess_resources` reaches the same helper while
holding `SCHEDULER`, so it is a second entry to the same defect.

A secondary defect is that the old mask was also *incomplete*: it only
targeted CPUs that currently own a non-terminated thread, so a CPU that ran
the process and is now idle still keeps a stale TLB entry for the freed page.

### Causal chain

```text
trigger   child with resident heap pages calls sys_exit
   ↓
handler_exit holds SCHEDULER, calls terminate_current
   ↓
terminate_current → heap_free_range/mmap_free_range
   ↓
shootdown_range → build_tlb_target_mask → current_scheduler().lock()   [recursive]
   ↓
same-CPU spin on non-reentrant spin::Mutex, IF=0
   ↓
ChildExit wake never published  →  parent stays Blocked in OB_WAIT
   ↓
( SMP>1: every other CPU also spins on SCHEDULER, IF=0 )
   ↓
observable hang: shell never returns
```

---

## 8. Fix

`neodos-kernel/src/arch/x64/paging.rs` only. `build_tlb_target_mask()` no
longer queries the scheduler; it computes a lock-free mask of **all online
CPUs except the caller**:

```rust
#[inline]
fn tlb_target_mask(my_cpu: usize, online_cpus: usize) -> u64 {
    if online_cpus == 0 || my_cpu >= 64 { return 0; }
    let online = if online_cpus >= 64 { u64::MAX } else { (1u64 << online_cpus) - 1 };
    online & !(1u64 << my_cpu)
}

fn build_tlb_target_mask() -> u64 {
    let my_cpu = unsafe { crate::arch::x64::cpu_local::this_cpu_id() } as usize;
    let count = crate::arch::x64::cpu_local::cpu_count() as usize;
    tlb_target_mask(my_cpu, count)
}
```

Why this restores the invariant:

- **Lock discipline:** the page-free helpers are reachable with `SCHEDULER`
  held. `build_tlb_target_mask` is now pure/lock-free, so no recursive
  acquisition is possible. `cpu_count()` and `this_cpu_id()` are per-CPU
  atomics/GS reads, not the scheduler lock.
- **Completeness:** the kernel uses one shared address space (one CR3) with the
  user window mapped on every CPU, so every online CPU can cache a freed user
  page. Targeting all of them is strictly more correct; a spurious remote
  invalidation is harmless.
- Boundedness is preserved: `tlb_shootdown` keeps its ACK timeout, so even a
  target CPU that is momentarily spinning with interrupts disabled cannot
  re-introduce a hard deadlock.

Regression tests (`paging.rs::register_paging_tests`, wired in
`testing.rs`): `tlb_target_mask_excludes_self`, `tlb_target_mask_empty_on_up`,
`tlb_target_mask_is_lock_free` (calls the builder **while holding
`SCHEDULER`** — would self-deadlock on the old code), and
`tlb_target_mask_pure_under_scheduler_lock`.

---

## 9. Validation

Kernel suite: `neodev test` → **749/749 PASS** (745 on `develop` + 4 new
regression tests). No `SCHED_WARN`, `READY_WHILE_RUNNING`, `STALE_RSP_DISPATCH`
or `STACK_OWNERSHIP_CONFLICT` are reported.

<!-- VALIDATION_RESULTS -->

### 9.1 Target scenario: child exit wakes the parent

`tree C:` is the reliable trigger (large resident heap at exit).

| Build | Scenario | Result |
| ----- | -------- | ------ |
| `develop` (pre-fix) | SMP2, `tree C:` | **3/3 hang** (parent stuck in `OB_WAIT`) |
| fix | SMP2, `tree C:` x8 boots | **7/7 real runs returned**; 1 false positive from a lost `sendkey` batch (shell idle at the prompt, no keystrokes registered) |
| `develop` (pre-fix) | SMP4 mixed `ipconfig/tree/cpuinfo/drives` | 1 child hang (and 4 boot gaps, below) |
| fix | SMP4 mixed | shell reached, `ipconfig`/`tree` returned; the batch hit the boot gap 4/5 (below) |

Mixed sequence at SMP2 (`ipconfig, tree C:, cpuinfo, tree C:, drives, ...`)
also returns every command on the fixed build. The kernel suite is
749/749 with no scheduler invariant warnings.

### 9.2 Pre-existing boot gap (separate defect, NOT fixed here)

The issue's signature 1 ("kernel→user gap") reproduces independently and
identically on `develop` and on the fix, at SMP4:

| Build | SMP2 (8 boots) | SMP4 (5 boots) |
| ----- | -------------- | -------------- |
| `develop` (pre-fix) | 1 boot gap / 7 pass | **4 boot gap / 1 child-hang** |
| fix | 0 boot gap / 8 pass | **4 boot gap / 1 pass-through** |

The SMP4 boot-gap rate is unchanged (4/5 → 4/5), so the fix neither causes
nor resolves it. Frozen per-CPU evidence for the boot gap:

- `develop` SMP2 gap: CPU0 in `net::netd_entry`, CPU1 `hlt_once` (idle),
  serial stops right after the post-test evidence dump, before
  `NEOINIT_LOAD_START`.
- fix SMP4 gaps: two or three CPUs frozen in the `SCHEDULER` acquire spin
  inside `arch::x64::idt::timer_handler_inner`, while another CPU sits in a
  filesystem/block path (`fs::neodos_v2::…::read`, `globals::flush_cache_if_needed`)
  or `netd_entry`. The serial always stops at `[SM] start service idx=0`
  (starting `Dhcpc`).

This is consistent with a lock-order deadlock among `SCHEDULER`,
`PAGE_CACHE`, `BLOCK_DEVICES`/VFS during service auto-start, not with the
exit-path recursion. It is tracked separately (see §10) and is out of scope
for this change.

---

## 10. Limitations / follow-up

- **Signature 1 (boot kernel→user gap) is NOT fixed by this change.** It is a
  separate, pre-existing lock-order deadlock during service auto-start that
  reproduces on `develop` with the same rate (SMP4 4/5, §9.2). Tracked in #343;
  #331 is intentionally left open until it is resolved.
- **SMP1 is slow to print `tree`** and, on at least one run, hit a pre-existing
  **kernel-heap allocation panic** during NeoInit's shell-spawn loop
  (`allocation error: Layout { size: 12288, align: 8 }`). On a 1-CPU system the
  new mask is `0`, identical to the old code, so this is unrelated to the TLB
  change. Tracked separately.
- `neodev test` still reports `Shell tests FAILED or not run`: the harness only
  waits ~9 s after `ALL_TESTS_COMPLETE`, while the shell needs DHCP/service
  start-up (~40 s). That is a harness detection-window limitation, not #331.
- #338 (Ring-0 frame published `Ready` for a user thread) is a **separate**
  defect and is untouched here.
- The scheduler lock is still held across memory free + IPIs in
  `terminate_current`; the fix removes the deadlock, not the long critical
  section. A future change could move the free outside the scheduler lock.
