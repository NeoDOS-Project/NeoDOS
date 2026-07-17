# NeoDOS — Items Completados

> Items completados del roadmap, movidos desde `IMPROVEMENTS.md`.
> El roadmap maestro está en [`/ROADMAP.md`](../ROADMAP.md).
> Version actual: v0.50-dev (i18n I18N-P1/P3 completados, P2 parcial).
> Proximo milestone: v0.50 (Shell Phase 1 + NeoFS snapshot).

---

## CM-FIX. Registry bugfixes [COMPLETED]

- [x] **CM-FIX. Registry bugfixes** | Prereqs: -- | Files: `src/cm/hive.rs`, `src/cm/mod.rs`, `src/syscall/ob.rs`
  - **Fix free list:** reemplazar `free_head`/`scan_next_free` por next-fit linear scan con `next_alloc_hint`.
  - **Soft max cells:** cambiar `cells` de fijo `MAX_CELLS=2048` a `Vec<Option<Cell>>` dinámico.
  - **`delete_value()`:** desenlazar de lista de valores, liberar celda.
  - **`RegistryDeleteValue` handler:** llama a `cm_delete_value()` en vez del hack `REG_NONE`.
  - **`cm_unload_hive()`:** flush dirty data antes de desmontar.
  - **`cm_flush_key()` deadlock:** evitar doble adquisición de lock.
  - **`delete_key()` iterativo:** reemplazar recursión por `Vec` stack explícito.
  - **Tests:** `cm_free_list_next_fit`, `cm_delete_value`, `cm_delete_value_persist`, `cm_unmount_flush`, `cm_deep_key_deletion_iterative`, `cm_key_deletion_preserves_siblings`

---

### CB1..CB3, OBF-08: SMP-unsafe static mut bugs [COMPLETED]

- [x] **CB1. Fix WAIT_PID static mut SMP-unsafe** | Prereqs: KWait | Files: `src/usermode.rs`
  - **Descripcion:** `WAIT_PID` era un `static mut` en usermode.rs. Corregido: ahora es `AtomicU32`, seguro para SMP.
  - **Severidad:** ~~CRITICO~~ COMPLETADO
  - **Tests:** `smp_waitpid_concurrent`, `smp_waitpid_no_race`

- [x] **CB2. Fix ISOLATED_REGIONS static mut sin sincronizacion** | Prereqs: -- | Files: `src/drivers/isolation.rs`
  - **Descripcion:** `ISOLATED_REGIONS` era un array estatico mutable. Corregido: ahora es `Mutex<[...]>`, todo acceso via `.lock()`.
  - **Severidad:** ~~CRITICO~~ COMPLETADO
  - **Tests:** `smp_isolated_region_concurrent_access`

- [x] **CB3. Fix NXL_REGISTRY static mut sin proteccion SMP** | Prereqs: -- | Files: `src/nxl.rs`
  - **Descripcion:** `NXL_REGISTRY` era array fijo sin sincronizacion. Corregido: ahora es `Mutex<[...]>`, todo acceso via `.lock()`.
  - **Severidad:** ~~ALTA~~ COMPLETADO
  - **Tests:** `smp_nxl_concurrent_load`

- [x] **OBF-08. Migrar sys_waitpid a AtomicU32 (eliminar WAIT_PID static mut)** | Prereqs: CB1 | Files: `src/usermode.rs`, `src/syscall/mod.rs`
  - **Descripcion:** `WAIT_PID` migrado de `static mut` a `AtomicU32` con operaciones atómicas SeqCst en lugar de KWait. Suficiente para SMP-safety.
  - **Severidad:** ~~CRITICO~~ COMPLETADO
  - **Tests:** `smp_waitpid_concurrent`, `smp_waitpid_no_race`

### Fase 1: Maduracion (v0.40 - v0.45)

*Todos los items de la Fase 1 estan completados.*

1. ~~**v0.43** — SeAccessCheck NT-compatible, sys_poll(), Congelar pipe/IRP protocols~~ **COMPLETADO**
2. ~~**v0.44** — ASLR v1 (base aleatoria), Ob syscalls RAX 60-66~~ **COMPLETADO** (v0.44.2: Ob migration completa, todas las syscalls legacy desactivadas)
3. ~~**v0.45** — Ob migration, Device Tree + Resource Manager, Driver state machine freeze~~ **COMPLETADO** (Ob migration completada en v0.44.2; Device Tree y Resource Manager se mueven a v0.46)

### v0.48.7 (Registry config + Audits)

- [x] **B2.6. Registry defaults in boot** | Files: `src/main.rs`, `src/cm/mod.rs`
  - En Phase 3.881, crear `CurrentControlSet\Services\NeoInit\DefaultShell`,
    `Network\Interfaces\0\DHCPEnabled=1`, etc. Solo si no existen.
  - **Tests:** `cm_default_values_created`

- [x] **B4.10. NeoInit: leer Registry para config** | Files: `userbin/neoinit/`
  - NeoInit lee DefaultShell, AutoStartServices, EnableVT, WaitForNetwork desde
    `\Registry\Machine\System\CurrentControlSet\Services\NeoInit`.
  - **Tests:** boot con Registry, verificar shell spawn

- [x] **AUDIT-1. Registry info classes handled** | Files: `src/syscall/ob.rs`, `src/object/types.rs`
  - `ObInfoClass::RegistryKey (21)` y `::RegistryValue (22)` implementados.
  - `ObSetInfoClass::RegistryCreateKey (23)`, `::RegistryDeleteKey (24)`, `::RegistrySetValue (25)`, `::RegistryDeleteValue (26)` implementados.

