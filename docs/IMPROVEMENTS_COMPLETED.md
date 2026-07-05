# NeoDOS — Items Completados

> Items completados del roadmap, movidos desde `IMPROVEMENTS.md`.
> Version actual: v0.48.7 (Registry config, audits).
> Proximo milestone: v0.49 (Servicios de red).

---

### CB1..CB3, OBF-08: SMP-unsafe static mut bugs [COMPLETED]

* [x] **CB1. Fix WAIT_PID static mut SMP-unsafe** | Prereqs: KWait | Files: `src/usermode.rs`
  - **Descripcion:** `WAIT_PID` era un `static mut` en usermode.rs. Corregido: ahora es `AtomicU32`, seguro para SMP.
  - **Severidad:** ~~CRITICO~~ COMPLETADO
  - **Tests:** `smp_waitpid_concurrent`, `smp_waitpid_no_race`

* [x] **CB2. Fix ISOLATED_REGIONS static mut sin sincronizacion** | Prereqs: -- | Files: `src/drivers/isolation.rs`
  - **Descripcion:** `ISOLATED_REGIONS` era un array estatico mutable. Corregido: ahora es `Mutex<[...]>`, todo acceso via `.lock()`.
  - **Severidad:** ~~CRITICO~~ COMPLETADO
  - **Tests:** `smp_isolated_region_concurrent_access`

* [x] **CB3. Fix NXL_REGISTRY static mut sin proteccion SMP** | Prereqs: -- | Files: `src/nxl.rs`
  - **Descripcion:** `NXL_REGISTRY` era array fijo sin sincronizacion. Corregido: ahora es `Mutex<[...]>`, todo acceso via `.lock()`.
  - **Severidad:** ~~ALTA~~ COMPLETADO
  - **Tests:** `smp_nxl_concurrent_load`

* [x] **OBF-08. Migrar sys_waitpid a AtomicU32 (eliminar WAIT_PID static mut)** | Prereqs: CB1 | Files: `src/usermode.rs`, `src/syscall/mod.rs`
  - **Descripcion:** `WAIT_PID` migrado de `static mut` a `AtomicU32` con operaciones atómicas SeqCst en lugar de KWait. Suficiente para SMP-safety.
  - **Severidad:** ~~CRITICO~~ COMPLETADO
  - **Tests:** `smp_waitpid_concurrent`, `smp_waitpid_no_race`

### Fase 1: Maduracion (v0.40 - v0.45)

*Todos los items de la Fase 1 estan completados.*

1. ~~**v0.43** — SeAccessCheck NT-compatible, sys_poll(), Congelar pipe/IRP protocols~~ **COMPLETADO**
2. ~~**v0.44** — ASLR v1 (base aleatoria), Ob syscalls RAX 60-66~~ **COMPLETADO** (v0.44.2: Ob migration completa, todas las syscalls legacy desactivadas)
3. ~~**v0.45** — Ob migration, Device Tree + Resource Manager, Driver state machine freeze~~ **COMPLETADO** (Ob migration completada en v0.44.2; Device Tree y Resource Manager se mueven a v0.46)

### v0.48.7 (Registry config + Audits)

* [x] **B2.6. Registry defaults in boot** | Files: `src/main.rs`, `src/cm/mod.rs`
  - En Phase 3.881, crear `CurrentControlSet\Services\NeoInit\DefaultShell`,
    `Network\Interfaces\0\DHCPEnabled=1`, etc. Solo si no existen.
  - **Tests:** `cm_default_values_created`

* [x] **B4.10. NeoInit: leer Registry para config** | Files: `userbin/neoinit/`
  - NeoInit lee DefaultShell, AutoStartServices, EnableVT, WaitForNetwork desde
    `\Registry\Machine\System\CurrentControlSet\Services\NeoInit`.
  - **Tests:** boot con Registry, verificar shell spawn

* [x] **AUDIT-1. Registry info classes handled** | Files: `src/syscall/ob.rs`, `src/object/types.rs`
  - `ObInfoClass::RegistryKey (21)` y `::RegistryValue (22)` implementados.
  - `ObSetInfoClass::RegistryCreateKey (23)`, `::RegistryDeleteKey (24)`, `::RegistrySetValue (25)`, `::RegistryDeleteValue (26)` implementados.

