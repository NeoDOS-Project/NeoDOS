# Investigation — #340 netd / sys_yield / scheduling (2026-09-30)

**Date:** 2026-09-30
**Branch:** `develop` @ `65fb9ff`
**Method:** read-only source analysis, reproducible QEMU SMP1/2/4 captures,
`neotop` thread-table observation. **No source code was modified.**

---

## Baseline

```text
HEAD:        65fb9ff (develop)
branch:      develop
working tree: CLEAN
neodev test: 762/762 kernel PASS (Command/Shell = known #353 SMP2 harness flake)
```

## Issue

- **#340** — `[NET] netd is not reliably scheduled; sys_yield from dhcpd collapses
  the DHCP wait window`.
- Original symptom: `netd` is spawned but not reliably scheduled; the first DHCP
  DISCOVER can be lost; retries.

## Reproduction

Direct QEMU (`q35`, TCG, user NIC), same `disk_image.img`, only `-smp` varied:

| Config | Runs | `[NET] netd running` | DHCP `Network configured` |
|--------|------|----------------------|---------------------------|
| SMP1   | 5    | **0/5**              | **5/5**                   |
| SMP2   | 1    | 1/1                  | yes                       |
| SMP4   | 1    | 1/1                  | yes                       |

Reproducibility: **deterministic at SMP1** (5/5). `netd` never enters its loop,
**yet DHCP completes**. This independently reproduces the issue author's own
refutation.

## netd execution path

```text
main.rs:706  spawn_net_kthread(NETD_PTR)
  └─ scheduler/stack.rs:94  spawn_kthread_named(entry, PRIORITY_NORMAL, "netd")
       └─ lifecycle.rs:464  creates Kthread { pid: next_pid(1), cpu: 0, rsp: init_ring0_frame(...) }
            lifecycle.rs:484  state = Suspended
            lifecycle.rs:517  make_thread_ready(k)  -> Ready + enqueued on cpu 0
  └─ net/mod.rs:78  netd_entry() -> !   (Ring-0 kernel thread)
       loop {
         net_tick();                          net/mod.rs:120 (network_poll_all + arp/dns tick)
         scheduler::yield_current_thread();   scheduler/mod.rs:615
         ... (heartbeat iters<=5: "[NET] netd running"/"netd cpu=")
       }
```

netd is a **kernel thread** (Ring-0 frame), `PRIORITY_NORMAL`, `cpu=0`. It calls
`yield_current_thread()`, **not** `sys_yield` (RAX=1). #340's title conflates the
two actors; `dhcpd` is the Ring-3 caller of `sys_yield`.

## sys_yield path (dhcpd, Ring 3)

```text
libneodos sys_yield  (RAX=1)
  └─ arch/x64/idt.rs:371 syscall_handler_asm   (IDT[0x80].disable_interrupts(true))
       └─ syscall/handlers.rs:98 handler_yield
            k.yield_requested = true          (handlers.rs:108)
            crate::net::network_poll_all();   (handlers.rs:112)  <-- SYNCHRONOUS RX DRAIN
            set_need_resched()
       └─ syscall/resched.rs:125 syscall_try_resched
            k.rsp = current_rsp; k.yield_requested = false; k.cpu = this_cpu
            if k.state == Running { make_thread_ready(k) }   (resched.rs:171-177)
                 -> queue.rs:63 state=Ready, queue.rs:70 time_slice_remaining = FULL
            let next = scheduler.schedule_with(true)         (resched.rs:184)
```

`handler_yield` drains RX **synchronously** on every `sys_yield`. This is why
DHCP works on SMP1 even though netd never runs: RX is polled from the syscall
path, not from netd.

## SMP1 trace

`neotop` thread table on SMP1 (objective states):

```text
PID  PROCESO  TID  HILO      ESTADO   %CPU  CPU
  0  kernel     0  boot      Blocked
  0  kernel     1  idle/0    Ready          0
  1  kernel     2  netd      Ready    0.0    0    <-- never selected
  2  neoinit    3  neoinit   Blocked
  3  Netcfg     4  Netcfg    Ready   98.8    0    <-- dominates cpu0
  4  Dhcpc      5  Dhcpc     Ready    0.1    0
  5  neoshell   6  neoshell  Blocked
  6  neotop     7  neotop    Running  0.9    0
