# KBD SMP QUEUE VALIDATION REPORT

**Branch:** `investigation/kbd-write-stress`  
**Commit:** `91286f1` (+ vt diag ring, shell yield, debug syscall 99)  
**Kernel:** v0.50.0, VT_QUEUE_SIZE 4096, usable 4095, 716 tests  
**Date:** 2026-09-25  

---

## Yield starvation

**CONFIRMED**

`userbin/neoshell/src/shell.rs:158` tenía:
```rust
if b < 0 { continue; }
```
sin `yield`. Medición con `empty_loops`/`max_empty` (log en 100k/1M/10M):
- `abc` gap150ms → `max_empty ~2k` (timer preempta 1 KHz)
- `hello` gap0 tras `aabc` sin yield → `empty_loops 500k` en 50 ms, `READB` bloqueado 5s, VT `head9 tail15 occ6` sin pop
- Con `sys_yield()` → `hello` gap0 `pop` <10 ms, `max_empty <1k`

Experiment `if b<0 { sys_yield(); continue; }` `shell.rs:158`:
```
neodev test 716/716 PASS
gap0 hello  ~5s → <10ms
```
**Evidence strongly implicates shell busy-wait/starvation.** Fix ya en rama.

---

## SPSC producer count

**1** — Observado en todos los `VT_DIAG` con `gap150ms` y prompt limpio:
```
VT_PUSH byte=0x61 h=4 t=4→5 cpu=0 tid=5
VT_PUSH byte=0x61 h=5→6 cpu=0
...
```
Todos `cpu=0`, `tid=5` (shell) para pop, `tid=2` (netd) o `tid=0` (IRQ) para push pero siempre `cpu=0`. Nunca `cpu=1`.

Con `qemu -smp 2` el kernel reporta `1 CPU(s) online` y panica `PAGE_TABLE_CORRUPTION` tras `Enabling interrupts...`, por lo que **no se pudo observar un segundo producer**. La cola no llegó a usarse en SMP real.

Conclusión: con el kernel actual (1 CPU online) **existe exactamente 1 producer** (IRQ1 en CPU0).

## SPSC consumer count

**1** — `handler_read` `syscall/handlers.rs:114` solo lo ejecuta `neoshell` PID4 TID5 en `VT0`. `pop_byte_from_vt(vt)` con `vt=current_vt_num()` `scheduler/mod.rs:314` siempre `0` para shell. No hay otro reader concurrente en `VT0`. `VT_POP` siempre `tid=5 cpu=0`.

## IRQ33 affinity

**Caso A — Fijado a CPU0 — CONFIRMADO por código:**

- `interrupts/ioapic.rs:162` para ISA `has_handler irq==1`:
  ```rust
  let entry: u64 = vector as u64 | FIXED | PHYSICAL; // sin <<56
  ioapic_write_redir(pin, entry);
  ```
  Sin `apic_id <<56`, destino = 0 → APIC ID 0 → **CPU0**. No se usa `Logical` ni `LowestPriority`.

- Si `IOAPIC.is_active()==false` (MADT no encontrado), se usa `arch/x64/pic.rs:74` `ChainedPics` con `outb 0x21/0xA1` y `idt[33]=keyboard_handler` — PIC también entrega a CPU0 (único BSP).

- `QEMU` con `q35` expone `IOAPIC at 0xfec00000` `ioapic.rs:131` y `I/O APIC active, PIC disabled`, por lo que IRQ33 → pin1 → vector33 → CPU0.

**No se observó `KBD_PUSH cpu=1`.** `vt_diag` confirma `cpu=0` para todos los pushes.

Conclusión: **producer = CPU0 exclusivo**, SPSC podría seguir siendo correcto **si** la garantía es arquitectónica (ver Fase 5).

## SMP2

**FAIL (no por cola, por SMP init)**

```
qemu-system-x86_64 -smp 2 -monitor tcp:4445 ...
```

Boot log `smp2_serial.log`:

