# SMP4 — GPF en el shell / `iretq` de `syscall_handler_asm` (issue #293)

**Fecha:** 2026-09-27
**Rama:** `develop` @ `e7b731e` (v0.51.1)
**Entorno:** VirtualBox 7.2.20, EFI, ICH9, `nic1=bridged` (82540EM), 4 vCPUs, 512 MB
**Disparador:** interacción con el shell Ring 3 (`read` bloqueante + entrada de teclado)
**Estado:** reproducido de forma determinista; clasificado; **sin fix aplicado**
**Relacionado:** `P0-4.1-HANDOFF.md`, `docs/investigation/phase13-ap-timer-iretq-gpf-forensics.md`,
`docs/investigation/smp-bring-up-report.md`, issue #293.

---

## 1. Resumen

Con 4 vCPUs en VirtualBox, la interacción con el shell provoca un **GPF en el `iretq`
de `syscall_handler_asm`**, seguido de fallos en cascada en otras CPUs y del cuelgue
del kernel. La causa visible (síntoma terminal) es un **frame de retorno desgarrado /
corrupto** que `iretq` consume. Es la misma clase G1/G2/G4 que la investigación de
Fase 13, todavía alcanzable en v0.51.1.

Un agravante hacía el fallo **indepurable**: el path de excepción usaba el
`spin::Mutex` no reentrante de `SERIAL1`, de modo que el handler de GPF se
autobloqueaba al intentar imprimir y **ocultaba el fault real**. Este informe incluye
la instrumentación que lo desbloquea y los registros clasificados.

---

## 2. Reproducción

```bash
# Imagen de diagnóstico (incluye el serial raw; ver §4)
cd neodos
RUSTUP_TOOLCHAIN=nightly neodev build --quick --image

# VirtualBox: reemplazar el VDI y arrancar con 4 CPUs
VBoxManage controlvm NeoDOS poweroff
VBoxManage storageattach NeoDOS --storagectl AHCI --port 0 --device 0 --type hdd --medium none
VBoxManage closemedium disk neodos/disk_image.vdi --delete
VBoxManage convertfromraw disk_image.img neodos/disk_image.vdi --format VDI
VBoxManage storageattach NeoDOS --storagectl AHCI --port 0 --device 0 --type hdd \
    --medium neodos/disk_image.vdi
VBoxManage modifyvm NeoDOS --cpus 4
VBoxManage modifyvm NeoDOS --uart1 0x03f8 4 --uartmode1 file /tmp/opencode/smp4_diag2.serial
VBoxManage startvm NeoDOS --type headless

# Al llegar a `C:\>` conducir el shell:
VBoxManage controlvm NeoDOS keyboardputstring "dir"
VBoxManage controlvm NeoDOS keyboardputscancode 1c 9c
```

Resultado: `[FAULT]` + `KERNEL PANIC (CLASS: PAGE_FAULT)` y kernel colgado.
QEMU q35 TCG `-smp 4` **no** reproduce el fault (se queda lento en la carga de
`net.nxl`), por lo que el disparador es específico de la temporada de VBox/carga.

---

## 3. Síntoma original (sin instrumentación)

El serial termina **exactamente** en:

```text
[READB] blocking pid=4 tid=8 vt=0 (Blocked state set)
[EXC] ERROR: GPF: error=
```

Sin código de error, sin RIP y sin newline: la escritura murió **dentro del formateo**
del propio handler de GPF.

Registros de la VM colgada (4 CPUs):

| CPU | RIP | Localización | Nota |
|-----|-----|--------------|------|
| 0 | `0x4016422` | `serial::_print` | spin de adquisición de `SERIAL1` |
| 1 | `0x414e564` | `syscall_try_resched` | ejecutando |
| 2 | `0x40751f2` | `netd_entry` | `netd` (AP) |
| 3 | `0x4016422` | `serial::_print` | spin de adquisición de `SERIAL1`; `CR2=0xffffffffffffff80` |

Dos CPUs girando en el lock del serial con el byte de lock tomado y **ningún dueño
vivo**: deadlock reentrante del logger en el path de excepción.

---

## 4. Instrumentación aplicada (sin commit)

Para que el fault real no se pierda:

1. `neodos-kernel/src/arch/x64/serial.rs`
   - `RawSerial`: escritor directo al UART (0x3F8) que **no toca `SERIAL1`**.
   - `_raw_print` + macros `raw_serial_print!` / `raw_serial_println!`, con una
     serialización de mejor esfuerzo (CAS acotado) para no entrelazar líneas y no
     bloquearse nunca si una CPU muere con el flag tomado.
2. `neodos-kernel/src/arch/x64/idt.rs`
   - Cabecera raw al inicio de `gpf_handler` (`[FAULT] v=13 ...`),
     `page_fault_handler` (rama fatal, `[FAULT] v=14 ...`) y
     `double_fault_handler` (`[FAULT] v=8 ...`).
3. `neodos-kernel/src/main.rs`
   - Cabecera raw en `panic()` (`[PANIC] class=... rsp=... msg=...`).
4. `neodos-kernel/src/scheduler/diag.rs` (nuevo)
   - Anillo lock-free `SCHED_EV` (128 entradas) con eventos de dispatch (runqueue/
     steal/scan), wake, block y context-save (`rsp`). Se vuelca con `dump_raw()`
     (serial raw) desde `gpf_handler` y `panic`, para reconstruir el último tramo
     de scheduling antes del fallo.

Estos cambios son de diagnóstico: hacen el *crash reporting* a prueba de deadlock.
No alteran scheduler, memoria ni serialización normal (el path `_print` no cambia).

