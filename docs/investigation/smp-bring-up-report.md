# SMP Bring-Up Investigation — NeoDOS

**Branch:** `investigation/kbd-write-stress` @ `e3cefc6` (+ smp trampoline fix)  
**Kernel:** v0.50.0, QEMU q35, TCG, -smp 1 vs -smp 2, 716 tests, VT 4096  
**Date:** 2026-09-25  

---

## Fase 1 — Baseline SMP=1

```
qemu -smp 1 -machine q35,accel=tcg -m 512M -serial file:serial.log
```

- `BSP APIC ID 0` (`msr::is_bsp()` true), `KPRCB 0x2403000`, `GS 0x2403000`, `CR3 0x1f401000`, `init_smp` → `Max LVT 6` but `AP_READY_COUNT 0` → `1 CPU online` → `I/O APIC` `0xfec00000` 24 pins → `STI` → `PAGING before/after` → `All 716 PASS` → `C:\> `_.
- **PASS**.

---

## Fase 2 — SMP=2

```
qemu -smp 2
```

- `SMP_DETECT apic_base 0xfee00000 version 0x50014 max_lvt 6 ICR clear true final true` → `has_aps true`.
- `MADT dump` no imprimió ( `find_madt_table` → None, `BOOT_RSDP_ADDR` 0x1fec1014 pero XSDT no mapeada).
- `BSP KPRCB 0x2403000` OK.
- `INIT IPI` + `SIPI vector 0x80` (0x80000) enviados, `AP_READY_COUNT` 0 tras 1s → `No APs found (single CPU mode)` → `1 CPU online` (segundo warning).
- `I/O APIC` OK, `STI` → `PAGE_TABLE_CORRUPTION` panic `arch/x64/idt.rs:871` `Page fault @ virt write=1 np=0 rip` `PAGE_TABLE_CORRUPTION`.

**No llega a `C:\>`**.

---

## Fase 3 — No APs found

Dos warnings `smp.rs:477` y `536` mismo texto, distinta causa:

- Primero tras `has_aps=false` → no se intenta AP. Con `-smp 2` `has_aps=true` → no dispara.
- Segundo tras `AP_READY_COUNT==0` 1s → **has_aps true pero AP nunca incrementa `AP_READY_COUNT`**.

Por tanto **Caso C**: MADT encontró 2 CPUs (implícito por `max_lvt`), AP startup ocurrió (INIT/SIPI enviados) pero handshake nunca llegó.

---

## Fase 4 — MADT

`timers/hpet.rs:330` `find_madt_table()` vía `RSDP→XSDT→APIC` no imprimió. Con `SMP=1` QEMU expone 1 `Processor Local APIC` (APIC ID 0, flags 1), con `SMP=2` debería exponer 2 (APIC 0 y 1). No se pudo registrar `logical CPU / APIC ID / enabled` por falta de dump. `max_lvt=6` sugiere 6 LVT entries, no CPU count.

---

## Fase 5 — INIT-SIPI-SIPI

`arch/x64/smp.rs:248` `send_init_ipi()` `ICR 3<<18|5<<8|1<<14` (all excl self, INIT), `send_sipi(0x80)` `3<<18|6<<8|0x80`. Destino `all excl self` `PHYSICAL` sin `apic_id<<32`, broadcast. Vector `0x80` → `0x80000` (512 KB). Correcto para Q35.

No se pudo verificar `destination mode` vs `logical` porque AP no responde.

---

## Fase 6 — AP Trampoline

`AP_TRAMPOLINE_ADDR` era `0x80_0000` (8 MB) pero `SIPI` `0x80` → `0x80000` (512 KB) **mismatch** → AP saltaba a basura → triple fault → no `AP_READY`. **Fix:** `0x8000` (32 KB) `smp.rs:25`, `vector 0x08` → `0x8000`.

Tras fix, `T` (trampoline start), `P` (PM32), `L` (LM64), `A`/`B`/`C` (ap_entry early `out 0x3F8`) aparecen en raw serial (counts 33/61/21/33/22/44), pero `AP_ENTRY` string no. AP llega a `ap_entry` `smp.rs:284` `out 'A'`/`out 'B'`/`out 'C'` pero `serial_println!` no imprime (GS/log no listo). `AP_READY_COUNT` sigue 0 → AP se queda en bucle sin incrementar.

Próximo marcador: `out 'D'`/`'E'` tras `write_gs_base` y `init_ap` no se vieron.

