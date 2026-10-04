# Investigation — #474 SMP2 Ring-0 INVALID_OPCODE (`rip=0x148`)

**Date:** 2026-10-04
**Branch:** `investigation/474-invalid-opcode-smp`
**Base:** `develop` @ `d584664`
**Environment:** QEMU 10.x, TCG, `q35`, SMP2/1/4, `-net user` (e1000 + SLiRP)
**Related:** #338 (`Ring-0 Ready` frame publication), #293 (idle CPU ownership),
#346 (stale `KPRCB` after rejected dispatch), #348 (idle stack canary),
#376 (VFS lock-holder deadlock — independent, closed).

---

## 1. Symptom

On SMP2 one boot out of the historical campaign panicked:

```text
[FAULT] v=6 INVALID_OPCODE rip=0x148 cs=0x8 rsp=0x24b1210 cpu=0
[PANIC] class=UNKNOWN_CPU_EXCEPTION rsp=0x24b0ff0 msg=Invalid opcode: rip=0x148
```

The crash dump register set is byte-for-byte reproducible across runs
(`RAX=0`, `RCX=0xfffffffffff00000`, `RDX=0x24af0a8`, `RSI=0x3bc`,
`RDI=0x24b0e88`, `R11=0x195`, `R14=0x24af0a8`, `R15=0x24b0ff0`,
`RFL=0x86`). The corrupted execution is a **wild control-flow transfer to
`0x148` in Ring 0** while the faulting CPU is running a *user* thread
(`current_pid == 3`, Ntpd/TID 5) inside a syscall.

`rip=0x148` is not code: it is *arbitrary stack content* consumed as a return
frame. `rsp=0x24b1210` is on Ntpd's 16 KiB kernel stack
(`kernel_stack_top = 0x24b12c0`).

---

## 2. Reproduction

Direct QEMU TCG (the historical harness used `neodev run`, which also enables
`-d int,cpu_reset`):

```bash
qemu-system-x86_64 -machine q35,accel=tcg -smp 2 -display none \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
  -drive if=pflash,format=raw,file=$OVMF_VARS \
  -device ahci,id=ahci \
  -drive if=none,format=raw,file=neodos/disk_image.img,id=mydisk \
  -device ide-hd,drive=mydisk,bus=ahci.0 \
  -netdev user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1 \
  -device e1000,netdev=net0 -m 512M -no-reboot -monitor none \
  -d int,cpu_reset -D qemu.qtrace -serial file:serial.log
```

Reproduced in the **first** SMP2 run (`PANIC_smp2_1`,
`/tmp/opencode/474/PANIC_smp2_1.qtrace`). The `-d int` trace records every
delivered interrupt with the full vCPU state, which pins down the exact
transition.

---

## 3. First corrupt state (QEMU `-d int` trace)

The transition is a single timer tick on CPU0:

```text
#216917  v=20 (timer) IP=0008:00000000040fd081 SP=0x24b11d0  GS=0x2408000 (cpu0)
         -> 0x40fd081 = apc::dispatch_kernel_apcs, called from
            apc_dispatch_on_syscall_return (the syscall-return path)
#216918..#216926  all CPU1 (GS=0x2409000)
#216927  v=20 (timer) IP=0008:0000000000000140 SP=0x24b1210  GS=0x2408000 (cpu0)
#216929  v=06 INVALID_OPCODE IP=0008:0000000000000148 SP=0x24b1210
```

At `#216927` CPU0 is already executing at `0x140` in Ring 0. The register state
of `#216927`/`#216929` is identical and is exactly what a
`mov rsp,next_rsp; pop 15 GPRs; iretq` leaves behind:

| Popped "frame" | Value |
|----------------|-------|
| RAX RBX RCX RDX | `0`, `0x283`, `0`, `0x16` |
| RSI RDI RBP | `0x1bd43a20`, `0x1bd43ab0`, `0x97f7b0` (a **user** stack) |
| R8 R13 R15 | `0x24b1220`, `0x24b1220`, `0x4a817c800` (=20·10⁹ ns) |
| RIP / CS / RFLAGS | `0x140` / `0x8` / `0x246` |