* [x] **AUDIT-2. libneodos ObInfoClass/ObSetInfoClass sync** | Files: `libneodos/src/syscall.rs`
  - Añadidos 6 variantes faltantes a ObInfoClass. ObSetInfoClass convertido a enum con 27 variantes.
  - `sys_ob_set_info` ahora toma `ObSetInfoClass` en vez de `u32`.

* [x] **AUDIT-3. Dual mount systems (MAX_MOUNTS)** | Files: `src/fs/vfs.rs`
  - Renombrado `MAX_MOUNTS` a `MAX_SUBDIR_MOUNTS` en fs/vfs.rs para eliminar ambigüedad.

* [x] **AUDIT-4. DPC overflow handling + tests** | Files: `src/dpc/mod.rs`
  - Añadido `DPC_DROPPED_COUNT` global. 3 nuevos tests: queue_overflow, dispatch_pending_global_api.

* [x] **AUDIT-9. Kernel link address in docs** | Files: `docs/ARCHITECTURE.md`, `docs/memory.md`, `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`, `docs/DEBUG.md`
  - Corregido `0x200000`/`0x100000` → `0x4000000` en todas las referencias.

* [x] **AUDIT-10. ObSetInfoClass::Security implementado** | Files: `src/syscall/ob.rs`
  - Reemplazado `err_to_u64(NoSys)` por implementación funcional que parsea SD y llama `ob_set_security`.

### v0.47 (Networking TCP/IP)

* [x] **v0.47. Networking: NIC driver NEM + TCP/IP stack** | Prereqs: — | Files: `src/net/`
  - **Descripcion:** Stack TCP/IP completo (e1000 NIC, Ethernet, ARP, IPv4, ICMP, UDP, TCP, \Device\Tcp, \Device\Udp). **COMPLETADO**
  - **Tests:** 17 tests (ver sección B3)

- [x] **B3.1 D9. Network I/O | NT: Winsock (ws2_32.dll) -> NtCreateFile(\Device\Tcp)** | Prereqs: A4.1, A4.2 | Files: `src/net/`, `src/syscall.rs` | **COMPLETADO en v0.47**
  - **Descripcion:** Modelo NT: el kernel expone `\Device\Tcp` y `\Device\Udp` como objetos de dispositivo en el namespace NT5. La API de red user-mode va en `src/syscall/ob.rs` (ObCreate Socket, ObSetInfo SocketConnect/SocketBind/SocketListen/SocketSend/SocketClose, ObQueryInfo SocketInfo/SocketAddr/TcpStatus/NicInfo).
  - **Severidad:** COMPLETADO
  - **Tests:** 17 tests: `net_mac_addr_basics`, `net_ipv4_addr_basics`, `net_ipv4_checksum`, `net_arp_cache_insert_lookup`, `net_arp_cache_eviction`, `net_arp_cache_static_survives_eviction`, `net_tcp_state_machine_simple`, `net_tcp_connection_lifecycle`, `net_tcp_connect_and_close`, `net_icmp_echo_reply_build`, `net_socket_manager_lifecycle`, `net_socket_bind_connect`, `net_udp_header_checksum`, `net_socket_addr_fmt`, `net_ipv4_classification`, `net_nic_registry_empty`.

- [x] **B3.2 E3. TCP/IP stack | NT: AFD (Ancillary Function Driver)** | Prereqs: B3.1 | Files: `src/net/` | **COMPLETADO en v0.47**
  - **Descripcion:** Stack de red completo en kernel como driver de dispositivo `\Device\Tcp` y `\Device\Udp`. Capas: Ethernet, ARP (tabla 64 entries, timeout 300s, static entries), IPv4 (header parse/build, checksum, TTL), ICMP (echo request/reply), UDP (header + pseudo-header checksum), TCP (3-way handshake, sequence numbers, sliding window 16 KB, FIN/RST). NIC driver via e1000 (82540EM/82543GC/82545EM/82574L).
  - **Severidad:** COMPLETADO
  - **Tests:** 17 tests (incluye tcp lifecycle, icmp echo reply build).

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

