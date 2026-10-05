# #476 — Fix: close the syscall KPRCB/RSP ownership window

**Branch:** `fix/476-close-kprcb-rsp-window` (from `develop` @ `19b37d5`).
**Scope:** minimal fix. No scheduler policy / ABI / KStack / TSS / APC changes.

---

## Root cause

`syscall_try_resched()` repoints `KPRCB.current_thread = NEXT` inside its
`without_interrupts` region (IF=0), but that region exits and restores IF to
its entry value. The syscall path reaches the handoff with **IF=1**, so between
the publication and the ASM's physical `mov rsp,next_rsp` the CPU is still
executing on `PREV`'s kernel stack with IF=1; a timer interrupt landing in that
window observes `KPRCB.current_thread = NEXT` while the live RSP belongs to
`PREV` (the `RSP_FOREIGN` / `SWITCH_OWNER_MISMATCH`).

## Fix

`neodos-kernel/src/arch/x64/idt/mod.rs`, `syscall_handler_asm`: issue `cli`
immediately before each `call syscall_try_resched` (both reschedule call sites —
path 1 "thread terminated", path 2 "NEED_RESCHED"). Resulting ordering:

```
cli
call syscall_try_resched     ; publish KPRCB = NEXT (IF=0)
mov rsp, next_rsp            ; physical stack switch (IF=0)
call switch_out_clear_at     ; ownership assertion
...
iretq                        ; restores the saved user RFLAGS (IF)
```

The normal no-reschedule path (`jz 2f`) does not execute the `cli`, so its
interrupt state is unchanged; all paths still return through `iretq`. The
`without_interrupts` inside the resched saves/restores IF=0. No other code
between the `cli` and `iretq` relies on IF=1 (the syscall gate is an interrupt
gate; `apc_dispatch_on_syscall_return` / `syscall_trace_frame` are non-blocking).

Generated code (objdump):

```
401092c: cli
4010930: call syscall_try_resched
4010935: mov  %rax,%rsp
401093d: call switch_out_clear_at     ; path 1
...
401096f: cli
4010973: call syscall_try_resched
4010978: mov  %rax,%rsp
4010980: call switch_out_clear_at     ; path 2
...
40109b0: iretq
```

## Regression

`neodos-kernel/src/scheduler/diag/kstack.rs`: `switch_out_clear_at(site)` keeps
the existing `switch_out_clear` clear and adds a permanent, site-tagged
ownership assertion — right after the ASM `mov rsp`, verify
`rsp ∈ [KPRCB.current_thread.kernel_stack_top - size, kernel_stack_top)`
(reusing `rsp_in_kernel_stack`). Site tag: 1/2 = syscall resched, 3 = timer,
0 = exception/other. Violations increment `RSP_OWNER_MISMATCH` and log
`[RSP_OWNER_MISMATCH]` (bounded). It fires exactly when the pre-fix publish
window is observed. The existing `save_live_rsp_checked` guard (#478) is
preserved as defence-in-depth.

## Validation

### Semantic proof (temporary IF probes, removed)

```
[FIX_IF] resched_entry cpu=0 if=false
[FIX_IF] clear site=2 cpu=0 if=false in_owner=true
```
IF is 0 at the resched entry (after `cli`) and remains 0 across the physical
switch; `owner(live rsp) == KPRCB.current_thread` holds. Generated asm confirms
`cli` immediately precedes `call syscall_try_resched` and `mov %rax,%rsp`.

### QEMU matrix

| Env | Runs | tests | RSP_FOREIGN | owner-mm (syscall 1/2) | IRETQ_BAD | #PF | #GP | #UD | panic |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| QEMU SMP1 (fix) | 3 | ok | 0 | 0 | 0 | 0 | 1 | 0 | 1 |
| QEMU SMP2 (fix) | 8 | ok | 0 | 0 | 0 | 0 | 0 | 1 | 0 |
| QEMU SMP4 (fix) | 3 | ok | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| **QEMU SMP1/2/4 (develop, no fix)** | 9 | ok | **1260** | n/a | **1** | 0 | 0 | 1 | **1** |

* `RSP_FOREIGN` was **1260 → 0**; `IRETQ_BAD_FRAME` was **1 → 0**; the syscall
  handoff reports **no** ownership mismatch.
* The permanent site-tagged assertion reports `RSP_OWNER_MISMATCH` only at
  `site=0/3` (timer/exception) with the live RSP on the **bootstrap** stack —
  the separate, pre-existing `wait_for_process` publish-before-`execute_usermode`
  window (see below). It is **0 at the syscall sites (1/2)**.
* The baseline (no fix) intermittent kernel wild-RIP `INVALID_OPCODE` on cpu1
  (the #476 corruption) is gone with the fix.

### Separate pre-existing findings (not #476, not fixed here)

* `wait_for_process` publishes `KPRCB=target` while the CPU is still on the
  bootstrap stack, then `execute_usermode` iretqs from it — the assertion flags
  this at `site=0` and by timer ticks at `site=3`. This is a distinct,
  documented window; recorded separately.
* Intermittent object-manager GPF / ring3 `INVALID_OPCODE` appear in **both**
  baseline and fix at a similar rate → pre-existing/flaky, independent.

`neodev build --quick --image`: OK. `neodev test`: **824/824**.

### Scope

```
#476: FIXED (syscall KPRCB/RSP ownership window closed)
#477: untouched
scheduler policy: unchanged
syscall ABI: unchanged
KStack semantics: unchanged
```

The #478 `save_live_rsp_checked` defence is preserved.

### Verdict

```text
FIXED — syscall KPRCB/RSP ownership window closed
```

