# Investigation — #476 VirtualBox SMP2 Ring-0 `INVALID_OPCODE` (`rip` in another thread's stack)

**Date:** 2026-10-04
**Branch:** `investigation/476-vbox-invalid-opcode` (from `develop` @ `f1b5bb3`)
**Environment:** VirtualBox 7.2.20, EFI, SMP2, bridged (82540EM), 512 MB.
**Related:** #474 (QEMU SMP2 `rip=0x148`, fixed), #383 (allocator free-list
corruption, closed as not-reproducible), #384 (NeoInit user `#PF`, open),
#346 / F-02 (kernel-stack reap lifetime).

---

## Verdict

**Root cause NOT demonstrated.** The panic is real and intermittent, but the
first corrupt state could not be captured in a reproducible campaign. The
evidence points to a **memory-corruption family shared with #383/#384** in the
process lifecycle (kernel-stack reclaim / kernel-heap free-list), not to the
#474 publication mechanism. No speculative fix is applied.

---

## 1. Confirmed facts

### 1.1 The panic

```text
[EXC] ERROR: User exception unhandled (Terminate): type=14 rip=0xc8011c   (crashing run)
[FAULT] v=6 INVALID_OPCODE rip=0x24b922a cs=0x8 rsp=0x2519a20 cpu=1
[PANIC] class=UNKNOWN_CPU_EXCEPTION rsp=0x2519800 msg=Invalid opcode: rip=0x24b922a
```

Crash dump: `TID: 6 PID: 7 CPU: 1 ThreadState: 1`;
`Current TID: 6 Next TID: 10 Next PID: 8`.

### 1.2 Thread / stack map (from the crash run)

