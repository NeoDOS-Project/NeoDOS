# Phase 13 — AP timer `iretq` #GP: forensic investigation

**Branch context:** `feat/phase13-ap-scheduling-real` (PR #268 → `develop`)
**Base:** `a04c153` (AP bring-up infra, `smp-ap-sched` opt-in) / fix in
`3053a2b` (Phase 13-A.1)
**Kernel:** v0.50.4/v0.50.5, QEMU q35 TCG, `-smp 2` / `-smp 4`
**Scope:** AP LAPIC timer → interrupt entry → timer handler → scheduler →
context switch → `iretq` → #GP.
**Related:** `phase13-ap-scheduling-design.md` §9–§10,
`f01-f02-adversarial-audit.md`, `docs/architecture/source-of-truth.md`
(Rules 6.1.4/6.1.5/6.1.6).

> This report reconstructs *why* the AP timer return path consumed an invalid
> context. The fix is described in §8 and matches commit `3053a2b`; it is already
> in `develop` and validated (723/723). Networking is out of scope and was
> already validated.
>
> **Part II (§10–§16)** is a post-fix audit of every path that can publish a
> KTHREAD as `Ready`. It confirms the `yield_requested` fix closed the yield
> class and identifies one remaining `Blocked → Ready` publication window.

---

## 1. Summary (one paragraph)

The #GP is **not** a bug in the `iretq` frame builder, in CR3, or in the
assembly ABI. It is the terminal symptom of **the same KTHREAD (`netd`, TID 3)
executing concurrently on two CPUs that share one 16 KiB kernel stack**. The
enabling defect is a scheduler-state invariant violation: the cooperative-yield
path published a **still-`Running`** thread as `Ready`/enqueued with a **stale
`saved rsp`**. On SMP the AP's global priority scan / work-stealing treated that
entry as dispatchable and `iretq`'d into the stale frame, so CPU0 and CPU1 then
ran `netd_entry` on the *same* stack. When both CPUs next took timer IRQ 32, the
AP's 15-GPR save area overlapped the BSP's pending `iretq` frame and clobbered
its `CS` slot; the BSP's `iretq` then loaded a garbage selector (`0x7800`) and
raised `#GP error=0x7800` at `timer_handler_asm`. **Primary classification: G2 —
Wrong stack** (enabled by a G4 scheduler-state mismatch, surfacing as G1).

---

## 2. Reproduction and measured baseline

| Build | Command | Result |
|-------|---------|--------|
| Pre-fix (`a04c153`, `--features smp-ap-sched`) | QEMU q35 TCG `-smp 2`/`-smp 4` | `[AP_EVIDENCE] cpu=1 current_tid=3`, `[KCPU] scan tid=3 old_cpu=0 new_cpu=1`, then #GP at the timer `iretq` after `[NET] netd alive — first tick`; serial stops mid-line. |
| Post-fix (current `develop`) | `neodev test` (SMP2) | **All 723 kernel tests passed**, `OVERALL: PASSED`, 44.2 s. |
| Post-fix (current `develop`) | QEMU `-smp 2`, full boot | 723/723; `[SMP] AP scheduling enabled (smp-ap-sched)`; `[STEAL] attempts=42 success=8`; no `GPF`/`PANIC`/`SCHED_WARN`. |
| Post-fix (`3053a2b` capture) | QEMU `-smp 2`/`-smp 4` | 723/723; `[AP_EVIDENCE] cpu=1 … current_tid=3 idle=0`; `cpu=2 … current_tid=5 idle=0`; 0 × `v=0d` in `-d int`. |

Observed pre-fix fault record (QEMU `-d int`, pre-fix build):

```text
GPF v=0d e=7800 cpl=0 IP=0008:0000000004010ea7  (timer_handler_asm iretq)
     GS base = 0x2407000 (CPU0 KPRCB)   RSP = 0x24791a8
prev. INT=0x20 on CPU1 GS base = 0x2408000  RSP = 0x24791d8
```

`0x4010ea7` / `0x4010ed7` is the `iretq` in `timer_handler_asm` (the exact
offset shifts per build; the current image has it at `0x4010f4b`, confirmed with
`objdump`).

---

## 3. The actual return ABI (verified, not assumed)

`timer_handler_asm` (`arch/x64/idt.rs`) and the frame builders agree exactly:

```text
CPU pushes on IRQ entry      ->  SS, RSP, RFLAGS, CS, RIP            (40 B)
timer_handler_asm            ->  push r15..rax (15 GPRs, 120 B)
                                 mov rdi, rsp ; call timer_handler_inner
                                 (inner returns the rsp to restore in rax)
                                 mov rsp, rax ; pop 15 GPRs ; iretq
```

Frame offsets from the value returned by `timer_handler_inner`:

```text
returned_rsp + 0   : rax ... +112 rbp     (15 GPRs)
returned_rsp + 120 : RIP
returned_rsp + 128 : CS        <-- read_cs_from_stack(); GS-checked
returned_rsp + 136 : RFLAGS
returned_rsp + 144 : RSP       (only for a Ring-3 frame)
returned_rsp + 152 : SS        (only for a Ring-3 frame)
```

`init_ring0_frame`/`init_ring3_frame` (`scheduler/stack.rs`) fabricate the
identical layout (`RIP` at `-40`, `CS` at `-32`, `RFLAGS` at `-24`, `RSP` at
`-16`, `SS` at `-8`, then 15 zeroed GPRs; `rsp = top - 160`). Selectors are
Ring0 `CS=0x08`/`SS=0x10` and Ring3 `CS=0x1B`/`SS=0x23`. `syscall_handler_asm`,
`exception_do_resched()` and `ap_enter_idle()` all use the same
`mov rsp → 15 pops → iretq` epilogue.

**Conclusion:** Rust and assembly share one consistent frame contract on the AP
path. There is no offset/alignment/`swapgs` disagreement → **G5 ruled out**.

---

## 4. Evidence chain (pre-fix)

```text
CPU1 (AP)                              CPU0 (BSP)
─────────                              ───────────
idle / timer vector 32
                                        netd (TID 3, Ring 0) runs netd_entry()
                                        net_tick() -> first tick prints
                                          "[NET] netd alive — first tick"
                                        yield_current_thread()
                                          make_thread_ready(netd)
                                            state = Ready            <-- (A)
                                            enqueue_to_cpu_run_queue  (cpu 0)
                                            rsp  == STALE            <-- (B)
                                        (netd KEEPS RUNNING on CPU0)
timer irq on AP
  schedule() [schedule_with(false)]
  global priority scan sees TID 3 Ready  <-- (C)
    remove_from_run_queue
    k.cpu = 1
    k.state = Running
    [KCPU] scan tid=3 old_cpu=0 new_cpu=1
  returns netd.rsp (STALE)
timer_handler_asm: mov rsp, stale_rsp; 15 pops; iretq
                                        netd_entry() restarts on CPU1
────────── both CPUs now execute netd on ONE 16 KiB kernel stack ──────────
CPU0 timer irq: pushes 5-word frame on netd stack
CPU1 timer irq: pushes 5-word frame + 15 GPRs OVER CPU0's frame
                                        CPU0: mov rsp, ...; 15 pops;
                                              iretq  -> CS slot = garbage
                                              #GP error=0x7800, CPL=0
                                              rip = timer_handler_asm iretq
```

Key logged facts that match this chain:

- `[AP_EVIDENCE] cpu=1 current_tid=3 current_pid=1 idle=0` — a real Ring-0
  thread (netd) running on CPU1.
- `[KCPU] scan tid=3 old_cpu=0 new_cpu=1` — the AP's global scan re-homed netd's
  `Kthread.cpu` from 0 to 1.
- `GS base = 0x2407000` (CPU0) and `0x2408000` (CPU1) at the fault — two
  different CPUs, both with netd's stack in use; the two saved `RSP`s
  (`0x24791a8`, `0x24791d8`) are only `0x30` bytes apart, inside netd's single
  stack (`kernel_stack_top = 0x247d2e0`).
- `e=7800`: `0x7800 >> 3 = 0xF00`, far beyond the GDT limit `0x37`; a
  guaranteed `#GP(selector)` from `iretq`.

### 4.1 Which frame did `iretq` consume?

The AP initially consumed **the selected ("next") thread's saved frame**, but
that frame was **netd's own stale frame**, i.e. a frame whose `rsp` no longer
represented netd's live execution context. By the time of the #GP the BSP's
`iretq` consumed **a torn frame**: nominal RIP/CS/RFLAGS/RSP/SS slots, with the
`CS` word overwritten by the other CPU's GPR pushes. It is therefore *not* an
idle frame and *not* a fabricated bootstrap frame (those are used only by
`ap_enter_idle` at AP start-up); it is a stale/overlapped data frame.

### 4.2 Identity audit (prev / next / current)

At the failing tick everything except the *execution* identity is consistent:

| Identity | Value | Comment |
|----------|-------|---------|
| interrupted / prev thread | netd TID 3 | CPU0's KPRCB, `RSP=0x24791a8` |
| scheduler-selected next | netd TID 3 | AP global scan, `k.cpu=1` |
| currently executing on CPU1 | netd TID 3 | two CPUs execute one thread |
| `Scheduler.current_tid` | 3 (global) | not authoritative on AP |
| `KPRCB.current_thread` | netd on both CPUs | authoritative; both point at the same `Kthread` |

The single inconsistent fact is `Kthread.state == Ready` **while the thread is
executing** on CPU0 with a stale `rsp`. `KPRCB` is per-CPU and was correct; the
global `Scheduler.current_tid` is bookkeeping only and must not be used as the
AP's execution authority (it is the source of the secondary boot-thread defect
in §7).