- [x] **AUDIT-2. libneodos ObInfoClass/ObSetInfoClass sync** | Files: `libneodos/src/syscall.rs`
  - Añadidos 6 variantes faltantes a ObInfoClass. ObSetInfoClass convertido a enum con 27 variantes.
  - `sys_ob_set_info` ahora toma `ObSetInfoClass` en vez de `u32`.

- [x] **AUDIT-3. Dual mount systems (MAX_MOUNTS)** | Files: `src/fs/vfs.rs`
  - Renombrado `MAX_MOUNTS` a `MAX_SUBDIR_MOUNTS` en fs/vfs.rs para eliminar ambigüedad.

- [x] **AUDIT-4. DPC overflow handling + tests** | Files: `src/dpc/mod.rs`
  - Añadido `DPC_DROPPED_COUNT` global. 3 nuevos tests: queue_overflow, dispatch_pending_global_api.

- [x] **AUDIT-9. Kernel link address in docs** | Files: `docs/ARCHITECTURE.md`, `docs/memory.md`, `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`, `docs/DEBUG.md`
  - Corregido `0x200000`/`0x100000` → `0x4000000` en todas las referencias.

- [x] **AUDIT-10. ObSetInfoClass::Security implementado** | Files: `src/syscall/ob.rs`
  - Reemplazado `err_to_u64(NoSys)` por implementación funcional que parsea SD y llama `ob_set_security`.

- [x] **AUDIT-36. Docs: ARCHITECTURE.md HAL ABI self-contradiction** | Files: `docs/ARCHITECTURE.md:125 vs 178`
  - Line 125 now says `HAL ABI v0.4` (was `v0.3`), matching line 178.
  - **Tests:** (doc fix only)

- [x] **AUDIT-37. Docs: ARCHITECTURE.md kernel heap address wrong** | Files: `docs/ARCHITECTURE.md:581`
  - Changed `0x1000000` → `0x0240_0000` to match `src/memory/layout.rs:107`.
  - **Tests:** (doc fix only)

- [x] **AUDIT-38. Docs: ARCHITECTURE.md event type count stale** | Files: `docs/ARCHITECTURE.md:222-243`
  - Updated from "16 event types (0-15)" to "18 event types (0-17)". Added `EVENT_MOUSE_INPUT=16` and `EVENT_NETWORK_PACKET=17`.
  - **Tests:** (doc fix only)

- [x] **AUDIT-39. Docs: memory.md nxl_region address typo** | Files: `docs/memory.md:84`
  - Changed `0x1E00000` → `0x1E000000` to match actual layout.
  - **Tests:** (doc fix only)

- [x] **AUDIT-40. Docs: objects.md ObSetInfoClass count stale** | Files: `docs/objects.md:185-216`
  - Updated from "Supports 27 set classes" to "Supports 28 set classes". Added `SetNicIp = 27`.
  - **Tests:** (doc fix only)

- [x] **AUDIT-41. Docs: syscalls.md documents removed syscalls as active** | Files: `docs/syscalls.md` sections 5-13
  - RAX 5, 7, 8, 9, 10, 11 now marked as **REMOVED** with replacement guidance (Ob equivalents).
  - **Tests:** (doc fix only)

- [x] **AUDIT-42. Docs: ipc.md event struct field sizes wrong** | Files: `docs/ipc.md:180-213`
  - Event struct: `u16`→`u32` for `event_type`, `source`, `flags`; added `driver_target: u32`; corrected event type table values and added missing entries (RTC_READ=10, RTC_DATA=11, NMI_WATCHDOG=15, MOUSE_INPUT=16, NETWORK_PACKET=17).
  - **Tests:** (doc fix only)

- [x] **AUDIT-43. Docs: ipc.md pipe storage description stale** | Files: `docs/ipc.md:7`
  - "16 static pipe buffers" → dynamic `Vec<Option<Mutex<PipeInner>>>`.
  - **Tests:** (doc fix only)

- [x] **AUDIT-44. Docs: drivers.md 8-state vs 7-state lifecycle** | Files: `docs/drivers.md:94`
  - Verified: doc already correct (8 states including `Unloading`), code has 8 `DriverState` variants. No change needed.
  - **Tests:** (doc fix only)

- [x] **AUDIT-45. Docs: ARCHITECTURE.md references "MEM.NXE" binary renamed** | Files: `docs/ARCHITECTURE.md:118`
  - Updated MEM reference to point to `userbin/neomem/`.
  - **Tests:** (doc fix only)

- [x] **AUDIT-46. Docs: ARCHITECTURE.md ObType count stale** | Files: `docs/ARCHITECTURE.md:576`
  - Changed "ObType=17 variants" → "ObType=18 variants", added `Socket` to the list.
  - **Tests:** (doc fix only)

- [x] **AUDIT-53. CRC32 deduplicated to shared `fs/crc32.rs`** | Files: `src/fs/neodos_io.rs`, `src/fs/snapshot.rs`, `src/fs/freelist.rs`, `src/fs/btree.rs`
  - Four private `fn crc32` implementations and one `pub fn crc32` consolidated into a single shared module `src/fs/crc32.rs`. All callers now `use super::crc32::crc32`.
  - **Tests:** `crc32_single_implementation`

- [x] **AUDIT-53. Docs: filesystem.md page cache capacity stale** | Files: `docs/filesystem.md:209`
  - Changed "64 entries" → "128 entries" (matches `CACHE_SIZE = 128` in `page_cache.rs:3`).
  - **Tests:** (doc fix only)

- [x] **AUDIT-54. Docs: ARCHITECTURE.md test count stale (537 vs 656)** | Files: `docs/ARCHITECTURE.md:528,561`
  - Updated "537 tests" → "656 tests" in both locations.
  - **Tests:** (doc fix only)