### B9. Shell command migration Ring 0 -> Ring 3 [COMPLETED]

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

Los comandos de gestion de archivos (DEL, REN, MD, RD, COPY, TYPE, DIR, TREE, CD, CLS, ECHO, DATE, TIME, VOL, NEOMEM, VER, CPUINFO, DATETIME, VER) tambien estan migrados a Ring 3 como `.NXE`. El comando `MEM` fue reemplazado por `NEOMEM` (NeoMem v0.1).

### OBF-01..06, OBF-09 (Fase 1 Objectification)

* [x] **OBF-01. Anadir ObInfoClass::ReadContent=15, VolumeLabel=16 al enum** | Prereqs: — | Files: `src/object/types.rs`
  - **Severidad:** BAJA — 5 min
  - **Tests:** (cobertura de compilacion)

* [x] **OBF-02. Anadir ObSetInfoClass::ProcessTerminate=4, VfsRename=6, WriteContent=7, SetCwd=8, SetVolumeLabel=9 al enum** | Prereqs: — | Files: `src/object/types.rs`
  - **Severidad:** BAJA — 5 min
  - **Tests:** (cobertura de compilacion)

* [x] **OBF-03. Anadir ObType::Thread = 16 al enum + to_str()** | Prereqs: — | Files: `src/object/types.rs`
  - **Severidad:** BAJA — 5 min
  - **Tests:** `ob_type_thread_enum`

* [x] **OBF-04. Implementar ob_create(Thread) en handler_ob_create** | Prereqs: OBF-03 | Files: `src/syscall/mod.rs`
  - **Descripcion:** Crea KTHREAD, devuelve fd
  - **Severidad:** MEDIA — 2-3h
  - **Tests:** `ob_thread_create_and_destroy`

* [x] **OBF-05. Implementar ob_wait(Thread) en handler_ob_wait** | Prereqs: OBF-03 | Files: `src/syscall/mod.rs`
  - **Descripcion:** kwait_block(ThreadJoin)
  - **Severidad:** MEDIA — 1h
  - **Tests:** `ob_thread_join`

* [x] **OBF-06. Implementar ob_set_info(ThreadPriority) usando fd thread** | Prereqs: OBF-03 | Files: `src/syscall/mod.rs`
  - **Severidad:** BAJA — 30 min
  - **Tests:** `ob_thread_priority`

* [x] **OBF-06b. Eliminar handler_thread_create (RAX 22) y handler_thread_join (RAX 23) del SSDT** | Prereqs: OBF-04, OBF-05 | Files: `src/syscall/mod.rs`
  - **Severidad:** BAJA — 5 min
  - **Tests:** (verificar SSDT None)

* [x] **OBF-09. Tests kernel: 8 tests (thread create/wait/kill via Ob, enum completos, error unificado)** | Prereqs: OBF-01..08 | Files: `src/testing.rs`
  - **Severidad:** MEDIA — ~150 lines
  - **Tests:** `ob_thread_create_and_destroy`, `ob_thread_type_in_enum_snapshot`, etc.

### A5.3. AHCI NCQ [COMPLETADO]