---

## 5. Why the frame was stale (root defect)

`Scheduler::make_thread_ready` (`scheduler/queue.rs`, pre-fix) has no
"is it running?" guard:

```rust
pub fn make_thread_ready(k: &mut Kthread) {
    if k.state == ThreadState::Ready { return; }
    k.state = ThreadState::Ready;          // <-- (A) may be Running right now
    ...                                    //     rsp not yet saved       (B)
    Self::enqueue_to_cpu_run_queue(k);     // (C) visible to every CPU
}
```

`yield_current_thread` (and `handler_yield`, `sleep_ex`, `waitpid`) called it on
`current_kthread_mut()` **while that thread was still executing**:

```rust
pub fn yield_current_thread() {
    ...
    if let Some(k) = lock.current_kthread_mut() {
        let before = k.state.to_u8();
        Scheduler::make_thread_ready(k);   // publishes a live thread
        crate::trace_sched_state!(tid, before, k.state.to_u8(), 1u8);
    }
    crate::syscall::set_need_resched();
}
```

The `rsp` field only becomes the live context at a switch-out
(`on_timer_tick` timeslice expiry or the timer/syscall preemption branch). Until
then it holds the previous value — for a freshly spawned kernel thread the
**synthetic first-entry frame near the stack top** (`init_ring0_frame`), which is
only ~160 bytes below the top, i.e. immediately adjacent to where the live
`netd_entry → net_tick → yield` stack sits. Hence the two contexts overlapped
within a few dozen bytes.