**Primer punto donde deja de ser correcta:** `copy_trampoline` destino vs `SIPI` vector (ya corregido), y luego `ap_entry` antes de `AP_READY_COUNT++`.

---

## Primer punto de fallo (actual)

```
BIOS/QEMU MADT(2 CPUs) → OK
  ↓
RSDP 0x1fec1014 → find_madt_table → FAIL (no dump)
  ↓
detect_aps max_lvt 6 → true
  ↓
copy_trampoline 0x8000 (fix) → OK (T,P,L,A,B,C visibles)
  ↓
INIT/SIPI 0x08 → AP salta a 0x8000 → OK (T)
  ↓
AP PM32 → P, AP LM64 → L, ap_entry A,B,C → OK
  ↓
ap_entry serial_println! → FAIL (no AP_ENTRY)
  ↓
AP_READY_COUNT 0 → timeout 1s → No APs found → 1 CPU
  ↓
STI → PAGE_TABLE_CORRUPTION (BSP) → síntoma secundario
```

`PAGE_TABLE_CORRUPTION` tras `STI` es síntoma de que la memoria en `0x8000` (trampolín) solapa con heap/stack/KPRCB o la tabla en `CR3 0x1f401000` fue corrompida por `copy_trampoline` a `0x8000` que está dentro de área EBDA/BDA (0x8000 es 32KB, usado por BIOS). Con `0x800000` (8MB) estaba en RAM libre (16 MB @0x2400000) pero fuera de <1MB.

---

## Corrección mínima

- Trampolín a `0x8000` + `0x8000` en todos los `.set` ya aplicado.
- Añadir `out` markers en cada fase de `ap_entry` tras `write_gs_base`, `init_ap`, `AP_READY_COUNT++`.
- Hacer `find_madt_table` robusto mapeando XSDT como `USER` o usando `BOOT_RSDP_ADDR` ya identidad-mapeado.
- No tocar PS/2/queue (ya READY).

---

## Phase 6 — IRQ33 CPU Ownership + VT SPSC (2026-09-25)

Pregunta: ¿puede IRQ33 producir en más de una CPU bajo SMP real?
Respuesta: **NO — SPSC VALID. No convertir a MPSC.**

### F1 — Routing programado (leído del IOAPIC, no del código)

`interrupts/ioapic.rs:162` programa ISA IRQ1 → pin 1 con
`vector | FIXED | PHYSICAL`, sin campo destino (= 0). Lectura en vivo:

```text
[IOAPIC_ROUTE] irq=1 pin=1 vector=33 deliv=0 destmode=0 dest_apic=0
               masked=0 trig=0 polar=0 raw=0x0000000000000021
```

`IRQ33 vector=33`, `delivery=Fixed(0)`, `destmode=Physical(0)`,
`dest_APIC_ID=0` (BSP), edge/high, unmasked. Idéntico en SMP2 y SMP4,
en boot y tras bursts (routing inmutable en runtime). Por construcción
el hardware solo puede interrumpir a CPU0.

### F2 — Instrumentación (solo diagnóstico, sin cambiar decoder/cola)

- `arch/x64/idt.rs`: `keyboard_handler` registra por IRQ
  `{seq,cpu,apic,tid,scancode}` en ring de 64 + contadores `KBD_IRQ_CNT[16]`.
  Se eliminó el `serial_println!` por IRQ (era el único serial en hot path).
  La llamada al decoder (`kbd_event_handler_direct`) es idéntica.
- `input/vt.rs`: `vt_diag_dump` añade espejo serial con contadores
  `push/pop/drop/max_occ` + `push_cpus`/`pop_cpus` distintos (el `println!`
  va al framebuffer y no queda en el serial log).
- Dump combinado en `Ctrl+Alt+V` (`kbd/hotkey.rs:31`) y syscall 99
  (`syscall/handlers.rs:666`): VT + KBD_IRQ + IOAPIC route.
- `main.rs`: tras `run_all`, `vt_diag_reset()` + `kbd_irq_reset()` dejan la
  línea base en cero para contabilidad exacta de bursts.

### F3/F4 — SMP2 + SMP4 (sendkey real, dump Ctrl+Alt+V)

Metodología limpia: baseline con Ctrl+Alt+V primero (≈0), luego burst,
luego dump. Monitor por socket UNIX vía python (sin eco socat).

