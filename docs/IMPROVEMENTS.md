# NeoDOS — Roadmap Pendiente

> Items pendientes del roadmap. Los completados están en
> [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md).

> Version actual: v0.46 (Fase 2 Objectification completada — Timer, Semaphore, Section Objects).
> Objetivo: v1.0 — executive NT-like arquitectonicamente solido.
> **GUIA:** Leer [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) antes de planificar cualquier cambio.
> Fuente de verdad arquitectonica: [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md)

**Proximo milestone: v0.47** (Networking TCP/IP).

---

## Reglas de ejecucion

1. Una fase no empieza hasta que sus prerequisitos esten marcados **[COMPLETED]**.
2. Cada item pendiente incluye: ID, equivalente NT, archivos, prereqs, criterio de aceptacion, tests.
3. Al completar un item: actualizar `CHANGELOG.md`, `AGENTS.md` y moverlo a `IMPROVEMENTS_COMPLETED.md`.
4. Validar antes de cerrar: `cargo build` en `neodos-kernel/` + `python3 scripts/auto_test.py` + `scripts/check_deps.py`.

### Checklist por item completado

- [ ] Codigo implementado
- [ ] Tests en `testing.rs` (minimo 1 por invariante)
- [ ] `auto_test.py` pasa
- [ ] `check_deps.py` pasa
- [ ] `CHANGELOG.md` actualizado
- [ ] `AGENTS.md` / `ARCHITECTURE_SOURCE_OF_TRUTH.md` si cambia contrato
- [ ] Movido a `IMPROVEMENTS_COMPLETED.md`

---

## ROADMAP PENDIENTE (v0.40 -> v1.0)

> Basado en el analisis completo de `docs/ARCHITECTURAL_VISION.md`.
> **Regla de oro:** No anadir features nuevas antes de completar la fase de maduracion (v0.40-v0.45).
> Cada feature nueva se apoya en abstracciones existentes; si esas abstracciones son fragiles, la feature sera fragil.

---

### Fase 1: Maduracion (v0.40 - v0.45) [COMPLETED]

*Todos los items de la Fase 1 estan completados.*

1. ~~**v0.43** — SeAccessCheck NT-compatible, sys_poll(), Congelar pipe/IRP protocols~~ **COMPLETADO**
2. ~~**v0.44** — ASLR v1 (base aleatoria), Ob syscalls RAX 60-66~~ **COMPLETADO** (v0.44.2: Ob migration completa, todas las syscalls legacy desactivadas)
3. ~~**v0.45** — Ob migration, Device Tree + Resource Manager, Driver state machine freeze~~ **COMPLETADO** (Ob migration completada en v0.44.2; Device Tree y Resource Manager se mueven a v0.46)

---

### Code Quality & Maintenance

### Critical Bugs (SMP-unsafe static mut)

* [ ] **CB1. Fix WAIT_PID static mut SMP-unsafe** | Prereqs: KWait | Files: `src/usermode.rs`
  - **Descripcion:** `WAIT_PID` es un `static mut` en usermode.rs usado para comunicacion entre sys_waitpid y el manejador de terminacion de proceso. En SMP, dos CPUs pueden ejecutar sys_waitpid concurrentemente. Migrar a KWait: `kwait_block(ChildExit(pid))` + `kwait_wake(ChildExit(pid))`.
  - **Severidad:** CRITICO — data corruption en sistemas multicore
  - **Tests:** `smp_waitpid_concurrent`, `smp_waitpid_no_race`

* [ ] **CB2. Fix ISOLATED_REGIONS static mut sin sincronizacion** | Prereqs: -- | Files: `src/drivers/driver_runtime.rs`
  - **Descripcion:** `ISOLATED_REGIONS` es un array estatico mutable accedido desde boot loader, NDREG y hot reload sin Mutex. Envolver en `spin::Mutex<[Option<IsolatedRegion>; MAX_ISOLATED_DRIVERS]>`.
  - **Severidad:** CRITICO — data corruption si 2 CPUs hacen operaciones de driver concurrentes
  - **Tests:** `smp_isolated_region_concurrent_access`

* [ ] **CB3. Fix NXL_REGISTRY static mut sin proteccion SMP** | Prereqs: -- | Files: `src/nxl.rs`
  - **Descripcion:** `NXL_REGISTRY` es array fijo de 8 slots accedido desde sys_loadlib sin sincronizacion. Envolver en `spin::Mutex<[Option<NxlEntry>; 8]>`.
  - **Severidad:** ALTA — dos procesos cargando NXLs concurrentemente pueden corromper el registry
  - **Tests:** `smp_nxl_concurrent_load`

### Documentation & Housekeeping

* [ ] **DH1. Actualizar README.md a v0.44.3** | Prereqs: -- | Files: `README.md`
  - **Descripcion:** README actual muestra v0.39.11 con 320+ tests y 36 syscalls. Actualizar a v0.44.3: 528 tests, 66 syscalls, Ob API, input subsystem, virtual terminals.
  - **Criterio:** README refleja estado real del sistema (version, tests, syscalls, arquitectura)

* [ ] **DH2. Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md** | Prereqs: -- | Files: `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`
  - **Descripcion:** El documento menciona MAX_PROCESSES como limite fijo, pero el scheduler usa Vec. Menciona 320+ tests (real 528). Boot phases incompletas (falta Phase 3.86 NXL load, Phase 3.9 ABI freeze).
  - **Accion:** Corregir Rule 6.3.1 (MAX_PROCESSES -> Vec dinamico), actualizar test counts, completar boot phase list.

* [ ] **DH3. Completar libneodos syscall wrappers** | Prereqs: -- | Files: `libneodos/src/syscall.rs`
  - **Descripcion:** Faltan wrappers para: `sys_thread_create` (RAX 22), `sys_thread_join` (RAX 23), `sys_sleep_ex` (RAX 41), `sys_poll` (RAX 59), `sys_ob_destroy` (RAX 66), `sys_driver_unload` (RAX 57). Anadir con macros asm igual que los wrappers existentes.
  - **Criterio:** Cada wrapper nuevo tiene test en cmdtest.nxe. 528 kernel tests + cmdtest pasan.
  - **Tests:** 6 nuevos (uno por wrapper)

### Architectural Issues (No criticos)

* [ ] **AI-1. Completar ObInfoClass/ObSetInfoClass enums** | Prereqs: -- | Files: `src/object/types.rs`
  - **Descripcion:** Anadir clases faltantes: `ObInfoClass::ReadContent = 15`, `ObInfoClass::VolumeLabel = 16`, `ObSetInfoClass::ProcessTerminate = 4`, `ObSetInfoClass::VfsRename = 6`, `ObSetInfoClass::WriteContent = 7`, `ObSetInfoClass::SetCwd = 8`, `ObSetInfoClass::SetVolumeLabel = 9`.
  - **Criterio:** Enums reflejan implementacion real del handler. Tests verifican mapping.

* [ ] **AI-2. Consolidar exports duplicados v3loader.rs / hst.rs** | Prereqs: -- | Files: `src/drivers/v3loader.rs`, `src/drivers/hst.rs`
  - **Descripcion:** 7 funciones `hst_*` estan exportadas en ambos archivos. Unificar en hst.rs como unico punto de exportacion. v3loader.rs solo llama a hst.rs.
  - **Criterio:** Zero exports duplicados. Todos los drivers NEM cargan correctamente.

* [ ] **AI-3. Unificar codigos de error ObError y SyscallError** | Prereqs: -- | Files: `src/object/types.rs`, `src/syscall/mod.rs`
  - **Descripcion:** ObError (-1 a -9) y SyscallError (16 codigos) son conjuntos independientes con traduccion manual en handler_ob_*. Unificar en un solo enum NeoDosError reutilizado por ambas capas.
  - **Criterio:** Mapping formal verificado por tests. No hay traduccion manual.

* [ ] **AI-4. Arreglar TOCTOU race en kobj_register** | Prereqs: -- | Files: `src/kobj/mod.rs`
  - **Descripcion:** `kobj_register()` checkea si el object existe (read lock) y luego inserta (write lock) sin atomicidad. Convertir a operacion atomica: `registry.iter_mut().find(|e| e.is_none())` + insert en un solo lock scope.
  - **Criterio:** Dos CPUs registrando el mismo objeto no pueden resultar en duplicados.

* ~~**[COMPLETED] AI-5. CQ1. Libneodos-nxl ya modularizado** — `libneodos-nxl/src/` ya usa modulos separados (`syscall.rs`, `io.rs`, `fs.rs`, `process.rs`, `mem.rs`, `error.rs`). Con la limpieza ABI v7, se eliminaron las funciones `nxl_sys_pipe/dup2/waitpid/chdir/chdir_parent/readdir` (process.rs) y `nxl_sys_mkdir/unlink/rmdir/rename/writefile` (fs.rs). No requiere mas reorganizacion.~~

---

### Fase 2: Expansion (v0.46 - v0.50)
*Anadir funcionalidades transformadoras. Ejecucion secuencial dentro de la fase.*

Orden de implementacion dentro de la fase:

1. **v0.46** — Device Tree + Resource Manager completo, PCI auto-vinculacion, VirtIO block driver (BOOT_DRIVER), sys_ioctl()
2. **v0.47** — Networking: NIC driver NEM + TCP/IP stack (B3.1-B3.2)
3. **v0.48** — Async I/O: IOCP v1, sys_accept/send/recv, AHCI NCQ (A5.3), DHCP (B3.3)
4. **v0.49** — ASLR v2 (pila/heap aleatorios), PGO, Benchmarking suite, NTP (B3.4)
5. **v0.50** — Registry hive database (B2.1-B2.5)

> **Regla:** No se pasa a la Fase 3 hasta que v0.50 este completo y todos los tests pasen.

---

### FASE A3 — Fault Tolerance (NT: Bugcheck, KD, SEH)

El kernel actual no sobrevive a fallos estructurados. Ring 3 mata el proceso en cualquier excepcion.