`RSP` after the `iretq` is `0x24b1210`, so the Ring-0 `iretq` frame was at
`0x24b11f8` and the handler popped 15 GPRs from `next_rsp = 0x24b1180`:

```text
next_rsp = 0x24b11f8 - 120 = 0x24b1180     (= kernel_stack_top - 0x140)
```

`0x24b1180` is **not** a dispatch frame: it is deep inside Ntpd's live syscall
call chain. The timer handler published Ntpd's `rsp` with that value, and a
later dispatch misread the stack as RIP/CS/RFLAGS.

---

## 4. Root cause

### 4.1 The window

A user thread that calls `sys_yield` (Ntpd/Dhcpc do this in their polling
loops) executes `scheduler::yield_current_thread()` inside the syscall:

```text
yield_current_thread():
    k.yield_requested = true;      // intent, not yet published
    set_need_resched();
```

The syscall return path (`syscall_try_resched`) is supposed to save the real
Ring-3 frame and consume the intent. But a **timer interrupt can land in the
window before that**, while the thread is still in Ring 0 (`cs == 0x08`).

### 4.2 The missed gate (#338's blind spot)

`timer_handler_inner` classifies the interrupted context as:

```rust
if is_user_mode && !current_is_idle { ... }   // Ring 3 -> user preempt (gated)
else if current_is_idle { ... }               // idle
else {                                         // documented "kernel thread only"
    let should_preempt = current_state == Ready || current_yield;
    ...
    k.rsp = current_rsp;                       // <-- deep Ring-0 rsp
    Scheduler::make_thread_ready(k);           // publishes it Ready
}
```

The `else` branch's comment (from #338) says it "serves non-user, non-idle
threads — kernel threads like netd". That assumption is **false**: a user
thread interrupted in Ring 0 is also non-idle and not `is_user_mode`, so it
lands in the same branch. When `current_yield` is true it takes the preemption
path and:

1. saves `k.rsp = current_rsp` — a transient Ring-0 call frame;
2. publishes the user thread `Ready` via `make_thread_ready` (no
   `frame_is_ring3` gate here, unlike the user-preempt branch and
   `on_timer_tick`);
3. `schedule()` hands the thread to some CPU; the next
   `mov rsp,next_rsp; pop 15; iretq` treats the deep stack as a frame and jumps
   to a wild RIP.

#338 explicitly recorded this gap:

> `idt.rs` kernel-preempt branch: **NO gate** — it serves kernel threads
> (netd), which run in Ring 0 by design.

#474 is the direct consequence of that gap.

### 4.3 Confirmation instrumentation

A temporary marker was added to the branch and reproduced the condition before
the crash (`next_would_be_saved` is the value the branch would publish):

```text
[474-RING0-USER] cpu=0 tid=5 pid=3 state=Some(Running) yield=true rsp=0x24b1220 cs=0x8 next_would_be_saved=0x24b10c0
[474-RING0-USER] cpu=1 tid=5 pid=3 state=Some(Running) yield=true rsp=0x24b1220 cs=0x8 next_would_be_saved=0x24b1180
```

The observed `0x24b1180` is exactly the `next_rsp` derived from the crash trace.
The marker was removed after confirmation.

---

## 5. Fix

Minimal, one gate. The Ring-0 preemption branch may publish a context only when
its saved `rsp` is a valid dispatch frame for that thread:

- genuine kernel/idle threads run in Ring 0 by design (dispatched via
  `schedule(require_ring3 = false)`), so their frame is valid;
- a *user* thread in Ring 0 is inside a syscall; its live `rsp` is not a
  dispatch frame and must be deferred to the syscall-return path.

`neodos-kernel/src/scheduler/schedule.rs`:

```rust
pub(crate) fn ring0_publish_is_dispatchable(interrupted_cs: u64, is_kernel_thread: bool) -> bool {
    (interrupted_cs & 3) == 3 || is_kernel_thread
}
```