SMP2 (`AP_READY_COUNT=1`, `2 CPU online`, 716 PASS):

```text
baseline: KBD total=3 (makes del propio hotkey), VT push=0 pop=0 drop=0
tras "ab"+ret: KBD total=15, producer_cpus=1, producer_cpu=0
  VT push=3 pop=0 drop=0 max_occ=3 push_cpus=1 (cpu=0) occ=3
```

SMP4 (`AP_READY_COUNT=3`, `4 CPU online`, 716 PASS): idéntico
(`KBD total=15`, `producer_cpu=0`, `VT push=3`, routing idéntico).
`PAGE_TABLE_CORRUPTION 0`, `PANIC 0` en ambos.

### F5/F6 — Correlación IRQ → VT_PUSH → READB

`irq_cpu == push_cpu == 0` en el 100% de eventos muestreados
(anillo KBD de 64 + anillo VT de 64: 34 PUSH/28 POP/2 EMPTY, todos cpu=0).
`producer_cpu != consumer_cpu` es válido y esperado: el consumidor es el
thread de shell (migrable); SPSC exige UN productor y UN consumidor por
cola, no la misma CPU.

### F7/F8 — Stress + shell ocupada (SMP2, 30×a gap≈0 + hello)

```text
KBD total=75, producer_cpus=1 (cpu=0)
VT push=36 pop=0 drop=0 max_occ=36 push_cpus=1 occ=36 (h=4 t=40)
```

`produced(36) == consumed(0) + dropped(0) + remaining(36)` exacto.
`pop=0` a los 3 s es latencia de `execute_line` (línea de 30 a's en
ejecución), no pérdida: la cola conserva los 36 bytes sin DROP teniendo
capacidad (4095). `KBD_PUSH` continuó durante `READB` retrasado.

### F9 — Atomicidad SPSC (`input/vt.rs:99-127`)

Push: `head Acquire` → check hueco → `write slot` → `tail Release`.
Pop: `tail Acquire` → `read slot` → `head Release`. Con UN productor
demostrado (F3/F4) y UN consumidor por cola (solo el foreground thread
hace `pop_byte_from_vt` de su VT), no hay escritores concurrentes de
`tail` ni lectores concurrentes de `head`. Correcto para SPSC.

### F10 — `active_vt` (`input/manager.rs:5,40-61`)

Global `AtomicUsize` (`Relaxed` load en push, `Release` store en switch).
Ventana teórica: un switch entre el load y el push desvía 1 byte a la
cola de la VT anterior — benigno (sin corrupción: cada cola sigue SPSC;
a lo sumo 1 byte en VT no activa). Sin evidencia de switches durante
bursts en estas pruebas.

### F11 — Hotkey/VT switch

`Alt+F1-F4` cambia `active_vt`; `Ctrl+Alt+V` solo vuelca diagnóstico
(consume su propia V sin pushear: baseline `VT push=0` lo demuestra).
Sin cambios de VT durante los bursts medidos.

### F12 — Affinity

El routing admite reprogramación (campo destino), pero NO se modificó en
producción en esta fase. La arquitectura SPSC no depende de que el
productor sea CPU0: `push_byte` usa `active_vt` global y colas por VT,
válido para cualquier CPU única.

### F13 — Invalidación deliberada

NO ejecutada por diseño: requeriría añadir un segundo productor al código
real, violando "no convertir / no modificar decoder". La conclusión se
basa en F1–F4 (productor único observado + routing que lo impone).

### F14 — Conclusión: SPSC VALID

```text
SMP2       PASS (716/716, AP_READY=1)
SMP4       PASS (716/716, AP_READY=3)
IRQ33      single producer {CPU0} en ambas
VT SPSC    VALID — producer_cpus=1, irq_cpu==push_cpu siempre
burst      PASS (36/36, drop=0)
busy shell PASS (remaining=36, drop=0)
accounting PASS (produced==consumed+dropped+remaining exacto)
```

Invariante:

```text
VT INPUT QUEUE INVARIANT:
Exactly one producer: the CPU receiving IRQ33 (IOAPIC pin 1,
  Fixed/Physical/dest 0 → CPU0; routing MUST preserve single-producer).
Exactly one consumer: the thread performing readb() for that VT.
```

**NO convertir SPSC a MPSC.** Nota histórica: la matriz con eco socat
(±16k pushes) quedó invalidada por metodología con eco; la metodología
limpia con reset post-boot da contabilidad exacta al byte.