- [ ] **A3.2. Kernel debugger (KD)** | NT: WinDbg kernel-mode debugging | Prereqs: A3.1
  - **Archivos:** `src/debugger/mod.rs`, `src/debugger/breakpoint.rs`, `src/debugger/watchpoint.rs`, `src/shell/commands/debug.rs`, `scripts/kd_client.py` (GDB stub adapter)
  - **Descripcion:** Debugger residente en el kernel para inspeccion interactiva de fallos y ejecucion en vivo. No depende de una GDB externa, pero expone un stub remoto por serial para depuracion desde host cuando haga falta. El objetivo es poder detener el sistema de forma controlada, inspeccionar contexto, modificar puntos de control y reanudar sin perder el estado del bug.
    - **Breakpoints software:** INT3 (0xCC) instruction replacement. `set_breakpoint(addr)` guarda original byte, escribe 0xCC. `#BP` (INT3) handler chequea si breakpoint registrado, pausa kernel si match.
    - **Breakpoints hardware:** 4 registro DR0-DR3 + DR7 (debug control). `set_hw_breakpoint(addr, type: execute|read|write|readwrite, len: 1/2/4/8)` configura DR7. `#DB` (INT1) handler dispara si DR6 flag match.
    - **Pause model:** al dispararse un breakpoint valido, el debugger congela el flujo normal del kernel y entra en estado `Paused`, preservando RIP, RSP, GPRs, CR0-CR4 y el motivo de parada. En ese estado solo se aceptan comandos de depuracion explicitos.
    - **Resume model:** `DEBUG CONTINUE` reanuda exactamente desde la instruccion siguiente al breakpoint o desde el RIP ajustado por watchpoint, sin reentrar en panic ni perder el contexto capturado.
    - **Shell commands:**
      - `DEBUG BREAK <addr>` — set INT3 breakpoint
      - `DEBUG UNBREAK <addr>` — remove
      - `DEBUG WATCH <addr> <type: r|w|rw>` — set hardware watchpoint
      - `DEBUG CONTINUE` — resume ejecucion (solo legal desde breakpoint)
      - `DEBUG REG` — dump GPRs, CR0-4
      - `DEBUG MEM <addr> <len>` — hex dump memoria
      - `DEBUG STACK <depth=16>` — stack trace
      - `DEBUG SCHED` — dump scheduler state (runqueues, current thread)
    - **GDB protocol (serial):** Implementar GDB remote protocol subset (qSupported, vCont, g/G, m/M, Z/z) para que `gdb kernel.elf -ex 'target remote /dev/ttyUSB0'` funcione. El stub solo necesita ser suficiente para stop/resume, lectura de registros y memoria, y gestion basica de breakpoints.
    - **State:** Global `debugger_state: DebuggerState { breakpoints: [Option<BreakpointInfo>; 8], hw_watchpoints: [DrReg; 4], paused_rip: u64, last_stop_reason: StopReason }`. Las estructuras deben vivir en memoria kernel fija y no depender de heap durante la captura.
  - **Criterio:**
    - Breakpoint en `sys_write` entry. Kernel pausa, shell imprime "Breakpoint at 0xXXXX", espera comando.
    - `DEBUG REG` muestra RAX-R15 en ese punto.
    - `DEBUG CONTINUE` reanuda ejecucion (sin panic).
    - Watchpoint en direccion de heap: detiene si algo escribe. Log la instruccion (RIP) que escribio.
    - Un cliente GDB remoto puede conectar, listar registros, leer memoria y continuar sin corromper el estado interno.
  - **Tests:** `kd_breakpoint_set_and_hit`, `kd_breakpoint_invalid_addr`, `kd_watchpoint_write_detect`, `kd_register_snapshot`, `kd_gdb_protocol_qSupported` (5 tests).

---

- ~~**[COMPLETED] A4.4. Input subsystem redesign** | NT: ConDrv (Console Driver) | Prereqs: A4.7~~
  - ~~**Archivos:** `src/input/mod.rs` (reescritura), `src/input/manager.rs` (new), `src/input/vt.rs` (new), integracion `arch/x64/idt.rs` (PS/2 delivery)~~
  - ~~**Descripcion:** Sistema de entrada multiplexado soportando multiples terminales virtuales (VTs) con independencia de input. En vez de enviar bytes directamente al shell activo, el kernel clasifica y enruta el input a una cola por VT, permitiendo que varias sesiones coexistan sin pisarse entre si.~~
    - ~~**Virtual Terminals:** Max 4 VTs (Alt+F1-F4). Cada VT tiene:
      - Input queue (ring buffer 4 KB) independiente
      - Output buffer (attached framebuffer) independiente
      - Foreground pid (proces que recibe input)
      - Session leader (PID 1 NeoInit es sesion leader de todas)~~
    - ~~**Keyboard IRQ (PS/2 IRQ1):** Nueva ruta:
      1. IRQ1 handler lee scancode
      2. Convierte a ASCII (KBDUS/KBDSP layout)
      3. Chequea `active_vt`
      4. Inserta en `vt_queues[active_vt]`
      5. Envia event `EVENT_KEYBOARD_INPUT` al event bus (data0 = byte, data1 = vt_num)~~
    - ~~**VT switching:** Alt+F1 scancode detectado -> `InputManager::switch_vt(1)` -> `active_vt=0` -> framebuffer renderiza VT0, input lo recibe VT0 pid.~~
    - ~~**sys_read(fd=0, buf, len) stdin:** Bloquea en `vt_queues[active_vt].read()` hasta bytes disponibles.~~
    - ~~**Foreground policy:** solo el VT foreground recibe el teclado fisico; los demas conservan su cola y su framebuffer en pausa para poder volver sin perder estado.~~
  - ~~**Criterio:**
    - Alt+F1: pantalla cambia a VT0, teclado entrega a shell en VT0.
    - Alt+F2: pantalla cambia a VT1 (vacia), input sin proc -> silent (no error).
    - Type en VT1, Alt+F1, type en VT0: ambos buffers almacenan independiente.
    - El cambio de VT no altera el proceso foreground salvo que el propio scheduler o shell lo decida.~~
  - ~~**Tests:** `input_vt_switch_framebuffer`, `input_vt_independent_queues`, `input_vt_rapid_switching`, `input_4vt_concurrent_stress`, `input_event_bus_dispatch_vt` (5 tests).~~

---

### FASE A5 — Storage Unification (NT: IoStack)