`neodos-kernel/src/arch/x64/idt/mod.rs` (kernel-preempt branch):

```rust
let current_is_kernel_thread = scheduler
    .find_kthread(tid)
    .map(|k| scheduler.is_kernel_thread(k))
    .unwrap_or(true);
let should_preempt = ring0_publish_is_dispatchable(interrupted_cs, current_is_kernel_thread)
    && (current_state == Some(ThreadState::Ready) || current_yield);
```

When the gate refuses, control falls through to the existing "kernel mode
interrupt (no preemption)" path, which sets `NEED_RESCHED`; the in-flight
syscall finishes and `syscall_try_resched` publishes the thread with its real
Ring-3 frame. No new state, no scheduler policy change, no ABI change, no
`require_ring3` weakening.

---

## 6. Regression test

`neodos-kernel/src/scheduler/tests.rs`:

```text
n474_ring0_preempt_only_publishes_dispatchable_frame
```

Asserts the gate:

- user thread in Ring 0 (`cs = 0x08/0x10`, `is_kernel_thread = false`) →
  **not publishable**;
- kernel/idle thread in Ring 0 → publishable;
- Ring-3 interruption → publishable;
- cross-checks that a deep `rsp` whose `+128` slot is `0x08` is not a Ring-3
  dispatch frame (`frame_is_ring3 == false`), documenting why the gate must use
  the interrupted CS rather than the thread's stored frame.

---

## 7. Validation

| Config | Runs | Result |
|--------|-----:|--------|
| `neodev test` | 1 | **820/820** kernel + Command + Shell `PASSED` |
| SMP1 | 3 | 3/3 `ALL_TESTS_COMPLETE`, 0 panic/fault |
| SMP2 | 10 | 10/10 `ALL_TESTS_COMPLETE`, 0 panic/fault |
| SMP4 | 3 | 3/3 `ALL_TESTS_COMPLETE`, 0 panic/fault |

Every run also reached `netpump running` (the Ring-0 data-plane thread), and the
SMP2/SMP4 runs exercised the shell, NeoInit and the service syscall paths where
the fault originally landed.

The pre-fix build reproduced the panic in SMP2 (run 1 of the forensic campaign;
see `PANIC_smp2_1.qtrace`) and the confirmation instrumentation showed the exact
illegal publication
(`tid=5 pid=3 … cs=0x8 next_would_be_saved=0x24b1180`) firing repeatedly — the
same `0x24b1180` derived from the crash trace. The fix gates that publication.

`cargo build` (kernel) succeeds. `cargo fmt --check` is not clean on the
`develop` baseline (hundreds of pre-existing diffs in untouched files); the
added code follows the surrounding style. `npx markdownlint` was unavailable in
the environment (`could not determine executable to run`); the changed Markdown
was checked against `.markdownlint.json` by inspection.

---

## 8. Relationship to previous work

- **#338** — identified the three Ring-0 `Ready` publication sites and gated
  two of them; deliberately left the kernel-preempt branch ungated because it
  was believed to be kernel-thread-only. **#474 closes that gap.**
- **#293** — idle CPU ownership; untouched.
- **#346** — rejected-dispatch recovery; untouched.
- **#348** — idle stack canary sizing; untouched.
- **#376** — VFS lock-holder deadlock; independent and closed. The `#376`
  `preempt_disable` protection is preserved (a deferred user thread keeps
  running, which is strictly safer for lock holders).

---

## 9. Remaining risks / follow-ups

- The `[SYSCALL_CORRUPT]` diagnostic ring still reports up to 16 false
  positives per boot (a blocking syscall preempted by another thread's syscall
  on the same CPU). It is diagnostics-only and unrelated to #474; a future
  cleanup could suppress it.
- `on_timer_tick` still stores a transient Ring-0 `rsp` for a user thread on
  timeslice expiry (for accounting) before the syscall-return path corrects
  it. With the kernel-preempt branch gated, that value is no longer published.
  A defensive follow-up could avoid the transient store entirely.