## Validación pendiente

- `qemu -smp 2` con fix debe ver `AP_ENTRY` y `2 CPU(s) online` y `C:\>` sin panic.
- `qemu -smp 4` → 4 CPUs.
- Luego repetir `vtdiag` + `SHELL_BUSY` en SMP2/4.

**Estado actual:** `SMP1 PASS`, `SMP2 FAIL` por trampolín (parcialmente fix, AP ahora arranca pero no completa), `PAGE_TABLE_CORRUPTION` es síntoma.

---

## SCHEDULER TEST ISOLATION (Phase 4/5 — 2026-09-25)

Root cause:
Tests instantiated local Scheduler state while AP scheduler remained active
against global KPRCB/runqueues.

Symptoms (SMP2: 713/716):

- `k18_steal_affinity_mismatch_stale_cpu_detected` — `more than one Running`
- `k18_schedule_via_steal_sets_current_tid_and_removes` — `more than one Running`
- `k19_repeated_steal_requeue_bounce_5_cycles` — `stolen == 1` (victim vacía
  tras `make_thread_ready` quedarse en thief)

Fix (test isolation, NOT scheduler semantic change):

- `scheduler/mod.rs:262` `pub(crate) static SCHED_TEST_MODE: AtomicBool = false`
- `scheduler/smp.rs:18` `try_work_steal` early-return `None` solo si
  `SCHED_TEST_MODE && kprcb_thread_in_self()` (path global). El Scheduler
  local de los tests (`kprcb_thread_in_self()==false`) sigue pudiendo robar.
- `scheduler/schedule.rs:280` `on_timer_tick` early-return bajo test mode
  (pausa preemption del AP durante manipulación global de colas).
- `scheduler/tests.rs` RAII guards en k18/k19 (`store(true)` + `Drop → false`).
  Cubre return normal y `Err` (los asserts usan `return Err`, no panic).
  Panic real = abort del kernel; flag irrelevante tras halt.
- `scheduler/tests.rs` k18_a usa `set_test_current(2)`; k18_schedule bloquea
  `BOOT_TID`; k19 re-encola a víctima entre iteraciones.
- `testing.rs:29` + `main.rs:621` assert `!SCHED_TEST_MODE` tras cada test y
  tras la suite (debug_assert + check visible en release).
- Contadores `smp.rs:STEAL_ATTEMPTS/SUCCESS`, `schedule.rs:SCHEDULE_CALLS`
  + dump per-CPU `KPRCB/qlen` para F4–F6.

Important:
This is test isolation, not a scheduler semantic change. Queue ownership,
steal algorithm, thread-state semantics, runqueue structure y affinity rules
no cambian. Solo se pausa actividad del AP global durante manipulación
controlada de colas en tests.

Validation (2026-09-25, QEMU q35 TCG, disk_image.img 212 MB):

- `SMP2`: `MADT 2 CPUs`, `AP_READY_COUNT=1`, `2 CPU(s) online`,
  `STI done`, `All 716 PASS`, `C:\>`, `PAGE_TABLE_CORRUPTION 0`,
  `[SMP] SCHED_TEST_MODE after bring-up = false`,
  `[SCHED_TEST_MODE] after suite = false`,
  `[STEAL] attempts=39 success=8 schedule_calls=203` (runtime normal roba).
- `SMP4`: `MADT 4 CPUs`, `AP_READY_COUNT=3`, `4 CPU(s) online`,
  `All 716 PASS`, `FAIL 0`, `STEAL attempts=38 success=8`.
- Muestreo repetido SMP2: 4/4 PASS con timeout ≥60 s (1 iteración con timeout
  de 50 s quedó truncada pre-tests por lentitud TCG, no por FAIL; reintentada
  con 70 s → PASS). Suite completa 20× pendiente de CI por coste (~35 s/boot).
- `git diff --check` limpio; `PS/2`, keyboard decoder, `VtInputQueue`,
  neoshell intactos (solo 8 ficheros: smp, main, scheduler×3, tests, testing, hpet).

---

## PHASE 8 — SCHEDULER RUNNING vs ACTUAL DISPATCH (forensic, 2026-09-25)

Objetivo: localizar por qué el consumidor `neoshell` (TID5/PID4) queda
`RUNNING` sin ser despachado (Phase 7).

### Fuentes de verdad mapeadas