- [ ] **A5.2. VirtIO block driver (BOOT_DRIVER)**
  Prereqs: A2.1
  * **Archivos:** `src/drivers/virtio_blk.rs` (new, 400-500 lines), integracion `src/drivers/storage.rs`, `src/main.rs` PHASE 3.6 (priority init)
  * **Descripcion:** Controlador de bloques VirtIO para maquinas virtuales QEMU/KVM. Se clasifica como **BOOT_DRIVER**, no como `.NEM`, ya que participa directamente en la cadena de arranque del sistema y debe estar disponible antes del montaje del volumen raiz.
    * **PCI detection:** Bus 0, vendor 0x1AF4 (Red Hat), device 0x1001 (VirtIO Block).
    * **Initialization:**
      1. Read BAR0 (MMIO base)
      2. Write device status: ACKNOWLEDGE | DRIVER
      3. Allocate virtqueue (#0, 32 descriptors)
      4. Register queue physical address
      5. Negotiate legacy/modern features
      6. Write device status: DRIVER_OK
    * **I/O path:** `submit_irp(irp)` ->
      1. Allocate descriptor slot
      2. Fill request header
      3. Configure sector_start, sector_count, buffer address
      4. Notify device (doorbell)
      5. Wait completion (polling or interrupt)
      6. Process used ring
      7. Complete IRP
    * **Supported operations:** READ, WRITE, FLUSH, DISCARD
    * **Storage priority:**
      ```
      NVMe > VirtIO > BootAhci > BootAta
      ```
    * **Boot integration:**
      Available before VFS mount, before NeoInit, before NeoShell, before NEM loader. Used by GPT parser and NeoDOS filesystem loader.
  * **Criterio:**
    * Arrancar NeoDOS en QEMU usando `-drive if=virtio`
    * Deteccion automatica PCI.
    * Inicializacion correcta del dispositivo.
    * GPT parsing via VirtIO.
    * Carga del superblock NeoDOS.
    * Montaje de volumen raiz.
    * Arranque completo de NeoInit y NeoShell.
  * **Tests:**
    `virtio_pci_detect`, `virtio_virtqueue_init`, `virtio_submit_read_write`, `virtio_boot_load_kernel`, `virtio_gpt_parsing`, `virtio_mount_rootfs`, `virtio_boot_neoshell` (7 tests)

- [ ] **A5.3. AHCI NCQ** | NT: Storport Native Command Queuing | Prereqs: A2.2
  - **Archivos:** `src/drivers/boot_ahci.rs` (extend), `src/drivers/ahci/mod.rs` (NEM driver), `src/irp/mod.rs` (tag-based dispatch)
  - **Descripcion:** Native Command Queuing en AHCI permite hasta 32 operaciones simultaneas con finalizacion out-of-order.
    - **NCQ path:**
      1. Host prepara 32 command tables en memoria (FIS buffer per slot).
      2. Escribe descriptores a device: ATA FPDMA QUEUED READ (0x60) / WRITE (0x61)
      3. Device acepta hasta 32 cmds sin esperar completaciones.
      4. Device finaliza out-of-order: escribe SActive register (bit = completado), trigger IRQ.
      5. Host lee Successful NCQ Completion Notification (FIS D2H), extrae tag, localiza IRP via tag.
    - **Tag-based dispatch:** Per-device, map `[Option<IrpId>; 32]` indizado por tag.
    - **Fall back to legacy:** Si device no soporta NCQ (via IDENTIFY), usar single-command path.
  - **Criterio:**
    - 32 read IRPs encolados simultaneamente. Device AHCI completa out-of-order.
    - Time to complete 32 reads: ~0.1 ms (paralelo) vs 3.2 ms (serial). ~30x faster.
    - Stress: NCQ bajo carga, sin comando perdido, IRP_DONE count = 32.
  - **Tests:** `ahci_ncq_32_concurrent_dispatch`, `ahci_ncq_tag_based_completion`, `ahci_ncq_fallback_to_legacy`, `ahci_ncq_out_of_order_completion`, `ahci_ncq_stress_load` (5 tests).

---

### Fase 3: Estabilizacion (v0.51 - v1.0.0)
*Bugfixes, hardening, documentacion, y preparacion para API estable.*

Orden de implementacion dentro de la fase:

1. **v0.51** — sys_fork/clone (bajo demanda), sys_signal minimo, full Input subsystem (A4.4)
2. **v0.52** — Stack de red completo (UDP, DNS, DHCP), TFTP/NFS basico, Virtual Terminals (B4.5)
3. **v0.53** — Rendimiento: per-CPU heaps NUMA-aware, scheduler lock-free, zero-copy pipes (B6.1), COW fork (B6.2)
4. **v0.54-v0.59** — Documentacion API completa, test coverage >95%, fuzzing, module signatures (B5.1), secure boot (B5.3)
5. **v1.0.0** — Primera API estable. Todo lo anterior debe estar COMPLETED.

---

### FASE B — Features (userland + servicios)

Prereqs globales: A4.7 minimo para items userland; NT5/NT6 para items de seguridad.

#### B1. Tracing & Observability

- [ ] **B1.1 Y1. Kernel tracing infrastructure** | Prereqs: A2.4 | Files: `src/trace/mod.rs`
  - **Descripcion:** Ampliar el `TraceBuffer` existente (1024 entries, lock-free ring buffer en `trace.rs`) con trace points registrables dinamicamente. Actualmente el buffer soporta 7 tipos de evento (`ContextSwitch`, `SyscallEnter/Exit`, `IrqEnter/Exit`, `SchedDecision`, `Panic`) con 4 argumentos u64 por entry. Esta mejora anade: registro dinamico de trace points por subsistema (scheduler, VFS, memory, drivers), filtrado por categoria/nivel, y dump formateado via serial con timestamps HPET.
  - **Criterio:** Trace points registrables desde cualquier modulo kernel. Dump via serial legible. Filtrado por categoria funcional.
  - **Tests:** `trace_register_dynamic_point`, `trace_filter_by_category`, `trace_dump_serial_format`.

- [ ] **B1.2 Y2. NeoTrace system** | Prereqs: B1.1 | Files: `userbin/neotrace/`
  - **Descripcion:** Comando de shell Ring 3 `NEOTRACE` que expone la infraestructura de tracing (B1.1) al usuario. Subcomandos: `START` (activa captura global), `STOP` (pausa captura), `DUMP [N]` (vuelca las ultimas N entradas del TraceBuffer a consola), `FILTER <category>` (filtra por categoria). Usa `TRACE.dump()` internamente.
  - **Criterio:** `NEOTRACE START` + ejecutar proceso + `NEOTRACE DUMP 32` muestra ultimas 32 entradas con timestamps.
  - **Tests:** `neotrace_start_stop_toggle`, `neotrace_dump_output`.

#### B2. NeoReg & Configuration Infrastructure

* [ ] **B2.1 Z6. Registry hive database | NT: Cm (Configuration Manager), cell-based hive** | Prereqs: NT5 (Ob), NT6 (SID/ACL), A5.1 (IoStack) | Files: `src/cm/`, `src/cm/hive.rs`, `src/cm/cell.rs`, `src/cm/key.rs`, `src/cm/cache.rs`
  * **Descripcion:**
    Implementar NeoReg, sistema de configuracion jerarquico persistente como el Cm de Windows NT. El diseno sigue el modelo NT de celulas (cells) y bins, con integracion directa en el Object Manager NT5.
    **Cell-based hive format** (en vez de arbol simple):
    ```
    Hive
    +- Base Block (4 KB) — magic "neoR", seq numbers, checksum
    +- Bins (4 KB cada uno)
    |  +- Cell — Key: name, parent_cell, subkeys_list, values_list, class, sec_desc, last_write
    |  +- Cell — Value: name, type (REG_SZ/DWORD/BINARY), data
    |  +- Cell — Security descriptor (SID + ACL, reutilizado entre keys)
    +- Free cells (linked list for reuse)
    ```
    Cada celda tiene un indice dentro del bin, y los bins se numeran secuencialmente.
    **ObNamespace integration** — cada key registry es un objeto en NT5:
    ```
    \Registry
      \Machine           -> KObj::Directory
        \System           -> KObj::Key (backed by SYSTEM.HIV)
        \Drivers          -> KObj::Key (backed by DRIVERS.HIV)
      \User
        \Default          -> KObj::Key (backed by DEFAULT.HIV)
    ```
    `sys_open("\\Registry\\Machine\\System\\BootShell")` funciona via NT5 path resolution.
    **Syscall API** — expuesta como syscalls NT-style:
    ```
    RAX 50  sys_open_key(path)        -> handle (NtOpenKey)
    RAX 51  sys_create_key(path)      -> handle (NtCreateKey)
    RAX 52  sys_query_value(key, name, buf, len) -> value (NtQueryValueKey)
    RAX 53  sys_set_value(key, name, type, data, len) (NtSetValueKey)
    RAX 54  sys_enum_key(key, index, buf) -> subkey name (NtEnumerateKey)
    RAX 55  sys_enum_value(key, index, buf) -> value name (NtEnumerateValueKey)
    RAX 56  sys_delete_key(key)               (NtDeleteKey)
    RAX 57  sys_flush_key(key)                (NtFlushKey)
    RAX 58  sys_load_hive(path, mount_point)  (NtLoadKey, admin)
    RAX 59  sys_unload_hive(mount_point)      (NtUnloadKey, admin)
    ```
  * **Criterio:**
    - Keys y values expuestos como objetos en NT5 namespace
    - `sys_open("\\Registry\\Machine\\System")` devuelve handle a la key raiz de SYSTEM.HIV
    - `sys_set_value(key, "PATH", REG_SZ, "C:\\Programs")` persiste y es recuperable tras reboot
    - Cell cache: 2da lectura de misma key no toca disco (cache hit)
    - Hive persistente: tras reboot, valores anteriores siguen presentes
  * **Tests:**
    `cm_create_key_ob`, `cm_query_value_cache_hit`, `cm_set_value_persist`,
    `cm_enum_keys_multi`, `cm_hive_reload_integrity`, `cm_cell_corruption_isolated`,
    `cm_syscall_open_key`, `cm_syscall_set_get_value` (8 tests)

* [ ] **B2.2-2.5. Registry enhancements** | Prereqs: B2.1
  * **B2.2 Z6. Registry transaction journal | NT: Hive LOG (.LOG1/.LOG2)** | Files: `src/cm/journal.rs`
    Write-Ahead Log (WAL) para cada hive. Sigue el modelo NT de `.LOG` / `.LOG1` / `.LOG2`.
    Recovery al boot: replay del log si seq numbers no coinciden.
  * **B2.3 Z6. Multi-Hive Architecture | NT: SYSTEM/SOFTWARE/SECURITY/DEFAULT hives** | Files: `src/cm/hive.rs`, `src/cm/manager.rs`
    Multiples hives bajo `\Registry` con independencia de carga, persistencia y recovery.
  * **B2.4 Z6. Registry Security | NT: SECURITY.HIVE, Key ACLs (NT6)** | Files: `src/cm/security.rs`
    Control de acceso sobre keys registry usando NT6 Security Reference Monitor (SID + ACL + SeAccessCheck).
  * **B2.5 Z6. Registry notification + load/unload | NT: RegNotifyChangeKeyValue, NtLoadKey, NtUnloadKey** | Files: `src/cm/notify.rs`
    Key change notifications via Event Bus. Hive load/unload for user profiles.

#### B3. Networking

- [ ] **B3.1 D9. Network I/O | NT: Winsock (ws2_32.dll) -> NtCreateFile(\Device\Tcp)** | Prereqs: A4.1, A4.2 | Files: `src/net/`, `src/syscall.rs`
  - **Descripcion:** Modelo NT: el kernel expone `\Device\Tcp` y `\Device\Udp` como objetos de dispositivo en el namespace NT5. La API de red user-mode va en `libneodos/src/net.rs` como wrapper que abre `\Device\Tcp` via `sys_open` y opera via `sys_ioctl` (NtDeviceIoControlFile). No hay syscalls socket-style — se usa el modelo NT de File + IoControl.
  - **Criterio:** User-mode puede hacer `net_open(b"\\Device\\Tcp")`, `net_connect(fd, ip, port)`, `net_send(fd, buf)`. `PING` funciona via ICMP.
  - **Tests:** `net_open_device_tcp`, `net_tcp_connect_send_recv`, `net_icmp_ping`.

- [ ] **B3.2 E3. TCP/IP stack | NT: AFD (Ancillary Function Driver)** | Prereqs: B3.1 | Files: `src/net/`
  - **Descripcion:** Stack de red completo en kernel como driver de dispositivo `\Device\Tcp` y `\Device\Udp`. Capas: Ethernet, ARP (tabla 64 entries, timeout 300s), IPv4 (header parse/build, checksum, TTL), ICMP (echo request/reply), UDP, TCP (3-way handshake, sequence numbers, sliding window 16 KB, retransmit timer, FIN/RST). NIC driver via VirtIO-net o e1000.
  - **Criterio:** `PING 10.0.2.2` recibe reply. TCP connection a host funciona.
  - **Tests:** `tcp_handshake_3way`, `udp_send_recv`, `arp_table_lookup`, `icmp_echo_reply`.

- [ ] **B3.3 D8. DHCP client | NT: DHCP Client Service** | Prereqs: B3.2 | Files: `src/net/dhcp.rs`
  - **Descripcion:** Cliente DHCP (RFC 2131) que obtiene configuracion de red automaticamente al boot.
  - **Criterio:** Al boot con NIC presente, kernel obtiene IP automaticamente sin configuracion manual.
  - **Tests:** `dhcp_discover_offer_sequence`, `dhcp_lease_renewal`.

- [ ] **B3.4 D7. NTP client | NT: W32Time (Windows Time Service)** | Prereqs: B3.2 | Files: `src/net/ntp.rs`
  - **Descripcion:** Cliente NTP (RFC 5905, modo SNTP simplificado) que sincroniza el RTC del sistema con un servidor NTP externo.
  - **Criterio:** Tras boot con red, RTC sincronizado con servidor NTP (offset < 1s).
  - **Tests:** `ntp_request_parse_response`, `ntp_offset_calculation`.

#### B4. Userland Usable System

- [ ] **B4.3 S3. Shell redirection (`>`, `<`, `>>`)** | Prereqs: A4.7 | Files: `userbin/neoshell/`
  - **Descripcion:** Redireccion de I/O en neoshell. Parser detecta tokens `>` (write), `>>` (append), `<` (read). Para `cmd > file`: neoshell abre/crea `file` via syscall Ob, luego spawna `cmd` con `sys_dup2` redirigiendo fd 1 (stdout) al handle del archivo. Para `cmd < file`: abre archivo y redirige fd 0 (stdin). Para `>>`: abre con flag append.
  - **Criterio:** `DIR > output.txt` crea archivo con listado. `TYPE < input.txt` lee de archivo.
  - **Tests:** `redirect_stdout_to_file`, `redirect_stdin_from_file`, `redirect_append`.

- ~~**[COMPLETED] B4.5 B1. Virtual terminals** | Prereqs: A4.4, B4.4 | Files: `userbin/neoshell/`, `src/input/`~~
  - ~~**Descripcion:** Multiplexar el framebuffer y el input en hasta 4 terminales virtuales (VTs). Depende de A4.4 (input subsystem redisenado con `InputManager` y `vt_queues[4]`). Cada VT tiene su propio buffer de framebuffer, cola de input independiente, y PID foreground.~~
  - ~~**Criterio:** Alt+F1 y Alt+F2 muestran shells independientes. Input en un VT no afecta al otro.~~
  - ~~**Tests:** `vt_switch_alt_f1_f2`, `vt_independent_input`, `vt_framebuffer_swap`.~~

- [ ] **B4.6 B6. NeoEdit text editor** | Prereqs: A4.7, B4.4 | Files: `userbin/neoedit/`
  - **Descripcion:** Editor de texto modal Ring 3 (`.NXE`). Usa `ob_open` + `ob_query_info(ReadContent)` para cargar archivos y `ob_set_info(WriteContent)` para guardar. Renderiza via `sys_write` con secuencias ANSI.
  - **Criterio:** `NEOEDIT C:\System\Config\system.cfg` abre, edita, guarda correctamente.
  - **Tests:** `neoedit_open_display`, `neoedit_edit_save`, `neoedit_scroll`.

- [ ] **B4.7 B6b-v2. Shared library per-process binding | NT: Ldr (Loader, PEB->LdrData)** | Prereqs: sys_loadlib | Files: `src/elf.rs`, `libneodos/`
  - **Descripcion:** Evolucionar el sistema NXL actual (slots globales fijos en 0x1E000000-0x1E200000 compartidos entre procesos) a binding per-process. Cada EPROCESS mantiene su propia tabla de NXLs cargadas.
  - **Criterio:** Dos procesos cargan versiones distintas de `libmath.nxl` sin interferencia.
  - **Tests:** `nxl_per_process_isolation`, `nxl_unload_on_exit`, `nxl_version_coexistence`.

- [ ] **B4.8 B7. NeoTOP** | Prereqs: A4.7, A1.5 | Files: `userbin/neotop/`
  - **Descripcion:** Monitor de sistema Ring 3 en tiempo real (`.NXE`). Muestra lista de procesos, uso de CPU por core, estadisticas de memoria, drivers cargados. Refresco cada 1 segundo via `sys_sleep`. Renderiza con ANSI escape codes.
  - **Criterio:** `NEOTOP` muestra procesos activos actualizandose en tiempo real.
  - **Tests:** `neotop_display_processes`, `neotop_refresh_loop`, `neotop_exit_clean`.

- [ ] **B4.9 B11. NeoShell scripting (`.BAT`)** | Prereqs: B4.1, B4.2, B4.3 | Files: `userbin/neoshell/`
  - **Descripcion:** Interprete de scripts batch en neoshell. Soporta archivos `.BAT`/`.CMD` con: `ECHO`, `SET`, `IF %VAR%==valor cmd`, `GOTO :label`, `CALL script.bat`, `FOR`, `REM`, `@`.
  - **Criterio:** Script `.BAT` con IF/GOTO/CALL ejecuta correctamente.
  - **Tests:** `bat_echo_set`, `bat_if_goto`, `bat_call_subroutine`, `bat_for_loop`.

- [ ] **B4.10 B12. Compositor 2D** | Prereqs: B4.4, framebuffer | Files: `userbin/compositor/`
  - **Descripcion:** Compositor de ventanas 2D sobre el framebuffer GOP 1280x800. Modelo: cada ventana tiene un back-buffer, posicion, z-order, titulo. El compositor blittea ventanas en orden z sobre el framebuffer principal. Renderiza a 30 FPS maximo.
  - **Criterio:** Dos ventanas superpuestas, una encima de otra. Mover ventana actualiza framebuffer.
  - **Tests:** `compositor_create_window`, `compositor_z_order`, `compositor_blit_overlap`.

#### B5. Security

- [ ] **B5.1 U1. Module signature validation** | Prereqs: NT6 | Files: `src/drivers/loader.rs`
  - **Descripcion:** Validacion criptografica de modulos `.nem` antes de que entren al runtime del driver loader.
  - **Criterio:** Un `.nem` alterado o sin firma no puede pasar de `Loaded` a `Initialized`.
  - **Tests:** `nem_signature_valid_accepts`, `nem_signature_invalid_rejects`, `nem_signature_tamper_detected`.

- [ ] **B5.2 U3. Driver permission enforcement** | Prereqs: NT6.3, B5.1 | Files: `src/drivers/caps.rs`
  - **Descripcion:** Cruza la capacidad declarada por el driver con el token del proceso que intenta cargarlo y con la ACL del objeto driver en el namespace.
  - **Criterio:** Un driver sin `CAP_ADMIN` no puede abrir objetos protegidos aunque este firmado.
  - **Tests:** `driver_caps_allow_admin`, `driver_caps_deny_user`, `driver_caps_acl_intersection`.

- [ ] **B5.3 U4. Secure boot chain** | Prereqs: B5.1 | Files: `neodos-bootloader/`, `src/boot/secure.rs`
  - **Descripcion:** Encadena la verificacion desde bootloader hasta kernel y drivers para que ningun binario de arranque se ejecute sin validacion previa.
  - **Criterio:** Si falla la verificacion del kernel o de un driver critico, el boot se detiene.
  - **Tests:** `secure_boot_kernel_verified`, `secure_boot_driver_verified`, `secure_boot_fail_closed`.

#### B6. Performance

- [ ] **B6.1 V2. Zero-copy pipes** | Prereqs: A4.5, S2 | Files: `src/pipe.rs`
  - **Descripcion:** Optimiza el camino de pipes para que, cuando el buffer del productor o consumidor este alineado y sea seguro, los datos se pasen por referencia a paginas compartidas o pinneadas en lugar de copiarse byte a byte dentro del kernel.
  - **Criterio:** Un pipeline con buffers alineados evita al menos una copia completa entre procesos.
  - **Tests:** `pipe_zero_copy_aligned_buffers`, `pipe_zero_copy_fallback_copy`, `pipe_zero_copy_integrity`.

- [ ] **B6.2 V3. Copy-on-write fork** | Prereqs: A1.5 | Files: `src/memory/cow.rs`, `src/syscall.rs`
  - **Descripcion:** Implementa `sys_fork` como clonacion perezosa del espacio de direcciones: el hijo comparte paginas con el padre en modo read-only hasta que cualquiera escribe.
  - **Criterio:** Padre e hijo comparten memoria al nacer y divergen solo al escribir.
  - **Tests:** `cow_fork_shares_pages`, `cow_write_triggers_copy`, `cow_fork_isolated_writes`.

#### B9. Shell command migration Ring 0 -> Ring 3 [COMPLETED]

Migracion completa de todos los comandos del kernel shell Ring 0 a `.NXE` en Ring 3.

El Ring 0 solo mantiene:
- **RUN** — bootstrap loader necesario para lanzar el primer binario Ring 3 (NeoInit/neoshell) desde el kernel.
- **CRASH** — crash dump management; es inherentemente kernel-level.

**Completados:**
- HELP -> corehelp.nxe (B9.1)
- SET -> neoshell built-in (B9.2)
- EXIT -> neoshell built-in (B9.3)
- PS -> ps.nxe (B9.4) — migrado a Ob
- KILL -> kill.nxe (B9.5) — migrado a Ob
- PRI -> pri.nxe (B9.6) — migrado a Ob
- DRIVES -> drives.nxe (B9.8) — migrado a Ob
- KEYB -> keyb.nxe (B9.10) — migrado a Ob
- CALL -> neoshell built-in (B9.13)
- LABEL -> label.nxe — migrado a Ob
- FSCK -> fsck.nxe
- NDREG -> ndreg.nxe — migrado a Ob
- LOADNEM -> loadnem.nxe — partial Ob (create(Driver) done, unload via legacy RAX 58)
- KOBJ -> kobj.nxe — migrado a Ob

Los comandos de gestion de archivos (DEL, REN, MD, RD, COPY, TYPE, DIR, TREE, CD, CLS, ECHO, DATE, TIME, VOL, MEM, VER, CPUINFO, DATETIME, VER) tambien estan migrados a Ring 3 como `.NXE`.

---

### X7. NeoDOS Object Manager (Ob) — Unificacion de Handles, KOBJ, URN y Seguridad [COMPLETED v0.44.2]

> **NT Reference:** Ob (Object Manager) — `ObOpen`, `ObCreate`, `ObQueryInfo`, `ObReferenceObject`
> **Documento de diseno:** [`docs/OBJECT_MANAGER_ARCHITECTURE.md`](OBJECT_MANAGER_ARCHITECTURE.md)
> **Version objetivo:** v0.41-v0.44.2
> **Estado:** [DONE] COMPLETADO (v0.44.2)

#### Arquitectura Implementada

El Object Manager (Ob) unifica handles, objetos, seguridad y namespace en una sola abstraccion:

```
ObObject (kernel object)
+-- id: ObId (64-bit)
+-- type: ObType (16 tipos: Process, Driver, Device, Pipe, ..., Timer)
+-- name: [u8; OB_NAME_LEN=128]
+-- refcount: u32
+-- flags: u32
+-- native_id: u64 (back-pointer al recurso real)
+-- ops: Option<&'static dyn ObOperations> (vtable polimorfica)

ObHandle (per-process)
+-- object_id: ObId -> referencia a ObObject
+-- access_mask: u32 (READ|WRITE|EXEC|DELETE)
+-- offset: u64

ObDirectory (namespace)
+-- \Global\, \Device\, \Driver\, \FileSystem\, \Registry\, \Ob\
+-- \Global\Info\ (virtual: Version, DateTime, Memory, CpuInfo, Cwd, Keyboard, Drives, Drivers)
+-- \Ob\Process\ (virtual: PID-indexed)
```

#### Syscalls Ob (RAX 60-66)

| RAX | Syscall | Args | Descripcion |
|-----|---------|------|-------------|
| 60 | `sys_ob_open` | RBX=path, RCX=access | Open named object -> handle (SeAccessCheck integrado) |
| 61 | `sys_ob_create` | RBX=path, RCX=type, RDX=fds_out, R8=attrs | Create named object (Process=1, Driver=2, Pipe=4, Directory=11, Event=13) |
| 62 | `sys_ob_query_info` | RBX=fd, RCX=class, RDX=buf, R8=len | Query object metadata (0-16 classes, incl. ReadContent=15, VolumeLabel=16) |
| 63 | `sys_ob_set_info` | RBX=fd, RCX=class, RDX=buf | Set object metadata (0-9 classes, incl. WriteContent=7, SetCwd=8, SetVolumeLabel=9, VfsRename=6) |
| 64 | `sys_ob_enum` | RBX=dir_fd, RCX=buf, RDX=max | Enumerate directory (VFS-backed + Ob namespace) |
| 65 | `sys_ob_wait` | RBX=count, RCX=handles, RDX=type, R8=to | Wait on objects (multi-type via KWait) |
| 66 | `sys_ob_destroy` | RBX=fd | Destroy/delete object by fd (files, dirs, drivers, namespace objects) |

#### Syscalls Legacy Migrados a Ob

| RAX | Legacy | Estado SSDT | Equivalente Ob |
|-----|--------|-------------|----------------|
| 11 | `sys_readfile` | None | ob_query_info(ReadContent) |
| 12 | `sys_writefile` | None | ob_set_info(WriteContent) |
| 24 | `sys_getcpuinfo` | None | ob_open("\Global\Info\CpuInfo") + ob_query_info |
| 25 | `sys_mkdir` | None | ob_create(Directory) |
| 26 | `sys_unlink` | None | ob_destroy |
| 27 | `sys_rmdir` | None | ob_destroy |
| 28 | `sys_rename` | None | ob_set_info(VfsRename) |
| 33 | `sys_get_drives` | None | ob_open("\Global\Info\Drives") + ob_query_info |
| 43 | `sys_get_version` | None | ob_open("\Global\Info\Version") + ob_query_info |
| 44 | `sys_get_datetime` | None | ob_open("\Global\Info\DateTime") + ob_query_info |
| 45 | `sys_get_meminfo` | None | ob_open("\Global\Info\Memory") + ob_query_info |
| 46 | `sys_get_volume_label` | None | ob_query_info(VolumeLabel) |
| 48 | `sys_kobj_enum` | None | ob_enum |
| 49 | `sys_set_keyboard_layout` | None | ob_open("\Global\Info\Keyboard") + ob_set_info |
| 51 | `sys_set_priority` | None | ob_set_info(ProcessPriority) |
| 52 | `sys_kill_process` | None | ob_set_info(ProcessTerminate) |
| 54 | `sys_set_volume_label` | None | ob_set_info(SetVolumeLabel) |
| 56 | `sys_driver_enum` | None | ob_open("\Global\Info\Drivers") + ob_query_info |
| 57 | `sys_driver_load` | None | ob_create(Driver) |

#### Syscalls Legacy que Permanecen

| RAX | Syscall | Motivo |
|-----|---------|--------|
| 0 | `sys_exit` | Demasiado especifica para abstraer |
| 1 | `sys_write` | Foundation: stdout/stderr/pipe write |
| 2 | `sys_yield` | Foundation: ceder CPU |
| 3 | `sys_getpid` | Foundation: PID actual |
| 4 | `sys_read` | Foundation: stdin/pipe read |
| 5 | `sys_pipe` | Foundation: pipe creation (paralelo a ob_create(Pipe)) |
| 6 | `sys_dup2` | Foundation: redireccion |
| 7 | `sys_spawn` | Foundation: crear proceso (needed by neoinit) |
| 8 | `sys_readdir` | Foundation: directory read (paralelo a ob_enum) |
| 9 | `sys_waitpid` | Foundation: wait child |
| 10 | `sys_open` | Foundation: open file (paralelo a ob_open) |
| 13 | `sys_close` | Foundation: close handle |
| 16 | `sys_chdir` | Foundation: change dir |
| 18 | `sys_brk` | Demasiado especifica |
| 19 | `sys_mmap` | Demasiado especifica |
| 20 | `sys_munmap` | Demasiado especifica |
| 21 | `sys_loadlib` | Demasiado especifica |
| 22 | `sys_thread_create` | Foundation: thread creation |
| 23 | `sys_thread_join` | Foundation: thread join |
| 29 | `sys_set_exception_handler` | Foundation: SEH |
| 40 | `sys_wait_alertable` | Foundation: alertable wait |
| 41 | `sys_sleep_ex` | Foundation: sleep |
| 42 | `sys_poweroff` | Foundation: poweroff |
| 47 | `sys_chdir_parent` | Foundation: parent cwd change |
| 50 | `sys_ndreg` | Internal: driver registry admin |
| 53 | `sys_cursor_blink` | Foundation: cursor control |
| 55 | `sys_fsck` | Foundation: filesystem check |
| 58 | `sys_driver_unload` | Foundation: driver unload |
| 59 | `sys_poll` | Foundation: I/O polling |

#### Metricas Objetivo Alcanzadas

| Metrica | Antes (v0.40) | Despues (v0.44.2) |
|---------|---------------|-------------------|
| HandleEntry tipo-seguro | [PENDING] (kind hardcoded) | [DONE] (ObId ref) |
| KOBJ + handles unificados | [PENDING] | [DONE] |
| Security en open | [PENDING] (solo syscall 50) | [DONE] (todo acceso via SeAccessCheck) |
| URN funcional | Parcial (file + device) | Full (all schemes via Ob) |
| Tipos de objeto | ~8 implicitos | 16 explicitos (ObType enum) |
| Syscalls Ob | 0 | 7 nuevas (RAX 60-66) |
| OB_NAME_LEN | 32 | 128 |

#### Estado por Binario

| Binario | Estado Ob | Syscalls Ob | Syscalls Legacy Restantes |
|---------|-----------|-------------|--------------------------|
| ps | [DONE] COMPLETO | ob_open, ob_enum, ob_query_info | -- |
| kill | [DONE] COMPLETO | ob_open, ob_set_info | -- |
| pri | [DONE] COMPLETO | ob_open, ob_set_info | -- |
| kobj | [DONE] COMPLETO | ob_open, ob_enum | -- |
| neoshell | [DONE] COMPLETO | ob_open, ob_enum, ob_create(Pipe), ob_create(Process), ob_wait, ob_set_info(SetCwd), ob_query_info(ReadContent) | sys_cursor_blink, sys_poweroff |
| cd | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| coredir | [DONE] COMPLETO | ob_open, ob_enum | -- |
| corehelp | [DONE] COMPLETO | ob_open, ob_enum, ob_create(Pipe), ob_query_info(ReadContent), ob_create(Process), ob_wait | -- |
| coretype | [DONE] COMPLETO | ob_open, ob_query_info(ReadContent) | -- |
| tree | [DONE] COMPLETO | ob_open, ob_enum | -- |
| corecopy | [DONE] COMPLETO | ob_open, ob_destroy, ob_query_info(ReadContent), ob_set_info(WriteContent) | -- |
| cmdtest | [DONE] COMPLETO | ob_open, ob_create(Directory), ob_destroy, ob_set_info, ob_query_info(ReadContent) | -- |
| cpuinfo | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| neoinit | [DONE] N/A (PID 1) | ob_create(Process), ob_wait | sys_spawn (needed for bootstrap) |
| datetime | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| ver | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| mem | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| vol | [DONE] COMPLETO | ob_open, ob_query_info(VolumeLabel) | -- |
| coredel | [DONE] COMPLETO | ob_open, ob_destroy | -- |
| coreren | [DONE] COMPLETO | ob_open, ob_set_info(VfsRename) | -- |
| coremd | [DONE] COMPLETO | ob_create(Directory) | -- |
| corerd | [DONE] COMPLETO | ob_open, ob_destroy | -- |
| drives | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| keyb | [DONE] COMPLETO | ob_open, ob_set_info(KeyboardLayout) | -- |
| label | [DONE] COMPLETO | ob_open, ob_query_info(VolumeLabel), ob_set_info(SetVolumeLabel) | -- |
| fsck | [DONE] N/A | -- | sys_fsck (comando de reparacion con argumentos) |
| ndreg | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| loadnem | [DONE] PARCIAL | ob_create(Driver) | sys_driver_unload (RAX 58) para /U |
| echo | [DONE] N/A | -- | (foundation only, solo sys_write) |
| cls | [DONE] N/A | -- | (foundation only, solo sys_write) |

---

### B7. Experimental

- [ ] **B7.1 E4. Full GUI system** | NT: Desktop Window Manager | Prereqs: B4.10 | Files: `userbin/gui/` | Desktop con iconos, menu, ventanas redimensionables.
- [ ] **B7.2 E5. Advanced secure boot (TPM)** | NT: BitLocker / TPM | Prereqs: B5.3 | Files: `src/boot/tpm.rs` | Medicion PCR + sealed storage.
- [ ] **B7.3 E6. Package manager** | NT: MSI / Windows Update | Prereqs: B5.1, A5.1 | Files: `userbin/neopkg/` | Install/remove paquetes `.NPK` firmados.
- [ ] **B7.4 T4. Time-travel debugging** | NT: WinDbg time travel | Prereqs: A3.2, B1.1 | Files: `src/debugger/timetravel.rs` | Replay de trace buffer en debugger.
- [ ] **B7.5 T5. Live kernel patching** | NT: Windows Hotpatch | Prereqs: A2.4, A3.2 | Files: `src/patch/mod.rs` | Hot-patch de funcion kernel sin reboot.
- [ ] **B7.6 T2. Distributed NeoDOS nodes** | NT: DFS | Prereqs: B3.2 | Files: `src/cluster/` | 2 nodos QEMU se descubren y comparten FS read-only.

---

## Matriz de Problemas Arquitectonicos

| # | Problema | Fase | NT ref | Estado | Riesgo si no se hace |
|---|----------|------|--------|--------|---------------------|
| 1 | Frame allocator O(n), 4 GB max | A0.1-A0.2 | Mm | COMPLETED | -- |
| 2 | Direcciones fijas, solapamiento | A0.3 | Mm | COMPLETED | -- |
| 3 | Handle table fijo (16) | A0.4 | Ob handles | COMPLETED | -- |
| 4 | Thread model ausente (1 hilo/proceso) | A1.5 | KTHREAD | COMPLETED | sys_thread_create/join, per-process KTHREADs |
| 5 | Scheduler monoprocesador | A1.1-A1.2 | Ps | COMPLETED | Per-CPU run queues, work stealing SMP |
| 6 | Slab allocator lock global | A1.3 | Lookaside | COMPLETED | Throughput no escala con CPUs |
| 7 | Sin IPI / TLB shootdown | A1.4 | KeIpi | COMPLETED | Data corruption en SMP |
| 8 | IRQL ausente (solo cli/sti) | A2.4 | IRQL | COMPLETED | Per-CPU IRQL levels, IrqMutex, INV-14 |
| 9 | DPC ausente (work queue parche) | A2.5 | DPC | COMPLETED | Per-CPU DPC queues, SPSC ring buffer |
| 10 | PCI port I/O asume x86 | A2.1 | HAL | COMPLETED | ECAM MMIO (MCFG) + PIO fallback |
| 11 | PIC legacy como default | A2.2 | IOAPIC | COMPLETED | IOAPIC init (MADT), PIC disable, MSI-X |
| 12 | HAL mezcla raw y safe | A2.3 | HAL | COMPLETED | asm confinado a hal/ |
| 13 | Sin crash dump ni recovery | A3.1-A3.3 | Bugcheck | COMPLETED | CrashDumpHeader, stack walk |
| 14 | SEH ausente | A3.4 | SEH | COMPLETED | TEB exception handler chain |
| 15 | Stack unwinding inexistente | A3.2 | KD | [PENDING] | Sin backtrace |
| 16 | Shell en Ring 0 | A4.7 | CSRSS | COMPLETED | neoshell.nxe en Ring 3, solo RUN/CRASH |
| 17 | NeoInit no implementado | Z1 | smss.exe | COMPLETED | Doc/codigo divergen |
| 18 | Syscall dispatch manual | A4.2 | SSDT | COMPLETED | SSDT table-based, permission check |
| 19 | ELF loader sin validacion | A4.3 | Ldr | COMPLETED | Triple fault con binarios maliciosos |
| 20 | APC ausente | A4.5 | APC | COMPLETED v0.34.0 | I/O completion en contexto incorrecto |
| 21 | Input sin multiplexion | A4.4 | ConDrv | COMPLETED | -- |
| 22 | FAT32 + NeoFS duplicados | A5.1 | IoStack | COMPLETED | Ambos usan IoStack para I/O |
| 23 | Ob flat (no namespace) | NT5 | Ob | COMPLETED | Hardcode C:, sin symlinks |
| 24 | SRM ausente | NT6 | Se | COMPLETED | SID, Token, ACL, SeAccessCheck |
| 25 | Registry flat -> cell-based hive | B2.1-B2.5 | Cm | [PENDING] | Sin config jerarquica transaccional |
| 26 | Handles/KOBJ/URN/security no unificados | X7 | Ob | COMPLETED v0.44.2 | Unified via ObObjectTable |

---

## Architectural Initiatives

Las siguientes iniciativas arquitectonicas son cambios transversales que afectan a multiples subsistemas y requieren coordinacion.

### AI-1: Clean up ObInfoClass/ObSetInfoClass enums

**Estado:** [PENDING]
**Archivos:** `src/object/types.rs`

El handler `handler_ob_query_info` soporta info classes 0-16, pero el enum `ObInfoClass` en `types.rs` solo define hasta 14 (KeyboardLayout). Similarmente, `ObSetInfoClass` solo define hasta 5 (KeyboardLayout), mientras que el handler soporta clases 4 (ProcessTerminate), 6 (VfsRename), 7 (WriteContent), 8 (SetCwd), 9 (SetVolumeLabel).

**Accion requerida:** Anadir las clases faltantes a los enums para que reflejen la implementacion real:
- `ObInfoClass::ReadContent = 15`, `ObInfoClass::VolumeLabel = 16`
- `ObSetInfoClass::ProcessTerminate = 4`, `ObSetInfoClass::VfsRename = 6`, `ObSetInfoClass::WriteContent = 7`, `ObSetInfoClass::SetCwd = 8`, `ObSetInfoClass::SetVolumeLabel = 9`

### AI-2: Consolidate legacy syscall wrappers

**Estado:** [PENDING]
**Archivos:** `src/syscall/mod.rs`

Tras la migracion a Ob, varias syscalls legacy son wrappers finos que podrian eliminarse:
- `handler_readfile` (RAX 11) y `handler_writefile` (RAX 12) ya estan en None
- `handler_mkdir`/`handler_unlink`/`handler_rmdir`/`handler_rename` (RAX 25-28) ya estan en None
- Sin embargo, `handler_open` (RAX 10), `handler_readdir` (RAX 8), `handler_pipe` (RAX 5) siguen activos y son wrappers de Ob

**Decision:** Mantener las syscalls legacy activas por compatibilidad con binarios antiguos. No eliminar hasta que todos los binarios conocidos usen exclusivamente Ob API (v1.0).

### AI-3: ObObjectTable lock granularity

**Estado:** [PENDING]
**Archivos:** `src/object/mod.rs`

El `ObObjectTable` usa un unico `spin::Mutex` global. Bajo carga de multiple proceso con operaciones Ob concurrentes (open, query, set, destroy), esto puede convertirse en cuello de botella.

**Propuesta:** Migrar a lock striping (16 locks, hash de ObId para elegir lock) o a una `RwLock` para operaciones de solo lectura vs escritura. Evaluar si es necesario tras medir contention real.

### AI-4: Standardize error codes between Ob and syscall layer

**Estado:** [PENDING]
**Archivos:** `src/object/types.rs`, `src/syscall/mod.rs`

Actualmente `ObError` tiene su propio conjunto de codigos (-1 a -9), y `SyscallError` tiene otro conjunto separado. La capa de syscall traduce entre ellos manualmente. Esto puede producir discrepancias (e.g., `ObError::NotFound` -> `SyscallError::NoEnt` y `ObError::InvalidParam` -> `SyscallError::Inval`).

**Propuesta:** Unificar en un solo conjunto de codigos de error reutilizado por ambas capas, o anadir un mapping formal verificado por tests.

---

## Objectification Roadmap — Syscall → Object Migration Plan

> **Documento de diseño:** [`docs/OBJECT_MANAGER_ARCHITECTURE.md`](OBJECT_MANAGER_ARCHITECTURE.md)
> **Visión arquitectónica:** [`docs/ARCHITECTURAL_VISION.md`](ARCHITECTURAL_VISION.md) §4.2
> **Versión actual:** v0.44.3 (7 Ob syscalls: RAX 60-66, 16 ObTypes definidos)
> **Objetivo:** v1.0 — toda syscall que gestione un recurso del sistema debe ser un Object accesible via Ob.

### Principios

1. **Todo recurso del sistema es un objeto** administrado por el Object Manager (Ob).
2. **Toda syscall nueva** (RAX ≥ 67) DEBE implementarse como `sys_ob_*`.
3. **Retrocompatibilidad**: Las syscalls legacy permanecen activas en SSDT mientras existan binarios que las usen.
4. **Migración gradual**: Cada fase es autónoma y testeable.
5. **Unificación de errores**: `ObError` y `SyscallError` se fusionan en `NeoDosError` antes de v1.0.

### Mapa Actual: 16 ObTypes Definidos vs 7 con API Completa

```
ObType::Unknown  = 0  ⬜ Sin uso
ObType::Process  = 1  ✅ ob_create(Process), ob_query_info, ob_set_info, ob_wait
ObType::Driver   = 2  ✅ ob_create(Driver), ob_query_info, ob_set_info, ob_destroy
ObType::Device   = 3  ⚠️ abierto via ob_open(\Device\*) pero no create/set
ObType::Pipe     = 4  ✅ ob_create(Pipe), ob_query_info, ob_wait, auto-destroy
ObType::EventBus = 5  ⬜ Solo kernel, no expuesto a user-mode
ObType::BlockDevice = 6  ⬜ Solo kernel, no expuesto a user-mode
ObType::Filesystem  = 7  ⬜ Solo interno, handle files via ob_open
ObType::MemoryRegion= 8  ⬜ Solo kernel
ObType::Symlink  = 9  ⚠️ Usado internamente por namespace
ObType::MountPoint=10  ⬜ Solo kernel
ObType::Directory=11  ✅ ob_create(Directory), ob_enum, ob_destroy
ObType::Key     =12  ⚠️ Info objects virtuales, no registry persistente
ObType::Event   =13  ✅ ob_create(Event), ob_wait
ObType::Semaphore=14  ❌ Definido pero sin API
ObType::Timer   =15  ⚠️ ob_wait(Timer) soportado, falta ob_create(Timer)
```

### Fase 1: Completar ObTypes existentes (v0.44.7 — Prioridad Inmediata)

Syscalls que ya tienen toda la infraestructura Ob necesaria. Solo falta:
- Completar enums
- Añadir `ObType::Thread = 16`
- Implementar thread como objeto

| ID | Tarea | Archivos | Esfuerzo | Dependencias |
|----|-------|----------|----------|-------------|
| OBF-01 | Añadir `ObInfoClass::ReadContent=15`, `VolumeLabel=16` al enum | `src/object/types.rs` | 5 min | — |
| OBF-02 | Añadir `ObSetInfoClass::ProcessTerminate=4`, `VfsRename=6`, `WriteContent=7`, `SetCwd=8`, `SetVolumeLabel=9` al enum | `src/object/types.rs` | 5 min | — |
| OBF-03 | Añadir `ObType::Thread = 16` al enum + `to_str()` | `src/object/types.rs` | 5 min | — |
| OBF-04 | Implementar `ob_create(Thread)` en handler_ob_create (type=16): crea KTHREAD, devuelve fd | `src/syscall/mod.rs` | 2-3h | OBF-03 |
| OBF-05 | Implementar `ob_wait(Thread)` en handler_ob_wait: kwait_block(ThreadJoin) | `src/syscall/mod.rs` | 1h | OBF-03 |
| OBF-06 | Implementar `ob_set_info(ThreadPriority)` usando fd thread | `src/syscall/mod.rs` | 30 min | OBF-03 |
| OBF-06b | Eliminar `handler_thread_create` (RAX 22) y `handler_thread_join` (RAX 23) del SSDT → `None` | `src/syscall/mod.rs` | 5 min | OBF-04, OBF-05 |
| OBF-07 | Unificar ObError y SyscallError en NeoDosError | `src/object/types.rs`, `src/syscall/mod.rs` | 1 día | — |
| OBF-08 | Migrar `sys_waitpid` a KWait nativo (eliminar `WAIT_PID` static mut) | `src/usermode.rs`, `src/syscall/mod.rs` | 2-3h | CB1 |
| OBF-09 | Tests kernel: 8 tests (thread create/wait/kill via Ob, enum completos, error unificado) | `src/testing.rs` | ~150 líneas | OBF-01..08 |

**Criterio de aceptación:**
- `sys_ob_create("\\MyThread", Thread)` devuelve fd
- `sys_ob_wait(thread_fd)` espera terminación
- `sys_ob_set_info(thread_fd, ThreadPriority, &prio)` funciona
- `ObInfoClass::ReadContent` y `VolumeLabel` están en el enum
- `ObSetInfoClass::ProcessTerminate`, `VfsRename`, `WriteContent`, `SetCwd`, `SetVolumeLabel` están en el enum
- `ObError` y `SyscallError` comparten base común
- 528 kernel tests + cmdtest siguen pasando

### Fase 2: Nuevos Object Types (v0.46 — Timer, Semaphore, Section) ~~COMPLETADO~~

~~Requieren nuevos tipos en el Object Manager y extensión de las syscalls Ob existentes.~~

| ID | Tarea | Estado | Syscalls |
|----|-------|--------|----------|
| OBF-10 | Timer Object: create (oneshot/periodic, period_ms), set, cancel | ~~COMPLETADO~~ | ob_create(Timer) via RAX 61, ob_set_info(TimerStart/TimerCancel), ob_wait(Timer) |
| OBF-11 | Semaphore Object: create (initial_count, max_count), release, wait | ~~COMPLETADO~~ | ob_create(Semaphore) via RAX 61, ob_set_info(SemaphoreRelease), ob_wait(Semaphore) |
| OBF-12 | Section Object: create (size, prot), map_view, unmap | ~~COMPLETADO~~ | ob_create(Section) via RAX 61, ob_set_info(MapView), ob_set_info(UnmapView) |
| OBF-13 | Registry Key Object: open, create key, query/set value, enum | 🔶 PENDIENTE | v0.50 (B2.1) |

**Criterio de aceptación cumplido:**
- ✅ Timer: `ob_create(Timer, period_ms=1000)` + `ob_wait(timer_fd)` → despierta al expirar
- ✅ Semaphore: `ob_create(Semaphore, initial=0, max=5)` + `ob_set_info(SemaphoreRelease)` + `ob_wait(sem_fd)` → OK
- ✅ Section: `ob_create(Section, size=4096, prot=RW)` → fd → `ob_set_info(MapView)` → dirección mapeada
- ✅ 560 kernel tests pasan (32 nuevos: 6 timer + 8 semaphore + 5 section + 4 kwait + 9 object)

### Fase 3: Syscalls Futuras como Objects (v0.47+) — RAX 67+

Todas las syscalls NUEVAS deben implementarse como `sys_ob_*`.

| RAX | Syscall Propuesta | Object Type | NT Equivalent | Versión |
|-----|-------------------|-------------|---------------|---------|
| 67 | `sys_ob_logon` | Session | `LsaLogonUser` | v0.48 |
| 68 | `sys_ob_logoff` | Session | `LsaLogoffUser` | v0.48 |
| 69 | `sys_ob_query_token` | Token | `NtQueryInformationToken` | v0.48 |
| 70 | `sys_ob_impersonate` | Token | `NtImpersonateThread` | v0.49 |
| 71 | `sys_ob_revert_to_self` | Token | `RevertToSelf` | v0.49 |
| 72 | `sys_ob_set_security` | Generic | `NtSetSecurityObject` | v0.48 |
| 73 | `sys_ob_query_security` | Generic | `NtQuerySecurityObject` | v0.48 |
| 74 | `sys_ob_elevate` | Token | Elevar token | v0.49 |
| 75 | `sys_ob_check_access` | Generic | Check ACL sin open | v0.49 |
| 76 | `sys_ob_consent_response` | Elevation | UAC consent | v0.49 |
| 77 | `sys_ob_create_section` | Section | `NtCreateSection` | v0.47 |
| 78 | `sys_ob_map_view_section` | Section | `NtMapViewOfSection` | v0.47 |
| 79 | `sys_ob_socket` | Network | `NtCreateFile(\Device\Tcp)` | v0.47 |
| 80 | `sys_ob_bind` | Network | `NtDeviceIoControlFile` | v0.47 |
| 81 | `sys_ob_connect` | Network | `NtDeviceIoControlFile` | v0.47 |
| 82 | `sys_ob_send` | Network | `NtWriteFile` | v0.47 |
| 83 | `sys_ob_recv` | Network | `NtReadFile` | v0.47 |
| 77 | `sys_ob_create_section` | Section | `NtCreateSection` | v0.47 |
| 78 | `sys_ob_map_view_section` | Section | `NtMapViewOfSection` | v0.47 |
| 84 | `sys_ob_create_timer` | Timer | `NtCreateTimer` | v0.47 |
| 85 | `sys_ob_set_timer` | Timer | `NtSetTimer` | v0.47 |
| 86 | `sys_ob_create_semaphore` | Semaphore | `NtCreateSemaphore` | v0.47 |
| 87 | `sys_ob_release_semaphore` | Semaphore | `NtReleaseSemaphore` | v0.47 |
| 88 | `sys_ob_ioctl` | Device | `NtDeviceIoControlFile` | v0.47 |

### Tabla Evolución de Syscalls (v0.44.3 → v1.0)

| Estado | v0.44.3 | v0.47 | v1.0 |
|--------|---------|-------|------|
| Foundation (non-Object) | 19 | 15 | 12 |
| Ob syscalls (RAX 60-66) | 7 | 10 | 10 |
| Legacy parallel (compat) | 6 | 3 | 0 |
| Fase 1 nuevos Ob | 0 | 3 (Thread, Timer, Semaphore) | 3 |
| Fase 2+ nuevos Ob (token, net, registry) | 0 | 8 | 21 |
| **Total syscalls** | **35** | **45** | **55+** |

### Reglas de no-cambio

1. **RAX 0-29**: No se reasignan. Los números congelados en v0.40 permanecen como `None` en SSDT.
2. **sys_exit/write/read/yield/getpid**: Permanecen directas. Son mecanismos, no recursos.
3. **sys_mmap/munmap/brk**: Se mantienen directas. El Section Object (Fase 2) es una abstracción *nueva* que convive con mmap.
4. **sys_open/close/readfile/pipe/dup2/spawn**: Se eliminan del SSDT. El usuario DEBE usar `ob_open`, `ob_close`, `ob_query_info(ReadContent)`, `ob_create(Pipe)`, `ob_create(Process)`, `ob_wait(Process)`. Sin wrappers legacy.
5. **thread_create/join (RAX 22-23)**: Se eliminan del SSDT en Fase 1. El usuario DEBE usar `ob_create(Thread)`, `ob_wait(Thread)`. **No hay wrapper legacy.**

### Dependencias entre fases

```
Fase 1 (v0.44.7) ← No tiene prerequisitos externos
Fase 2 (v0.46)   ← Fase 1 completa + Device Tree
Fase 3 (v0.47+)  ← Fase 2 completa + Networking/Registry
```

### Tests Planificados (18 nuevos)

| Test | Fase | Descripción |
|------|------|-------------|
| T-OBF-01 | F1 | ob_create(Thread): thread creado via Ob devuelve fd válido |
| T-OBF-02 | F1 | ob_wait(Thread): espera thread terminar |
| T-OBF-03 | F1 | ob_set_info(ThreadPriority): cambiar prioridad thread |
| T-OBF-04 | F1 | ObInfoClass enums completos (ReadContent, VolumeLabel) |
| T-OBF-05 | F1 | ObSetInfoClass enums completos (ProcessTerminate, VfsRename, WriteContent, SetCwd, SetVolumeLabel) |
| T-OBF-06 | F1 | NeoDosError unificado: ObError → SyscallError mapping automático |
| ~~T-OBF-07~~ | ~~F2~~ | ~~ob_create(Timer): crear timer oneshot~~ COMPLETADO |
| ~~T-OBF-08~~ | ~~F2~~ | ~~ob_wait(Timer): timer expires y despierta~~ COMPLETADO |
| ~~T-OBF-09~~ | ~~F2~~ | ~~ob_create(Semaphore): crear semáforo con count inicial~~ COMPLETADO |
| ~~T-OBF-10~~ | ~~F2~~ | ~~ob_set_info(SemaphoreRelease): incrementa count~~ COMPLETADO |
| ~~T-OBF-11~~ | ~~F2~~ | ~~ob_wait(Semaphore): bloquea hasta count > 0~~ COMPLETADO |
| ~~T-OBF-12~~ | ~~F2~~ | ~~ob_create(Section): section anónima~~ COMPLETADO |
| ~~T-OBF-13~~ | ~~F2~~ | ~~ob_set_info(MapView): mapear vista de sección~~ COMPLETADO |
| ~~T-OBF-14~~ | ~~F2~~ | ~~ob_set_info(UnmapView): desmapear vista~~ COMPLETADO |
| T-OBF-15 | F3 | ob_wait(Thread) + KWait: múltiples threads WAIT_ALL |
| T-OBF-16 | F3 | ob_wait(Process, Pipe, Event, Timer) WAIT_ANY |
| T-OBF-17 | F3 | ob_set_info(SecuritySet): cambiar SD de objeto |
| T-OBF-18 | F3 | ob_query_info(Security): leer SD de objeto |

---

## USR: Sistema de Usuarios NT-style (SAM + UAC + SUDO)

### Filosofía NT vs Unix

| Concepto | Unix | NT (NeoDOS) |
|----------|------|-------------|
| Identidad | UID numérico | **SID** (S-1-5-21-RID) |
| DB usuarios | `/etc/passwd` texto | **SAM** binario (`C:\System\Config\SAM`) |
| Hash passwords | SHA-256 | **MD4** (NT hash) + salt |
| Permisos archivo | rwx bits | **DACL** con ACEs deny/allow por SID+Grupos |
| Owner | UID en inodo | **SecurityDescriptor** (Owner SID + DACL + Integrity) |
| Elevación | `sudo` (setuid) | **Split Token** + `sys_elevate` + Consent prompt |
| Suplantación | `su` | `sys_impersonate` + LogonSession |
| Cambio permisos | `chmod` | `sys_set_security` (SD completo) |
| Control acceso | owner/group/other | SID + Group SIDs + Integrity Level + Privileges |
| Grupos | GID en `/etc/group` | Group SIDs en el token, evaluados en SeAccessCheck |
| Privilegios | Solo root vs user | **PrivilegeSet** (SeTcbPrivilege, etc.) |

### Token NT-style

```rust
pub struct Token {
    pub user_sid: Sid,
    pub username: [u8; 32],
    pub groups: Vec<Sid>,
    pub privileges: PrivilegeSet,
    pub is_admin: bool,
    pub impersonation_level: ImpersonationLevel,
    pub session_id: u32,
}
```

### Privilegios NT

| Flag | Nombre | Descripción |
|------|--------|-------------|
| 1 | `SeTcbPrivilege` | Actuar como parte del SO |
| 2 | `SeBackupPrivilege` | Backup (ignora ACLs) |
| 4 | `SeRestorePrivilege` | Restaurar archivos |
| 8 | `SeTakeOwnershipPrivilege` | Tomar ownership |
| 16 | `SeDebugPrivilege` | Depurar procesos |
| 32 | `SeShutdownPrivilege` | Apagar sistema |
| 64 | `SeLoadDriverPrivilege` | Cargar drivers |
| 128 | `SeChangeNotifyPrivilege` | Atravesar directorios |

### Split Token + Elevación (sudo NT-style)

Cada admin tiene **dos tokens vinculados**:
- **Token filtrado**: Medium integrity, sin privilegios, grupos limitados (sesión normal)
- **Token completo**: High integrity, todos los privilegios (elevado vía `sys_ob_elevate`)

```
Login (admin) → Kernel crea split tokens
  → Shell inicia con token filtrado (Medium)
  → SUDO COMMAND → sys_ob_elevate(\Security\Elevation\{pid})
    → Policy check (Consent/Auto/CredUI/Deny)
    → Returns ELEVATION_REQUIRED (-42) → spawn consent prompt
    → sys_ob_consent_response(\Security\Consent\{id}, 1)
    → Kernel swappa al token completo (High)
    → Spawn COMMAND con token elevado
  → Al terminar COMMAND → sys_ob_revert_to_self (vuelve a token filtrado)
```

### SAM (Security Accounts Manager)

`C:\System\Config\SAM` — formato binario:

```
SAM Header (32 B): magic "SAM\0", version, entry_count, checksum
SAM Entry (128 B c/u):
  - rid: u32 (500=admin, 1000+ = users)
  - username: [u8; 32]
  - full_name: [u8; 64]
  - hash_nthash: [u8; 16]     ← MD4 hash
  - salt: [u8; 16]
  - flags: u32                 ← ACCOUNT_DISABLED, LOCKOUT, etc.
  - profile_path: [u8; 64]    ← C:\Users\Username
```

### Ob Architectural Rule

Toda syscall de seguridad (RAX 67-76) DEBE implementarse como `sys_ob_*` — opera sobre objetos en el namespace Ob, recibe fds de objetos Ob y entrega fds obtenidos via `ob_open`/`ob_create`. El Object Manager gestiona lifecycle y seguridad. **No se aceptan syscalls planas legacy para funcionalidad nueva.**

Flujo Ob para el sistema de usuarios:

```
\Security\                        # Raíz de objetos de seguridad
├── Session\{id}\                 # LogonSession (sys_ob_logon devuelve fd aquí)
│   └── LinkedToken               # Token elevado vinculado
├── Token\{pid}\                  # Token de proceso (sys_ob_query_token lee aquí)
├── Elevation\{pid}\              # Solicitud de elevación activa
├── Consent\{elev_id}\            # Prompt de consentimiento pendiente
└── Policy                        # Política UAC global
```

### Syscalls Nuevas (Ob-style)

| RAX | Syscall | Args | NT Equivalent |
|-----|---------|------|---------------|
| 67 | `sys_ob_logon` | RBX=username, RCX=hash, RDX=hash_len | `LsaLogonUser` |
| 68 | `sys_ob_logoff` | RBX=fd | `LsaLogoffUser` |
| 69 | `sys_ob_query_token` | RBX=fd, RCX=info_class, RDX=buf, R8=size | `NtQueryInformationToken` |
| 70 | `sys_ob_impersonate` | RBX=fd | `NtImpersonateThread` |
| 71 | `sys_ob_revert_to_self` | — | `RevertToSelf` |
| 72 | `sys_ob_set_security` | RBX=fd, RCX=sd_ptr, RDX=sd_len | `NtSetSecurityObject` |
| 73 | `sys_ob_query_security` | RBX=fd, RCX=sd_buf, RDX=sd_len | `NtQuerySecurityObject` |
| **74** | **`sys_ob_elevate`** | **RBX=fd, RCX=password_or_null** | **Elevar token** |
| **75** | **`sys_ob_check_access`** | **RBX=path, RCX=desired_access** | **Check ACL sin open** |
| **76** | **`sys_ob_consent_response`** | **RBX=fd, RCX=response** | **Responder prompt UAC** |

### InfoClass para sys_ob_query_token (RAX 69)

| Class | Value | Returns |
|-------|-------|---------|
| `TokenUser` | 1 | User SID |
| `TokenGroups` | 2 | Group SIDs |
| `TokenPrivileges` | 3 | PrivilegeSet |
| `TokenSessionId` | 4 | Session ID |
| `TokenLogonId` | 5 | Logon ID |
| `TokenIntegrityLevel` | 6 | Integrity Level |
| `TokenElevationType` | 7 | Full/Filtered/Default |
| `TokenLinkedToken` | 8 | Linked (elevated) token handle |

### Integrity Levels

| Level | Value | SID | Default para |
|-------|-------|-----|-------------|
| Untrusted | 0 | S-1-16-0 | Guest, sandbox |
| Low | 1 | S-1-16-4096 | Internet-facing processes |
| Medium | 2 | S-1-16-8192 | Usuarios normales |
| High | 3 | S-1-16-12288 | Admin elevado |
| System | 4 | S-1-16-16384 | PID 1 (NeoInit) |

Regla MIC: un proceso NO puede escribir a un objeto con IL mayor.

### Built-in SIDs

| SID | Nombre | Descripción |
|-----|--------|-------------|
| S-1-5-18 | NT AUTHORITY\SYSTEM | Sistema |
| S-1-5-21-500 | Builtin\Administrator | Admin por defecto |
| S-1-5-21-0-0-0-1000+ | Usuarios | RID > 1000 = usuario normal |
| S-1-5-32-544 | BUILTIN\Administrators | Grupo admin |
| S-1-5-32-545 | BUILTIN\Users | Grupo usuarios |
| S-1-5-32-546 | BUILTIN\Guests | Grupo invitados |
| S-1-1-0 | Everyone | Todos los usuarios |
| S-1-5-11 | NT AUTHORITY\Authenticated Users | Usuarios autenticados |

### Logon Flow

```
Boot → NeoInit (PID 1, SYSTEM token, IL=System)
  → Spawn WINLOGON.NXE
    → Muestra pantalla: "Press Ctrl+Alt+Del to log on"
    → Lee username + password
    → sys_ob_logon(user, hash)
      → Kernel: SAM lookup → verify NT hash
      → Crea LogonSession como ObObject en \Security\Session\{id}
      → Retorna fd al objeto LogonSession con split tokens
    → sys_ob_query_token(fd, TokenLinkedToken, ...)
      → Obtiene fd del Token vinculado (elevado)
    → sys_ob_impersonate(fd) → swap al token filtrado de la sesión
    → Spawn NEOSHELL.NXE con token filtrado
      → Shell en C:\Users\Alejandro\ con IL=Medium

SUDO COMMAND:
  → sys_ob_elevate(fd)
    → Policy: Consent → retorna -42 (ELEVATION_REQUIRED)
    → sudo spawnea consent.nxe en Secure Desktop
    → sys_ob_consent_response(elev_fd, 1)
    → Kernel swappa a token completo (IL=High)
    → Spawn COMMAND elevado
  → COMMAND termina → sys_ob_revert_to_self (IL=Medium)
```

### Políticas de Elevación (en `C:\System\Config\SECURITY`)

```ini
[NEOUAC]
DefaultPolicy=Consent
AutoList=C:\Programs\NeoInit.nxe;C:\Programs\WinLogon.nxe
DenyList=C:\Programs\Format.nxe
AdminOnly=C:\Programs\DriverLoad.nxe
```

| Policy | Comportamiento |
|--------|----------------|
| Auto | Elevación automática (sin prompt) |
| Consent | Mostrar prompt de confirmación (default admin) |
| CredUI | Pedir contraseña (default no-admin) |
| Deny | Denegar siempre |

### Archivos de Configuración del Sistema

```
C:\System\Config\
├── SAM              # Security Accounts Manager (binario)
├── SECURITY         # UAC policy + security settings
└── SUDOERS          # Elevation policy por usuario/comando (opcional)
```

### Directorio de Usuarios

```
C:\Users\
├── Default\         # Perfil template
│   ├── Desktop\
│   ├── Documents\
│   └── ...
├── Administrator\  # RID 500
│   └── ...
└── Alejandro\      # RID 1000
    └── ...
```

### Binarios Ring 3

| Binario | NT Eq. | Descripción |
|---------|--------|-------------|
| `winlogon.nxe` | winlogon.exe | Login screen + SAS handler |
| `sudo.nxe` | — | Elevación de privilegios |
| `consent.nxe` | consent.exe | UAC prompt en Secure Desktop |
| `samutil.nxe` | — | Gestión de SAM (adduser, deluser, passwd) |
| `whoami.nxe` | whoami.exe | Mostrar SID, grupos, privilegios |
| `runas.nxe` | runas.exe | Ejecutar como otro usuario |
| `secedit.nxe` | secedit.exe | Ver/modificar Security Descriptors |

### SecurityDescriptor en Inodos

Cada inodo puede tener un `security_descriptor_id: u32`:
- 0 = sin SD (comportamiento legacy: world-accessible)
- 1..N = índice en `SD_CACHE` global

Al crear archivo, se asigna SD con:
- Owner = token.user_sid
- DACL = ACE allow(owner, FULL_CONTROL) + ACE allow(Authenticated Users, READ)

Al abrir archivo, VFS llama a `se_access_check()` con el SD del inodo.

### Plan de Implementación

#### Fase 1 — SAM + Token NT (v0.48)

| ID | Item | Archivos | Esfuerzo |
|----|------|----------|----------|
| USR-001 | SAM database: formato binario + parse/save | `src/security/sam.rs` | ~250 líneas |
| USR-002 | Token NT extendido (SID, grupos, privileges, session_id) | `src/security/token.rs` | +80 líneas |
| USR-003 | PrivilegeSet struct + verificación | `src/security/privilege.rs` | ~80 líneas |
| USR-004 | Integrity Levels + verificación MIC | `src/security/integrity.rs` | ~60 líneas |
| USR-005 | SeAccessCheck con grupos + integrity | `src/security/access.rs` | +100 líneas |
| USR-006 | SecurityDescriptor por inodo (SD_CACHE) | `src/fs/neodos_fs.rs`, `src/security/acl.rs` | +150 líneas |
| USR-007 | sys_ob_logon / sys_ob_logoff (RAX 67-68) | `src/syscall/mod.rs` | +80 líneas |
| USR-008 | sys_ob_query_token (RAX 69) | `src/syscall/mod.rs` | +60 líneas |
| USR-009 | sys_ob_set_security / sys_ob_query_security (RAX 72-73) | `src/syscall/mod.rs` | +100 líneas |
| USR-010 | Load SAM en PHASE 2.77.6 | `src/main.rs` | +5 líneas |
| USR-011 | Tests kernel: 10 tests | `src/testing.rs` | ~150 líneas |

#### Fase 2 — Login + Sesiones + SUDO (v0.49)

| ID | Item | Archivos | Esfuerzo |
|----|------|----------|----------|
| USR-012 | winlogon.nxe — login screen | `userbin/winlogon/` | ~300 líneas |
| USR-013 | NeoInit → spawn WINLOGON.NXE | `userbin/neoinit/` | +5 líneas |
| USR-014 | LogonSession manager | `src/security/logon.rs` | ~150 líneas |
| USR-015 | Split token + linked_token | `src/security/linked_token.rs` | ~100 líneas |
| USR-016 | Elevation manager + cache + policy | `src/security/elevation.rs` | ~250 líneas |
| USR-017 | SECURITY policy file parser | `src/security/policy.rs` | ~150 líneas |
| USR-018 | sys_ob_elevate / sys_ob_check_access / sys_ob_consent_response (RAX 74-76) | `src/syscall/mod.rs` | +120 líneas |
| USR-019 | sudo.nxe — elevation frontend | `userbin/sudo/` | ~300 líneas |
| USR-020 | consent.nxe — UAC prompt en Secure VT | `userbin/consent/` | ~200 líneas |
| USR-021 | samutil.nxe — adduser/deluser/passwd | `userbin/samutil/` | ~300 líneas |
| USR-022 | whoami.nxe — SID/grupos/privilegios/IL | `userbin/whoami/` | ~100 líneas |
| USR-023 | sys_ob_impersonate / sys_ob_revert_to_self (RAX 70-71) | `src/syscall/mod.rs` | +80 líneas |
| USR-024 | Tests kernel: 8 tests | `src/testing.rs` | ~120 líneas |

#### Fase 3 — Hardening + Grupos (v0.50)

| ID | Item | Archivos | Esfuerzo |
|----|------|----------|----------|
| USR-025 | runas.nxe — ejecutar como otro usuario | `userbin/runas/` | ~200 líneas |
| USR-026 | secedit.nxe — ver/modificar SD | `userbin/secedit/` | ~200 líneas |
| USR-027 | group SIDs + group file parser | `src/security/group.rs` | ~100 líneas |
| USR-028 | Per-process home enforcement | `src/scheduler/mod.rs` | +30 líneas |
| USR-029 | SUDOERS policy file opcional | `src/security/policy.rs` | +80 líneas |
| USR-030 | Integrity Level enforcement en VFS writes | `src/fs/vfs.rs` | +40 líneas |
| USR-031 | Actualizar create_neodos_image.py (SAM template) | `scripts/` | +50 líneas |
| USR-032 | Tests kernel: 6 tests | `src/testing.rs` | ~80 líneas |

### Totales

| Fase | Líneas Nuevas | Tests |
|------|--------------|-------|
| Fase 1 (SAM + Token NT) | ~900 | 10 |
| Fase 2 (Login + SUDO) | ~1600 | 8 |
| Fase 3 (Hardening) | ~600 | 6 |
| **Total** | **~3100** | **24** |

### Variables de Entorno (nuevas)

| Variable | Descripción | Default |
|----------|-------------|---------|
| `%USERNAME%` | Nombre del usuario actual | — |
| `%USERPROFILE%` | Path al perfil del usuario | `C:\Users\%USERNAME%` |
| `%USERDOMAIN%` | Nombre del dominio/equipo | `NEODOS` |
| `%LOGONSERVER%` | Servidor de logon | `\\NEODOS` |

### Tests Kernel (24 total)

| Test | Descripción |
|------|-------------|
| T-USR-1 | SAM parse: leer entry, verificar rid/username/hash/flags |
| T-USR-2 | SAM save: escribir y re-leer mantiene integridad |
| T-USR-3 | SAM add/remove user: cuenta crece/decrece |
| T-USR-4 | Token NT: user_sid, groups, privileges, session_id |
| T-USR-5 | PrivilegeSet: set/check/clear bits, deny no-owned |
| T-USR-6 | Integrity Level: compare, write-denied si IL menor |
| T-USR-7 | SeAccessCheck con grupos: miembro del grupo accede |
| T-USR-8 | SeAccessCheck sin grupo: no-miembro denegado |
| T-USR-9 | sys_ob_logon: usuario válido → fd a Ob object LogonSession |
| T-USR-10 | sys_ob_logon: hash inválido → denied |
| T-USR-11 | sys_ob_query_token: TokenUser retorna SID correcto |
| T-USR-12 | sys_ob_set_security: cambiar SD de archivo via fd, luego verificar |
| T-USR-13 | Split token: admin login crea filtered + linked |
| T-USR-14 | sys_ob_elevate: Auto policy → linked token asignado |
| T-USR-15 | sys_ob_elevate: Deny policy → access denied |
| T-USR-16 | sys_ob_consent_response: approve → elevation granted |
| T-USR-17 | sys_ob_impersonate: cambiar token activo via fd |
| T-USR-18 | sys_ob_revert_to_self: restaurar token original |
| T-USR-19 | MIC: proceso Medium no escribe a archivo High |
| T-USR-20 | MIC: proceso High sí escribe a archivo Medium |
| T-USR-21 | VFS: archivo sin SD es world-accessible (retrocompat) |
| T-USR-22 | VFS: archivo con SD y DACL restrictivo deniega acceso |
| T-USR-23 | Owner en creación: archivo nuevo tiene SID del creador |
| T-USR-24 | Integrity en proceso: NeoInit IL=System, user IL=Medium |

### Dependencias

| Fase | Prerequisito |
|------|-------------|
| Fase 1 | Ninguno (base sobre NT6 existente) |
| Fase 2 | Fase 1 completa |
| Fase 3 | Fase 2 completa |

---

## Recommended Next Steps

Priorizados por impacto y dependencias (con bugs criticos como prioridad 0):

| Prioridad | Item | Fase | Dependencias | Esfuerzo estimado |
|-----------|------|------|-------------|-------------------|
| 0 | **CB1: Fix WAIT_PID SMP-unsafe** | v0.44.4 | KWait | 2-3 horas |
| 0 | **CB2: Fix ISOLATED_REGIONS sin sync** | v0.44.4 | -- | 1 hora |
| 0 | **CB3: Fix NXL_REGISTRY sin sync** | v0.44.4 | -- | 30 min |
| 1 | **DH1: Actualizar README** | v0.44.5 | -- | 30 min |
| 2 | **DH2: Corregir ARCHITECTURE_SOURCE_OF_TRUTH** | v0.44.5 | -- | 1 hora |
| 3 | **DH3: Completar libneodos wrappers** | v0.44.6 | -- | 3-4 horas |
| 4 | **AI-1: Completar ObInfoClass/ObSetInfoClass enums** | v0.44.7 | -- | 15 min |
| 5 | **AI-4: Arreglar TOCTOU race en kobj_register** | v0.44.7 | -- | 30 min |
| 6 | **AI-2: Consolidar exports duplicados** | v0.44.7 | -- | 1 hora |
| 7 | **AI-3: Unificar codigos de error** | v0.44.7 | -- | 1 dia |
| 8 | **AI-5/CQ1: Reorganizar libneodos-nxl** | v0.44.8 | -- | 2 horas |
| 9 | **VirtIO block driver (A5.2)** | v0.46 | A2.1 (ECAM) | 400-500 lineas |
| 10 | **Device Tree + Resource Manager** | v0.46 | NT5, Driver Runtime | 600-800 lineas |
| 11 | **sys_ioctl() and PCI device binding** | v0.46 | A2.1, A2.2 | 300-400 lineas |
| 12 | **Networking (B3.1-B3.2)** | v0.47 | VirtIO-net, IRP | 3000-5000 lineas |
| 13 | **AHCI NCQ (A5.3)** | v0.48 | A2.2, IRP | 400-600 lineas |
| 14 | **USR-F1: SAM + Token NT** | v0.48 | NT6 (existente) | 900 lineas |
| 15 | **USR-F2: Login + SUDO** | v0.49 | USR-F1 | 1600 lineas |
| 16 | **USR-F3: Hardening + Grupos** | v0.50 | USR-F2 | 600 lineas |
| 17 | **NeoReg transaction journal (B2.2)** | v0.50 | B2.1 | 500-700 lineas |
| 18 | **Shell redirection (B4.3)** | v0.46+ | neoshell | 300-400 lineas |
| 19 | **Registry hive database (B2.1)** | v0.50 | NT5, NT6, IoStack | 2000-3000 lineas |
| 20 | **Kernel debugger (A3.2)** | v0.51+ | A3.1 | 1500-2000 lineas |

---

## Referencias

- [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md) — invariantes MUST/MUST NOT
- [AGENTS.md](../AGENTS.md) — build, test, convenciones de commit
- [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) — vision a largo plazo v0.40 -> v1.0
- [OBJECT_MANAGER_ARCHITECTURE.md](OBJECT_MANAGER_ARCHITECTURE.md) — diseno completo del Object Manager
- [KERNEL.md](KERNEL.md) — documentacion del kernel
