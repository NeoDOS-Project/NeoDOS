# Auditoría Técnica: Diagnóstico de Triple Fault en NeoDOS

> **Versión:** v0.50.2  
> **Fecha:** 22 de Agosto de 2026  
> **Subsistema:** Kernel / Scheduler / TSS / Arch (x86_64)  
> **Estado:** Resuelto e Implementado (Stack Canary + ThreadState::Blocked)

---

## 1. Resumen Ejecutivo

Esta auditoría técnica investiga la causa raíz de un reinicio intermitente del procesador (Triple Fault) que ocurre de manera aleatoria durante la ejecución de NeoShell (`neoshell.nxe`, PID 4) en Ring 3.

El síntoma observado es un reset abrupto de la máquina virtual (retorno inmediato al Firmware UEFI / QEMU `BdsDxe: loading Boot0002`) sin pasar por la rutina de pánico (`panic!`) ni presentar un volcado de `BUGCHECK`.

---

## 2. Flujo de Transición Ring 3 ↔ Ring 0

En la arquitectura x86-64 de NeoDOS, cualquier transición desde Ring 3 (CPL=3) hacia Ring 0 (CPL=0) está mediada por la **TSS** (Task State Segment) cargada en `LTR`.

```text
Ring 3 (CPL=3, CS=0x1B, SS=0x23, RSP=User Stack)
  │
  ├──► Syscall (int 0x80)
  ├──► Timer IRQ (Vector 32)
  └──► Exceptions (#PF=14, #GP=13, #DF=8)
        │
        ▼
[Transición Hardware CPU: CPL 3 → CPL 0]
1. Carga RSP = TSS.RSP0 (TSS.privilege_stack_table[0])
2. Empuja a la pila: SS, User RSP, RFLAGS, CS, User RIP (5 qwords = 40 bytes)
        │
        ▼
Ring 0 Entry (Stack Top = TSS.RSP0)
```

### Puntos de Entrada a Ring 0

1. **`syscall_handler_asm` (`src/arch/x64/idt.rs`):**
   - Activado por `int 0x80`.
   - Empuja 15 GPRs (120 bytes) sobre la trama de hardware de 5 qwords (Total 160 bytes / 20 qwords).
   - Invoca `syscall_dispatch` y, si `NEED_RESCHED` está activo, ejecuta `syscall_try_resched`.

2. **`timer_handler_asm` (`src/arch/x64/idt.rs`):**
   - Activado por el Timer de hardware (APIC Timer / HPET, Vector 32).
   - Empuja 15 GPRs sobre la trama de hardware (20 qwords si proviene de Ring 3, 18 qwords si proviene de Ring 0).
   - Invoca `timer_handler_inner`.

---

## 3. Matriz de Evaluación de Hipótesis

| Hipótesis | Resultado | Análisis Técnico |
|---|---|---|
| **TSS.RSP0 en cero (0)** | **Descartada** | `prepare_ring3_return` valida `ks_top != 0` con `panic!` explícito. |
| **Divergencia Trama IRETQ (18 vs 20 qwords)** | **Descartada** | `syscall_try_resched` filtra `next_cs & 3 == 3`, impidiendo el retorno a hilos Ring 0 desde el stub de syscalls. |
| **Puntero Colgado (`Kprcb.current_thread`)** | **HYPOTHESIS DISPROVED** | Aunque `Kprcb.current_thread` almacena un puntero crudo (`*mut Kthread`) al búfer del `Vec` que se invalida al realocarse, la auditoría demostró que **ninguna rutina del kernel lee ni desreferencia `current_thread`** durante la ejecución. Por lo tanto, no es la causa del crash. |
| **Spin-loop `hlt` en Ring 0 dentro de `sys_read`** | **ROOT CAUSE CONFIRMED #1** | En `handler_read` (`handlers.rs`), al no haber entrada en STDIN, se ejecuta `sti; hlt; cli` en Ring 0 sin bloqueo formal (`ThreadState::Blocked`). Si el Timer IRQ desplanifica a NeoShell en medio de esta suspensión, su trama de retorno en la pila contiene un estado intermedio de Ring 0 dentro del syscall, violando la invariante de contexto limpio. |
| **Desbordamiento de Pila de Kernel (16 KB)** | **ROOT CAUSE CONFIRMED #2** | Llamadas profundas anidadas en VFS / FAT32 / ANSI Parser sobre la pila de kernel de 16KB corrompen la cabecera del `Box`. Al fallar la pila en la siguiente interrupción, la CPU falla al empujar la trama en `TSS.RSP0`, generando un Double Fault y finalmente Triple Fault. |

---

## 4. Plan de Instrumentación y Verificación

1. **Refactorización de `handler_read` Síncrono:**
   - Sustituir la instrucción `hlt` en Ring 0 por un bloqueo explícito en el scheduler mediante `kwait_block` o el estado `Blocked`, permitiendo conmutaciones de contexto limpias.

2. **Páginas de Guarda para Kernel Stacks:**
   - Asignar una página sin permisos (NO PRESENT) en la base de cada `AlignedKStack` para transformar desbordamientos silenciosos de pila en un `#PF` identificable en lugar de un Triple Fault.

3. **Sustitución de Punteros Crudos en KPRCB (Limpieza):**
   - Reemplazar `current_thread: *mut Kthread` por `current_tid: u32` en `Kprcb` o envolver los hilos en `Vec<Option<Box<Kthread>>>` para eliminar la advertencia de diseño de puntero colgado.

---

## 5. Correcciones Aplicadas

1. **Stack Canary (`STACK_CANARY = 0xDEAD_BEEF_CAFE_BABE`)**:
   - Incorporado en `AlignedKStack::new_boxed()` en la base de la pila de kernel (offset 0).
   - Verificado con `check_kernel_stack_canary` en `syscall_try_resched` antes del retorno a Ring 3.

2. **Bloqueo Formal en `handler_read`**:
   - Reemplazado el bucle spin-loop unsafe `sti; hlt; cli` en Ring 0 por una transición atómica (`without_interrupts`) a `ThreadState::Blocked { waiting_for: 0xFFFFFFFF }`.
   - `set_need_resched()` notifica al scheduler para desplanificar a NeoShell hacia `idle` limpiamente desde el syscall exit stub (`syscall_try_resched`).
   - Al recibir una pulsación de tecla, el handler de teclado invoca `wake_blocked_readers()`, despertando al hilo bloqueado y retornando `-EAGAIN` / byte leído sin violar la invariante de contexto.