| tid | pid | name | notes |
|----:|----:|------|-------|
| 0/1/2 | 0 | boot/idle | |
| 3 | 1 | netpump | |
| 4 | 2 | neoinit | `ks_top=0x247d2c0` |
| 5 | 3 | Ntpd | **absent from the table at panic** (terminated/recycled) |
| 6 | 4 | NetApplier | stack frame `0x24b5220` |
| 7 | 5 | Dhcpc | stack frame `0x24b9220` |
| 8 | 6 | neoshell | |
| 9 | 7 | Ntpd | spawned right after the terminate; reuses `heap_base=0x10200000` (pid 3's) |

`rip=0x24b922a` is `0x24b9220 + 0xa`, i.e. a value **inside Dhcpc (tid 7)'s
kernel stack**, while `rsp=0x2519a20` is on the Ntpd² (tid 9) stack. The CPU
executed on one thread's stack pointer but transferred control into a *different*
thread's stack region.

### 1.3 It is not #474

- Different wild RIP (`0x24b922a` vs `0x148`) and different context (cpu 1, after
  a user `#PF` / terminate, shell already up).
- It occurred on a build that contains the #474 fix and its
  `ring0_publish_is_dispatchable` gate.
- Healthy VBox boots in the same sample show **no** `[EXC] … Terminate`; the
  panic only ever co-occurred with an unhandled user fault + termination.

### 1.4 The trigger is a service (`#PF` → terminate → restart)

A surviving run (`vbox_diag_5`) recorded the same trigger **without** a kernel
crash:

```text
[EXC] ERROR: User exception unhandled (Terminate): type=14 rip=0x1680fd0
      fault=0x2d4221d fcode=1 pid=5 tid=7 cpu=0      (Dhcpc)
[SPAWN] pid=7 tid=9 name=Dhcpc ... heap_base=0x10600000   (restart reuses pid 5's heap)
```

`fault=0x2d4221d` lies in the **kernel heap** (`0x2400000..0x3400000`), reported
as a user-mode Protection fault (`fcode=1`): a user thread dereferenced a
kernel-heap address. In the crashing run the terminated process was pid 3
(Ntpd) and its heap `0x10200000` was reused by pid 7 — the same pattern.

So: a service faults on corrupted memory, is terminated, is restarted reusing
the dead process's resources, and the kernel *occasionally* corrupts fatally in
that window.

---

## 2. What the existing guards say

- `READY_GUARD_STATS`: `rejected=0 ready_while_running=0 stale_rsp_dispatch=0
  stack_ownership_conflict=0`.
- `[DOUBLE_RUNNING] head=0`, no `SCHED_WARN`, no `STACK_OWNER_MISMATCH`.
- 16 `[SYSCALL_CORRUPT]` reports (pre-existing diagnostic noise, not a fault).

No existing invariant fires before the corruption: the corruption is below the
scheduler's current observability.

---

## 3. Hypotheses (not yet proven)

### H1 — kernel stack freed/reused while a CPU still executes on it (cross-CPU reap window)

Documented reachable path (F-02 audit, `docs/investigation/f01-f02-adversarial-audit.md` §F-02-A):

```text
exception (on faulting thread's stack)
  -> terminate_user_process -> terminate_current  (state=Terminated; defer_reap(pid))
  -> exception_do_resched -> schedule_with(true)
       commit next + sync_per_cpu_current(next)   // KPRCB now points to next
       reap_pending_zombies(self, prev_pid)       // excludes only THIS cpu's prev
  -> lock released, then asm: mov rsp,next_rsp ; pop ; iretq   // still on old stack
```

`is_pid_running_on_any_cpu(old_pid)` is false once `KPRCB` is repointed, so
**another** CPU's `reap_pending_zombies` can free the still-in-use stack. The
F-02-A fix excludes the switching CPU's own `prev_pid`; it does not close the
cross-CPU window. A freed 16 KB stack reallocated as another thread's stack
would corrupt that thread's frames → wild RIP when dispatched (matches #476),
and writes into a freed fallback-heap block would corrupt its free-list next
pointer (matches #383's `write` to `0xfffffffffffffffc`).

Caveat: the window between lock release and `mov rsp` is only a few
instructions; a 1-in-N-boot rate would require the race to be hit often. H1 is
plausible and unifying but **not demonstrated**.

### H2 — corrupted indirect call (APC/DPC/IRP callback) to a stack address

`dispatch_kernel_apcs` executes `(apc.function)(apc.context)` from the current
thread's `kernel_apc_queue`. A `call 0x24b922a` with the caller's `rsp` on the
current thread's stack matches the register/fault shape. What corrupts the
callback is unknown (same underlying corruption).

### H3 — user heap/page-table slot reuse without clearing mappings

`free_heap_slot` / `free_user_slot` + `alloc_*` on restart could leak stale
mappings or reuse a slot while its pages are still writable, explaining the
user `#PF` at a kernel-heap address. Not yet audited to a concrete defect.

---

## 4. #383 / #384 / #476 relationship

- **#383** (CLOSED): the range-based free routing fix (`2170577`) removed one
  free-list misroute, but the issue was closed with the residual corruption
  **"currently NOT reproducible"**, not root-caused. The audit itself lists
  double free / freeing a non-owned pointer in the kernel-stack or process
  teardown path as the remaining source.
- **#384** (OPEN): NeoInit user `#PF` at entry, ~3–5 % of VBox SMP2 boots, shell
  never starts; co-occurs with #383.
- **#476** (OPEN): user `#PF` on a service → unhandled terminate → intermittent
  kernel `#UD` with a wild RIP in another thread's stack.

Classification: **related, same family, root cause not established for any**.
All three are consistent with corruption in the process-lifecycle
memory/stack path; none is proven. #476 is **not** a #474 regression.

---

## 5. Diagnostics added (kept, not merged)

On the branch, all append-only and only executed on the fatal path:

- `panic_classification.rs::dump_forensic_info`: per-thread `cpu / rsp /
  ks_top / ks_size / idle / name`, a per-CPU `KPRCB` dump (`cur / tid / pid /
  rsp / ks_top / idle`), and a `[DUP_KSTOP]` scan for two live threads sharing a
  kernel stack.
- `arch/x64/idt/mod.rs` `invalid_opcode_handler`: `[#UD_STACK]` raw dump of the
  Ring-0 handler stack (includes the CPU-pushed exception frame).
- `exception/dispatcher.rs`: the user-terminate log now carries
  `fault / fcode / pid / tid / cpu`.

These make the first corrupt state reconstructable on the next reproduction.

---

## 6. Reproduction campaign (this session)

| Config | Runs | Result |
|--------|-----:|--------|
| VBox SMP2, diagnostic build | 10 | 10/10 `ALL_TESTS_COMPLETE`, 0 `#UD`; run 5 hit the `#PF`/terminate trigger and survived |
| VBox SMP2, #474-fixed build (earlier) | 4 | 1 `#UD` (this issue) |
| VBox SMP2, unfixed base | 1 | OK |
| VBox SMP2 + `stress_spawn` harness (128 spawn/exit children) | 1 | **VFS-contention hang**, not #476 |

The spawn-stress attempt was aborted: it produced a pre-existing VFS
lock-holder stall (owner `tid=8 state=Ready`, `preempt=1`), i.e. a #376-class
condition, before any #476 panic. The harness was re-disabled.

Limitations: VirtualBox offers no `-d int` low-level trace; the diagnostic rings
are global and were flooded by cpu0, so cpu1's context was not retained. The
issue is intermittent (~1 in 4–14 VBox boots in small samples).

---

## 7. Recommended next experiments

1. **Make the diagnostic rings per-CPU** (SCHED_EV/RSP/CTX/KCPU) or add a
   per-CPU "last N" so the faulting CPU's context survives.
2. **Precise switch-window detector/fix (H1):** per-CPU `SWITCH_OUT_KS`
   recorded at commit and cleared by a `call` inserted after `mov rsp` in
   `timer_handler_asm`, `syscall_handler_asm` and `exception_do_resched`;
   `recycle_terminated` refuses (or logs `[REAP_RACE]`) when a stack being freed
   matches a CPU's `SWITCH_OUT_KS`. This both demonstrates and fixes H1 with a
   regression test.
3. **Allocator integrity check on free** (`#383`): validate that a pointer being
   freed is owned by the heap / slab before writing the free-list link; log
   `[FREE_BAD]` with the caller. This would localise the #383 corruption source.
4. **Trigger amplification:** a bounded, deterministic process
   spawn→exit→restart loop (not the VFS-heavy `stress_spawn`) to raise the
   number of lifecycle transitions per boot without #376 contention.

---

## 8. Release recommendation

Do **not** cut `v0.51.4` claiming SMP stability while #476 (and #384/#383's
family) is unexplained. #474's fix is independently valid and release-ready; the
release decision depends on whether the project accepts shipping a known,
low-rate VBox SMP2 panic.

---

## 9. KStack ownership (Fase 2 audit)

```text
Quién crea la KStack
   spawn_usermode / add_ring3_process_with_stack:
     AlignedKStack::try_new_boxed() -> Box<AlignedKStack> (16 KiB)
     kernel_stack_top = box.ptr + KERNEL_STACK_SIZE
   spawn_kthread_named: same, init_ring0_frame.
   AP idle (register_ap_idle): pre-allocated AP stack, kernel_stack = None.
   BSP idle: static IDLE_STACK, kernel_stack = None.

Quién posee la KStack
   Kthread.kernel_stack: Option<Box<AlignedKStack>>, owned by
   Scheduler.kthreads[slot] (Box<Kthread>). Dropping the Kthread drops the stack.
   While the thread runs / is ready, its CPU(s) use it as the Ring-0 stack and
   TSS.RSP0 = kernel_stack_top for Ring-3 entry.

Cuándo deja de ser utilizable
   Only after every CPU that could still execute on it has left it. The
   intended proxy is `is_pid_running_on_any_cpu(pid)` (KPRCB.current_pid), but
   KPRCB is repointed *before* the `mov rsp`, so the proxy can already be false
   while a CPU is still physically on the stack -> the H1 window.

Quién decide liberarla
   recycle_terminated (via reap_pending_zombies / defer_reap_with_scheduler /
   cleanup_terminated_process), kill_pid (non-running branch), recycle_thread.

Qué CPUs pueden seguir teniendo una referencia
   Any CPU whose KPRCB.current_thread was the thread before the repoint and that
   has not yet executed `mov rsp, next_rsp`. The switching CPU itself is excluded
   by F-02-A's `prev_pid`; *other* CPUs are not.

Qué evento demuestra que una CPU ya dejó de utilizarla
   Executing `mov rsp, next_rsp` in the context-switch ASM. The H1 experiment
   marks this explicitly (`SWITCH_OUT_KS` before the KPRCB write,
   `switch_out_clear` right after `mov rsp`).
```

## 10. H1 experiment (`SWITCH_OUT_KS`)

Falsifiable experiment implemented on this branch (diagnostic only):

- `scheduler/diag/kstack.rs`: per-CPU `ACTIVE/KS/TID/PID/RSP/SIZE/NEXT_KS`, a
  256-entry event ring (`OUT`/`CLR`/`CONFLICT`), and `reclaim_conflict(ks_top)`.
- `cpu_local::this_cpu_set_current_thread_site`: calls `kstack::note(old, new,
  raw_read_rsp())` immediately *before* the `KPRCB.current_thread` write.
- `switch_out_clear` inserted in ASM **after** `mov rsp, …` in
  `timer_handler_asm`, `syscall_handler_asm` (both resched paths),
  `exception_do_resched` and `ap_enter_idle`.
- All stack-free sites (`recycle_terminated`, `kill_pid` non-running,
  `recycle_thread`) call `guard_kstack_reclaim`: if another CPU is mid-switch for
  that stack, they log `[KSTACK_RECLAIM_CONFLICT]` and **leak** the Box (bounded)
  instead of freeing it, so the experiment continues without corruption.
- `[KSTACK_RING]` / `[KSTACK_STATE]` are dumped on panic and all counters
  (`noted/cleared/conflicts`) are exported.

Hypothesis decision rule:

- **H1 CONFIRMED** if `[KSTACK_RECLAIM_CONFLICT]` is observed (CPU _x_ still owns
  stack _S_ while another CPU tries to reclaim _S_).
- **H1 REFUTED** if a sufficient termination-pressure campaign never produces a
  conflict.

### 10.1 Detector validation (essential)

Before trusting a negative result, the detector data path was unit-tested
(`scheduler/tests.rs::n476_kstack_switch_out_conflict_detector`): after `note`,
`reclaim_conflict(stack)` returns `Some` (with the recorded tid/pid/rsp); a
different stack returns `None`; after `switch_out_clear` it returns `None`.
Suite: **821/821 PASS** (QEMU).

### 10.2 Evidence — H1 NOT observed

| Campaign | Terminations | `noted` | `cleared` | **conflicts** | panics |
|----------|-------------:|--------:|----------:|--------------:|-------:|
| VBox SMP2, normal boots ×12 | 0 | — | — | **0** | 0 |
| VBox SMP2, churn 48 | 48 | 3865 | 71392 | **0** | 0 |
| VBox SMP2, churn 200 (123 ok / 77 NoMem backpressure) | 123 | 4006 | 74137 | **0** | 0 |

Raw evidence (`churn_3.log`, `churn_200.log`):

```text
[SPAWN_STRESS] done pid=6 ok=48  err=0   mode=CreateActivate
[KSTACK_STATS] after-churn noted=3865 cleared=71392 conflicts=0
[SPAWN_STRESS] done pid=6 ok=123 err=77  mode=CreateActivate
[KSTACK_STATS] after-churn noted=4006 cleared=74137 conflicts=0
```

The runtime `note`/`clear` counters prove the detector was executing on every
context switch; `reclaim_conflict` was consulted for every reap
(`guard_kstack_reclaim`) and never matched.

### 10.3 Verdict — **H1 NOT DEMONSTRATED (unsupported)**

- **0 conflicts in 171 terminations** with a validated detector.
- The observed #476 rate (≈1 panic in ~29 VBox boots, ≈0.2 terminations/boot)
  would require the switch-out window to be hit a large fraction of the time per
  termination; the churn is inconsistent with that.
- Caveat: the churn terminations are `sys_exit` (Ring-3 syscall path); a formal
  refutation of a vanishingly rare window is not possible. H1 is therefore
  classified as **not demonstrated / unlikely**, not as proven impossible.

No H1 fix is applied. The detector and all diagnostics are retained on the
branch for future campaigns; `stress_spawn` is left disabled.