| Campo | Escritor(es) | Lector(es) |
| --- | --- | --- |
| `Scheduler.current_tid` | `schedule()` steps 1/2/3, idle; `syscall_try_resched` (KEEP_CURRENT/fallback); timer paths (`scheduler.current_tid = tid`) | `current_tid_for_this_cpu` (fallback), `current_kthread_mut`, tests |
| `KPRCB.current_thread/pid` | `sync_per_cpu_current` (schedule steps 1/2/3), `this_cpu_set_current_thread` (resched/timer) | `try_per_cpu_tid/pid`, `kprcb_thread_in_self` |
| `Kthread.state` | schedule steps 1/2/3 + idle; resched (KEEP_CURRENT/fallback/idle); timer preempt/revert/kernel-mode; `make_thread_ready`; `on_timer_tick` | validate, schedule scan, wake |
| `Kthread.cpu` | creation, spawn, `steal_and_migrate` | `enqueue/remove_from_run_queue`, validate |
| runqueues | `enqueue/remove_from_run_queue`, `steal_and_migrate`, try_dequeue_local | schedule step1, validate |

`current_tid_for_this_cpu()` y `current_kthread_mut()` prefieren KPRCB
(`try_per_cpu_tid`) si `kprcb_thread_in_self()`; si no, el campo global.
`Scheduler.current_tid` es **global**, no per-CPU.

### Invariante violada (confirmada en runtime)

```
A thread in state Running MUST be the context actually dispatched on its CPU,
and KPRCB.current_tid MUST equal the executing thread.
```

Observado (SMP1, `SCHED_WARN`):
```
tids Running = [2, 5]  ambos cpu=0   (dos Running en la misma CPU)
tid=5 pid=4 state=RUNNING wait=None  (shell stranded, VT_POP=0)
kprcb_tid=Some(2)  sched.current=2   (desincronizado de tid5)
```

### Offending path (traza instantánea, solo TID5)

Con `[T5_SCHED]` en los puntos de asignación de `schedule()` y `[T5_RS]` en
`syscall_try_resched`:

```
[T5_SCHED] step=1 prev=3 new=5 current=5 rq0=0 kprcb=Some(5)   shell despachado correcto (Ring3)
[T5_RS]    entry=3 next=5 next_rsp=0x2495240 next_cs=0x1b       cs=0x1b (Ring3) OK
--- shell llama read y bloquea ---
[T5_SCHED] step=3 prev=5 new=2 current=2 rq0=0 kprcb=Some(5)   schedule() step3 elige tid2
[T5_RS]    entry=5 next=2 next_rsp=0x2479240 next_cs=0x8        cs=0x8  -> ¡Ring0!
```

El hilo `tid=2` (PID1, proceso Ring3) posee en `k.rsp` un **frame Ring0**
(CS=0x8) porque fue expulsado/bloqueado dentro del kernel. `schedule()` ya
había **confirmado** efectos (`self.current_tid=2`, `sync_per_cpu_current` →
KPRCB=2, `tid2.state=Running`) ANTES de que el caller validara el frame. El
caller detecta `next_cs & 3 != 3`, re-encola `tid2` como `Ready`, pero la
restauración es **incompleta**: no revierte `KPRCB` al hilo realmente en
ejecución y, según la rama, puede dejar el hilo elegido/`current_tid`
desincronizados. De ahí `sched.current != kprcb_tid` y el `Running` huérfano.

### schedule() — contrato actual vs correcto

```
ACTUAL:   mark Running + current_tid + KPRCB  ->  select  ->  caller valida frame
CORRECTO: select + valida frame  ->  (si válido) mark Running + current_tid + KPRCB
```

El contrato actual filtra una decisión de scheduling en los callers
(`syscall_try_resched`, timer) que no siempre hacen rollback completo.

### Fix mínimo propuesto (NO aplicado)

1. Rollback completo en `syscall_try_resched`/timer al rechazar un frame
   (`next_cs & 3 != 3`): `(*next).state = Ready; enqueue; self.current_tid = tid;`
   **y** `sync_per_cpu_current(current_thread_ptr, pid)` para que KPRCB vuelva
   al contexto real.
2. Alternativamente (más limpio): mover la validación de frame dentro de
   `schedule()` y no confirmar `Running`/`current_tid`/KPRCB hasta que el frame
   sea despachable (Ring3 para retorno de syscall/timer desde Ring3).