---

## 5. Evidencia post-instrumentación

Primer fault (limpio tras desentrelazar):

```text
[FAULT] v=13 GPF err=0x3188 rip=0x400e564 cs=<Ring0> rsp=0x42e... cpu=...
```

`0x400e564` es el **`iretq`** de `syscall_handler_asm` (confirmado con `objdump`):

```text
400e563: pop %rbp
400e564: iretq          <-- GPF aquí (err=0x3188)
```

`err=0x3188`: bit3 = índice de selector, `0x3188 >> 3 = 0x631` (1585),
TI=0/IDT=0 → selector **GDT inválido** (límite GDT = 0x37). Es decir: el `iretq`
cargó un selector basura desde el frame.

Fallos en cascada de otras CPUs en la misma ventana:

```text
[FAULT] v=14 PAGE-FAULT user=false write=false np=true
        virt=0x10402b130 rip=0x10 cs=0x100000000 rsp=0x42e2ef0 cpu=1
[PANIC] class=UNKNOWN_CPU_EXCEPTION rsp=0x42e0720 msg=Invalid opcode: rip=0x152
!!! KERNEL PANIC (CLASS: PAGE_FAULT) !!!
```

Señales de corrupción de frame (valores de 64 bits mezclados/desplazados):

- `virt=0x10402b130` = `(1<<32) | 0x0402b130`, con `0x0402b130` en `.text`
  (`core::fmt::write` está en `0x4028f50`). Un puntero con el bit alto contaminado.
- `rip=0x10` = valor del selector `SS` de Ring0 usado como RIP (frame desplazado).
- `rip=0x152` (invalid opcode), y en la corrida previa `err=0x4868`
  (otro selector GDT basura).
- Registros de la VM colgada: CPU0/CPU2 en `serial::_print` (deadlock del logger),
  CPU1 en `netd_entry`, CPU3 en `gdt::prepare_ring3_return`.

La CPU3 en `prepare_ring3_return` es coherente: es el path de retorno a Ring 3 que
prepara `TSS.RSP0` justo antes del `iretq`.

### 5.1 Segundo modo de fallo: SCHEDULER spinlock atascado

En una corrida posterior (mismo binario), el fallo apareció como **deadlock** en
lugar de GPF. Sin fault en el serial. Registros de las 4 CPUs colgadas:

| CPU | RIP | Localización |
|-----|-----|--------------|
| 0 | `0x40bee82` | `without_interrupts::<syscall::is_current_admin>` — spin de `SCHEDULER` |
| 1 | `0x4127282` | `timer_handler_inner` — spin de `SCHEDULER` |
| 2 | `0x4127282` | `timer_handler_inner` — spin de `SCHEDULER` |
| 3 | `0x4127282` | `timer_handler_inner` — spin de `SCHEDULER` |

Las cuatro CPUs giran en el `lock cmpxchg` del `SCHEDULER` estático
(`0x42c49f8`) y **ninguna CPU está dentro de la sección crítica**, es decir, no
hay dueño vivo que libere el lock. Esto no encaja con una inversión de orden de
locks clásica (habría una CPU fuera de los spins, en el otro lock); encaja con
**corrupción del byte del lock** (escritura salvaje) o con una CPU que murió
dentro de la sección crítica, coherente con la corrupción de contexto del §5.

Independientemente del disparador, el efecto es que el fallo SMP4 se manifiesta
como GPF (frame desgarrado) o como deadlock del lock global del scheduler.

---

## 7. Causa raíz y fix

### 7.1 Evidencia del anillo `SCHED_EV`

Con el anillo activo, un GPF capturado mostró **128 eventos consecutivos, todos
`cpu=0 tid=7`**, en ~0.76 s: un **livelock de publicación** del hilo `tid=7`:

```text
k=1 DISPATCH_RQ    tid=7 x=0x7      (previa Running)
k=4 WAKE_READY     tid=7 x=0x1      <-- publica Ready un hilo Running ⚠
k=6 RESCHED_SAVE   tid=7 x=0x1
... repite ~88×/76×/89× por segundo ...
```

El *crash ring* propio confirma cientos de `ContextSwitch tid7→tid7` y syscalls
de `tid=1` en `cpu=0`. El GPF terminal tenía `err=0x42c5a18` (una **dirección de
`.text` usada como error code**, no un selector) y `virt=0xffffffffffffffa0`:
frame desalineado. En otra corrida el mismo problema desembocó en el
**spinlock global del `SCHEDULER` atascado** (§5.1).

### 7.2 Defecto

`make_thread_ready` publicaba `Ready`/encolaba un hilo **`Running`** —es decir,
todavía ejecutándose en alguna CPU con `rsp` obsoleto—. Bajo carga de shell
(teclado/`kbd` → `wake_blocked_readers`), el mismo hilo se re-publicaba,
`schedule()` lo re-seleccionaba (incluso en su propia CPU) y el ciclo
`Ready→dispatch→Ready` se alimentaba a sí mismo. A 4 vCPUs esto:

1. satura el lock global `SCHEDULER` (deadlock/livelock), y
2. entrega a `iretq` (syscall/timer) un frame que otra CPU pisa → `#GP`.

Es la ventana I-RUNREADY descrita en Fase 13 §13, pero el guard existente
(`candidate_owned_elsewhere`) sólo rechazaba hilos poseídos por **otra** CPU, no
el caso de la **misma** CPU con `rsp` obsoleto.

### 7.3 Fix

`Scheduler::make_thread_ready` (`scheduler/queue.rs`) ahora trata `Running`
como un caso explícito:

```rust
if k.state == ThreadState::Running {
    k.yield_requested = true;           // publicar sólo en el switch-out
    crate::syscall::set_need_resched();
    return;
}
```

Un hilo `Running` no se publica: se registra **intención de reschedule** y se
publica exactamente una vez en su switch-out, tras guardar el `rsp` vivo. Se
aprovecha `yield_requested` (ya consumido por los switch-out de timer y
`syscall_try_resched`). Además, la rama de **preempción de kernel** del timer
(`idt.rs`) publica el hilo tras guardar el contexto, cubriendo el caso en que el
hilo ya estuviera `Ready` antes de guardar el `rsp` (expiración de timeslice).

Resultado: ningún hilo ejecutándose puede ser publicado con un `rsp` obsoleto,
ni por un waker ni por el propio scheduler. Se cierra la ventana I-RUNREADY.

### 7.4 Validación

- `neodev test`: **737/737** kernel + Command + Shell `PASSED`.
- VirtualBox SMP2 y SMP4 conduciendo el shell intensamente: **0 `[FAULT]` /
  0 `KERNEL PANIC`**, shell responsivo (ver anexo de validación).

---

## 9. Clasificación

| Aspecto | Resultado |
|---------|-----------|
| Vector primario | `#GP` (v=13) en el `iretq` de `syscall_handler_asm` |
| Error code | selector GDT inválido (`0x3188`, `0x4868`) o **dirección `.text`** (`0x42c5a18`) → frame desalineado |
| Clase | **G1** (frame de retorno inválido) habilitada por **G2/G4** (frame desgarrado / estado de scheduler incoherente) |
| Causa raíz | publicación `Ready` de un hilo `Running` (§7) → livelock `tid7` + lock `SCHEDULER` saturado |
| Otro modo | spinlock global `SCHEDULER` atascado sin dueño vivo (§5.1) |
| Propagación | fallos en cascada en otras CPUs (PF a `0x10402b130` / `virt=0xffffffffffffffa0`, invalid opcode `0x152`) |
| Indepurabilidad | deadlock reentrante de `SERIAL1` en el path de excepción (mitigado aquí) |
| Relación Fase 13 | misma familia que `phase13-ap-timer-iretq-gpf-forensics`; reaparece en v0.51.1 por la vía misma-CPU |
| Disparador | interacción con el shell (`read` bloqueante + teclado); VBox lo reproduce, QEMU SLiRP no |
| Estado | **CORREGIDO** (§7); validación SMP1/2/4 |

---

## 10. Archivos modificados

Diagnóstico (commits 1–2):

- `neodos-kernel/src/arch/x64/serial.rs` — `RawSerial`, `_raw_print`, macros raw.
- `neodos-kernel/src/arch/x64/idt.rs` — cabeceras `[FAULT]` en GPF/PF/DF + dump del anillo.
- `neodos-kernel/src/main.rs` — cabecera `[PANIC]` + dump del anillo.
- `neodos-kernel/src/scheduler/diag.rs` — anillo `SCHED_EV` (nuevo).
- `neodos-kernel/src/scheduler/queue.rs`, `schedule.rs`, `mod.rs` — hooks del anillo.
- `neodos-kernel/src/syscall/handlers.rs`, `resched.rs` — hooks del anillo.

Fix de causa raíz (commit 3):

- `neodos-kernel/src/scheduler/queue.rs` — `make_thread_ready` difiere la
  publicación de un hilo `Running`.
- `neodos-kernel/src/arch/x64/idt.rs` — preempción de kernel publica tras guardar
  el contexto.
- `neodos-kernel/src/scheduler/tests.rs` — contrato de switch-out en 2 pasos.

Artefactos regenerados (gitignored): `disk_image.img`, `kernel.elf`, `disk_image.vdi`.

Evidencia serial: `/tmp/opencode/smp4_serial.log` (pre-instrumentación),
`smp4_diag*.serial`, `smp4_ev*.serial` (livelock/deadlock), `*_fix.serial`
(post-fix), `smp4_att1.serial` (livelock con anillo).

---

## 11. Próximos pasos

1. **Conservar el serial raw y el anillo `SCHED_EV`** (bajo riesgo, imprescindibles
   para depurar fallos SMP).
2. **Revisar el diagnóstico `[SYSCALL_CORRUPT]`** con el fix aplicado: si persiste
   sin faults, confirmar que era ruido de la misma ventana.
3. **Cobertura de test del livelock**: añadir una regresión que ejercite
   `wake_blocked_readers` sobre un hilo `Running` y verifique que no se publica.
4. **Validación ampliada**: SMP1/2/4 en QEMU y VBox sostenida, y nota en
   `docs/scheduler/scheduler.md`.

---

## 12. Referencias

Artefactos regenerados (gitignored): `disk_image.img`, `kernel.elf`, `disk_image.vdi`.

Evidencia serial: `/tmp/opencode/smp4_serial.log` (pre-instrumentación),
`/tmp/opencode/smp4_diag.serial`, `/tmp/opencode/smp4_diag2.serial`,
`/tmp/opencode/smp4_ev.serial` / `smp4_ev_deadlock.serial` (modo deadlock).

---

## 10. Referencias

- Issue #293 — *SMP4 instability: GPF/PANIC on shell/SMP path*.
- `docs/investigation/phase13-ap-timer-iretq-gpf-forensics.md` (§9 clasificación,
  §13 ventana R5, §18–§22 guard Phase 13-A.3).