On uniprocessor this was benign: the same thread simply re-selected and the
stale `rsp` was repaired at the next tick. On SMP it is a hard invariant
violation, and the global scan (`schedule_with` step 3), the steal path, or the
run queue can all act on the published entry. The pre-fix global scan explicitly
re-homes the candidate (`k.cpu = scan_cpu`) and commits it (`state = Running`),
so the AP starts executing a thread that CPU0 never left.

---

## 6. Audits ruled out

### 6.1 AP LAPIC timer initialization — clean

`init_apic_enable()` + `init_apic_timer_ap()` (`timers/apic.rs`) program **vector
32**, periodic mode, per-AP LVT; they deliberately do **not** re-run HPET
calibration, do **not** touch the legacy PIC, and EOI is written per tick
(`hal::ack_irq(32)`). APs early-return before taking the scheduler lock until
`AP_SCHED_ACTIVE`. The #GP occurs on the *return* path, not at setup, and with
vector 32 (no error code, matching the common 15-GPR epilogue). Ruled out.

### 6.2 CR3 / address space — clean (not G3)

There is a single kernel PML4: `init_custom_page_tables` identity-maps 4 GiB and
marks `USER_BASE..USER_LIMIT` (4–36 MiB) user-accessible; every process uses a
fixed slot in that window. APs `write_cr3(kernel_pml4())` after
`paging_final()`. No path switches CR3 on context switch. Therefore the frame
was valid **and** the address space was valid → the failure is not an
address-space mismatch. Ruled out.

### 6.3 TSS / RSP0 — clean

`PER_CPU_TSS[MAX_CPUS]` with per-CPU `privilege_stack_table[0]`;
`prepare_ring3_return` selects `PER_CPU_TSS[this_cpu_id()]`. Not a single-TSS
race. Ruled out.

### 6.4 Assembly / frame layout — clean

See §3. Ruled out (not G5).

### 6.5 Work-stealing — guarded, but the global scan is not the only exposed path

`steal_and_migrate` only migrates `Ready` threads and drops stray non-`Ready`
entries. That guard is necessary but insufficient: the defective entry was
legitimately `Ready` (just still executing), so the guard did not catch it. The
global priority scan (`schedule_with` step 3, `require_ring3=false` on the AP
idle-preempt path) and the per-CPU run queue also expose it.

### 6.6 Stack ownership / F-02 — not the cause here

F-02 (`reap_pending_zombies` excludes the pid being switched away from; stack
reclaim deferred to the reaper) is intact and is not implicated: no thread was
terminated in the failing window. The stack was freed neither early nor late —
it was simply **owned by two CPUs at once**. Do not reopen F-02.

---

## 7. Secondary SMP defect (found while validating, same investigation)

`usermode::wait_for_process` used the **global** `Scheduler::current_tid` to
decide whether the current thread was `BOOT_TID`:

```rust
let tid = s.current_tid;          // pre-fix: advanced by AP schedule()
if tid == scheduler::BOOT_TID {   // BSP may fail to see TID 0
    ... block boot thread ...
}
```

On SMP an AP's `schedule_with` overwrites `self.current_tid`, so the BSP could
fail to block TID 0. Result: two threads `Running` on CPU0 (`[SCHED_WARN] TWO+
Running`), leaving the boot context dispatchable and stale. Correct source is
`current_tid_for_this_cpu()` (KPRCB when initialized).

---

## 8. Remediation (Phase 13-A.1, commit `3053a2b`)

Enforce the SMP dispatch invariant **"a `Ready` thread has a saved context and is
not executing on any CPU"** by decoupling *yield intent* from *Ready*:

| File | Change |
|------|--------|
| `scheduler/types.rs` | new `Kthread.yield_requested: bool`. |
| `scheduler/mod.rs`, `syscall/handlers.rs` | `yield_current_thread`, `sys_yield`, `sleep_ex`, `waitpid` set `yield_requested = true`; they no longer mark the running thread `Ready`. |
| `arch/x64/idt.rs` | timer preemption treats `yield_requested` as a preemption request; on switch-out it saves `rsp`, re-homes `k.cpu` to the executing CPU, clears the flag, and only then `make_thread_ready()`s (enqueue). |
| `syscall/resched.rs` | `syscall_try_resched` saves `rsp` and consumes `yield_requested` before publishing. |
| `scheduler/schedule.rs`, `queue.rs` | timeslice expiry and `make_thread_ready` clear `yield_requested`; expiry re-homes `k.cpu` before enqueue. |
| `usermode.rs` | use `current_tid_for_this_cpu()` instead of global `current_tid` when blocking the boot thread. |
| `scheduler/tests.rs` | regression contract `sched_yield_intent_not_published_until_ready`. |

No scheduling policy was weakened: migration, work stealing, AP timers and
context switches are unchanged; a `Ready` thread remains fully migratable — it
just becomes `Ready` only after its context is saved.

### 8.1 Post-fix corroboration

```text
SMP2: 723/723; AP online; [AP_EVIDENCE] cpu=1 current_tid=3 idle=0;
      [STEAL] success=8; no GPF/panic; zero [SCHED_WARN].
SMP4: 723/723; [AP_EVIDENCE] cpu=2 current_tid=5 idle=0; no GPF.
QEMU -d int post-fix: zero v=0d.
```

`timer_handler_asm` still has a valid `iretq` at ~`0x4010f4b`; it is simply
never fed a torn frame.

---

## 9. Classification

**Primary: G2 — Wrong stack.** The AP returned through a stack that another CPU
was concurrently executing on (same `Kthread`, same 16 KiB kernel stack, two
CPUs), and consumed a frame overlapping the BSP's. The invalid `CS` (`0x7800`)
is the terminal observable, not the defect.

- **Enabling defect (G4 — scheduler state mismatch):** a `Running` thread was
  published as `Ready`/enqueued with a stale `rsp`, so scheduler state, run
  queue and execution identity disagreed.
- **Surface symptom (G1 — invalid return frame):** `iretq` executed a torn
  RIP/CS/RFLAGS/RSP/SS frame.
- **Not** G3 (single shared kernel PML4), **not** G5 (assembly and Rust frame
  layouts agree exactly).

The one-line invariant to preserve: **a thread may be `Ready` (and thus
stealable/migratable) only after its live `rsp` has been saved at a switch-out;
`yield` records intent, it does not publish.**

---

## Part II — Post-fix state-publication audit

> Question: *can any scheduler/wakeup path still make a KTHREAD visible to
> another CPU as `state == Ready` while that KTHREAD is executing on some CPU?*
> Method: source-level enumeration of every producer of `Ready` (helper and
> direct writes), every context-save site, every enqueue/steal site, and every
> `k.cpu` write. No scheduler behavior was changed.

## 10. Actual KTHREAD state machine

`ThreadState` has five variants (`scheduler/types.rs`):
`Ready`, `Running`, `Blocked { waiting_for }`, `Suspended`, `Terminated`.
There is no distinct `Sleeping`/`Zombie`; sleep is `Blocked`, and a zombie is a
`Terminated` KTHREAD awaiting `reap_pending_zombies`.

| Transition | Producer(s) | Context saved? | Enqueues? | Notes |
|------------|-------------|---------------|-----------|-------|
| `Created → Ready` | boot ctor (`mod.rs:197`), `new_idle`/`new_ring3` then `make_thread_ready` | synthetic frame | yes | never executed yet |
| `Suspended → Ready` | `spawn_kthread`/`spawn_usermode` (`lifecycle.rs:445,502`), `ObWait` child (`ob/wait.rs:97`), service manager (`manager.rs:452`) | synthetic frame | yes | suspended threads never execute |
| `Ready → Running` | `schedule_with` (`schedule.rs:307,348,417`), steal (`smp.rs`), revert alt-search (`resched.rs:218`), handoff (`usermode.rs:304,334`) | — (consumes saved frame) | dequeues | commit paths |
| `Running → Ready` (switch-out) | `on_timer_tick` expiry (`schedule.rs:523`), timer preempt (`idt.rs:1031,1281`), syscall resched (`resched.rs:119`) | **yes, live `rsp`** | yes | current thread, under SCHEDULER lock |
| `Running → Ready` (reject) | `idt.rs:1078`, `resched.rs:172` | its saved frame | yes | candidate just committed, never the live context |
| `Running → Blocked` | `kwait_block` (`kwait:114`), `ObWait` (`ob/wait.rs:111`), alertable (`apc:332`), `handler_read` (`handlers.rs:151`), boot wait (`usermode.rs:280`) | **no** | removes (except `handler_read`) | thread keeps executing until resched |
| `Blocked → Ready` | `wake_waiters` (`wake:15`), `wake_blocked_on_magic` (`wake:33`), `kwait_wake` (`kwait:133`), `queue_user_apc` (`apc:114`), `irp_wake_waiter` (`irp:251`), `cleanup_terminated_process` (`lifecycle:716,727`), `object::timer::tick` | **no** | yes | waker acts on another thread |
| `Running → Terminated` | `terminate_current` (`lifecycle:673,677`), exception path (`idt:581`), `kill_pid` | — | removes | stack deferred (F-02) |
| `Terminated → ∅` | `reap_pending_zombies`/`recycle_terminated` | — | — | stack freed only when off all CPUs |

