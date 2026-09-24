# Auditoría Forense: Camino de Arranque de NeoDOS a NeoShell

**Fecha:** 2026-09-17  
**HEAD Commit:** `5e756e1d38d2e859e954a756fd3c0c58a25aa317`  
**Branch:** `fix/p0-scheduler-runqueue-correctness`  
**Estado:** Diagnóstico Forense Completado y Verificado  

---

## 1. Contexto y Objetivos

Se ha realizado una auditoría forense completa paso a paso del flujo de arranque de NeoDOS desde la entrada del kernel (`rust_start`) hasta el lanzamiento e inicio de `NeoShell` (`NEOSHELL.NXE`).

El objetivo principal fue localizar la causa raíz exacta del comportamiento anómalo observado en el baseline, donde `NeoInit` entra en un bucle infinito `[neoinit] shell exited, respawning...` sin que `NeoShell` llegue a presentar su interfaz ni ejecutar su bucle principal de comandos.

---

## 2. Hallazgo Principal y Causa Raíz

### Diagnóstico

El fallo **NO se debe a un error en `neoshell.nxe`**, ni a una corrupción en el binario ELF, ni a una falta de memoria inicial.

La causa raíz reside en el mecanismo de conmutación de contexto durante el retorno de llamadas al sistema en `syscall_resched_if_needed` (`neodos-kernel/src/syscall/mod.rs:361-378`):

1. **Lanzamiento de NeoShell:** `NeoInit` (PID 3, Ring 3) ejecuta `sys_ob_create("\Global\FileSystem\C:\Programs\neoshell.nxe", ObType::Process)`. El kernel crea el proceso `NeoShell` en estado `ThreadState::Suspended`.
2. **Espera en NeoInit:** `NeoInit` llama a `sys_ob_wait(fd)` para bloquearse hasta que la shell termine.
3. **Transición en `handler_ob_wait` (`syscall/ob.rs:3245-3265`):**
   - El hilo de `NeoShell` se activa (`Suspended` → `Ready`).
   - El hilo de `NeoInit` pasa a `ThreadState::Blocked { waiting_for: magic }`.
   - Se marca `need_resched = true`.
4. **Retorno de Syscall:** `syscall_entry` detecta `need_resched == true` e invoca `syscall_resched_if_needed()`.
5. **Selección del Scheduler:** El planificador `schedule()` selecciona como candidato a **`netd` (TID 2, Ring 0, `cs=0x08`)**.
6. **Filtro de Ring 0 en Syscall Return:** `syscall_resched_if_needed` comprueba si el hilo destino es Ring 0 (`next_cs & 3 != 3`). Al ser `netd` un hilo de kernel, determina que no puede conmutar a un hilo de Ring 0 desde un marco de retorno de syscall de usuario.
7. **Violación de Invariante del Scheduler:** Para abortar el cambio de contexto a Ring 0, `syscall_resched_if_needed` ejecuta:
   ```rust
   if next_cs & 3 != 3 {
       ...
       scheduler.current_tid = tid;
       if let Some(current) = scheduler.find_kthread_mut(tid) {
           current.state = ThreadState::Running; // ← SOBREESCRIBE EL ESTADO BLOCKED DE NEOINIT
       }
       ...
       return current_rsp;
   }
   ```
8. **Efecto Anómalo:** El estado `Blocked` de `NeoInit` es forzosamente sobrescrito y restaurado a `Running`, retornando inmediatamente de la syscall con valor `0`.
9. `NeoInit` recibe el retorno `0` creyendo que la shell se ha cerrado, imprime `[neoinit] shell exited, respawning...` y reintenta `sys_ob_create` continuamente.
10. **Resultado:** `NeoShell` (que estaba en `Ready`) **nunca es seleccionado para ejecutarse**, y los reintentos continuos de `NeoInit` agotan los slots de heap (`HEAP_SLOT_USED`).

---

## 3. Matriz de Auditoría por Fases (Fases 0 — 14)

| Fase | Descripción del Hito | Resultado | Evidencia / Detalle Técnico |
| :--- | :--- | :--- | :--- |
| **0** | Baseline | **COMPLETADO** | Entorno limpio en `fix/p0-scheduler-runqueue-correctness`. Bucle de respawn reproducido. |
| **1** | Boot Kernel | **PASS** | `rust_start`, GDT, IDT, TSS, Paging identity map (0..4 GiB) inicializados. |
| **2** | Heap & Allocations | **PASS** | Heap 16 MB @ `0x2400000`. `AlignedKStack` (16 KB) con canary `0xDEADBEEFCAFEBABE`. |
| **3** | GDT/IRETQ/KStack | **PASS** | Frames Ring 0 (`CS=0x08`, `SS=0x10`) validados. |
| **4** | Scheduler Invariants | **FAIL** | **Violación:** Estado `Blocked` sobrescrito a `Running` en `syscall_resched_if_needed`. |
| **5** | Spawn Kthreads | **PASS** | Hilos TID 0 (`BOOT`), TID 1 (`IDLE`), TID 2 (`netd`) instanciados. |
| **6** | Netd Subsystem | **PASS** | `NETD_CREATED` → `NETD_QUEUED` → `NETD_FIRST_RUN` → `NETD_ENTRY` → `NETD_READY`. |
| **7** | Tests Kernel | **PASS** | 711 pruebas de kernel pasadas. |
| **8** | Transición Post-Tests | **PASS** | Transición hacia servicios, NXL, NEM y usermode completa. |
| **9** | NeoInit | **PASS** | PID 3 cargado en Ring 3 (`CS=0x1b`, `SS=0x23`). Registro `DefaultShell` leído. |
| **10** | Creación NeoShell | **PASS** | `NEOSHELL_CREATE` completa. ELF parseado, slot asignado, thread en `Suspended`. |
| **11** | Primera Ejecución Shell | **FAIL** | Transición `Ready → Running` de `NeoShell` cancelada por `syscall_resched_if_needed`. |
| **12** | Inicialización Shell | **BLOQUEADO** | Imposible alcanzar mientras Phase 11 falle. |
| **13** | NeoShell Ready | **BLOQUEADO** | Imposible alcanzar mientras Phase 11 falle. |
| **14** | Monitor Stack/Heap | **FAIL** | Bucle infinito de `NeoInit` agota slots de heap (`[USER] WARN: no free heap slots`). |

---

## 4. Estrategia de Solución Propuesta

Para resolver esta falla de arquitectura en el scheduler durante la conmutación desde retornos de syscall:

1. **Corrección del Filtro de Ring 0 en Syscall Return:**
   - En `syscall_resched_if_needed`, cuando `schedule()` selecciona un hilo de Ring 0 (como `netd`), el planificador no debe abortar la conmutación devolviendo la ejecución al hilo actual si este está `Blocked`.
   - En su lugar, el planificador debe buscar el siguiente hilo candidato de **Ring 3 que esté en estado `Ready`** (como `NeoShell`), o permitir que la conmutación ocurra de manera segura.
2. **Preservación del Estado `Blocked`:**
   - Si no hay ningún hilo de Ring 3 listo para ejecutarse, el hilo actual que se ha bloqueado NO debe ser cambiado a `Running`. Debe mantenerse en `Blocked` e invocar el bucle de espera de kernel / idle hasta que ocurra una interrupción de temporizador que reprograme el sistema.