- [x] **A5.3. AHCI NCQ** | NT: Storport Native Command Queuing | Prereqs: A2.2
  - **Archivos:** `src/drivers/boot_ahci.rs` (extend), `drivers/ahci/src/lib.rs` (NEM driver), `src/irp/mod.rs` (tag-based dispatch)
  - **Descripcion:** Native Command Queuing en AHCI permite hasta 32 operaciones simultaneas con finalizacion out-of-order.
    - **NCQ path:**
      1. Host prepara 32 command tables en memoria (FIS buffer per slot).
      2. Escribe descriptores a device: ATA FPDMA QUEUED READ (0x60) / WRITE (0x61)
      3. Device acepta hasta 32 cmds sin esperar completaciones.
      4. Device finaliza out-of-order: escribe SActive register (bit = completado), trigger IRQ.
      5. Host lee Successful NCQ Completion Notification (FIS D2H), extrae tag, localiza IRP via tag.
    - **Tag-based dispatch:** Per-device, map `[Option<IrpId>; 32]` indizado por tag.
    - **Fall back to legacy:** Si device no soporta NCQ (via IDENTIFY), usar single-command path.
    - **Completado en v0.46.2:**
      - IrpTagMap: alloc_tag/free/lookup/in_use/is_full/is_empty, 4 tests unitarios.
      - boot_ahci.rs: IDENTIFY DEVICE para NCQ detection, `ncq_batch_xfer()` 32-slot batch FPDMA, `ncq_submit_irp_batch()` IRP batch, tag-based `poll_irp()`, fallback automatico a legacy DMA EXT.
      - NEM AHCI driver: per-slot NCQ buffers (32 slots × 2 puertos), NCQ path en `ahci_read`/`ahci_write`, `ahci_ncq_batch_read` export, IDENTIFY per-port en `driver_init`.
  - **Criterio:**
    - 32 tags allocados concurrentemente, dispatch via IrpTagMap.
    - Tag-based completion lookup con out-of-order.
    - Fallback a legacy cuando tag map full o NCQ no soportado.
    - Stress 100 cycles con 32 tags sin perdida.
  - **Tests:** `ahci_ncq_32_concurrent_dispatch`, `ahci_ncq_tag_based_completion`, `ahci_ncq_fallback_to_legacy`, `ahci_ncq_out_of_order_completion`, `ahci_ncq_stress_load` (5 tests, completados + 4 irp tag map tests = 9 total).

### A4.4. Input subsystem redesign [COMPLETED]

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

### B4.5. Virtual terminals [COMPLETED]

- ~~**[COMPLETED] B4.5 B1. Virtual terminals** | Prereqs: A4.4, B4.4 | Files: `userbin/neoshell/`, `src/input/`~~
  - ~~**Descripcion:** Multiplexar el framebuffer y el input en hasta 4 terminales virtuales (VTs). Depende de A4.4 (input subsystem redisenado con `InputManager` y `vt_queues[4]`). Cada VT tiene su propio buffer de framebuffer, cola de input independiente, y PID foreground.~~
  - ~~**Criterio:** Alt+F1 y Alt+F2 muestran shells independientes. Input en un VT no afecta al otro.~~
  - ~~**Tests:** `vt_switch_alt_f1_f2`, `vt_independent_input`, `vt_framebuffer_swap`.~~

### AI-5 [COMPLETED]

* ~~**[COMPLETED] AI-5. Libneodos-nxl ya modularizado** | Prereqs: — | Files: `libneodos-nxl/src/`~~
  - ~~**Descripcion:** `libneodos-nxl/src/` ya usa modulos separados (`syscall.rs`, `io.rs`, `fs.rs`, `process.rs`, `mem.rs`, `error.rs`). Con la limpieza ABI v7, se eliminaron las funciones `nxl_sys_pipe/dup2/waitpid/chdir/chdir_parent/readdir` (process.rs) y `nxl_sys_mkdir/unlink/rmdir/rename/writefile` (fs.rs). No requiere mas reorganizacion.~~

### Fase 2 Ob: Timer, Semaphore, Section [COMPLETADO]

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

---

### v0.48 (NeoFS estabilidad — VFS Fase 2/4 — NET-1 F1-F4 — DHCP)