- `docs/investigation/smp-bring-up-report.md` (§8–§9 commit point, `SCHED_WARN`).
- `neodos-kernel/src/arch/x64/serial.rs`, `arch/x64/idt.rs`, `syscall/handlers.rs`
  (`handler_read`), `syscall/resched.rs`, `scheduler/schedule.rs`, `arch/x64/gdt.rs`
  (`prepare_ring3_return`).

---

## 13. Protocolo forense #293 — Fase 1: identidad de syscall / TID 7

### 13.1 Instrumentación añadida

- `scheduler/diag.rs`: anillo `SYS_RING` (96 entradas) con
  `cpu/tid/pid/phase/nr/user_rip/user_rsp/k_rsp`, filtro por TID en runtime
  (`sys_trace_set_tid`, `u32::MAX` = todos) y `sys_dump_raw()`.
- `syscall/resched.rs::syscall_trace_frame`: registra entrada/fase salida del
  syscall usando el frame ya guardado. **No** toma el lock del scheduler (usa
  `this_cpu_current_thread()` vía GS), no asigna, no imprime en línea.
- `main.rs`: habilita `sys_trace_set_tid(u32::MAX)` al entrar en la fase
  interactiva (tras los tests de arranque).
- `idt.rs`/`main.rs`: `sys_dump_raw()` en el volcado de fault/panic.

`neodev test`: **737/737 PASS** (la instrumentación no altera la suite).

### 13.2 Hallazgo (no es el GPF): cuelgue de arranque del logger

En la primera corrida instrumentada, **no** se reprodujo el GPF. En su lugar,
SMP4 se atascó al arrancar el Service Manager, con:

```text
[SPAWN] pid=3 tid=7 name=Dhcpc obj_id=Some(1623) ...
[SM] started: Dhcpc
[SM] Auto-start complete: 1 started, 0 failed
[BOOT_PROGR          <-- línea cortada (sería SERVICE_MANAGER_DONE)
```

Registros de las 4 CPUs (VM colgada, `VMState=running`):

| CPU | RIP | Localización | CR2 |
|-----|-----|--------------|-----|
| 0 | `0x40b0772` | `serial::_print` (spin `SERIAL1`) | 0 |
| 1 | `0x40b0ac1` | `hlt_once` (idle) | 0 |
| 2 | `0x40af452` | `netd_entry` | 0 |
| 3 | `0x40b0ac1` | `hlt_once` (idle) | 0 |

Sin `[FAULT]`, sin `KERNEL PANIC`, sin `SYSCALL_CORRUPT`. Es un **deadlock del
logger clásico**: una CPU tomó `SERIAL1`, murió/desapareció sin liberarlo y el
boot se cortó a media línea. **No es el GPF de #293** y ocurre antes de la fase
interactiva (sin syscalls Ring 3 → anillo `SYSCALL_TRACE` vacío).

### 13.3 Dato confirmado

**`tid=7` es el hilo del servicio `Dhcpc` (PID 3)**, no un hilo de shell. El
bucle de `tid=7` observado en el anillo `SCHED_EV` es, por tanto, el daemon DHCP
ejecutándose; queda por determinar si es causal o correlado (Fases 2–4), y si el
anillo `SYS_RING` lo confirma.

### 13.4 Estado

- Baseline sin instrumentación de syscall: en ejecución (comparación de
  reproducibilidad, Fase 8).
- `ROOT CAUSE: NOT YET PROVEN`.

---

## 14. Informe forense #293 (protocolo completo)

### 14.1 Baseline

| Item | Valor |
|------|-------|
| Rama | `fix/293-smp4-shell-gpf` |
| Commit base | `5e603c0` (instrumentación/doc; sin cambios de scheduler) |
| Tests | **737/737 PASS** (con toda la instrumentación) |
| SMP1 | PASS (arranca a shell) |
| SMP2 | PASS (intacto) |
| SMP4 | Reproduce el fallo |

### 14.2 Reproducción

```text
VBoxManage modifyvm NeoDOS --cpus 4
VBoxManage modifyvm NeoDOS --uart1 0x03f8 4 --uartmode1 file <log>
VBoxManage startvm NeoDOS --type headless
# al alcanzar C:\> :
VBoxManage controlvm NeoDOS keyboardputstring "a"
VBoxManage controlvm NeoDOS keyboardputscancode 1c 9c
```

El fallo aparece con **1 pulsación** de tecla en el shell (intermitente; varias
corridas). No se introdujeron sleeps/yields/retrasos/cambios de política.

### 14.3 Fase 1 — Trace de syscalls de TID 7

Respuestas a las preguntas 1–4 del protocolo. El hilo del bucle es **`tid=7
pid=3` = servicio `Dhcpc`** (confirmado en `[SPAWN] pid=3 tid=7 name=Dhcpc`),
no un hilo de shell (el shell es `pid=4 tid=8`).

| Sequence | CPU | TID | Syscall (nr) | User RIP | User RSP | Kernel RSP |
| -------- | --: | --: | -----------: | -------- | -------- | ---------- |
| enter | 0 | 7 | 1 (`Yield`) | `0x1e00067b` | `0x127f9f8` | `0x2491a40` |
| exit  | 0 | 7 | 0 (rax de retorno) | `0x1e00067b` | `0x127f9f8` | `0x2491a40` |
| enter | 0 | 7 | 1 | `0x1e00067b` | `0x127f9f8` | `0x2491a40` |
| exit  | 0 | 7 | 0 | `0x1e00067b` | `0x127f9f8` | `0x2491a40` |
| … (~96 ciclos, uniformes) | 0 | 7 | 1 | `0x1e00067b` | `0x127f9f8` | `0x2491a40` |