- [x] **AUDIT-54. GPT parsing deduplicated** | Files: `src/drivers/gpt.rs`, `src/vfs/partition.rs`
  - `read_u64_le`/`read_u32_le`/`read_sector_from_dev` helpers and GPT partition loop logic consolidated into `drivers/gpt.rs` as `pub(crate)`. `vfs/partition.rs` now delegates to `gpt::parse_gpt_filter` and re-exports constants.
  - **Tests:** `gpt_parse_consistent`

- [x] **AUDIT-57. MODE_DIR/MODE_FILE constants deduplicated** | Files: `src/fs/vfs.rs:43-44`, `src/fs/neodos_dir.rs:26-27`
  - `MODE_DIR = 0x40` and `MODE_FILE = 0x80` removed from `neodos_dir.rs`; re-exported via `pub use super::vfs::{MODE_DIR, MODE_FILE}`.
  - **Tests:** (compile-only)

- [x] **AUDIT-5 / AUDIT-81. Dead code: processes.rs removed** | Files: `src/processes.rs`
  - `proc_a()`/`proc_b()`/`proc_c()`/`proc_d()` — 4 vestigial prototyping functions removed entirely. `processes.rs` deleted. Zero external references.
  - **Tests:** Remove, verify build

### v0.47 (Networking TCP/IP)

- [x] **v0.47. Networking: NIC driver NEM + TCP/IP stack** | Prereqs: — | Files: `src/net/`
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