* [x] **VFS-1.1. Unificar MountManager** — COMPLETADO en v0.47.1
* [x] **VFS-1.2. Arreglar ownership ObOpen → VFS** — COMPLETADO en v0.48.0
* [x] **VFS-1.3. Eliminar stale namespace entries** — COMPLETADO en v0.48.1: ob_remove_by_id(), cleanup en destroy/close
* [x] **VFS-1.4. HandleTable → ObObject consistency** — COMPLETADO en v0.48.1: is_valid(), close() guardado, has_ob_object() bugfix
* [x] **v0.48. NeoFS estabilidad** — COMPLETADO en v0.48.2: FS-1.1/1.2/1.3 (dynamic allocators, sector offsets), NS-1.1/1.2 (ownership, protected dirs), CAP_NS_WRITE
* [x] **VFS-2.1. Privatizar métodos de NeoFS** — COMPLETADO en v0.48.3: 5 métodos pub→pub(crate)
* [x] **VFS-2.4. PageCache con contexto de drive** — COMPLETADO en v0.48.3: drive_id en clave PageCache
* [x] **VFS-4.1. Device IDs estables** — COMPLETADO en v0.48.4: register escanea slots libres (índices estables), find_by_name()
* [x] **VFS-4.2. Hot-unload safety** — COMPLETADO en v0.48.4: IoStack.stale flag, operaciones fallan en stale
* [x] **VFS-4.3. Refcount de block devices** — COMPLETADO en v0.48.4: refcounts[], acquire/release, remove() protegido
* [x] **OBF-07. Unificar ObError y SyscallError** — COMPLETADO: ob_err_to_syscall() + test
* [x] **B3.3 D8. DHCP client** — COMPLETADO en v0.48.5: dhcp.rs con Discover/Offer/Request/Ack, arranque automático
* [x] **B2.1 Z6. Registry hive database** — COMPLETADO en v0.48.0
* [x] **B2.7. Registry disk persistence (cm_flush_key)** — COMPLETADO en v0.48.6: NEOH serialization format, VFS file `C:\System\Registry\<name>.hiv`, dirty tracking, flush on shutdown. Tests: `cm_set_value_persist_roundtrip`, `cm_hive_serialization_integrity`
* [x] **NET-1 F1-F4** — COMPLETADO en v0.48.5: Ethernet/UDP/ARP builders, ICMP Port Unreachable, socket_send, UDP/TCP dispatch, TCP three-way handshake real

### NET-1.5..NET-1.15: Networking userland [COMPLETED]

* [x] **NET-1.5. libneodos: SOCKET constants + wrappers** | Files: `libneodos/src/syscall.rs`
  - Añadido `ob_type::SOCKET = 18`, `ObInfoClass::SocketRecv = 23`, y wrappers `ob_socket_create/connect/bind/listen/send/recv/close`.
  - Añadido `SocketAddrV4` struct, `sys_cm_set_value` (RAX=70) con macro `ob_syscall_5!`.

* [x] **NET-1.6. Kernel: ObInfoClass::SocketRecv (class 23)** | Files: `src/object/types.rs`, `src/syscall/ob.rs`
  - Handler en `ob_query_info` copia `socket.recv_buf` a usuario; si vacío retorna `-EAGAIN`.
  - Tests: `net_socket_recv_data`, `net_socket_recv_empty`.

* [x] **NET-1.8. net.nxl: userland network library** | Files: `libnet/` (new), `libnet-nxl/` (new)
  - NXL slot 3 (`0x1e0c0000`). API (16 funciones): `iface_count/info/stats`, `socket_create/bind/connect/listen/send/recv/close`, `set_ip/get_ip/get_gateway/get_mask/get_dhcp_bound`.
  - `libnet/` — static library wrapper con lazy loading via `loadlib`.
  - `sys_cm_set_value` añadido a libneodos.

* [x] **NET-1.15. netcfg.nxe: network service** | Files: `userbin/netcfg/` (new)
  - Servicio auto-iniciado por NeoInit. Lee Registry (`DHCPEnabled`), aplica IP estática o espera DHCP del kernel.
  - Si DHCP falla, asigna APIPA (169.254.1.1). Corre como daemon.
  - `ObSetInfoClass::SetNicIp = 27` para aplicar IP desde userspace.
  - Incluido en imagen NeoFS via `scripts/create_neodos_image.py`.

## Referencias

- [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md) — invariantes MUST/MUST NOT
- [skills/build/SKILL.md](../skills/build/SKILL.md) — build, test, convenciones de commit
- [AGENTS.md](../AGENTS.md) — permanent rules (minimal)
- [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) — vision a largo plazo v0.40 -> v1.0
- [OBJECT_MANAGER_ARCHITECTURE.md](OBJECT_MANAGER_ARCHITECTURE.md) — diseno completo del Object Manager
- [Boot](boot.md) — boot sequence, fases, GPT layout