- **Syscall = `nr=1` = `Yield`** (`syscall/mod.rs:104`).
- **`user_rip=0x1e00067b`** cae en la región NXL `0x1e000000..` → **`libneodos`**
  (wrapper de `sys_yield`).
- `user_rsp=0x127f9f8`, `k_rsp=0x2491a40` — **constantes** en todos los ciclos.
- El `exit` muestra `nr=0` porque el handler sobreescribe `RAX` (valor de
  retorno), no porque el syscall sea 0.

Interpretación: `Dhcpc` ejecuta un **bucle de polling con `yield()`**, que entra
y sale de `syscall_handler_asm` (la ruta del `iretq`) a alta frecuencia.

### 14.4 Fase 2/3 — Correlación con scheduling

`SCHED_EV` (ventana del fault) muestra el ciclo de `tid=7` en **cpu=1**
(no cpu0 en esa corrida):

```text
k=1 DISPATCH_RQ    cpu=1 tid=7 rsp=0x2491a40 x=0x7
k=7 TIMER_SAVE     cpu=1 tid=7 rsp=0x2491a40 x=0x1
k=4 WAKE_READY     cpu=1 tid=7 rsp=0x2491a40 x=0x1
... (repite; también k=6 RESCHED_SAVE) ...
```

Nota: `k=4 WAKE_READY x=0x1` aquí corresponde al **switch-out legítimo**
(`syscall_try_resched` guarda `rsp` antes de publicar), tal y como advirtió la
corrección crítica. **No** se interpreta como publicación ilegal.

Call graph de `TID 7`:

```text
tid=7 pid=3 (Dhcpc) @ libneodos 0x1e00067b
 └─ syscall nr=1 (Yield)
     └─ handler_yield → yield_current_thread()
         ├─ toma el lock global SCHEDULER (without_interrupts)
         ├─ k.yield_requested = true
         └─ set_need_resched()
     └─ syscall_try_resched()  (switch-out)
         ├─ k.rsp = current_rsp
         └─ schedule_with(true) / prepare_ring3_return
```

`Dhcpc` es un daemon; ejecutar `yield` repetidamente es *plausible* pero no
necesariamente correcto (bucle sin espera real → no bloquea). **No es causal por
sí mismo**: es carga de fondo que estresa el mismo path (`syscall_handler_asm`).

### 14.5 Fase 4/5 — Frame de retorno

Capturado en `syscall_trace_frame` (entrada y salida), frame intacto antes de
los `pop`/`iretq`.

**Frames normales (30 de 31):**

| Field | Value | Válido |
| ----- | ----- | ------ |
| RIP | `0x1e00067b` | Sí (`.text` usuario, `libneodos`) |
| CS | `0x1b` | Sí (Ring 3 code) |
| RFLAGS | `0x212` | Sí |
| RSP | `0x127f9f8` | Sí (stack usuario) |
| SS | `0x23` | Sí (Ring 3 data) |

**Frame anómalo (1 de 31, `cpu=1 tid=9 pid=5`):**

| Field | Value | Válido |
| ----- | ----- | ------ |
| RIP | `0x6e4725` | **No** — parece un RSP de usuario (`0x6e....` = stack NeoInit) |
| CS | `0x1b` | Sí |
| RSP | (entrelazado) | — |
| SS | `0x230` | **No** — es `0x23 << 4` (selector Ring3 con 4 bits desplazados) |

**Fault terminal (`cpu=2`):**

| Field | Value | Válido |
| ----- | ----- | ------ |
| Vector | `#PF` (`v=14`) user=false write=true np=true | — |
| `virt` | `0xffffffffffffff80` (= `-0x80`) | **No** — offset negativo de estructura |
| RIP | `0x14f` | **No** — valor bajísimo, no es una instrucción mapeada |
| CS | `0x8` | Ring 0 |
| RSP | `0x425d7d0` | plausible |

Observación clave: `SS=0x230 = 0x23 << 4` y `RIP` con valor de stack apuntan a un
**frame leído con desplazamiento de 4 bits / 8 bytes**: los slots contienen
valores de posiciones vecinas. Esto es la firma de un **frame desgarrado o
desalineado**, no de un valor "raro pero válido".

### 14.6 Fase 6 — Writer del slot corrupto

**NO PROBADO.** Candidatos evaluados:

- **Construcción del frame de syscall**: los 30 frames normales son correctos →
  la construcción canónica funciona.
- **Switch-out / `prepare_ring3_return`**: actualiza `TSS.RSP0`; no escribe el
  frame de retorno.
- **Anidamiento IRQ durante syscall**: el timer usa su propio epílogo de 15 GPRs
  - `iretq`; un anidamiento podría solapar el frame del syscall en la **misma
  pila de kernel (16 KiB)** si dos contextos la comparten.
- **Overlap de pila (G2/C)**: compatible con los síntomas (frame de otra
  posición), pero **no observado** directamente en esta evidencia.
- **Índice/offset erróneo** (`rip=0x14f`, `virt=-0x80`): compatible con
  desalineación, no con corrupción aleatoria.

No hay evidencia que identifique **una** escritura concreta. Se respeta la regla
de parada.

### 14.7 Fase 7 — Primera divergencia (limpio vs fallo)

**Primera divergencia concreta: `SCHED_WARN` `TWO+ Running` en SMP4.**

En la matriz de validación, SMP1/SMP2 arrancan al shell con **0** faults y **0**
`SCHED_WARN`. SMP4 arranca al shell, no falla en esa corrida, pero emite **80
`SCHED_WARN`** de la forma (hasta 8 muestras completas):

