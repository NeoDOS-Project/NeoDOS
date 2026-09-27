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

---

## 6. Clasificación

| Aspecto | Resultado |
|---------|-----------|
| Vector primario | `#GP` (v=13) en el `iretq` de `syscall_handler_asm` |
| Error code | selector GDT inválido (`0x3188`, y `0x4868` en la corrida previa) |
| Clase | **G1** (frame de retorno inválido) habilitada por **G2/G4** (frame desgarrado / estado de scheduler incoherente) |
| Propagación | fallos en cascada en otras CPUs (PF a `0x10402b130`, invalid opcode `0x152`) |
| Indepurabilidad | deadlock reentrante de `SERIAL1` en el path de excepción (mitigado aquí) |
| Relación Fase 13 | misma familia que `phase13-ap-timer-iretq-gpf-forensics` (ya corregida allí para el caso AP-timer), reaparece por otra vía en v0.51.1 |
| Precursores | múltiples `[SYSCALL_CORRUPT]` (saved RIP/RSP ≠ now) inmediatamente antes |
| Disparador | `read` bloqueante del shell (`handler_read` → `Blocked`) seguido de tecla |

**Conclusión:** en v0.51.1 sigue existiendo una vía que entrega a `iretq` (syscall
return / timer return) un frame cuyo contenido está corrupto, con el síntoma terminal
de selector basura. El fallo es de temporización (VBox lo reproduce, QEMU SLiRP no) y
aparece justo tras el bloqueo/desbloqueo del shell.

---

## 7. Hipótesis de causa raíz (a confirmar)

1. **Ventana de publicación de un hilo bloqueado.** `handler_read` pone el hilo
   `Blocked` (`handlers.rs:151`) y sólo más tarde `syscall_try_resched` guarda su `rsp`
   (`resched.rs:128`). Un *waker* (entrada de teclado) que llame a
   `make_thread_ready` en esa ventana publica el hilo `Ready`/encolado **antes** de que
   su contexto esté guardado (la ventana R5 descrita en Fase 13 §13/§16). El guard
   `candidate_owned_elsewhere` (Fase 13-A.3) debería rechazar el despacho por otra CPU,
   pero conviene verificar que cubre **todas** las vías de selección y el caso de
   mismo-CPU / `KPRCB` aún no publicado.
2. **Consumo de un frame compartido por dos CPUs.** Si dos CPUs llegan a ejecutar el
   mismo `Kthread` sobre una única pila de 16 KiB, sus marcos de IRQ/syscall se solapan
   y el `iretq` de una consume bytes de la otra → selector/RIP basura. Es el mecanismo
   exacto documentado en Fase 13 (G2).
3. **Relación con `[SYSCALL_CORRUPT]`.** El diagnóstico de `syscall_trace_frame`
   detecta que el frame guardado por CPU no corresponde al hilo actual; puede ser
   ruido conocido, pero la coincidencia temporal invita a instrumentar el frame de
   syscall/PCR actual para descartar corrupción real.

---

## 8. Archivos modificados (sin commit)

- `neodos-kernel/src/arch/x64/serial.rs` — `RawSerial`, `_raw_print`, macros raw.
- `neodos-kernel/src/arch/x64/idt.rs` — cabeceras `[FAULT]` en GPF/PF/DF.
- `neodos-kernel/src/main.rs` — cabecera `[PANIC]`.
- `docs/investigation/smp4-shell-gpf-2026-09-27.md` — este informe.

Artefactos regenerados (gitignored): `disk_image.img`, `kernel.elf`, `disk_image.vdi`.

Evidencia serial: `/tmp/opencode/smp4_serial.log` (pre-instrumentación),
`/tmp/opencode/smp4_diag.serial`, `/tmp/opencode/smp4_diag2.serial`.

---

## 9. Próximos pasos

1. **Conservar el serial raw** en el path de excepción/panic (evita cuelgues que
   ocultan fallos; bajo riesgo).
2. **Cerrar la ventana de publicación de hilos bloqueados**: hacer que ningún *waker*
   publique `Ready` antes de que el `rsp` vivo esté guardado (protocolo de *wake*
   pendiente análogo a `yield_requested`) o extender el guard de ownership a todas las
   vías y al caso mismo-CPU.
3. **Instrumentar el frame de syscall** (RIP/CS/RSP/SS y su dueño por CPU) para
   distinguir ruido de corrupción real en `[SYSCALL_CORRUPT]`.
4. **Validación**: SMP1/2/4 (QEMU y VBox) + `neodev test`; criterio de aceptación de
   #293: shell responsivo a 4 vCPUs sin GPF/PANIC y nota en
   `docs/scheduler/scheduler.md`.

---

## 10. Referencias

- Issue #293 — *SMP4 instability: GPF/PANIC on shell/SMP path*.
- `docs/investigation/phase13-ap-timer-iretq-gpf-forensics.md` (§9 clasificación,
  §13 ventana R5, §18–§22 guard Phase 13-A.3).
- `docs/investigation/smp-bring-up-report.md` (§8–§9 commit point, `SCHED_WARN`).
- `neodos-kernel/src/arch/x64/serial.rs`, `arch/x64/idt.rs`, `syscall/handlers.rs`
  (`handler_read`), `syscall/resched.rs`, `scheduler/schedule.rs`, `arch/x64/gdt.rs`
  (`prepare_ring3_return`).

*Documento generado 2026-09-27. Sin commits; cambios sólo en working tree.*