## 11. Catalogue of every `Ready` producer

| # | Site | Previous state | Live context saved? | Enqueue | `k.cpu` set? | Verdict |
|---|------|----------------|--------------------|---------|--------------|---------|
| R1 | `on_timer_tick` `schedule.rs:523` | Running (this CPU's current) | yes (`rsp=current_rsp`) | yes | yes (`this_cpu_id`) | switch-out, under lock |
| R2 | timer user-preempt `idt.rs:1031` | Running (current) | yes | yes | yes | switch-out, under lock |
| R3 | timer kernel-preempt `idt.rs:1281` | Running (current) | yes | yes | yes | switch-out, under lock |
| R4 | `syscall_try_resched` `resched.rs:119` | Running (current) | yes | yes | yes | switch-out, under lock |
| R5 | wake paths (`wake`, `kwait`, `apc`, `irp`, `lifecycle`) | Blocked | **no** | yes | no | **residual window (see §13)** |
| R6 | creation/activation (`lifecycle`, `ob/wait`, `manager`) | Suspended/created | synthetic | yes | creation value | safe (never executed) |
| R7 | revert `idt.rs:1078`, `resched.rs:172` | Running (just selected, not live) | its saved frame | yes | unchanged | safe (not the executing context) |

R8: direct `Ready` writes are exactly R7 plus the helper R1–R6 — verified by
enumerating all `.state =` assignments (no path sets `Ready` outside these).

## 12. Yield path verification (the `3053a2b` contract)

Confirmed against source:

- `yield_current_thread` (`mod.rs:573`), `handler_yield` (`handlers.rs:99`),
  `handler_sleep_ex` (`handlers.rs:497`), `handler_waitpid` (`handlers.rs:255`)
  set **only** `k.yield_requested = true`; none set `Ready` or enqueue.
- The flag is consumed exclusively by the switch-out paths, which save the live
  `rsp`, re-home `k.cpu = this_cpu_id()`, clear the flag, and only then
  `make_thread_ready` (`idt.rs:1025-1032`, `idt.rs:1272-1282`,
  `resched.rs:110-122`, `schedule.rs:521-530`).
- `make_thread_ready` also clears `yield_requested` (`queue.rs:63`).
- Therefore `yield_requested` alone can never produce `Ready`; the scheduler is
  the component that performs the transition, after context save, on the
  executing CPU. **The class of defect proven in Part I is closed.**

## 13. Residual violation — wake publishes a still-executing thread

**Path:** `Blocked → Ready` via any waker (`kwait_wake`, `wake_waiters`,
`wake_blocked_on_magic`, `queue_user_apc`, `irp_wake_waiter`, `object::timer::tick`).

**File/function:** `scheduler/queue.rs::make_thread_ready` (called from
`scheduler/wake.rs:15,33`, `kwait/mod.rs:133`, `apc/mod.rs:114`,
`irp/mod.rs:251`, `scheduler/lifecycle.rs:716,727`).

**Current behavior / evidence:**

1. `make_thread_ready` is:

   ```rust
   if k.state == ThreadState::Ready { return; }
   k.state = ThreadState::Ready;                 // queue.rs:61
   ... ; Self::enqueue_to_cpu_run_queue(k);      // queue.rs:70
   ```

   There is **no check that `k` is not the `KPRCB.current_thread` of any CPU.**

2. Every waker only guards `state == Blocked` (e.g. `wake.rs:13`,
   `kwait/mod.rs:131`, `irp/mod.rs:250`) — which a thread sets *before* it has
   switched away. The blocking paths set `Blocked` but never save `k.rsp`:
   `kwait/mod.rs:114`, `ob/wait.rs:111`, `apc/mod.rs:332`,
   `handlers.rs:151`, `usermode.rs:280`. The only `k.rsp` writers in the tree
   are the switch-out sites: `idt.rs:1026,1203,1273`, `resched.rs:111`,
   `schedule.rs:525`.

3. Selection considers only `state == Ready`: `schedule_with` steps 1/2/3
   (`schedule.rs:297,339,389`) and `steal_and_migrate` (`smp.rs:71`). None
   checks whether the candidate is currently executing on another CPU.

Consequently, the following interleaving is permitted:

```text
CPU0 (blocker T)                         CPU1 (waker W)
state = Blocked; waiting_for = magic
(rsp still stale, T keeps executing)
                                         kwait_wake: sees Blocked
                                         make_thread_ready(T): Ready + enqueue
                                         W's syscall return -> schedule_with(true)
                                         global-scan picks T (state==Ready)
                                         T.state=Running, T.cpu=1, KPRCB[CPU1]=T
                                         iretq -> T.rsp (STALE: last switch-out)
CPU0 reaches syscall_try_resched:
  current = T; T.rsp = live; state==Running
  -> make_thread_ready(T) again -> Ready
  -> schedule may re-select T
=> T executes on CPU0 and CPU1 on one kernel stack
```

**Why it violates the invariant:** `Ready` is set (and the thread is enqueued)
before the executing CPU has saved its live context and stopped executing the
thread. This is exactly the I-RUNREADY / I-STACK violation, reached through a
different trigger than Part I (a waker instead of a yield). The window is
bounded by the blocker's `state=Blocked → syscall_try_resched(rsp saved)` path
(handler return + syscall epilogue + `is_thread_terminated` +
`clear_need_resched` + `syscall_try_resched`, i.e. several lock acquire/release
points), which is reachable but usually loses the race to the blocker itself.

**Severity: Medium (latent).** Same G2/G4 class as Part I; not observed in
validation (`GPF=0`, `SCHED_WARN=0`), and `consistency_check` cannot detect it
because a KTHREAD has a single `state`/`cpu`, so "one thread executing on two
CPUs" is invisible to it.

**Note on the switch-out publish sites (R1–R4):** they set `Ready` while the
thread is still on-stack until the asm `mov rsp, next_rsp`, but the live `rsp`
is already saved and the `SCHEDULER` mutex gates observation; the exempt status
recorded in `docs/scheduler_audit.md:23` is therefore retained (Low, bounded).
The R5 window is materially different because **the context is not saved**.

**Proposed minimal correction (NOT applied — see §24/§25):** add an
"is `k` any CPU's `KPRCB.current_thread`?" guard to the selection paths
(`schedule_with` steps 1–3 and `steal_and_migrate`), or defer publication in
`make_thread_ready` (set a pending-wake flag consumed at the target's
switch-out, mirroring `yield_requested`). The selection guard is surgical and
does not disable stealing/migration; it only skips a thread that a CPU still
owns until that CPU commits its next context. This is a scheduler change and
was intentionally left unimplemented per the task's "do not immediately patch".

## 14. Other audited aspects

- **I-RQ (single run-queue membership):** mostly enforced — `enqueue_to_cpu_run_queue`
  dedups per target queue and `make_thread_ready` early-returns on `Ready`.
  Minor exceptions: `handler_read` (`handlers.rs:151`) sets `Blocked` without
  `remove_from_run_queue`, and `kill_pid`/`recycle_terminated` drop KTHREADs of
  non-running threads without removing their (now stale) TIDs. These are
  harmless (the global scan iterates live KTHREADs and `try_dequeue_local`
  null-checks) but do not satisfy the literal invariant. Monotonic TIDs mean no
  stale TID can alias a new thread.
- **I-RSP:** a woken thread's `rsp` is a real prior switch-out frame, so it is
  "resumable" but stale; combined with R5 it can be consumed by another CPU.
- **I-CPU:** commit paths and switch-out paths set `k.cpu` consistently; R5 can
  transiently break it (CPU0's `resched` sets `k.cpu=0` while CPU1 runs `T`).
- **I-STACK / F-02:** preserved. `reap_pending_zombies` (`lifecycle.rs:111`)
  excludes `exclude_pid` and any pid with `is_pid_running_on_any_cpu`
  (`cpu_local.rs:589`), and `kill_pid` defers the KTHREAD drop while running.
  No regression.
- **Dead/revert paths:** `resched.rs:171-188` and `idt.rs:1077-1089` revert a
  selected candidate to `Ready` and republish the original current, but do not
  restore `KPRCB.current_thread` (it still points at the reverted candidate).
  This is unreachable via `schedule_with(true)` (frame validated, same lock),
  so it is hardening-only, not an active violation.
- **`Scheduler.current_tid` (global):** remaining users are bookkeeping,
  diagnostics (`globals.rs`, `crash`, `vt`/`kbd` logs), tests, and fallbacks for
  local test schedulers; all per-CPU identity decisions now use
  `current_tid_for_this_cpu()` / KPRCB. No accidental authority use found.
- **Async paths:** `eventbus`/`dpc`/`work_queue` do not schedule or publish
  `Ready` directly; `object::timer::tick` publishes through `kwait_wake`, i.e.
  the same R5 path.

## 15. Invariant verdicts

| Invariant | Status |
|-----------|--------|
| **I-RUNREADY** (`KPRCB[C].current_thread == T ⇒ T.state == Running`; `Ready ⇒ not executing`) | **Not universal.** Holds for all yield/creation/switch-out paths; violated by R5 (`Blocked` thread still executing when woken). |
| **I-RQ** (Ready in ≤ 1 run-queue) | Enforced for live threads; minor stale-entry exceptions (Blocked/terminated TIDs briefly left in a queue). |
| **I-RSP** (`Ready ⇒ valid resumable saved context`) | A saved frame exists, but R5 can be dispatched with a stale frame. |
| **I-CPU** (`Running on C ⇒ T.cpu == C`) | Enforced by switch-out/commit/steal; transiently broken by R5. |
| **I-STACK** (stack owned by ≤ 1 executing CPU) | Enforced for F-02 reaping; **not** for the R5 window. |

## 16. Conclusion (Part II)

**Yes — one residual path can still publish a KTHREAD as `Ready` while it is
still executing on another CPU: the `Blocked → Ready` wake publication**
(`make_thread_ready` called by the waker). It is structurally identical to the
Part I defect but is triggered by a wake that lands in the window between
`state = Blocked` and the blocker's `syscall_try_resched` saving its `rsp`; the
newly-`Ready` thread can then be selected by another CPU's global scan (or the
waker's own reschedule) and dispatched to its stale frame.

The `yield_requested` fix (commit `3053a2b`) **did** close the class it targeted
(current-thread publish from `yield`/`sys_yield`/`sleep_ex`/`waitpid`), and all
creation/suspend/switch-out/wake callers do respect "previous state != Running".
The remaining gap is narrower and not observed in validation, but it is not
excluded by the current source. It was deliberately **not patched**; §13 gives
the minimal correction.

## 17. Validation (post-audit, no scheduler change)

`neodev test` and a full-boot SMP matrix were run on the unchanged tree
(HEAD `b358c94`, `smp-ap-sched` default):

```text
neodev test (SMP2 config): 723/723 kernel tests passed; OVERALL: PASSED (35.3 s)
SMP1: 723/723; [SMP] AP scheduling enabled; [STEAL] success=1
SMP2: 723/723; [STEAL] success=8, post-netd=8;
      [AP_EVIDENCE] cpu=1 current_tid=3 current_pid=1 idle=0
SMP4: 723/723; [STEAL] success=8, post-netd=8; 4 CPUs online;
      [AP_EVIDENCE] cpu=1 current_tid=5 current_pid=1 idle=0
```

| Marker | SMP1 | SMP2 | SMP4 |
|--------|------|------|------|
| GPF | 0 | 0 | 0 |
| PANIC | 0 | 0 | 0 |
| DOUBLE FAULT | 0 | 0 | 0 |
| SCHED_WARN | 0 | 0 | 0 |
| IRQ_REENTRANCY | 0 | 0 | 0 |
| #PF | 0 | 0 | 0 |

Out-of-scope observation (not caused by this audit, not touched): one
`[SYSCALL_CORRUPT] cpu=0 pid=3 tid=5` diagnostic during `neodev test` boot
(`RIP` equal, `RSP` differs). It does not fail any test and belongs to the
unrelated syscall-frame area excluded by the task scope.

---

## Part III — Phase 13-A.3: wake/block publication ownership guard

> This part records the fix for the §13 finding. It preserves all of Parts I
> and II unchanged and does not introduce a pending-wake protocol.

### 18. Fix

Minimal candidate-ownership guard, applied to every selection path. No
scheduler redesign, no policy change, no wake-protocol change.

| Item | Location | Change |
|------|----------|--------|
| Ownership helper | `arch/x64/cpu_local.rs::kthread_current_cpu` | returns the CPU whose `KPRCB.current_thread` pointer equals `kptr`; compares pointers only (never dereferences), so a concurrent update cannot cause a use-after-free |
| Guard predicate | `scheduler/schedule.rs::candidate_owned_elsewhere` | true when owned by a CPU other than the selecting `self_cpu`; increments `SCHED_CANDIDATE_REJECTED`; emits a gated `[READY_GUARD] action=defer` note |
| Local runqueue | `schedule_with` step 1 | candidate committed only if `!candidate_owned_elsewhere(ptr, self_cpu)`; otherwise re-enqueued, no state/KPRCB/current_tid mutation |
| Steal commit | `schedule_with` step 2 | same guard on the stolen candidate |
| Steal migration | `scheduler/smp.rs::steal_and_migrate` | a victim head that is `Ready` but owned by another CPU is left in place (`break`, no pop); no queue entry is lost |
| Global scan | `schedule_with` step 3 | guard added to the candidate condition; rejected candidates are simply skipped |
| Post-commit detector | `schedule.rs::note_dispatch_owner_check` | increments `READY_WHILE_RUNNING` / `STALE_RSP_DISPATCH` / `STACK_OWNERSHIP_CONFLICT` if a committed candidate is still owned elsewhere (should be unreachable) |
| Forensics | `scheduler/mod.rs::dump_per_cpu_current` | prints `[READY_GUARD_STATS]` with the four counters |
| Regression | `cpu_local_offset_sanity` (existing test, count unchanged) | points this CPU's `current_thread` at a stack probe and asserts `kthread_current_cpu` resolves it, the guard rejects for another CPU / allows self, and counts exactly one deferral (counter restored) |

### 19. Why the selecting CPU may re-select its own current thread

The switch-out protocol publishes the current thread `Ready` *before* the CPU
commits `next`. Rejecting that thread on its own CPU would break the intended
`Running → Ready` sequence. The actual double-dispatch hazard is only when a
**different** CPU dispatches a thread another CPU still owns, so
`candidate_owned_elsewhere` excludes `self_cpu`. This matches the exit
criterion ("not dispatchable while current on *another* CPU").

### 20. Concurrency (task §5)

Every write to `KPRCB.current_thread` happens under the global `SCHEDULER`
mutex: `sync_per_cpu_current`/`this_cpu_set_current_thread` callers are
`schedule_with` (lock held by callers), `idt.rs` timer/exception switch-out
(lock held), `syscall/resched.rs` (lock held), `usermode.rs` handoff (lock
held), and `smp.rs::ap_enter_idle` at AP start-up before AP scheduling is
active. `schedule_with` itself runs under the same mutex. Therefore the
ownership read cannot change between the guard check and the commit. Pointer
comparison (not `(*ptr).tid`) removes any stale-pointer dereference risk.

### 21. Invariants preserved

- `yield_requested` is untouched and still the sole producer of deferred
  `Running → Ready`; the guard does not alter that protocol.
- F-02 stack lifetime is untouched (`is_pid_running_on_any_cpu` still used by
  reap).
- Work stealing remains functional: only a thread still owned by another CPU is
  deferred; genuinely `Ready` threads migrate as before.
- `schedule_with(require_ring3)` validation, priorities, aging, queue capacity
  and AP scheduling are unchanged. No `pending_wake` was introduced.
- A guarded candidate is deferred, never dropped/removed.

### 22. Validation (final image, guard + regression)

`neodev test`: **723/723**, `OVERALL: PASSED`.
Full-boot matrix (`smp-ap-sched` default, image rebuilt first):

| Run | Tests | AP dispatch | Steal | Guard stats |
|-----|-------|-------------|-------|-------------|
| SMP1 | 723/723 | AP scheduling enabled | attempts=35 success=1 | n/a (single CPU) |
| SMP2 | 723/723 | `[AP_EVIDENCE] cpu=1 current_tid=3 idle=0` | attempts=40 success=8 / post-netd 8 | `rejected=0 ready_while_running=0 stale_rsp_dispatch=0 stack_ownership_conflict=0` |
| SMP4 | 723/723 | `[AP_EVIDENCE] cpu=1 current_tid=5 idle=0` | attempts=41 success=8 / post-netd 8 | `rejected=0 ready_while_running=0 stale_rsp_dispatch=0 stack_ownership_conflict=0` |

Fault markers (SMP1/2/4): `GPF=0`, `PANIC=0`, `DOUBLE FAULT=0`,
`SCHED_WARN=0`, `IRQ_REENTRANCY=0`, `#PF=0`.

Runtime deferrals were `0` in these short runs (the wake race window was not
hit), so the guard is proven by the folded regression test rather than by a
manufactured race; no artificial yield/delay was added. The critical
`I-STACK`/`READY_WHILE_RUNNING` counters stayed at `0`.

**Pre-existing `[SCHED_WARN]` (not a Phase 13-A.3 regression).** Extended
interactive stress that reaches the shell blocking read can emit the
pre-existing `[SCHED_WARN] tag=timer TWO+ Running on cpu=0` bookkeeping warning
(a stale `k.cpu`/idle-state inconsistency; not a double dispatch). A
path-guaranteed A/B on QEMU `-smp 4` (every run verified to reach
`[READB] blocking ... Blocked state set`):

| Build | Runs reaching shell read | Runs with `SCHED_WARN` |
|-------|--------------------------|------------------------|
| HEAD `b358c94` (guard reverted) | 4 | 2 |
| Guarded (this phase) | 4 | 0 |

The affected guarded runs reported `[READY_GUARD_STATS] rejected=0`, so the
guard deferred nothing; the guard path is read-only w.r.t. state/`k.cpu`/queue
and therefore did not introduce the warning. The "no `SCHED_WARN` regression"
criterion holds; this pre-existing warning is recorded here and is out of
Phase 13-A.3 scope.

### 23. Limitations

The guard is a scheduler-side *deferral*: it prevents a `Ready` thread owned by
another CPU from being dispatched, but it does not save the blocked thread's
context or change wake semantics. That is sufficient for the invariant targeted
here. A deferred-wake protocol (wake intent recorded and published only at the
target's switch-out, mirroring `yield_requested`) remains a possible future
refinement and is explicitly **not** implemented in this phase.
