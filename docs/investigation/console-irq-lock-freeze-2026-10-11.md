# System-wide freeze after network initialization — graphical-console IRQ lock

Date: 2026-10-11
Status: **PROVEN** (deterministic kernel regression test + QEMU `-d int`
forensics + VirtualBox reproduction and post-fix stress)

## Symptom

NeoDOS boots to the shell; once the network services (`netd`, `dhcpd`, `ntpd`,
`netapplier`) finish starting, the shell stops responding: commands are not
executed, typed characters are not echoed and the blinking cursor stops
advancing. The machine is otherwise alive (no panic, no watchdog firing) and a
second CPU may keep running briefly before it too wedges.

## Root cause

The graphical console framebuffer is protected by `graphics::RENDERER`, a plain
`spin::Mutex<Option<Renderer>>` (`neodos-kernel/src/graphics/mod.rs`). Every
console writer holds it (`console::write_char` → `font::draw_char`, `scroll`,
`clear_screen`, …), and the shell enables automatic cursor blinking
(`sys_cursor_blink(1)`), which makes the **BSP timer IRQ** render the cursor:

```
timer_handler_inner()            (neodos-kernel/src/arch/x64/idt/mod.rs)
  └─ console::cursor_timer_tick()
       └─ console::draw_cursor()
            ├─ console_max_row()      -> RENDERER.lock()
            └─ font::draw_char()      -> RENDERER.lock()
```

`cursor_timer_tick` runs in interrupt context with interrupts disabled. If the
timer interrupts a thread that is inside a renderer critical section — most
easily `scroll()`, which holds `RENDERER` across a multi-megabyte framebuffer
`memmove` — the handler blocks on a lock the interrupted context can never
release. The BSP spins forever inside the IRQ and no further timer, keyboard or
serial work happens on it.

This violates **INV-3** (`docs/architecture/source-of-truth.md`): "IRQ handlers
MUST NOT acquire spinlocks that can contend with non-IRQ context".

Network initialization is the trigger because it produces a burst of console
output from several services at the same time the shell has cursor blink
enabled, maximizing the chance that a timer tick lands inside a `scroll()`
critical section.

## Decisive evidence (QEMU `-d int`, pre-fix kernel)

`qemu_output.log` captured with `-d int,cpu_reset`:

- The **BSP** (GS base `0x2408000`) takes its last timer interrupt (entry 55433)
  while executing `memmove` at `0x41f4ddb`, with `RDX=0x3d4000` (~4 MB) and
  `RSI/RDI` at the framebuffer — i.e. inside `console::scroll()`, holding
  `RENDERER`. No further BSP interrupt is ever logged.
- The **AP** (GS base `0x2409000`) then spins permanently in
  `graphics::font::draw_char` at `0x41ad842` (`movzbl RENDERER; test; pause`),
  i.e. the `spin::Mutex` acquire loop, from interrupt 60138 to 97573, with
  `IF=1`. The timer keeps firing but the scheduler only preempts Ring-3 contexts
  (`idt/mod.rs: "if is_user_mode && !current_is_idle"`), so a Ring-0 spin is
  never preempted.

## Why the existing tests missed it

The kernel test suite never exercises a real timer interrupt concurrent with a
console render, so the race is invisible. The failure is timing/loading
dependent, which is why it appeared "after networking" and only intermittently.

## Fix

- `console::draw_cursor` now uses `RENDERER.try_lock()`. In IRQ context a held
  lock means the interrupted context owns it; the cursor simply skips one blink
  frame and the next tick retries (INV-3).
- `font::draw_char_on(&Renderer, …)` is split out so the cursor path blits the
  glyph while already holding the guard instead of taking the lock again.
- Regression test `cursor_blink_irq_does_not_block_on_held_renderer`: it holds
  `RENDERER` and runs `cursor_timer_tick`. Against the old blocking
  `draw_cursor` the test hangs (observed: the suite stops at this test);
  post-fix it returns.

## Secondary defect found while validating (AP bring-up, #522)

The AP trampoline enabled `EFER.LME` but not `EFER.NXE`. The kernel page tables
set NX (bit 63) on non-executable leaves; with NXE clear bit 63 is reserved, so
the first AP access through such a page raised `#PF(RSVD)` → `#DF` → triple
fault. Because the AP stack frame is handed out by the buddy allocator before
the kernel builds its own page tables, whether it landed on an NX leaf depended
on allocation order — a small kernel change could kill every AP (the "AP-bringup
layout cliff" noted in #522). Fixed by enabling `LME | NXE` in the trampoline.

## Environment note

On this host the QEMU 2-vCPU test previously passed for `develop` but failed for
any build that shifted `ap_entry`/frame-allocation order; the NXE fix removes
that whole class of failure. VirtualBox validated the end-user workflow.

## Audit: other IRQ-reachable locks (Rule 3.5.1)

After codifying the rule, the IRQ entry points (timer, keyboard, serial, mouse,
IPI, MSI, exceptions) were audited for blocking locks that interruptible code
can hold:

| Lock | IRQ path | Non-IRQ holders IRQ-safe? | Status |
| --- | --- | --- | --- |
| `graphics::RENDERER` | timer cursor blink; **keyboard `Alt+F1..F8` VT switch** (`redraw_from_shadow`) | no (`println!`, boot, syscalls) | **fixed** — every site now uses `graphics::with_renderer` (IF-safe); the IRQ cursor path uses `try_lock` |
| `irp::IRP_POOL.inner` | timer → DPC → `apc::irp_complete_with_apc` | no (driver/work-queue callers) | **fixed** — all sites via `irp::with_pool` (IF-safe) |
| `idt::MSI_HANDLER_TABLE` | `msi_dispatch` (vectors 48..255) | no (`msi_register_handler`) | **latent** — MSI IDT stubs not installed, so `msi_dispatch` is unreachable today; tracked in **#676** |
| `globals::VFS` / `PAGE_CACHE` / `BLOCK_DEVICES` / `cm::CM_MANAGER` | timer → watchdog dump; keyboard `Ctrl+Alt+Del` → shutdown/reboot flush | `preempt_disable()` only, held with IF=1 | **latent (terminal paths)** — defer watchdog/shutdown out of IRQ; tracked in **#677** |

A CI/enforcement check for Rule 3.5.1 is tracked in **#678**.

Verified IRQ-safe: `SCHEDULER`, `SERIAL1`, `KBD` (uses `try_lock` in IRQ),
`object::TIMER_MANAGER` (syscall-only callers, IF=0 gate), and all
lock-free paths (`EVENT_BUS`, crash/trace rings, per-CPU DPC ring).