```
[BOOT] BSP KPRCB at 0x2403000, GS base set
[BOOT] WARN: No APs found (single CPU mode)
[+] 1 CPU(s) online
[IOAPIC] Initialised
[+] Enabling interrupts...
!!! KERNEL PANIC (CLASS: PAGE_TABLE_CORRUPTION) !!!
```

Se repite tras `Enabling interrupts`, QEMU no llega a `C:\>` (timeout 60s). Con `-smp 1` mismo binario **PASS** y llega a `C:\>`. Por tanto, el kernel actual **no soporta -smp 2** (detect_aps falla o trampoline no copia), no es un fallo de la cola.

No se pudo medir `produced==consumed+dropped+remaining` en SMP2 porque no hay prompt. La conservación **no se pudo verificar** en SMP.

## SMP4

**NOT TESTED** — Mismo panic que SMP2 (no se ejecuta). Con `-smp 1` la matriz `abcdefghijklmnopqrstuvwxyz` gap0/10/50/150ms no se ejecutó completa en esta fase por tiempo, pero `hello` gap0 con yield **PASS** en SMP1.

## Producer CPUs observed

```
SMP1: cpu=0 para todos los VT_PUSH (100×a, hello, abc)
SMP2: N/A (no boot)
SMP4: N/A
```

Nunca `cpu=1`.

## Consumer CPUs observed

```
READB cpu=0 tid=5 siempre
pop cpu=0
```

Shell migra pero con 1 CPU siempre 0.

## active_vt synchronization

`input/manager.rs:40` `active_vt: AtomicUsize Relaxed`, `switch_vt` `store Release`, `active_vt() load Relaxed`, `push_byte` `load Relaxed` luego `vt_queues[vt].push`.

- Escritor: `switch_vt` en hotkey `Alt+F1` `hotkey.rs:16` (en CPU0, IRQ context)
- Lector/Productor: `push_byte` en `keyboard_handler` (CPU0)

Con 1 CPU, `Relaxed` es suficiente porque no hay concurrencia con `switch_vt` (hotkey y push no se solapan en mismo CPU sin preemption, y `switch_vt` deshabilita interrupciones vía `without_interrupts` implicitamente? No, `hotkey` se llama con `KBD` lock held en IRQ, sin `without_interrupts` extra, pero `active_vt` es `Atomic`).

Con SMP y si IRQ puede migrar, `Relaxed` es **incorrecto**: necesita `Acquire` en `push_byte` y `Release` en `switch_vt` ya existe, pero `push_byte` usa `Relaxed` → podría leer `active_vt` viejo y empujar a VT equivocada:
```
CPU0: active_vt=0, IRQ decide queue0
CPU1: switch_vt(1) → active_vt=1
CPU0: push into queue0 (debería ser queue1)
```
No se observó porque no hay SMP real.

## Queue corruption

**NO** — `produced == consumed + dropped + remaining` se cumple en SMP1:
- `VT_PUSH_CNT 100, VT_POP_CNT 0 (si shell ocupada) → remaining 100, dropped 0`
- `VT_PUSH 6, VT_POP 6, remaining 0` para `abc` gap150

Ring `occ` nunca > `max_occ` y `max_occ` con 100 burst = 100 (<4095).

## Input loss

**NO** con gap150ms y prompt limpio (con yield).  
**NO** KBD→VT (KBD_PUSH==VT_PUSH siempre).  
**SÍ** stack `2N` si se fuerza `4096` sin consumir: `DROP_CNT 1` para `N+1`, visible vía `VT_DIAG`.

Con gap0 y sin yield, **latencia 5s** pero no pérdida (queda en queue). Con yield, latencia <10ms y **no pérdida**.

## Latency

- Sin yield, gap0 tras `aabc`: `head9 tail15` sin pop 15s → shell starvation.
- Con yield: `hello` gap0 pop <10ms.

## VirtualBox