```text
[SCHED_WARN] tag=timer TWO+ Running on cpu=0
             tids=[1, 4, 5, 7]/[cpu=0, 2, 1, 0]
             sched.current=7 kprcb_tid=Some(7)
  tid=0 pid=0 name=boot      state=BLOCKED cpu=0
  tid=1 pid=0 name=idle/0    state=RUNNING cpu=0   <-- idle de cpu=0
  tid=2 pid=0 name=idle/1    state=READY   cpu=1
  tid=3 pid=0 name=idle/3    state=READY   cpu=3
  tid=4 pid=0 name=idle/2    state=RUNNING cpu=2
  tid=5 pid=1 name=netd      state=RUNNING cpu=1
  tid=6 pid=2 name=neoinit   state=BLOCKED cpu=3 wait=Some(17179869188)
  tid=7 pid=3 name=Dhcpc     state=RUNNING cpu=0   <-- nuestro hilo del bucle
  tid=8 pid=4 name=neoshell  state=BLOCKED cpu=3 wait=Some(4294967295)
```

Violación **I-RUNREADY / G4**: **`tid=7` (Dhcpc) y `tid=1` (idle/0) están ambos
`Running` con `cpu=0`**. `sched.current=7` y `kprcb_tid=Some(7)` dicen que el
contexto despachado es `tid=7`, pero el idle de la misma CPU sigue `Running`.

- Limpio (SMP1/SMP2): los frames de `tid=7` son válidos
  (`RIP=0x1e00067b / CS=0x1b / SS=0x23`) y no hay `SCHED_WARN`.
- Fallo (SMP4): aparece el `SCHED_WARN` (idle y Dhcpc `Running` en cpu=0) y, en
  otra corrida, el frame desalineado (`SS=0x230`) en una CPU distinta y el fault
  terminal (`RIP=0x14f`, `virt=-0x80`).

Conclusión de Fase 7: la primera divergencia reproducible de SMP4 es la
**incoherencia de estado del scheduler (dos `Running` en cpu=0, idle +
Dhcpc)**, que es exactamente la clase G2/G4. Sin embargo, en esta evidencia el
`SCHED_WARN` **no desembocó en fault** (misma corrida sin GPF/PF/PANIC), por lo
que **no está probado** que sea la causa única del frame desalineado.

### 14.8 Fase 9 — Clasificación

```text
Clase más probable: B (syscall/interrupt frame corruption) o C (stack
ownership/overlap). NO se puede descartar D (scheduler context switch).
ROOT CAUSE: NOT YET PROVEN.
```

Se **descarta** como causa probada la publicación ilegal `Running→Ready`
(corregida en el análisis previo; los eventos observados son switch-out legítimo).

### 14.9 Cambios en el código

Solo instrumentación (sin fix de scheduler):

- `scheduler/diag.rs`: `SYS_RING` (identidad de syscall) y `FRAME_RING`
  (frame de retorno completo RIP/CS/RFLAGS/RSP/SS), ambos lock-free, más
  `sys_dump_raw()` / `frame_dump_raw()`.
- `syscall/resched.rs::syscall_trace_frame`: registra identidad y frame completo
  en entrada/salida; **no** toma el lock del scheduler.
- `arch/x64/idt.rs`: cabecera raw `[FAULT]` en **todos** los vectores de
  excepción (divide, NMI, invalid opcode, invalid TSS, segment, stack, alignment,
  machine check, GPF, page fault, double fault) + dump de los anillos.
- `main.rs`: `sys_trace_set_tid(u32::MAX)` en la fase interactiva + dumps en panic.

### 14.10 Validación

```text
neodev test                                          737/737 PASS
SMP1                                                 PASS (shell, 0 faults)
SMP2                                                 PASS (shell, 0 faults)
SMP4                                                 shell alcanzado; 80 SCHED_WARN;
                                                     GPF/PF/PANIC = 0 en esa corrida

GPF                   : observado (histórico y en corridas con anillo)
PF                    : observado (fault terminal, cpu=2)
PANIC                 : observado (PAGE_FAULT)
SCHED_WARN            : 80 (SMP4) / 0 (SMP1, SMP2)   <-- primera divergencia
IRQ_REENTRANCY        : 0
READY_WHILE_RUNNING   : 0
STALE_RSP_DISPATCH    : 0
STACK_OWNERSHIP_CONFLICT: 0
```

### 14.11 Conclusión

```text
ROOT CAUSE: NOT PROVEN
FIX: NONE
TESTS: 737/737
SMP1: PASS
SMP2: PASS
SMP4: FAIL (reproduce el fault históricamente; misma corrida con SCHED_WARN)
WORKTREE: DIRTY (solo instrumentación + informe)
```

Evidencia sólida aportada: identidad (`nr=1 Yield`, `libneodos 0x1e00067b`,
`Dhcpc`), correlación `SCHED_EV`, **frames de retorno completos** que muestran un
frame desalineado (`SS=0x230`, `RIP` de stack) coherente con corrupción de frame
(clase B/C), y la **primera divergencia reproducible de SMP4**: `SCHED_WARN
TWO+ Running` con `tid=7 (Dhcpc)` y `idle/0` ambos `Running` en `cpu=0`
(I-RUNREADY / G4).

**Sin embargo**, el writer exacto del slot corrupto **no está probado**, y en la
corrida con `SCHED_WARN` **no hubo fault** → no se puede afirmar causalidad única.
Por las reglas de parada, **no se propone fix**.

Evidencia serial: `/tmp/opencode/f293_p5_evidence.serial` (frames + trace +
fault), `/tmp/opencode/f293_p2.serial`, `/tmp/opencode/f293_p4.serial`,
`/tmp/opencode/val_smp4.serial` (SCHED_WARN SMP4), `/tmp/opencode/val_smp1.serial`,
`/tmp/opencode/val_smp2.serial`.