```

`netd` stays `Ready` with `%CPU 0.0` across every refresh; `Netcfg` gets 98.8%.

## SMP4 trace

```text
[SMP] AP scheduling enabled (smp-ap-sched)
[+] netd kernel thread spawned (TID 5)
[NET] netd running
[NET] netd cpu=2 rx=0 tx=0 ...
[AP_EVIDENCE] cpu=2 kprcb=... current_tid=5 current_pid=1 name=netd idle=0 qlen=0
[READY_GUARD_STATS] rejected=0 ready_while_running=0 stale_rsp_dispatch=0 stack_ownership_conflict=0
```

netd runs because APs have idle contexts: the idle-preemption path
(`idt.rs:1278`, `schedule()`) uses `require_ring3=false` and can select netd.

## First divergence

```text
SMP1:  spawn -> Ready (enqueued) -> [never selected] -> Ready forever
SMP4:  spawn -> Ready (enqueued) -> idle-preempt on an AP -> dispatched -> runs
```

The divergence is **not** in the yield or the frame; it is in **candidate
eligibility**: on SMP1 the only running context is Ring-3, so every selection
path is `require_ring3=true`.

## State transitions

Selection-path table (non-test):

| Context / path | Call site | `require_ring3` | can select netd (Ring-0)? |
|----------------|-----------|-----------------|---------------------------|
| syscall return | `resched.rs:184` | **true** | no |
| timer user-preempt | `idt.rs:1108` | **true** | no |
| exception resched | `idt.rs:596` | **true** | no |
| timer kernel-preempt | `idt.rs:1369` (`schedule()`) | false | yes |
| timer idle-preempt | `idt.rs:1278` (`schedule()`) | false | yes |

When the popped candidate is Ready but its frame is not Ring-3,
`schedule_with` re-enqueues it and does not commit (`schedule.rs:499-503`).

## CPU ownership

On SMP1 there is only cpu 0. netd is `cpu=0`, enqueued on cpu0's run queue.
`neotop` confirms `cpu=0`. No ownership anomaly.

## Runqueue

netd is Ready and enqueued (FIFO ring, `cpu_local.rs:55-93`). It is repeatedly
popped by the fast path and **rejected** by the Ring-3 gate, then re-enqueued
(`schedule.rs:499-503`). It never becomes head-and-committed.

## Return frame

netd's frame is the Ring-0 `init_ring0_frame` from
`lifecycle.rs:473`. It is a **legitimate** kernel-thread dispatch frame
(dispatched via `require_ring3=false`). It is not corrupt. The issue is purely
that no `require_ring3=false` path runs on SMP1.

## #354 relation

**Independent.** When netd/Netcfg are `Running`, the syscall return follows the
normal `make_thread_ready` branch (`resched.rs:171`), not the
`next_cs & 3 != 3` fallback (`resched.rs:222`) that #354 concerns. #354's
fallback only runs when the current thread is `Blocked`/`Terminated`.

## Root cause

**PROVEN (for the "netd never scheduled on SMP1" behaviour).**

```text
trigger:   SMP1 (single CPU), a Ring-3 thread (Netcfg) continuously runnable
divergence: every selection path while the current context is Ring-3 uses
            schedule_with(require_ring3 = true)
result:    netd (Ring-0 kernel thread) is structurally ineligible;
            it is popped, rejected by frame_is_ring3, and re-enqueued forever
consequence: netd never enters netd_entry; its Ready/rsp/frame stay valid
```

Aggravating factor: `Netcfg` re-arms its own time slice on every `sys_yield`
(`make_thread_ready` -> `queue.rs:70`) and spins 5 000 000 `spin_loop`s in Ring 3
between yields (`netcfg/src/main.rs:412`), so the current context on cpu0 is
essentially always Ring-3, denying the kernel-preempt path its chance.

**However, this is NOT the cause of the initial-DHCP symptom of #340:** DHCP
completes on SMP1 with netd never running (5/5), because the DORA wait loop
yields intensively and `handler_yield` drains RX synchronously. The DHCP defect
was the e1000 RX init order, proven and fixed in **#341** (merged `8ee470a`).

**But netd starvation DOES affect continuous connectivity.** `netd` is the
intended always-on RX pump; the timer IRQ does not poll RX. With netd never
selected on SMP1, RX is drained only when some user thread calls `sys_yield`,
and the only candidate (`dhcpd`, `loop { sys_yield() }` post-DORA) is itself a
Ring-3, intermittently-selected thread. Reporter-observed: with only `dhcpd`,
RX is intermittent. See #355.

## Evidence

- SMP1 netd `Ready`, `%CPU 0.0`, Netcfg `%CPU 98.8` (`neotop`).
- SMP1 5/5 netd absent + DHCP configured; SMP2/SMP4 netd present.
- Source: `resched.rs:184`, `idt.rs:596/1108` = `require_ring3=true`;
  `idt.rs:1278/1369` = `schedule()`.
- Prior art: **#264 / #282** (CLOSED, v0.50) describe the same kernel-thread
  starvation in `syscall_try_resched`; the mechanism is still present.

## Correction — continuous connectivity (2026-09-30)

The DHCP **initial** lease is not what netd is for; netd is the intended
always-on RX pump. The following corrects an earlier framing:

- `handler_yield` drains RX **only when a user thread calls `sys_yield`**
  (`handlers.rs:112`). The **timer IRQ does not poll RX** (verified: no
  `network_poll_all` in the timer/APIC path).
- Post-DORA, `dhcpd` enters `loop { sys_yield(); }` (`dhcpd/src/main.rs:759`),
  which drains RX — but `dhcpd` is a **Ring-3, never-blocking** thread and is
  therefore selected only **intermittently** (it competes with netcfg/shell for
  the Ring-3-only selection paths, exactly like netd competes for eligibility).
- Reporter-observed: with **only `dhcpd`** and netd starved, RX polling is
  **intermittent**, i.e. connectivity is not continuous.

Therefore on SMP1 there is **no reliable always-on RX pump**: netd is never
eligible (this report), and dhcpd is only intermittently selected. Initial DHCP
can still complete (intensive yields during DORA), but continuous connectivity
is not guaranteed.

This raises the severity of the netd starvation (#355) to **high** for
connectivity, although the *initial DHCP* symptom of #340 is still explained by
the e1000 defect fixed in #341.

## Conclusion

- #340's `netd`/`sys_yield` premise for the **DHCP lease**: **REFUTED**
  (independently reproduced; DHCP completes with netd never running).
- #340's real DHCP defect: **PROVEN and already fixed** in #341.
- Separate, proven, and connectivity-relevant defect: **Ring-0 kernel-thread
  starvation on single-CPU** → netd is never selected; with only `dhcpd` as a
  substitute, RX polling is intermittent → **no reliable continuous
  connectivity on SMP1**. Tracked as **#355** (severity high); same class as
  closed #264/#282.

Classification: **PROVEN SCHEDULER BUG** (kernel-thread starvation) affecting
**continuous connectivity**; the *initial-DHCP* symptom is a proven NET/e1000
defect already fixed.