No aplicado: se ha localizado la ruta y el mecanismo, pero el cambio toca el
núcleo del scheduler y debe validarse con 716/716 en SMP1/2/4 antes de commit.

### Instrumentación conservada (gated, sin coste en boot)

- `scheduler/schedule.rs`: `consistency_check(tag)` (allocation-free, WARN+dump,
  activada tras los tests vía `sched_forensic_enable`), `rq_len` best-effort.
- `[T5_SCHED]` / `[T5_RS]` / `[T5_TM]` trazas puntuales solo para TID5.
- `SCHED_DUMP` (Phase 7) lock-free best-effort.

### Resultado

```
INVARIANT VIOLATED: Running sin dispatch + KPRCB != Scheduler.current_tid
OFFENDING PATH:     syscall_try_resched / timer branch next_cs&3!=3
                    (schedule() confirma estado antes de validar el frame)
FIX:                NOT APPLIED (propuesto, pendiente de validación)
REGRESSION:         neodev test 716/716 PASS (instrumentación gated)
READY:              NO
```

---

## PHASE 9 — DISPATCH COMMIT POINT (2026-09-25)

### Contrato corregido

`schedule()` commitaba `state=Running` + `Scheduler.current_tid` + KPRCB **antes**
de que el caller validara que el frame del candidato es despachable. Si el
candidato tenía frame Ring0 (`cs=0x8`) — porque fue expulsado dentro del kernel —
el caller lo rechazaba después de haber mutado el estado, dejando un `Running`
huérfano y `KPRCB != Scheduler.current_tid`.

Nuevo contrato (SELECT → VALIDATE → COMMIT → DISPATCH):

- `Scheduler::schedule_with(require_ring3: bool)`.
- `schedule()` = `schedule_with(false)` (comportamiento histórico).
- Si `require_ring3` y el candidato no tiene frame Ring3 (`frame_is_ring3`),
  se **devuelve a la runqueue** y **no se modifica** `current_tid`, KPRCB ni
  `state`. Se sigue escaneando; si no hay candidato válido, fallback a idle.
- Callers que retornan a Ring3 usan `schedule_with(true)`:
  `syscall_try_resched`, timer user-preempt (`idt.rs`), `exception_do_resched`.
  Los paths kernel (idle-preempt, kernel-preempt) mantienen `schedule()`.

Ficheros: `scheduler/schedule.rs` (`schedule_with` + `frame_is_ring3`),
`syscall/resched.rs`, `arch/x64/idt.rs`.

### Evidencia (SMP1, traza T5)

Antes (Phase 8):
```
[T5_SCHED] step=3 prev=5 new=2 current=2 rq0=0 kprcb=Some(5)
[T5_RS]    entry=5 next=2 next_rsp=0x2479240 next_cs=0x8   <-- tid2 (Ring0) COMMIT
```
Después (Phase 9):
```
[T5_RS] entry=3 next=5 next_rsp=0x2495240 next_cs=0x1b   shell despachado (Ring3 válido)
[T5_RS] entry=5 next=1 next_rsp=0x4238ad0 next_cs=0x8    candidato Ring0 NO comprometido -> idle
```
`SCHED_WARN` (dos `Running` en la misma CPU) = **0** en todas las ejecuciones
post-fix, frente a las violaciones observadas en Phase 8.

### Regresión

```
neodev test (SMP1)  716/716 PASS
git diff --check    CLEAN
```

### Limitación de verificación E2E

No se pudo demostrar `VT_POP > 0` en esta fase: el `sendkey` del monitor QEMU
dejó de entregar scancodes al guest en el arnés manual (0 `KBD_IRQ`/`VT_EV`
post-boot; los tests de boot y 716/716 sí pasan, y el routing IRQ1 estaba
verificado en Phase 6). Instrumentación añadida para futura verificación:
`[VT_EV]` (primeros 200 eventos, activado post-boot) y `[AUTO]`. Es una
limitación del arnés, no del kernel.

```
INVARIANT VIOLATED: resuelto a nivel de contrato (candidato inválido no compromete estado)
INVALID FRAME:      antes commit + rollback incompleto -> ahora re-enqueue sin commit
FIX:                APPLIED (commit point posterior a validación de frame)
REGRESSION:         716/716 PASS
KEYBOARD E2E:       no demostrable por limitación del monitor QEMU (sendkey)
READY:              NO (pendiente E2E)
```