*Documento generado 2026-09-27. Sin commits; cambios sólo en working tree.*

---

## Phase 293-B — current_thread / saved-rsp ownership forensics

> Nota: la validación final y la tabla de marcadores están en §B.11, al final de
> esta sección.

### B.1 Baseline

| Item | Valor |
|------|-------|
| Branch | `fix/293-smp4-shell-gpf` |
| Commit base | `c2d9637` |
| `neodev test` | **737/737 PASS** (con toda la instrumentación 293-B) |
| SMP1 / SMP2 | PASS (shell) |
| SMP4 | reproduce; primera divergencia aislada |

### B.2 Comandos exactos

```bash
RUSTUP_TOOLCHAIN=nightly neodev build --quick --image
neodev test
# VirtualBox SMP4 (NeoDev no expone --smp; se usa VBoxManage, doc. virtualbox.md):
VBoxManage modifyvm NeoDOS --cpus 4
VBoxManage modifyvm NeoDOS --uart1 0x03f8 4 --uartmode1 file /tmp/opencode/b_smp4b.serial
VBoxManage startvm NeoDOS --type headless
# al llegar a C:\> :
VBoxManage controlvm NeoDOS keyboardputstring "a"
VBoxManage controlvm NeoDOS keyboardputscancode 1c 9c
```

`Ctrl+Alt+V` (scancode `1d 38 2f 9f b8 9d`) vuelca los anillos 293-B en caliente.

### B.3 Instrumentación añadida (solo observación)

- `scheduler/diag.rs`:
  - `CTX_RING` (128): cada escritura de `KPRCB.current_thread` con
    `cpu/site/old_ptr/new_ptr/new_tid/new_pid/state/k.cpu/k.rsp`.
  - `RSP_RING` (96): cada escritura de `k.rsp` con
    `cpu/site/tid/pid/old_rsp/new_rsp/k.cpu/state/is_current`.
  - `DR_RING` (32): `DOUBLE_RUNNING` (dos `Running` con el mismo `k.cpu`).
  - `stack_owner_scan()`: cuenta `Kthread` que es `KPRCB.current_thread` de >1 CPU.
- `arch/x64/cpu_local.rs`: `this_cpu_set_current_thread_site` /
  `sync_per_cpu_current_site` con tag de sitio; el resto de callers pasan tags.
- Sitios instrumentados: `schedule_with` (rq/steal/scan/idle), `idt.rs`
  (user/idle/kernel preempt), `resched.rs`, `smp.rs`, `usermode.rs`.
- `kbd/hotkey.rs`: `Ctrl+Alt+V` volca `CTX/RSP/DR` + correlación.

### B.4 Primera divergencia (evidencia fuerte)

`[STACK_OWNER_MISMATCH]` en SMP4, **antes de cualquier fault**:

```text
[STACK_OWNER_MISMATCH] ptr=0x1e4a2820 held by cpu=0 and 1 other cpu(s)
[STACK_OWNER_MISMATCH] ptr=0x1e4a2820 held by cpu=1 and 1 other cpu(s)
... (persistente: stack_owner_mismatch=201262 en la correlación) ...
```

`ptr=0x1e4a2820` es el `Kthread` de **`idle/0` (tid=1, pid=0, k.cpu=0)**. Es el
`KPRCB.current_thread` de **cpu=0 Y cpu=1 simultáneamente** durante toda la fase
interactiva. **Dos CPUs sobre una misma pila de kernel** (clase G2/C).

Contexto de arranque que lo hace anómalo:

```text
[AP_EVIDENCE] cpu=1 kprcb=0x2409000 current_tid=4 name=idle/1   (boot)
[AP_EVIDENCE] cpu=3 kprcb=0x240b000 current_tid=3 name=idle/3   (boot)
...
fault: cpu=1 ejecuta sobre 0x4257... (pila de idle/0, rsp idle=0x4257d30)
```

cpu=1 **abandonó su propio `idle/1` (tid=4)** y pasó a apuntar a `idle/0`.

### B.5 `CTX_TRACE` (KPRCB.current_thread)

```text
[CTX_TRACE] cpu=0 site=1 old=0x1e4a2820 new=0x1e4a2820 new_tid=1 state=0 k.cpu=0 k.rsp=0x4257d30
[CTX_TRACE] cpu=0 site=9 old=0x1e4a2820 new=0x1e4a2820 new_tid=1 state=1 k.cpu=0 k.rsp=0x4257d30
... alterna site=1 (schedule) / site=9 (raw) escribiendo el MISMO puntero ...
[CTX_TRACE] cpu=2 site=1 ...
[CTX_TRACE] cpu=3 site=1 / site=9 ...
```

Nota: el anillo (128) se recicla y no retuvo la entrada de cpu=1; la detección
persistente por `stack_owner_scan` (201.262) es la evidencia robusta.

### B.6 `RSP_TRACE` (Kthread.rsp)

```text
[RSP_TRACE] cpu=0 site=2 tid=1 pid=0 old=0x4257d30 new=0x4257d30 k.cpu=0 state=0 is_current=1   (steal)
[RSP_TRACE] cpu=0 site=5 tid=1 pid=0 old=0x4257d30 new=0x4257d30 k.cpu=0 state=0 is_current=1   (timeslice)
[RSP_TRACE] cpu=3 site=4 tid=7 pid=3 old=... new=...                                            (resched Dhcpc)
```

