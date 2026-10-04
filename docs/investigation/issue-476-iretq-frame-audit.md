# Investigation — #476 IRETQ frame audit (`IRETQ_BAD_FRAME`)

**Date:** 2026-10-04
**Branch:** `investigation/476-iretq-frame` (from `cac7cd8`, which already carries the
#474/#476 diagnostics; #477 is kept separate).
**Related:** #476 (`#UD`/`#PF`/GPF frame family), #474, #477 (paging slots), #384, #383.

---

## 1. IRETQ routes (Phase 1)

| Route | Stack / frame | `mov rsp` site | frame location |
|-------|---------------|----------------|----------------|
| Timer (`timer_handler_asm`) | interrupted kstack; `timer_handler_inner` returns `rax = next_rsp` (or `current_rsp`) | ASM `mov rsp, rax` | `next_rsp + 120` = RIP |
| Syscall (`syscall_handler_asm`) | syscall entry frame; if resched, `syscall_try_resched` returns `rax` | ASM `mov rsp, rax` | returned `rsp + 120` |
| Exception (`exception_do_resched`) | faulting kstack; `schedule_with(true)` → `next_rsp` | ASM `mov rsp, {0}` | `next_rsp + 120` |
| AP idle (`ap_enter_idle`) | AP idle stack; fabricated ring0 frame | ASM `mov rsp, {0}` | frame + 120 |
| Usermode entry (`execute_usermode`) | builds a 5-slot Ring-3 frame | `iretq` directly | synthetic |

All dispatch routes share the same layout: 15 saved GPRs (`0..112`) then
`RIP(120) CS(128) RFLAGS(136) RSP(144) SS(152)`.

### `[K355] resched handoff -> idle tid=N`

Emitted in `syscall_try_resched` (and `[K355] timer handoff -> idle` in the
timer) when the current thread is Blocked/Terminated with no Ring-3 candidate
and `take_kernel_handoff(this_cpu)` is set. The CPU accepts the committed **idle**
thread (a Ring-0 frame) so the next selection runs from Ring 0 and can dispatch a
starved kernel thread. It returns `next_rsp = idle.rsp`, which the ASM `iretq`s.

## 2. Valid-frame definition (Phase 2)

For the selected thread `k` with `ks_base = ks_top - kernel_stack_size` and
`frame_addr = rsp + 120`:

- placement: `ks_base <= frame_addr` and `frame_addr + (ring3 ? 40 : 24) <= ks_top`;
- alignment: `frame_addr & 7 == 0`;
- `CS`: `0x08` (Ring 0) or `0x1B` (Ring 3); ring must match the thread kind
  (`expect_ring3 = !is_kernel_thread`);
- `RFLAGS`: bit1 set, bits 63:22 clear;
- `RIP`: non-zero, canonical; Ring-3 RIP in the low canonical half;
- Ring 3 only: `SS == 0x23`, `RSP` non-zero, canonical, low half;
- cross-thread: `rsp` must not fall inside a *different* live thread's kernel
  stack (`TID_STACK_MISMATCH`).

## 3. Detector (Phase 4)

`scheduler/diag/iretq.rs`:

- `validate(frame_addr, ks_base, ks_top, &Frame, expect_ring3) -> Option<IretqBad>`
  (pure, unit-tested);
- `audit(sched, rsp, k, expect_ring3, site)` reads the frame, runs `validate`,
  additionally scans `sched.kthreads` for cross-stack ownership, and emits
  `[IRETQ_BAD_FRAME]` (allocation-free, lock-free, bounded counter + 32-entry ring).
- kinds: `FRAME_OUTSIDE_KSTACK / INVALID_RIP / INVALID_CS / INVALID_RSP /
  INVALID_SS / INVALID_RFLAGS / RING_MISMATCH / FRAME_ALIGNMENT /
  TID_STACK_MISMATCH`.

Hooked before every `next_rsp` dispatch return: `syscall_try_resched`
(K355-idle, chosen, idle-fallback, main), the three timer preempt branches, the
timer K355 idle hand-off, and `exception_do_resched`. `IRETQ_AUDIT` /
`IRETQ_BAD_RING` are dumped on panic.

## 4. Unit test (Phase 8)

`n476_iretq_frame_validator` covers valid Ring-3/Ring-0 frames plus
outside-kstack, misaligned, bad CS, ring mismatch, bad RFLAGS, non-canonical RIP,
bad SS, non-canonical RSP. `neodev test`: **823/823 PASS**, 0 false positives.

## 5. Campaign (Phase 9)

| Environment | runs | `IRETQ_BAD_FRAME` | #PF | #GP | #UD | panic |
|-------------|-----:|------------------:|----:|----:|----:|------:|
| QEMU `neodev test` (830 tests) | 1 | 0 (false positives: 0) | 0 | 0 | 0 | 0 |
| VBox SMP2 churn 2×32 (`iretq2`) | 10 | **2 runs, 3 hits** (`resched_k355_idle`) | 0 | 0 | 0 | 0 |
| VBox SMP2 churn 2×32 (`iretq3`, +ring dumps) | 10 | **2 runs, 108 hits** (`timer_preempt`, `resched_k355_idle`, `syscall_iretq`) | 0 | 1 | 0 | 0 |

Example (corrupted dispatch frame, `cs` garbage, followed by a GPF):

```text
[IRETQ_BAD_FRAME] kind=FRAME_OUTSIDE_KSTACK site=resched_k355_idle cpu=0 tid=1 pid=0
  name=idle/0 is_kernel=true k_cpu=0 k_state=Running kprcb_tid=1
  next_rsp=0x24b4f40 ks_base=0x434c668 ks_top=0x434d668
  frame_addr=0x24b4fb8 rip=0x202 cs=0x1e471820 rflags=0x24b4f40 user_rsp=0x0 ss=0x4017e0a
```

## 6. Root cause (Phase 10, Case A) — DEMONSTRATED

The kernel's believed context does **not** match the stack when `iretq` runs.

`RSP_TRACE` shows exactly who corrupts the idle's saved `rsp`:

```text
[RSP_TRACE] #25323 cpu=0 site=5(SITE_RSP_TIMESLICE=on_timer_tick) tid=1 pid=0
            old=0x434d510 new=0x24b4f40 k.cpu=0 state=1(Running) is_current=1
```

`0x434d510` is on the idle's `IDLE_STACK` (`0x434c668..0x434d668`); `0x24b4f40` is on
the **previous thread's** 16 KiB stack (the `KSTACK_RING` shows the CPU0
`tid=6(NetApplier) ↔ idle/0` hand-off oscillation). Mechanism:

```text
schedule_with_handoff / K355 handoff:
    set KPRCB.current_thread = idle        (inside without_interrupts)
  -> without_interrupts restores IF=1      <-- window
  -> ASM: mov rsp, idle.rsp                (still on the previous stack)
       |  a timer fires here:
       |  on_timer_tick(): current_kthread_mut() = idle (KPRCB already idle)
       |                   current_rsp = previous thread's stack
       |                   => idle.rsp = previous stack   (corruption)
  -> idle re-dispatched via the next K355 hand-off returns the foreign rsp
       -> the ASM iretq's a frame taken from another thread's stack
```

So the invariant **"a thread's saved `rsp`/frame lies inside its own kernel
stack"** is broken by the timer during the KPRCB-commit → `mov rsp` window
(interrupts re-enabled between the two). The timer later iretq's a corrupted
frame (`cs=0x1e471820`), producing the #476 `#GP`/`#PF`/`#UD` family.

## 7. Proposed minimal fix (NOT integrated)

Two equivalent ways to close the window; the first is the least invasive:

1. **Keep IF=0 from the KPRCB commit to the stack switch** in the syscall
   resched path (the timer/exception paths already run with IF=0 across
   `mov rsp`). The syscall IDT entry runs with IF=1; `syscall_try_resched`
   commits under `without_interrupts` and re-enables IF before the ASM
   `mov rsp`. Leaving IF=0 until `iretq` (which restores IF from the frame)
   closes it.
2. **Validate the save**: at the `k.rsp = current_rsp` sites, only store when
   `current_rsp` lies inside `k`'s kernel stack; otherwise the KPRCB is
   mid-switch and the store is skipped (with a diagnostic). This is
   defence-in-depth but must handle the boot thread (whose `ks_top` is not the
   live bootstrap stack, as the `timer_preempt` hit shows).

Not implemented here (delicate IF/`iretq` interaction); #476 stays OPEN.