```text
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
| ----- | --------- | ------ | ------------- |
| 60 | `sys_ob_open` | RBX=path, RCX=access | Open named object -> handle (SeAccessCheck integrado) |
| 61 | `sys_ob_create` | RBX=path, RCX=type, RDX=fds_out, R8=attrs | Create named object (Process=1, Driver=2, Pipe=4, Directory=11, Event=13) |
| 62 | `sys_ob_query_info` | RBX=fd, RCX=class, RDX=buf, R8=len | Query object metadata (0-16 classes, incl. ReadContent=15, VolumeLabel=16) |
| 63 | `sys_ob_set_info` | RBX=fd, RCX=class, RDX=buf | Set object metadata (0-9 classes, incl. WriteContent=7, SetCwd=8, SetVolumeLabel=9, VfsRename=6) |
| 64 | `sys_ob_enum` | RBX=dir_fd, RCX=buf, RDX=max | Enumerate directory (VFS-backed + Ob namespace) |
| 65 | `sys_ob_wait` | RBX=count, RCX=handles, RDX=type, R8=to | Wait on objects (multi-type via KWait) |
| 66 | `sys_ob_destroy` | RBX=fd | Destroy/delete object by fd (files, dirs, drivers, namespace objects) |

#### Syscalls Legacy Migrados a Ob

| RAX | Legacy | Estado SSDT | Equivalente Ob |
| ----- | -------- | ------------- | ---------------- |
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
| ----- | --------- | -------- |
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
| 42 | `sys_poweroff` | ⏩ **MIGRATED** to Ob: PowerManager object at `\System\PowerManager`. Use `sys_ob_open` + `sys_ob_set_info(PowerShutdown/Reboot)`. |
| 47 | `sys_chdir_parent` | Foundation: parent cwd change |
| 50 | `sys_ndreg` | Internal: driver registry admin |
| 53 | `sys_cursor_blink` | Foundation: cursor control |
| 55 | `sys_fsck` | Foundation: filesystem check |
| 58 | `sys_driver_unload` | Foundation: driver unload |
| 59 | `sys_poll` | Foundation: I/O polling |

#### Metricas Objetivo Alcanzadas

| Metrica | Antes (v0.40) | Despues (v0.44.2) |
| --------- | --------------- | ------------------- |
| HandleEntry tipo-seguro | [PENDING] (kind hardcoded) | [DONE] (ObId ref) |
| KOBJ + handles unificados | [PENDING] | [DONE] |
| Security en open | [PENDING] (solo syscall 50) | [DONE] (todo acceso via SeAccessCheck) |
| URN funcional | Parcial (file + device) | Full (all schemes via Ob) |
| Tipos de objeto | ~8 implicitos | 16 explicitos (ObType enum) |
| Syscalls Ob | 0 | 7 nuevas (RAX 60-66) |
| OB_NAME_LEN | 32 | 128 |

#### Estado por Binario

| Binario | Estado Ob | Syscalls Ob | Syscalls Legacy Restantes |
| --------- | ----------- | ------------- | -------------------------- |
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

- [x] **OBF-01. Anadir ObInfoClass::ReadContent=15, VolumeLabel=16 al enum** | Prereqs: — | Files: `src/object/types.rs`
  - **Severidad:** BAJA — 5 min
  - **Tests:** (cobertura de compilacion)

- [x] **OBF-02. Anadir ObSetInfoClass::ProcessTerminate=4, VfsRename=6, WriteContent=7, SetCwd=8, SetVolumeLabel=9 al enum** | Prereqs: — | Files: `src/object/types.rs`
  - **Severidad:** BAJA — 5 min
  - **Tests:** (cobertura de compilacion)

- [x] **OBF-03. Anadir ObType::Thread = 16 al enum + to_str()** | Prereqs: — | Files: `src/object/types.rs`
  - **Severidad:** BAJA — 5 min
  - **Tests:** `ob_type_thread_enum`

- [x] **OBF-04. Implementar ob_create(Thread) en handler_ob_create** | Prereqs: OBF-03 | Files: `src/syscall/mod.rs`
  - **Descripcion:** Crea KTHREAD, devuelve fd
  - **Severidad:** MEDIA — 2-3h
  - **Tests:** `ob_thread_create_and_destroy`

- [x] **OBF-05. Implementar ob_wait(Thread) en handler_ob_wait** | Prereqs: OBF-03 | Files: `src/syscall/mod.rs`
  - **Descripcion:** kwait_block(ThreadJoin)
  - **Severidad:** MEDIA — 1h
  - **Tests:** `ob_thread_join`

- [x] **OBF-06. Implementar ob_set_info(ThreadPriority) usando fd thread** | Prereqs: OBF-03 | Files: `src/syscall/mod.rs`
  - **Severidad:** BAJA — 30 min
  - **Tests:** `ob_thread_priority`

- [x] **OBF-06b. Eliminar handler_thread_create (RAX 22) y handler_thread_join (RAX 23) del SSDT** | Prereqs: OBF-04, OBF-05 | Files: `src/syscall/mod.rs`
  - **Severidad:** BAJA — 5 min
  - **Tests:** (verificar SSDT None)

- [x] **OBF-09. Tests kernel: 8 tests (thread create/wait/kill via Ob, enum completos, error unificado)** | Prereqs: OBF-01..08 | Files: `src/testing.rs`
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

- ~~**[COMPLETED] AI-5. Libneodos-nxl ya modularizado** | Prereqs: — | Files: `libneodos-nxl/src/`~~
  - ~~**Descripcion:** `libneodos-nxl/src/` ya usa modulos separados (`syscall.rs`, `io.rs`, `fs.rs`, `process.rs`, `mem.rs`, `error.rs`). Con la limpieza ABI v7, se eliminaron las funciones `nxl_sys_pipe/dup2/waitpid/chdir/chdir_parent/readdir` (process.rs) y `nxl_sys_mkdir/unlink/rmdir/rename/writefile` (fs.rs). No requiere mas reorganizacion.~~

### Fase 2 Ob: Timer, Semaphore, Section [COMPLETADO]

~~Requieren nuevos tipos en el Object Manager y extensión de las syscalls Ob existentes.~~

| ID | Tarea | Estado | Syscalls |
| ---- | ------- | -------- | ---------- |
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

- [x] **VFS-1.1. Unificar MountManager** — COMPLETADO en v0.47.1
- [x] **VFS-1.2. Arreglar ownership ObOpen → VFS** — COMPLETADO en v0.48.0
- [x] **VFS-1.3. Eliminar stale namespace entries** — COMPLETADO en v0.48.1: ob_remove_by_id(), cleanup en destroy/close
- [x] **VFS-1.4. HandleTable → ObObject consistency** — COMPLETADO en v0.48.1: is_valid(), close() guardado, has_ob_object() bugfix
- [x] **v0.48. NeoFS estabilidad** — COMPLETADO en v0.48.2: FS-1.1/1.2/1.3 (dynamic allocators, sector offsets), NS-1.1/1.2 (ownership, protected dirs), CAP_NS_WRITE
- [x] **VFS-2.1. Privatizar métodos de NeoFS** — COMPLETADO en v0.48.3: 5 métodos pub→pub(crate)
- [x] **VFS-2.4. PageCache con contexto de drive** — COMPLETADO en v0.48.3: drive_id en clave PageCache
- [x] **VFS-4.1. Device IDs estables** — COMPLETADO en v0.48.4: register escanea slots libres (índices estables), find_by_name()
- [x] **VFS-4.2. Hot-unload safety** — COMPLETADO en v0.48.4: IoStack.stale flag, operaciones fallan en stale
- [x] **VFS-4.3. Refcount de block devices** — COMPLETADO en v0.48.4: refcounts[], acquire/release, remove() protegido
- [x] **OBF-07. Unificar ObError y SyscallError** — COMPLETADO: ob_err_to_syscall() + test
- [x] **B3.3 D8. DHCP client** — COMPLETADO en v0.48.5: dhcp.rs con Discover/Offer/Request/Ack, arranque automático
- [x] **B2.1 Z6. Registry hive database** — COMPLETADO en v0.48.0
- [x] **B2.7. Registry disk persistence (cm_flush_key)** — COMPLETADO en v0.48.6: NEOH serialization format, VFS file `C:\System\Registry\<name>.hiv`, dirty tracking, flush on shutdown. Tests: `cm_set_value_persist_roundtrip`, `cm_hive_serialization_integrity`
- [x] **NET-1 F1-F4** — COMPLETADO en v0.48.5: Ethernet/UDP/ARP builders, ICMP Port Unreachable, socket_send, UDP/TCP dispatch, TCP three-way handshake real

### NET-1.5..NET-1.15: Networking userland [COMPLETED]

- [x] **NET-1.5. libneodos: SOCKET constants + wrappers** | Files: `libneodos/src/syscall.rs`
  - Añadido `ob_type::SOCKET = 18`, `ObInfoClass::SocketRecv = 23`, y wrappers `ob_socket_create/connect/bind/listen/send/recv/close`.
  - Añadido `SocketAddrV4` struct, `sys_cm_set_value` (RAX=70) con macro `ob_syscall_5!`.

- [x] **NET-1.6. Kernel: ObInfoClass::SocketRecv (class 23)** | Files: `src/object/types.rs`, `src/syscall/ob.rs`
  - Handler en `ob_query_info` copia `socket.recv_buf` a usuario; si vacío retorna `-EAGAIN`.
  - Tests: `net_socket_recv_data`, `net_socket_recv_empty`.

- [x] **NET-1.8. net.nxl: userland network library** | Files: `libnet/` (new), `libnet-nxl/` (new)
  - NXL slot 3 (`0x1e0c0000`). API (16 funciones): `iface_count/info/stats`, `socket_create/bind/connect/listen/send/recv/close`, `set_ip/get_ip/get_gateway/get_mask/get_dhcp_bound`.
  - `libnet/` — static library wrapper con lazy loading via `loadlib`.
  - `sys_cm_set_value` añadido a libneodos.

- [x] **NET-1.15. netcfg.nxe: network service** | Files: `userbin/netcfg/` (new)
  - Servicio auto-iniciado por NeoInit. Lee Registry (`DHCPEnabled`), aplica IP estática o espera DHCP del kernel.
  - Si DHCP falla, asigna APIPA (169.254.1.1). Corre como daemon.
  - `ObSetInfoClass::SetNicIp = 27` para aplicar IP desde userspace.
  - Incluido en imagen NeoFS via `scripts/create_neodos_image.py`.

### OB-FIX-001: Ob Socket object_id perdido en operaciones socket [COMPLETED]

- [x] **Causa raíz:** `SocketConnect` y el primer brazo duplicado de `SocketBind` usaban `entry.offset` para obtener el `socket_id`. Durante la creación del socket, `HandleEntry::ob_object(ob_id, socket_id)` recibe `socket_id` como `_access_mask`, que es **descartado** en el cuerpo de la función (siempre fija `offset: 0`). Por tanto, `entry.offset` siempre es 0 para sockets → `socket_id == 0` → `BadF`.

  **Adicional:** Existían dos brazos `SocketBind` con la misma guarda (`info_class == SocketBind`). El primero (buggy, offset) matcheaba antes que el segundo (correcto, `ob_lookup`), dejando el brazo correcto como código muerto.

- [x] **Análisis:**
  1. `handler_ob_set_info` en `syscall/ob.rs:2070-2088`: Primer brazo `SocketBind` que usaba `entry.offset` como socket_id. **Eliminado.**
  2. `handler_ob_set_info` en `syscall/ob.rs:2051-2068`: `SocketConnect` que usaba `entry.offset` como socket_id. **Corregido** para usar `ob_lookup(entry.object_id).native_id`.
  3. Todos los demás brazos del mismo handler (`SocketListen`, `SocketSend`, `SocketClose`, `SocketRecv`) ya usaban correctamente `ob_lookup`.
  4. El `SetNicIp` handler ignoraba la máscara de subred pasada en el buffer. **Corregido** para también fijar la máscara vía `nic_set_mask`.
  5. `socket_send` para UDP sólo acumulaba datos en `send_buf` sin transmitirlos. **Corregido** para transmitir directamente usando `socket_send_udp_raw` (extrae `local`/`remote`, suelta el lock de SOCKET_MANAGER para evitar inversión de locks con NIC_REGISTRY, construye y envía el datagrama UDP).
  6. Añadido `nic_set_mask` y `nic_get_mask` al `NicRegistry` para persistir la máscara de subred.

- [x] **Archivos modificados:**
  - `neodos-kernel/src/syscall/ob.rs`: SocketConnect corregido, brazo SocketBind duplicado eliminado, SetNicIp actualizado
  - `neodos-kernel/src/net/socket.rs`: `socket_send` ahora transmite UDP en lugar de bufferizar; `socket_send_udp_raw` añadida
  - `neodos-kernel/src/net/nic.rs`: Añadido campo `mask` a `NicSlot`, funciones `get_mask`/`set_mask`
  - `userbin/dhcpd/src/main.rs`: Habilitado flujo DORA (crea socket UDP, bind a puerto 68, connect a broadcast 255.255.255.255:67, ejecuta `DhcpClient::run()`)

- [x] **Tests:** 641/641 kernel tests pasan. Ningún otro tipo de objeto (file, process, event, mutex, timer, semaphore, section, pipe, etc.) se ve afectado porque todos usan `ob_lookup(entry.object_id).native_id` o `entry.offset` solo como posición de lectura/escritura para archivos.

### NFSv2-FSCK: fsck para NE2 [COMPLETED]

- [x] **NFSv2-FSCK. fsck para formato NE2** | Prereqs: NFSv2-FILESYSTEM | Files: `src/fs/fsck.rs`
  - Verificar checksum del superblock. Walk completo del B-tree verificando CRC32 de cada nodo.
  - Verificar que freelist + used_blocks = total_blocks.
  - Modo repair: reconstruir freelist desde B-tree walk.
  - Syscall 55 (`sys_fsck`) actualizada: llama `fs.fsck()` vía VFS, copia `FsckStatsRaw` a buffer usuario.
  - Bug corregido en `mkfs_ne2`: escribía nodo raíz en sectores 1–8 en vez de 8–15.
  - **Tests:** `neofs_v2_fsck_clean`, `neofs_v2_fsck_corrupt_btree` (ambos PASS).
  - Dependencias: `check_deps.py` — 0 violaciones.

### AUDIT-66..71: Documentación corregida [COMPLETED]

- [x] **AUDIT-66. ARCHITECTURE_SOURCE_OF_TRUTH.md Event struct layout wrong** | Files: `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md:379-390`
  - Documents Event as `source: u8`, `timestamp: u32`, `flags: u16` with no `driver_target` field. Actual code has `source: EventSource` (u32), `timestamp: u64`, `flags: u32`, plus `driver_target: u32` present. Corregido para coincidir con el código.
  - **Tests:** (docs fix only)

- [x] **AUDIT-67. boot.md KERNEL_VERSION_CODE at v0.10.5** | Files: `docs/boot.md:100`
  - `KERNEL_VERSION_CODE = (10 << 8) | 5 = 0x0A05` correspondía a v0.10.5. Actualizado a `(49 << 8) | 0 = 0x3100` (v0.49.0).
  - **Tests:** (docs fix only)

- [x] **AUDIT-68. roadmap.md version says v0.48** | Files: `docs/roadmap.md:3`
  - "Current: **v0.48**" cambiado a **v0.49.0**.
  - **Tests:** (docs fix only)

- [x] **AUDIT-69. Test count outdated in testing.md and ARCHITECTURAL_VISION.md** | Files: `docs/testing.md:5`, `docs/ARCHITECTURAL_VISION.md:96,778`
  - `testing.md` y `ARCHITECTURAL_VISION.md` decían "537 tests". Actualizado a **656**.
  - **Tests:** (docs fix only)

- [x] **AUDIT-70. filesystem.md structs missing checksum/version fields** | Files: `docs/filesystem.md:13-79`
  - Superblock ya tenía `version: u32`. Eliminada referencia a `BLOCK_CACHE` (eliminado en v0.49 VFS-5.1). Documentación de cache layers actualizada.
  - **Tests:** (docs fix only)

- [x] **AUDIT-71. syscalls.md missing Socket and Registry info classes** | Files: `docs/syscalls.md:284-296`
  - `sys_ob_query_info` ampliado de 2 clases a 24 (0-23). `sys_ob_set_info` ampliado de 11 clases a 28 (0-27). `sys_ob_create` ahora incluye `Socket=18`.
  - **Tests:** (docs fix only)

### AUDIT-23..29 + DH1: Documentación corregida [COMPLETED]

- [x] **AUDIT-23. NEM v3 header docs contradict code** | Files: `docs/ARCHITECTURE.md`, `docs/drivers.md`, `src/nem/mod.rs`
  - Fixed offset table (added padding row at 26, corrected all subsequent offsets). Rewrote `drivers.md` table to match actual `NemHeaderV3` struct.
- [x] **AUDIT-24. libneodos.md: syscall instruction vs int 0x80** | Files: `docs/libneodos.md`
  - Changed "syscall instruction" → "int 0x80".
- [x] **AUDIT-25. libneodos.md: user.ld base addr wrong** | Files: `docs/libneodos.md`
  - Changed "placing code at 0x400000" → "linking at address 0; runtime loads at 0x400000".
- [x] **AUDIT-26. scheduler.md: CpuRunQueue field names wrong** | Files: `docs/scheduler.md`
  - Fixed field names (head/tail → head_idx/tail_idx), added missing `count: u16`.
- [x] **AUDIT-27. objects.md: SocketRecv class 23 (re-check)** | Files: `docs/objects.md`
  - Already correct — SocketRecv=23 consistent everywhere.
- [x] **AUDIT-28. memory.md: kernel_image base wrong** | Files: `docs/memory.md`
  - Already fixed in prior audit.
- [x] **AUDIT-29. Version mismatch AGENTS/Cargo/CHANGELOG** | Files: `AGENTS.md`, `neodos-kernel/Cargo.toml`
  - Fixed: `Cargo.toml` bumped from 0.48.0 → 0.49.0.
- [x] **DH1. Actualizar README.md** | Files: `README.md`
  - Updated version badge to v0.49.0, test count to 656.

---

### SM-001. Service Manager [COMPLETED]

- [x] **SM-001. Service Manager (kernel)** | Prereqs: CM-FIX | Files: `src/services/` (new), `src/object/types.rs`, `src/syscall/mod.rs`, `src/syscall/ob.rs`, `src/globals.rs`, `src/main.rs`, `libneodos/src/syscall.rs`, `scripts/gen_system_hiv.py`
  - `ObType::Service = 20` en `src/object/types.rs`
  - 3 nuevos `ObInfoClass` (29=ServiceState, 30=ServiceConfig, 31=ServiceStatus)
  - 4 nuevos `ObSetInfoClass` (33=ServiceStart, 34=ServiceStop, 35=ServiceRestart, 36=ServiceSetConfig)
  - RAX 77 `sys_ob_service` en SSDT (handler_ob_service, admin-only)
  - `SERVICE_MANAGER: Mutex<ServiceManager>` global
  - Máquina de 5 estados (Stopped→Starting→Running→Stopping→Failed) con restart policy (Never/OnCrash/Always)
  - Dependencias entre servicios con orden topológico (Kahn)
  - `\Service\<Name>` en namespace Ob
  - Backend Registry: `\Registry\Machine\System\CurrentControlSet\Services\<Name>`
  - `sm_init()` en Phase 3.882, `sm_start_auto_services()` en Phase 4
  - Default Dhcpd service en `scripts/gen_system_hiv.py` (Auto, OnCrash, MaxFailures=3)
  - 22 tests unitarios (state machine, dependencies, registry backend, error codes, process exit handling)

### PM-PHASE1. HAL ACPI primitives [COMPLETED]

- [x] **PM-PHASE1. HAL ACPI reboot/FADT/S5 primitives** | Files: `src/hal/x64/cpu.rs`, `src/power/acpi.rs`, `src/hal/x64/mod.rs`, `src/hal/mod.rs`
  - `reboot()`: ACPI reset register → 0xCF9 → PS/2 fallback chain.
  - `acpi_parse_fadt()`: RSDP → RSDT/XSDT → FADT. PM1a/b, S5 sleep type, reset register.
  - `acpi_s5_write()`: SLP_TYPa + SLP_EN to PM1a control register.
  - `poweroff()`: ACPI S5 → QEMU debug ports → PS/2 fallback chain.
  - `src/power/acpi.rs`: stores `AcpiPowerState`, public API.
  - 7 tests: FADT parsing, reset register, S5 write.

### PM-PHASE2. Power Manager kernel core [COMPLETED] (v0.50.1)

- [x] **PM-PHASE2. Power Manager kernel core** | Files: `src/power/mod.rs`, `src/power/plan.rs`, `src/power/coordinator.rs`, `src/object/types.rs`, `scripts/gen_system_hiv.py`, `src/main.rs`
  - `PowerManager` struct con `POWER_MANAGER: Mutex<PowerManager>` global.
  - `PowerPlan`, `PowerPolicies`, `CpuPolicy`, `PowerAction` data structures.
  - `load_plan_from_registry()` / `save_plan_to_registry()` — Registry persistence.
  - `power::coordinator::shutdown()` / `reboot()` — HAL chain + hive flush + event bus.
  - `ObType::PowerManager = 21` en `src/object/types.rs`.
  - PHASE 3.883: `power::init_power_manager()` en `src/main.rs`.
  - Power defaults en `scripts/gen_system_hiv.py` (3 planes, 51 cells).
  - 12 tests unitarios kernel-side.

---

### KBD-PHASE1. NeoKBD kernel module [COMPLETED]

- [x] **KBD-PHASE1. NeoKBD kernel module + ObType + API** | Files: `src/kbd/`, `src/object/types.rs`, `src/syscall/ob.rs`, `src/eventbus/mod.rs`, `src/main.rs`, `src/cm/mod.rs`, `libneodos/src/keyboard.rs`, `docs/design/neokbd-design.md`
  - `ObType::KeyboardDevice = 22`, `ObInfoClass` 35-37, `ObSetInfoClass` 43-47.
  - 5 new Event Bus types (27-31): KEYDOWN, KEYUP, KEY_CHAR, KBD_MODIFIER, KBD_REPEAT.
  - `NeoKbd` struct with state, config, layouts, modifiers, dead key engine.
  - `kbd_init()`: PHASE 3.875, scans `C:\System\Keyboard\*.kbd`, loads config from Registry.
  - Layout engine: `KbdLayout` with `[KeyEntry; 256]` + compose table, Unicode mapping.
  - Hotkey dispatch: Ctrl+Alt+Del → poweroff, Alt+F1-F8 → VT switch.
  - Registry defaults: Layout, RepeatDelay, RepeatRate, NumLockOnBoot, CapsLockOnBoot.
  - `libneodos/src/keyboard.rs`: user-mode API with wrappers.

---

### KBD-PHASE2. ps2kbd simplification + kbdcompile [COMPLETED]

- [x] **KBD-PHASE2. ps2kbd driver simplification + .kbd format tool** | Files: `drivers/ps2kbd/`, `tools/kbdcompile/`, `data/keyboard/`
  - ps2kbd simplified: removed layout tables, translate_scancode, dead key logic (~150 lines).
  - `tools/kbdcompile/`: .klc → .kbd compiler. Supports US and Spanish layouts.
  - `.kbd` binary format: magic + version + name + lang_tag + 256 key entries + compose table.
  - Layout files in `data/keyboard/`: KBDUS.klc, KBDSP.klc, US.kbd, Spanish.kbd.

---

### ADM-NEOKEY. neokey CLI utility [COMPLETED]

- [x] **ADM-NEOKEY. neokey CLI utility** | Files: `userbin/neokey/`
  - Replaces `keyb.nxe`: `NEOKEY show/layout/layouts/repeat/delay/leds`.
  - Uses `libneodos::keyboard::*` API.
  - Integrated into disk image via `neodev/src/image.rs`.

### SH-TOKEN+QUOTE. Shell tokenizer + quoting [COMPLETED]

- [x] **SH-TOKEN+QUOTE. Shell tokenizer + quoting** | Prereqs: -- | Files: `userbin/neoshell/src/tokenizer.rs`
  - State machine para pipes, redirects, quoting. `"..."` (expande %VAR%), `'...'` (literal), `^` escape.
  - **Tests:** `tokenizer_pipe`, `tokenizer_redirect`, `tokenizer_quoted_arg`, `tokenizer_double_quotes`, `tokenizer_escape_char`, `tokenizer_semicolon`, `tokenizer_unmatched_double_quote`, `tokenizer_empty`, `tokenizer_escape_in_double_quote`, `tokenizer_multiple_spaces`

### I18N-P1. Runtime i18n NLTv2 + IDs numéricos [COMPLETED]

- [x] **I18N-P1. Runtime i18n NLTv2 (nunca NLTv1)** | Files: `libneodos/src/i18n.rs`, `libneodos/src/macros.rs`, `neodos-kernel/src/cm/init.rs`
  - Runtime reescrito: solo NLTv2 (formato binario con IDs numéricos u32).
  - Formato: magic `NLT2`, version=2, header 32 bytes, LanguageID, ApplicationID, Flags, CRC32.
  - Búsqueda binaria O(log n) sobre índice ordenado por ID.
  - Nuevas APIs: `i18n_get_id()`, `i18n_try_get_id()`, `i18n_unload()`, `i18n_reload_all()`, `i18n_active_locale()`.
  - `tr!()`: ahora es no-op (devuelve el literal). `tr_id!()`: nueva macro para IDs numéricos.
  - Eliminadas: `i18n_get()`, `try_get()` (string-key). Sin compatibilidad con NLTv1.

### I18N-P2. Migrar apps core a tr_id!() [PENDING]

- [ ] **I18N-P2. Migrar apps core a tr_id!()** | Prereqs: I18N-P1 | Files: `userbin/neoshell/`, `userbin/neoinit/`, `userbin/corehelp/`, `userbin/coredir/`, `userbin/corecopy/`, `userbin/kill/`, `userbin/ps/`
  - PENDIENTE: todas las apps existentes. `tr!()` ahora es no-op (devuelve literal).
  - Las apps deben migrar a `tr_id!(IDS_CONSTANT)` con constantes numéricas.

### I18N-P3a. Compilador nltc [COMPLETED]

- [x] **I18N-P3a. Compilador nltc** | Files: `tools/nltc/` (new)
  - `nltc`: compilador TOML → NLTv2 binario.
  - Subcomandos: compile, --check, --generate-ids, --generate-rust, --scaffold, --list-langs, --lang-id, --app-id, --info, --generate-all.
  - Validación: sintaxis TOML, [meta] requerido, IDs duplicados, CRC32.
  - Tablas de LanguageID (25 estándar) y ApplicationID (40 estándar).
  - `--generate-rust`: genera `pub const IDS_XXX: u32 = N;` para apps Rust.

### I18N-P3b. Fuentes TOML + NLTv2 binarios [COMPLETED]

- [x] **I18N-P3b. Fuentes TOML y binarios NLTv2** | Files: `data/locale/*/*.toml`, `data/locale/*/*.nlt`, `scripts/gen_nlt_toml.py`
  - 14 fuentes TOML (7 en-US + 7 es-ES) con IDs numéricos.
  - `scripts/gen_nlt_toml.py`: genera TOML desde tablas de traducción.
  - Compilados a NLTv2 via `nltc --generate-all`.
  - Apps: corehelp, coredir, corecopy, coretype, neoshell, neoinit, neolocale.

### I18N-P3c. neolocale tool (NLTv2) [COMPLETED]

- [x] **I18N-P3c. neolocale tool (NLTv2)** | Files: `userbin/neolocale/src/main.rs`
  - `neolocale` actualizado: solo soporta NLTv2.
  - validate: verifica magic, version, CRC32, IDs duplicados, ordenación.
  - stats: tamaño, entradas, idioma, app ID.
  - diff: comparación ID por ID entre dos archivos NLT.
  - create: scaffold NLTv2 vacío.
  - check: búsqueda de traducciones faltantes entre locales.

### I18N-P3d. Integración NeoDev + disco [COMPLETED]

- [x] **I18N-P3d. Integración NeoDev** | Files: `tools/neodev/src/build.rs`, `tools/neodev/src/main.rs`
  - `compile_nlt_files()` en build.rs: compila todos los .toml → .nlt antes del build completo.
  - Hook en `cmd_image()` y `build_all()` para compilar NLTs antes de generar imagen.
  - `image.rs` ya escanea `data/locale/` para incluir .nlt en `/System/Locale/`.
  - `docs/nlt.md`: documentación completa del sistema NLT.

---

### SH-REDIR. Shell redirection [COMPLETED]

- [x] **SH-REDIR. Shell redirection (>, <, >>, 2>)** | Prereqs: SH-TOKEN+QUOTE | Files: `userbin/neoshell/src/redir.rs`, `userbin/neoshell/src/tokenizer.rs`
  - Tokenizer parsea `>`, `>>`, `<`, `2>`. Antes del spawn: abrir archivo target via `ob_open`/`ob_create`, `dup2` sobre el fd, spawn.
  - **Tests:** `redirect_stdout_to_file`, `redirect_stdin_from_file`, `redirect_append`, `redirect_stderr`, `redirect_file_not_found`, `redirect_permission_denied`

---

### NET-1.7. Kernel: nic_id + ephemeral port [COMPLETED]

- [x] **NET-1.7. Kernel: nic_id + ephemeral port** | Prereqs: NET-1 F4 | Files: `src/syscall/ob.rs`, `src/net/socket.rs`
  - Asignar NIC por defecto y puerto efímero (49152-65535) si no especificado.
  - **Tests:** `socket_auto_port_assign`

---

### NET-E1000-NEM-REGRESSION. Regresion de red en e1000.nem [COMPLETED]

- [x] **NET-E1000-NEM-REGRESSION. Fix e1000.nem DHCP/ARP regression** | Prereqs: -- | Files: `drivers/e1000/src/lib.rs`, `neodos-kernel/src/net/mod.rs`, `neodos-kernel/src/net/e1000.rs`
  - **Causa raiz:** `probe_e1000()` llamaba a `init_e1000_hw(mmio)` antes de establecer `MMIO_BASE`. Todos los registros (RCTL, TCTL, RDBAL, TDBAL, IMS) se escribian a direccion 0x0 en vez del BAR MMIO del e1000. El hardware nunca se inicializaba.
  - **Fix:** `init_e1000_hw()` ahora establece `MMIO_BASE` al inicio antes de cualquier `write_reg`. Verificacion de retorno de `hst_virt_to_phys()` (no zero). Memory fences (`Release`) antes de doorbell TX/RX.
  - **Migracion:** Codigo kernel e1000 (`neodos-kernel/src/net/e1000.rs`) eliminado completamente. Solo `e1000.nem` gestiona el hardware. DHCP habilitado en SYSTEM hive.
  - **Doc:** `docs/network.md`, `docs/ARCHITECTURE.md`, `docs/boot.md` actualizados.

---

## Referencias

- [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md) — invariantes MUST/MUST NOT
- [skills/build/SKILL.md](../skills/build/SKILL.md) — build, test, convenciones de commit
- [AGENTS.md](../AGENTS.md) — permanent rules (minimal)
- [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) — vision a largo plazo v0.40 -> v1.0
- [OBJECT_MANAGER_ARCHITECTURE.md](OBJECT_MANAGER_ARCHITECTURE.md) — diseno completo del Object Manager
- [Boot](boot.md) — boot sequence, fases, GPT layout