**NOT AVAILABLE** — No se probó backend `virtualbox` (`neodev/src/vmm/vbox.rs` usa `VBoxManage keyboardputscancode`). QEMU TCG es el único probado. Se asume mismo `IOAPIC` y `PS/2` status `inb 0x64 &0x01`, por lo que `QEMU PASS` no implica `VirtualBox PASS`.

## Architecture decision

**KEEP SPSC** para `VtInputQueue` **con condición**:

- **IRQ33 debe quedar fijado a CPU0** (ya lo está vía `IOAPIC PHYSICAL` sin destino). No cambiar a `Logical`/`LowestPriority`.
- Documentar invariante: *“Exactly one producer: keyboard_handler on CPU0”*.
- Añadir `debug_assert!(cpu==0)` en `vt_diag_record` para detectar violación futura.

**Si SMP se arregla para traer APs (2/4 CPUs online):**

- Opción A **KEEP SPSC + affinity** (preferida, mínima complejidad):
  ```
  IRQ keyboard → CPU0 exclusivo → SPSC
  ```
  Ventaja: lock-free, sin CAS, latencia mínima, hotkey `active_vt` sigue en CPU0.

- Opción B **MPSC** (`tail` con `compare_exchange_weak` loop) — más complejo, IRQ no puede dormir, necesita `Release` + `Acquire` correctos, pero soportaría IRQ en cualquier CPU.

- Opción C **Per-CPU queues** (`queue[cpu]`) + consumer que hace `poll` round-robin — más memoria (4×4096), más latencia, hotkey `active_vt` se dispersa.

**Recomendación:** Mantener **A** y añadir `ioapic_write_redir(pin, entry | (0 <<56))` explícito + comentario `// Fixed to CPU0 for SPSC`. Si en futuro se quiere balancear IRQs, migrar a **B** con `AtomicUsize::compare_exchange`.

## neodev test

```
716/716 PASS
```

con `vt_diag` + `shell yield` + `e0_pending` + `handler_read` atomic.

## READY

**NO**

```
QEMU SMP1   PASS (SPSC, gap150ms, no drop, no corruption, no starvation con yield)
QEMU SMP2   FAIL (kernel panic antes de prompt, no es cola, es SMP init)
QEMU SMP4   NOT TESTED (mismo)
rapid input PASS* (100×a gap0 con yield, occ100, no drop, latencia <10ms con prompt limpio; pero gap0 tras comando previo aún latencia 5s→10ms, falta matriz completa 6×8)
burst       PASS (100 burst sin drop si consumer no está en execute_line)
no silent loss  PASS (ahora visible DROP_CNT)
no queue corruption PASS
no starvation PASS (con yield)
VirtualBox SMP1 PASS — NOT TESTED
```

Para `READY:YES` se requiere:
- Arreglar `smp_detect` para que `-smp 2` bootee con 2 CPUs y llegue a `C:\>` sin panic
- Repetir matriz `6 inputs × 8 gaps × 3 smp` y verificar `produced==consumed+dropped+remaining` en cada caso
- Probar `VirtualBox SMP1` con `abc`/`hello`/`rapid alphabet`

---

## Evidencia para revisión

- `neodos-kernel/src/input/vt.rs:31` ring 64 + `vt_diag_dump()` + syscall 99
- `neodos-kernel/src/input/manager.rs:74` helpers `vt_active_occupancy()`
- `neodos-kernel/src/kbd/hotkey.rs:31` `Ctrl+Alt+V → vt_diag_dump`
- `neodos-kernel/src/syscall/mod.rs:99` `DebugDump` + `handlers.rs:664` `handler_debug_dump`
- `userbin/neoshell/src/shell.rs:158` `if b<0 { sys_yield(); continue; }` + `SHELL_BUSY` log
- Build: `neodev build --image` PASS, `neodev test` 716 PASS
- Boot log `smp2_serial.log` con `PAGE_TABLE_CORRUPTION` tras `Enabling interrupts` con `-smp 2`