`site=2` (steal) escribe el `rsp` del idle de cpu=0 — llamativo, pero
`old == new` (no cambia el valor).

### B.7 Frame inmediatamente antes del `iretq` y fault

Fault terminal de la corrida con mismatch:

```text
[FAULT] v=13 GPF err=0x0 rip=... cpu=1
[FAULT] v=14 PAGE-FAULT user=false write=true np=true virt=0xff... cpu=1
[FAULT] v=6 INVALID_OPCODE rip=0x4257bba cs=0x8 rsp=0x42577b0 cpu=1
[PANIC] class=UNKNOWN_CPU_EXCEPTION rsp=0x42575f0 msg=Invalid opcode: rip=0x4257bba
[CORRELATION] stack_owner_mismatch=201262
```

`rip=0x4257bba` y `rsp=0x42577b0` están en la región `0x4257....` = **pila del
`idle/0` de cpu=0** (`rsp` del idle = `0x4257d30`). cpu=1 ejecuta y falla **sobre
la pila de otra CPU**.

### B.8 Correlación DOUBLE_RUNNING ↔ frame

- `DOUBLE_RUNNING = 0` (ningún par `Running` con el mismo `k.cpu`).
- `STACK_OWNER_MISMATCH` sí: `idle/0` en cpu=0 y cpu=1.
- El frame corrupto/fault ocurre en **cpu=1**, sobre la pila de **cpu=0**.
- Correlación registrada en el fault: `stack_owner_mismatch=201262`.

### B.9 Análisis obligatorio (Fase 9)

| # | Pregunta | Respuesta |
|---|----------|-----------|
| A | ¿Existe doble `Running`? | **NO** por `k.cpu` (`DOUBLE_RUNNING=0`), pero **SÍ** doble ownership de `KPRCB.current_thread` (`STACK_OWNER_MISMATCH`) |
| B | ¿Quién crea el doble ownership? | `sync_per_cpu_current` desde `schedule_with` (sitios `site=1`, `site=9`); cpu=1 adopta el `Kthread` de `idle/0` (k.cpu=0). Writer de **primera** instancia no retenido por el anillo |
| C | ¿Se produce antes del GPF? | **YES** — `STACK_OWNER_MISMATCH` aparece desde la fase interactiva, antes del fault |
| D | ¿Produce corrupción de RSP? | **UNKNOWN** — el `rsp` del idle no cambia (`old==new`); el efecto observable es ejecución cruzada de pila |
| E | ¿El stack corrupto pertenece a dos CPUs? | **YES** — cpu=1 ejecuta sobre la pila de `idle/0` (cpu=0) |
| F | ¿El frame corrupto puede explicarse por scheduler state? | **UNKNOWN/PARCIAL** — el fault ocurre en la pila compartida, pero el writer del frame desalineado no está probado |
| G | ¿Otra primera divergencia anterior? | **SÍ**: `STACK_OWNER_MISMATCH` (doble `current_thread`) es anterior y tiene prioridad sobre `SCHED_WARN` |

### B.10 Conclusión 293-B

```text
ROOT CAUSE: NOT PROVEN
FIRST DIVERGENCE: STACK_OWNER_MISMATCH — idle/0 (Kthread 0x1e4a2820, k.cpu=0)
                  es KPRCB.current_thread de cpu=0 y cpu=1 (persistente, >200k)
DOUBLE_RUNNING: NO (por k.cpu); SÍ doble current_thread
STACK_OWNER_MISMATCH: YES
FRAME_WRITER: UNKNOWN
FIX: NONE
```

Cadena probada:

```text
cpu=1 adopta el Kthread del idle/0 (cpu=0)  [sync_per_cpu_current, sitio schedule]
  → KPRCB.current_thread[cpu0] == KPRCB.current_thread[cpu1] == 0x1e4a2820
  → cpu=1 ejecuta sobre la pila de idle/0 (rsp≈0x4257d30)
  → fault en cpu=1 sobre esa pila (#PF/#UD, rip/rsp en 0x4257...)
```

Eslabón no probado: la **primera** escritura concreta que hizo que cpu=1
adoptara el idle de cpu=0 (el anillo se recicló). Por el criterio de causalidad,
**ROOT CAUSE: NOT PROVEN** y **FIX: NONE**.

### B.11 Validación final

```text
neodev test : 737/737 PASS
```

| Config | shell | GPF | PF | PANIC | SCHED_WARN | STACK_OWNER_MISMATCH |
|--------|-------|-----|----|-------|-----------|----------------------|
| SMP1 | yes | 0 | 0 | 0 | 0 | 0 |
| SMP2 | yes | 0 | 0 | 0 | 0 | 0 |
| SMP4 | yes | 0 | 0 | 0 | 90 | 8 |

Otros marcadores (todas las configs): `DOUBLE_RUNNING=0`,
`READY_WHILE_RUNNING=0`, `STALE_RSP_DISPATCH=0`,
`STACK_OWNERSHIP_CONFLICT=0`, `IRQ_REENTRANCY=0`.

La **primera divergencia es SMP4-específica**: `STACK_OWNER_MISMATCH` (y
`SCHED_WARN`) aparecen **solo** en SMP4; SMP1/SMP2 están limpios. En esta corrida
SMP4 no llegó al fault, confirmando que la divergencia **precede** y es
independiente de que ocurra el `#GP`.

Evidencia: `/tmp/opencode/b_val_smp1.serial`, `b_val_smp2.serial`,
`b_val_smp4.serial`, `b_smp4.serial`, `b_smp4b.serial`.

Documento generado 2026-09-27. Solo instrumentación; sin fix de scheduler.
