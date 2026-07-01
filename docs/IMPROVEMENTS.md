# NeoDOS — Roadmap Pendiente

> Items pendientes del roadmap. Los completados están en
> [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md).

> Version actual: v0.47 (Networking TCP/IP completado — e1000 NIC, TCP/IP stack, \Device\Tcp/\Device\Udp, ICMP ping, 17 tests).
> Objetivo: v1.0 — executive NT-like arquitectonicamente solido.
> **GUIA:** Leer [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) antes de planificar cualquier cambio.
> Fuente de verdad arquitectonica: [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md)

**Proximo milestone: v0.48** (NeoFS estabilidad — namespace ownership, dynamic allocators).

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


## PRIORITY OVERVIEW

| ID | Item | Priority |
|----|------|----------|
| VFS-1.1 | Unificar MountManager | CRITICAL |
| VFS-1.2 | Arreglar ownership ObOpen → VFS | CRITICAL |
| VFS-1.3 | ~~Eliminar stale namespace entries~~ | CRITICAL ✅ |
| VFS-1.4 | ~~HandleTable → ObObject consistency~~ | CRITICAL ✅ |
| v0.48 | NeoFS estabilidad (milestone) | HIGH |
| VFS-2.1 | Privatizar métodos de NeoFS | HIGH |
| VFS-2.4 | PageCache con contexto de drive | HIGH |
| VFS-4.1 | Device IDs estables | HIGH |
| VFS-4.2 | Hot-unload safety | HIGH |
| VFS-4.3 | Refcount de block devices | HIGH |
| OBF-07 | Unificar ObError y SyscallError en NeoDosError | HIGH |
| A5.2 | VirtIO block driver (BOOT_DRIVER) | HIGH |
| B3.3 | DHCP client | HIGH |
| B2.1 | Registry hive database | HIGH |
| DH3 | Completar libneodos syscall wrappers | HIGH |
| NS-1..4, FS-1..6 | NeoFS namespace + filesystem fixes (see REFERENCE) | HIGH |
| v0.49 | NeoFS robustez (milestone) | MEDIUM |
| VFS-3.1 | Separar \Global\FileSystem del Ob namespace | MEDIUM |
| VFS-3.3 | Proteger paths del namespace | MEDIUM |
| VFS-5.1 | Unificar BlockCache + PageCache | MEDIUM |
| VFS-5.2 | InodeCache con invalidación | MEDIUM |
| VFS-2.2 | Refactorizar FSCK | MEDIUM |
| VFS-2.3 | Eliminar acceso directo a NeoFS desde shell | MEDIUM |
| AI-1 | Completar ObInfoClass/ObSetInfoClass enums | MEDIUM |
| AI-4 | Arreglar TOCTOU race en kobj_register | MEDIUM |
| B1.1 | Kernel tracing infrastructure | MEDIUM |
| B1.2 | NeoTrace system | MEDIUM |
| B3.4 | NTP client | MEDIUM |
| B4.3 | Shell redirection (>, <, >>) | MEDIUM |
| B4.6 | NeoEdit text editor | MEDIUM |
| B4.7 | Shared library per-process binding | MEDIUM |
| B4.9 | NeoShell scripting (.BAT) | MEDIUM |
| B5.1 | Module signature validation | MEDIUM |
| B5.2 | Driver permission enforcement | MEDIUM |
| B5.3 | Secure boot chain | MEDIUM |
| DH1 | Actualizar README.md | MEDIUM |
| DH2 | Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md | MEDIUM |
| A3.2 | Kernel debugger (KD) | MEDIUM |
| USR-001..024 | USR Fase 1+2: SAM + Login + SUDO (see REFERENCE) | MEDIUM |
| v0.50 | Async I/O y Registry (milestone) | LOW |
| v0.51 | ASLR v2 y Benchmarking | LOW |
| v0.52 | Networking completo (UDP, DNS, DHCP) | LOW |
| v0.53 | Rendimiento (zero-copy pipes, COW fork) | LOW |
| v0.54-v0.59 | Documentacion y Hardening | LOW |
| v1.0.0 | API estable | LOW |
| v0.46 | Device Tree + Resource Manager | LOW |
| VFS-3.2 | \DosDevices dinámico | LOW |
| VFS-5.3 | Write-back ordenado | LOW |
| VFS-6.1..6.4 | VFS Features (overlay, attr, notifications, async) | LOW |
| VFS-7.1..7.3 | VFS Performance (lock, lookup cache, path cache) | LOW |
| B6.1 | Zero-copy pipes | LOW |
| B6.2 | Copy-on-write fork | LOW |
| B4.8 | NeoTOP (v0.1 exists, v0.2+) | LOW |
| B4.10 | Compositor 2D | LOW |
| B7.1..B7.6 | Experimental (GUI, TPM, package mgr, TT debug, hotpatch, DFS) | LOW |
| AI-2 | Consolidate legacy syscall wrappers | LOW |
| AI-3 | ObObjectTable lock granularity | LOW |
| B2.2..B2.5 | Registry features (journal, multi-hive, security, notification) | LOW |
| USR-025..032 | USR Fase 3: Hardening + Grupos (see REFERENCE) | LOW |

---

## CRITICAL (next sprint — data corruption risks)

### VFS ownership & mount manager risks

* [ ] **VFS-1.1. Unificar MountManager** | Prereqs: — | Files: `src/vfs/mount.rs`, `src/fs/vfs.rs`, `src/main.rs`
  - **Descripcion:** Fusionar `Vfs::mount()` con `vfs::mount::vfs_mount()`. Un solo punto de mount/unmount que sincronice Vfs.drives[] + MountPoint creation + \DosDevices symlinks. Actualmente `main.rs` llama a ambos para el mismo mount — eliminar la duplicación.
  - **Severidad:** CRITICO — dos tablas de montaje independientes sin validación cruzada
  - **Tests:** `vfs_mount_dual_sync`, `vfs_mount_unmount_removes_both`

* [ ] **VFS-1.2. Arreglar ownership ObOpen → VFS** | Prereqs: — | Files: `src/object/mod.rs`, `src/handle.rs`
  - **Descripcion:** No crear ObObject persistente en namespace para cada `ob_open_path()`. Usar ObObject efímero (sin namespace entry) para file handles, o crear un "file object" real con ObOperations cuyo `on_destroy` cierre el archivo en el FS subyacente.
  - **Severidad:** CRITICO — namespace entries huérfanos, sin callback de cleanup
  - **Tests:** `vfs_ownership_obid_valid_after_close`, `vfs_ownership_namespace_entry_cleanup`

* [x] **VFS-1.3. Eliminar stale namespace entries** | Prereqs: VFS-1.2 | Files: `src/object/mod.rs`, `src/object/namespace.rs`
  - **Descripcion:** Añadida `ob_remove_by_id()` en namespace que busca y elimina entries por ObId. `ob_destroy_object()` y `ob_close_object()` llaman `ob_remove_by_id()` al destruir. El parche reactivo (línea 336-338 de `object/mod.rs`) queda como safety net para casos extremos.
  - **Severidad:** ALTA — namespace inconsistente ✅
  - **Tests:** `vfs_namespace_cleanup_on_destroy`, `vfs_namespace_cleanup_on_close`, `vfs_namespace_no_orphan_on_close_with_refs`

* [x] **VFS-1.4. HandleTable → ObObject consistency** | Prereqs: — | Files: `src/handle.rs`
  - **Descripcion:** Añadidos `is_valid()` (verifica ObId vivo en Object Manager) e `is_open_and_valid()`. `close()` solo llama `ob_close_object` si `is_valid()`. Corregido `has_ob_object()` que falsamente trataba STDIN/STDOUT como ObObjects. Double-close y stale handles son seguros.
  - **Severidad:** MEDIA — colgar handles puede causar uso-after-free ✅
  - **Tests:** `vfs_ownership_is_valid`, `vfs_ownership_is_valid_after_obj_destroyed`, `vfs_ownership_double_close_safe`, `vfs_ownership_stdio_always_valid`, `vfs_ownership_closed_not_valid`

---

## HIGH (v0.48 — v0.49)

### v0.48. NeoFS estabilidad

* [ ] **v0.48. NeoFS estabilidad** | Prereqs: — | Files: `src/fs/neodos_fs.rs`, `src/fs/vfs.rs`
  - **Descripcion:** Namespace ownership (NS-1/NS-2), dynamic allocators (FS-1/FS-2/FS-4), CAP_NS_WRITE, e1000 cleanup
  - **Severidad:** ALTA — bugfixes de estabilidad post-v0.47
  - **Tests:** `ns_ownership_tracking`, `fs_dynamic_inode_alloc`, `fs_dynamic_bitmap`

### VFS Fase 2: Separación de Capas

* [ ] **VFS-2.1. Privatizar métodos de NeoFS** | Prereqs: — | Files: `src/fs/neodos_fs.rs`
  - **Descripcion:** Hacer `abs_lba()`, `find_entry_in_directory()`, `get_inode_block_ptr()`, `inode_data_block_count()`, `directory_byte_span()`, `rebuild_bitmap()` → `pub(crate)` o privados. Solo deben ser accesibles a través del trait `FileSystem`.
  - **Severidad:** ALTA — ruptura de encapsulación, cualquier módulo puede acceder a detalles internos de NeoFS
  - **Tests:** (compilación, no se rompen callers existentes)

* [ ] **VFS-2.4. PageCache con contexto de drive** | Prereqs: — | Files: `src/buffer/page_cache.rs`
  - **Descripcion:** PageCache global usa `inode_num` como clave primaria. Dos instancias de NeoDosFs (C: y D:) con mismo inode_num colisionan. Añadir `drive_idx` a la clave de PageCache.
  - **Severidad:** ALTA — corrupción silenciosa de datos entre drives
  - **Tests:** `vfs_cache_pagecache_drive_context`

### VFS Fase 4: Drivers y Block Devices

* [ ] **VFS-4.1. Device IDs estables** | Prereqs: — | Files: `src/vfs/io.rs`, `src/drivers/block/mod.rs`
  - **Descripcion:** Usar UUID o nombre simbólico (ej. el nombre del driver + número de serie) para identificar block devices en lugar de índice numérico en Vec. El `IoStack` debe referenciar por nombre, no por índice. Evita invalidación al insertar/eliminar dispositivos.
  - **Severidad:** ALTA — si un dispositivo se elimina, los índices cambian y todos los IoStack quedan inválidos
  - **Tests:** `vfs_iostack_device_id_stable`

* [ ] **VFS-4.2. Hot-unload safety** | Prereqs: VFS-4.1 | Files: `src/drivers/boot_loader/mod.rs`, `src/drivers/driver_runtime.rs`
  - **Descripcion:** Cuando un driver se descarga, notificar al VFS para invalidar IoStacks que referencien sus devices. Marcar NeoDosFs como "stale" si su device se va. Impedir reads/writes adicionales.
  - **Severidad:** ALTA — descarga de driver de disco con archivos abiertos causa uso de device_id inválido
  - **Tests:** `vfs_iostack_stale_device_handling`

* [ ] **VFS-4.3. Refcount de block devices** | Prereqs: VFS-4.1 | Files: `src/drivers/block/mod.rs`
  - **Descripcion:** Llevar contador de referencias a cada block device (cuántos IoStack lo usan). Prevenir unload si refcount > 0.
  - **Severidad:** ALTA — hot unload puede dejar referencias colgadas
  - **Tests:** `driver_stress_load_unload_cycle`

### Object Manager

* [ ] **OBF-07. Unificar ObError y SyscallError en NeoDosError** | Prereqs: — | Files: `src/object/types.rs`, `src/syscall/mod.rs`
  - **Severidad:** MEDIA — 1 dia
  - **Tests:** `ob_error_syscall_mapping_complete`

### Storage

* [ ] **A5.2. VirtIO block driver (BOOT_DRIVER)** | Prereqs: A2.1 | Files: `src/drivers/virtio_blk.rs` (new, 400-500 lines), `src/drivers/storage.rs`, `src/main.rs` PHASE 3.6
  - **Descripcion:** Controlador de bloques VirtIO para maquinas virtuales QEMU/KVM. Se clasifica como **BOOT_DRIVER**, no como `.NEM`, ya que participa directamente en la cadena de arranque del sistema y debe estar disponible antes del montaje del volumen raiz.
    - **PCI detection:** Bus 0, vendor 0x1AF4 (Red Hat), device 0x1001 (VirtIO Block).
    - **Initialization:**
      1. Read BAR0 (MMIO base)
      2. Write device status: ACKNOWLEDGE | DRIVER
      3. Allocate virtqueue (#0, 32 descriptors)
      4. Register queue physical address
      5. Negotiate legacy/modern features
      6. Write device status: DRIVER_OK
    - **I/O path:** `submit_irp(irp)` -> descriptor slot -> request header -> sector_start/count/buffer -> doorbell -> wait completion -> used ring -> complete IRP
    - **Supported operations:** READ, WRITE, FLUSH, DISCARD
    - **Storage priority:** NVMe > VirtIO > BootAhci > BootAta
    - **Boot integration:** Available before VFS mount, before NeoInit, before NeoShell, before NEM loader.
  - **Criterio:**
    - Arrancar NeoDOS en QEMU usando `-drive if=virtio`
    - Deteccion automatica PCI, inicializacion correcta del dispositivo.
    - GPT parsing via VirtIO, carga del superblock NeoDOS, montaje de volumen raiz.
    - Arranque completo de NeoInit y NeoShell.
  - **Tests:** `virtio_pci_detect`, `virtio_virtqueue_init`, `virtio_submit_read_write`, `virtio_boot_load_kernel`, `virtio_gpt_parsing`, `virtio_mount_rootfs`, `virtio_boot_neoshell` (7 tests)

### Networking

- [ ] **B3.3 D8. DHCP client | NT: DHCP Client Service** | Prereqs: B3.2 | Files: `src/net/dhcp.rs`
  - **Descripcion:** Cliente DHCP (RFC 2131) que obtiene configuracion de red automaticamente al boot.
  - **Criterio:** Al boot con NIC presente, kernel obtiene IP automaticamente sin configuracion manual.
  - **Tests:** `dhcp_discover_offer_sequence`, `dhcp_lease_renewal`.

### Registry

* [ ] **B2.1 Z6. Registry hive database | NT: Cm (Configuration Manager), cell-based hive** | Prereqs: NT5 (Ob), NT6 (SID/ACL), A5.1 (IoStack) | Files: `src/cm/`, `src/cm/hive.rs`, `src/cm/cell.rs`, `src/cm/key.rs`, `src/cm/cache.rs`
  - **Descripcion:** Implementar NeoReg, sistema de configuracion jerarquico persistente como el Cm de Windows NT. Diseno sigue el modelo NT de celulas (cells) y bins, con integracion directa en el Object Manager NT5.
    - **Cell-based hive format:** Hive → Base Block (4KB, magic "neoR", seq numbers) → Bins (4KB) → Cells (Key, Value, Security descriptor). Cada celda tiene un indice dentro del bin, bins se numeran secuencialmente.
    - **ObNamespace integration:** `\Registry\Machine\System` → Ob::Key (backed by SYSTEM.HIV). `sys_open("\\Registry\\Machine\\System\\BootShell")` funciona via NT5 path resolution.
    - **Syscall API:** RAX 50-59: `sys_open_key`, `sys_create_key`, `sys_query_value`, `sys_set_value`, `sys_enum_key`, `sys_enum_value`, `sys_delete_key`, `sys_flush_key`, `sys_load_hive`, `sys_unload_hive`.
  - **Criterio:** Keys/value expuestos como objetos en NT5 namespace. `sys_set_value(key, "PATH", REG_SZ, "C:\\Programs")` persiste y es recuperable tras reboot. Cell cache: 2da lectura no toca disco.
  - **Severidad:** ALTA — feature grande
  - **Tests:** `cm_create_key_ob`, `cm_query_value_cache_hit`, `cm_set_value_persist`, `cm_enum_keys_multi`, `cm_hive_reload_integrity`, `cm_cell_corruption_isolated`, `cm_syscall_open_key`, `cm_syscall_set_get_value` (8 tests)

### libneodos

* [ ] **DH3. Completar libneodos syscall wrappers** | Prereqs: -- | Files: `libneodos/src/syscall.rs`
  - **Descripcion:** Faltan wrappers para: `sys_thread_create` (RAX 22), `sys_thread_join` (RAX 23), `sys_sleep_ex` (RAX 41), `sys_poll` (RAX 59), `sys_ob_destroy` (RAX 66), `sys_driver_unload` (RAX 57). Anadir con macros asm igual que los wrappers existentes.
  - **Severidad:** MEDIA — funcionalidad incompleta en libneodos
  - **Tests:** 6 nuevos (uno por wrapper)

---

## MEDIUM (v0.49 — v0.50)

### v0.49. NeoFS robustez

* [ ] **v0.49. NeoFS robustez** | Prereqs: v0.48 | Files: `src/fs/neodos_fs.rs`
  - **Descripcion:** Indirect blocks (FS-3), journaling (FS-5), checksums (FS-6), ResourceRegistry extendido (NS-3), DOS name reservation
  - **Severidad:** MEDIA — tolerancia a fallos
  - **Tests:** `fs_indirect_blocks`, `fs_journal_replay`, `fs_checksum_verify`

### VFS Fase 3: Namespace Consistencia

* [ ] **VFS-3.1. Separar \Global\FileSystem del Ob namespace** | Prereqs: VFS-1.1 | Files: `src/object/mod.rs`, `src/object/namespace.rs`
  - **Descripcion:** Que `ob_enum("\Global\FileSystem\")` NO enumere el namespace Ob, sino que delegue al VFS para listar directorios reales del filesystem montado.
  - **Severidad:** MEDIA — ambigüedad semántica entre namespace Ob y paths de FS
  - **Tests:** `vfs_namespace_filesystem_isolation`

* [ ] **VFS-3.3. Proteger paths del namespace** | Prereqs: VFS-3.1 | Files: `src/syscall/ob.rs`
  - **Descripcion:** Impedir que `ob_create(ObType::Directory)` cree directorios dentro de `\Global\FileSystem\` — esa ruta debe ser solo para VFS, no para el namespace Ob.
  - **Severidad:** MEDIA — creación de directorios en namespace que parecen de FS pero no son reales
  - **Tests:** `vfs_namespace_protected_paths`

### VFS Fase 5: Caché Unificada

* [ ] **VFS-5.1. Unificar BlockCache + PageCache** | Prereqs: — | Files: `src/buffer/block_cache.rs`, `src/buffer/page_cache.rs`
  - **Descripcion:** Una sola cache de páginas 4KB con sub-sector dirty tracking. Eliminar duplicación de datos (mismo contenido en ambas caches). Política LRU unificada.
  - **Severidad:** MEDIA — dos caches con datos redundantes, posible incoherencia
  - **Tests:** `vfs_cache_coherency`

* [ ] **VFS-5.2. InodeCache con invalidación** | Prereqs: — | Files: `src/fs/neodos_fs.rs`
  - **Descripcion:** Añadir versión/secuencia en superblock. Invalidar InodeCache cuando versión cambie (otro proceso modificó el inodo). Actualmente la cache nunca invalida — TOCTOU race potencial.
  - **Severidad:** ALTA — stale inode data tras modificación concurrente
  - **Tests:** `vfs_cache_inode_invalidation`

### VFS Fase 2 (cont.)

* [ ] **VFS-2.2. Refactorizar FSCK** | Prereqs: — | Files: `src/fs/fsck.rs`
  - **Descripcion:** Extraer lógica común de FSCK a un trait `FsckIntegrity` o similar, con implementación para NeoFS. Mover `fs/fsck.rs` a `drivers/fsck_neodos.rs` para que quede junto a su FS. Si se añade FSCK para FAT32, compartir el trait.
  - **Severidad:** MEDIA — FSCK atado a layout de NeoFS, difícil de extender
  - **Tests:** Los 6 tests existentes de FSCK más 2 de integración

* [ ] **VFS-2.3. Eliminar acceso directo a NeoFS desde shell** | Prereqs: — | Files: `src/shell/commands/*.rs`, `src/fs/neodos_fs.rs`
  - **Descripcion:** `DosShell::cat()`, `list_directory()` y otros comandos usan `NeoDosFs` directamente. Deben ir por VFS + handles, no por NeoDosFs directo.
  - **Severidad:** MEDIA — bypass de capa VFS, imposibilita añadir chequeos de seguridad en VFS
  - **Tests:** (funcional — comandos existentes deben seguir funcionando)

### Architectural Issues

* [ ] **AI-1. Completar ObInfoClass/ObSetInfoClass enums** | Prereqs: -- | Files: `src/object/types.rs`
  - **Descripcion:** Anadir clases faltantes: `ObInfoClass::ReadContent = 15`, `ObInfoClass::VolumeLabel = 16`, `ObSetInfoClass::ProcessTerminate = 4`, `ObSetInfoClass::VfsRename = 6`, `ObSetInfoClass::WriteContent = 7`, `ObSetInfoClass::SetCwd = 8`, `ObSetInfoClass::SetVolumeLabel = 9`.
  - **Criterio:** Enums reflejan implementacion real del handler. Tests verifican mapping.


* [ ] **AI-4. Arreglar TOCTOU race en kobj_register** | Prereqs: -- | Files: `src/kobj/mod.rs`
  - **Descripcion:** `kobj_register()` checkea si el object existe (read lock) y luego inserta (write lock) sin atomicidad. Convertir a operacion atomica: `registry.iter_mut().find(|e| e.is_none())` + insert en un solo lock scope.
  - **Criterio:** Dos CPUs registrando el mismo objeto no pueden resultar en duplicados.

### Tracing & Observability

- [ ] **B1.1 Y1. Kernel tracing infrastructure** | Prereqs: A2.4 | Files: `src/trace/mod.rs`
  - **Descripcion:** Ampliar el `TraceBuffer` existente (1024 entries, lock-free ring buffer en `trace.rs`) con trace points registrables dinamicamente. Actualmente el buffer soporta 7 tipos de evento (`ContextSwitch`, `SyscallEnter/Exit`, `IrqEnter/Exit`, `SchedDecision`, `Panic`) con 4 argumentos u64 por entry. Esta mejora anade: registro dinamico de trace points por subsistema (scheduler, VFS, memory, drivers), filtrado por categoria/nivel, y dump formateado via serial con timestamps HPET.
  - **Criterio:** Trace points registrables desde cualquier modulo kernel. Dump via serial legible. Filtrado por categoria funcional.
  - **Tests:** `trace_register_dynamic_point`, `trace_filter_by_category`, `trace_dump_serial_format`.

- [ ] **B1.2 Y2. NeoTrace system** | Prereqs: B1.1 | Files: `userbin/neotrace/`
  - **Descripcion:** Comando de shell Ring 3 `NEOTRACE` que expone la infraestructura de tracing (B1.1) al usuario. Subcomandos: `START` (activa captura global), `STOP` (pausa captura), `DUMP [N]` (vuelca las ultimas N entradas del TraceBuffer a consola), `FILTER <category>` (filtra por categoria). Usa `TRACE.dump()` internamente.
  - **Criterio:** `NEOTRACE START` + ejecutar proceso + `NEOTRACE DUMP 32` muestra ultimas 32 entradas con timestamps.
  - **Tests:** `neotrace_start_stop_toggle`, `neotrace_dump_output`.

### Networking

- [ ] **B3.4 D7. NTP client | NT: W32Time (Windows Time Service)** | Prereqs: B3.2 | Files: `src/net/ntp.rs`
  - **Descripcion:** Cliente NTP (RFC 5905, modo SNTP simplificado) que sincroniza el RTC del sistema con un servidor NTP externo.
  - **Criterio:** Tras boot con red, RTC sincronizado con servidor NTP (offset < 1s).
  - **Tests:** `ntp_request_parse_response`, `ntp_offset_calculation`.

### Userland

- [ ] **B4.3 S3. Shell redirection (`>`, `<`, `>>`)** | Prereqs: A4.7 | Files: `userbin/neoshell/`
  - **Descripcion:** Redireccion de I/O en neoshell. Parser detecta tokens `>` (write), `>>` (append), `<` (read). Para `cmd > file`: neoshell abre/crea `file` via syscall Ob, luego spawna `cmd` con `sys_dup2` redirigiendo fd 1 (stdout) al handle del archivo. Para `cmd < file`: abre archivo y redirige fd 0 (stdin). Para `>>`: abre con flag append.
  - **Criterio:** `DIR > output.txt` crea archivo con listado. `TYPE < input.txt` lee de archivo.
  - **Tests:** `redirect_stdout_to_file`, `redirect_stdin_from_file`, `redirect_append`.

- [ ] **B4.6 B6. NeoEdit text editor** | Prereqs: A4.7, B4.4 | Files: `userbin/neoedit/`
  - **Descripcion:** Editor de texto modal Ring 3 (`.NXE`). Usa `ob_open` + `ob_query_info(ReadContent)` para cargar archivos y `ob_set_info(WriteContent)` para guardar. Renderiza via `sys_write` con secuencias ANSI.
  - **Criterio:** `NEOEDIT C:\System\Config\system.cfg` abre, edita, guarda correctamente.
  - **Tests:** `neoedit_open_display`, `neoedit_edit_save`, `neoedit_scroll`.

- [ ] **B4.7 B6b-v2. Shared library per-process binding | NT: Ldr (Loader, PEB->LdrData)** | Prereqs: sys_loadlib | Files: `src/elf.rs`, `libneodos/`
  - **Descripcion:** Evolucionar el sistema NXL actual (slots globales fijos en 0x1E000000-0x1E200000 compartidos entre procesos) a binding per-process. Cada EPROCESS mantiene su propia tabla de NXLs cargadas.
  - **Criterio:** Dos procesos cargan versiones distintas de `libmath.nxl` sin interferencia.
  - **Tests:** `nxl_per_process_isolation`, `nxl_unload_on_exit`, `nxl_version_coexistence`.

- [ ] **B4.9 B11. NeoShell scripting (`.BAT`)** | Prereqs: B4.1, B4.2, B4.3 | Files: `userbin/neoshell/`
  - **Descripcion:** Interprete de scripts batch en neoshell. Soporta archivos `.BAT`/`.CMD` con: `ECHO`, `SET`, `IF %VAR%==valor cmd`, `GOTO :label`, `CALL script.bat`, `FOR`, `REM`, `@`.
  - **Criterio:** Script `.BAT` con IF/GOTO/CALL ejecuta correctamente.
  - **Tests:** `bat_echo_set`, `bat_if_goto`, `bat_call_subroutine`, `bat_for_loop`.

### Security

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

### Documentation

* [ ] **DH1. Actualizar README.md a v0.44.3** | Prereqs: -- | Files: `README.md`
  - **Descripcion:** README actual muestra v0.39.11 con 320+ tests y 36 syscalls. Actualizar a v0.44.3: 528 tests, 66 syscalls, Ob API, input subsystem, virtual terminals.
  - **Severidad:** BAJA — documentacion
  - **Tests:** (validacion manual, README refleja estado real)

* [ ] **DH2. Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md** | Prereqs: -- | Files: `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`
  - **Descripcion:** El documento menciona MAX_PROCESSES como limite fijo, pero el scheduler usa Vec. Menciona 320+ tests (real 528). Boot phases incompletas (falta Phase 3.86 NXL load, Phase 3.9 ABI freeze).
  - **Severidad:** BAJA — documentacion desactualizada
  - **Tests:** (validacion manual, doc refleja codigo)

### Kernel Debugger

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

## LOW (v0.50+)

### v0.50. Async I/O y Registry

* [ ] **v0.50. Async I/O y Registry** | Prereqs: v0.49 | Files: `src/cm/`, `src/net/dhcp.rs`
  - **Descripcion:** IOCP v1, DHCP (B3.3), Registry hive database (B2.1-B2.5)
  - **Severidad:** MEDIA — feature nueva
  - **Tests:** `cm_hive_persist`, `dhcp_discover_offer`

### v0.51. ASLR v2 y Benchmarking

* [ ] **v0.51. ASLR v2 y Benchmarking** | Prereqs: v0.50 | Files: `src/memory/`, `src/util/bench.rs`
  - **Descripcion:** ASLR v2 (pila/heap aleatorios), PGO, Benchmarking suite, NTP (B3.4)
  - **Severidad:** BAJA — rendimiento
  - **Tests:** `aslr_stack_random`, `ntp_sync`

> **Regla:** No se pasa a la Fase 3 hasta que v0.50 este completo y todos los tests pasen.

### Fase 3: Estabilizacion milestones

* [ ] **v0.51. Fork/Clone + Signal + Input** | Prereqs: — | Files: `src/syscall/`
  - **Descripcion:** sys_fork/clone (bajo demanda), sys_signal minimo, full Input subsystem (A4.4)
  - **Severidad:** ALTA — base para multiproceso completo
  - **Tests:** `fork_basic`, `signal_delivery`, `input_vt_full`

* [ ] **v0.52. Networking completo** | Prereqs: v0.51 | Files: `src/net/`
  - **Descripcion:** UDP, DNS, DHCP, TFTP/NFS basico, Virtual Terminals (B4.5)
  - **Severidad:** ALTA — networking usable
  - **Tests:** `dns_resolve`, `dhcp_lease`, `vt_switch_alt_f1_f2`

* [ ] **v0.53. Rendimiento** | Prereqs: v0.52 | Files: `src/memory/`, `src/scheduler/`, `src/pipe.rs`
  - **Descripcion:** Per-CPU heaps NUMA-aware, scheduler lock-free, zero-copy pipes (B6.1), COW fork (B6.2)
  - **Severidad:** MEDIA — optimizaciones
  - **Tests:** `pipe_zero_copy_aligned_buffers`, `cow_fork_shares_pages`

* [ ] **v0.54-v0.59. Documentacion y Hardening** | Prereqs: v0.53 | Files: `docs/`, `src/drivers/`
  - **Descripcion:** Documentacion API completa, test coverage >95%, fuzzing, module signatures (B5.1), secure boot (B5.3)
  - **Severidad:** BAJA — calidad
  - **Tests:** `nem_signature_valid_accepts`, `secure_boot_kernel_verified`

* [ ] **v1.0.0. API estable** | Prereqs: v0.54-v0.59 | Files: —
  - **Descripcion:** Primera API estable. Todo lo anterior debe estar COMPLETED.
  - **Severidad:** — hito de release

### v0.46 (Device Tree + Resource Manager)

* [ ] **v0.46. Device Tree + Resource Manager** | Prereqs: — | Files: `src/drivers/device_tree.rs`, `src/drivers/resource.rs`
  - **Descripcion:** Device Tree completo, PCI auto-vinculacion, VirtIO block driver (BOOT_DRIVER), sys_ioctl(). Ver A5.2 para detalle de VirtIO.
  - **Severidad:** ALTA — dependencia para storage unification
  - **Tests:** `device_tree_enum`, `virtio_pci_detect`, `virtio_boot_load_kernel`

### VFS Fase 3 (cont.)

* [ ] **VFS-3.2. \DosDevices dinámico** | Prereqs: VFS-1.1 | Files: `src/vfs/mount.rs`
  - **Descripcion:** Registrar automáticamente symlinks en `\DosDevices\` para cada nuevo mount. Actualmente solo C: y A: se registran en boot.
  - **Severidad:** BAJA — los mounts adicionales no aparecen en DosDevices
  - **Tests:** `vfs_mount_dosdevices_autoregister`

### VFS Fase 5 (cont.)

* [ ] **VFS-5.3. Write-back ordenado** | Prereqs: VFS-5.1 | Files: `src/globals.rs`
  - **Descripcion:** Garantizar flush page → flush block en ese orden coordinado. Actualmente ambos flushean independientemente sin orden.
  - **Severidad:** BAJA — posible escritura duplicada, no pérdida de datos
  - **Tests:** `vfs_cache_writeback_order`

### VFS Fase 6: Características

* [ ] **VFS-6.1. Overlay mounts** | Prereqs: VFS-1.1 | Files: `src/fs/vfs.rs`
  - **Descripcion:** Montar un FS sobre otro (capa de solo lectura + escritura). Útil para live CDs, actualizaciones, configuraciones por defecto con override de usuario.
  - **Severidad:** BAJA — feature nueva
  - **Tests:** `vfs_overlay_read_through`, `vfs_overlay_write_copy`

* [ ] **VFS-6.2. Extended attributes VFS** | Prereqs: — | Files: `src/fs/vfs.rs`
  - **Descripcion:** Añadir atributos VFS al trait `FileSystem`: `VfsAttr::ReadOnly`, `VfsAttr::Hidden`, `VfsAttr::System`, `VfsAttr::Archive`. Que coexistan con los atributos específicos de cada FS.
  - **Severidad:** BAJA — feature nueva
  - **Tests:** `vfs_ext_attr_read`, `vfs_ext_attr_write`

* [ ] **VFS-6.3. File notifications via Event Bus** | Prereqs: — | Files: `src/fs/vfs.rs`, `src/eventbus/`
  - **Descripcion:** Emitir eventos de Event Bus para cambios de archivos (crear, borrar, modificar). Permite a drivers y procesos de usuario reaccionar a cambios en el FS.
  - **Severidad:** BAJA — feature nueva
  - **Tests:** `vfs_notify_create`, `vfs_notify_delete`, `vfs_notify_modify`

* [ ] **VFS-6.4. Async VFS operations via IRP** | Prereqs: IRP system estable | Files: `src/fs/vfs.rs`
  - **Descripcion:** Hacer que las operaciones del trait `FileSystem` soporten async via IRP en lugar de solo sync. Permitir lectura/escritura no bloqueante desde el VFS.
  - **Severidad:** BAJA — feature nueva
  - **Tests:** `vfs_async_read`, `vfs_async_write`

### VFS Fase 7: Rendimiento

* [ ] **VFS-7.1. Eliminar lock global de VFS** | Prereqs: — | Files: `src/globals.rs`, `src/fs/vfs.rs`
  - **Descripcion:** Reemplazar `Mutex<Vfs>` con read-write lock o lock-free path resolution. El lock global es contendedor — lecturas concurrentes de directorios diferentes se serializan innecesariamente.
  - **Severidad:** BAJA — optimización
  - **Tests:** `vfs_perf_concurrent_reads`

* [ ] **VFS-7.2. Lookup cache** | Prereqs: — | Files: `src/fs/vfs.rs`
  - **Descripcion:** Cache de resultados de `lookup()` para paths recientes. Evitar recorrer el árbol de directorios en disco repetidamente para paths usados con frecuencia.
  - **Severidad:** BAJA — optimización
  - **Tests:** `vfs_perf_lookup_cache_hit`

* [ ] **VFS-7.3. Path cache** | Prereqs: VFS-7.2 | Files: `src/fs/vfs.rs`
  - **Descripcion:** Cache de `resolve_path()` completa con invalidación por cambio de directorio. Almacenar el resultado (drive_idx, inode) para paths completos.
  - **Severidad:** BAJA — optimización
  - **Tests:** `vfs_perf_path_cache_hit`

### Performance

- [ ] **B6.1 V2. Zero-copy pipes** | Prereqs: A4.5, S2 | Files: `src/pipe.rs`
  - **Descripcion:** Optimiza el camino de pipes para que, cuando el buffer del productor o consumidor este alineado y sea seguro, los datos se pasen por referencia a paginas compartidas o pinneadas en lugar de copiarse byte a byte dentro del kernel.
  - **Criterio:** Un pipeline con buffers alineados evita al menos una copia completa entre procesos.
  - **Tests:** `pipe_zero_copy_aligned_buffers`, `pipe_zero_copy_fallback_copy`, `pipe_zero_copy_integrity`.

- [ ] **B6.2 V3. Copy-on-write fork** | Prereqs: A1.5 | Files: `src/memory/cow.rs`, `src/syscall.rs`
  - **Descripcion:** Implementa `sys_fork` como clonacion perezosa del espacio de direcciones: el hijo comparte paginas con el padre en modo read-only hasta que cualquiera escribe.
  - **Criterio:** Padre e hijo comparten memoria al nacer y divergen solo al escribir.
  - **Tests:** `cow_fork_shares_pages`, `cow_write_triggers_copy`, `cow_fork_isolated_writes`.

### Userland (Low)

- [ ] **B4.8 B7. NeoTOP** | Prereqs: A4.7, A1.5 | Files: `userbin/neotop/`
  - **Descripcion:** Monitor de sistema Ring 3 en tiempo real (`.NXE`). Muestra lista de procesos, uso de CPU por core, estadisticas de memoria, drivers cargados. Refresco cada 1 segundo via `sys_sleep`. Renderiza con ANSI escape codes. Ver tambien [System Tools / Administration Suite](#system-tools--administration-suite) para el roadmap completo de NeoTOP.
  - **Criterio:** `NEOTOP` muestra procesos activos actualizandose en tiempo real.
  - **Tests:** `neotop_display_processes`, `neotop_refresh_loop`, `neotop_exit_clean`.

- [ ] **B4.10 B12. Compositor 2D** | Prereqs: B4.4, framebuffer | Files: `userbin/compositor/`
  - **Descripcion:** Compositor de ventanas 2D sobre el framebuffer GOP 1280x800. Modelo: cada ventana tiene un back-buffer, posicion, z-order, titulo. El compositor blittea ventanas en orden z sobre el framebuffer principal. Renderiza a 30 FPS maximo.
  - **Criterio:** Dos ventanas superpuestas, una encima de otra. Mover ventana actualiza framebuffer.
  - **Tests:** `compositor_create_window`, `compositor_z_order`, `compositor_blit_overlap`.

### Experimental

- [ ] **B7.1 E4. Full GUI system** | NT: Desktop Window Manager | Prereqs: B4.10 | Files: `userbin/gui/` | Desktop con iconos, menu, ventanas redimensionables.
- [ ] **B7.2 E5. Advanced secure boot (TPM)** | NT: BitLocker / TPM | Prereqs: B5.3 | Files: `src/boot/tpm.rs` | Medicion PCR + sealed storage.
- [ ] **B7.3 E6. Package manager** | NT: MSI / Windows Update | Prereqs: B5.1, A5.1 | Files: `userbin/neopkg/` | Install/remove paquetes `.NPK` firmados.
- [ ] **B7.4 T4. Time-travel debugging** | NT: WinDbg time travel | Prereqs: A3.2, B1.1 | Files: `src/debugger/timetravel.rs` | Replay de trace buffer en debugger.
- [ ] **B7.5 T5. Live kernel patching** | NT: Windows Hotpatch | Prereqs: A2.4, A3.2 | Files: `src/patch/mod.rs` | Hot-patch de funcion kernel sin reboot.
- [ ] **B7.6 T2. Distributed NeoDOS nodes** | NT: DFS | Prereqs: B3.2 | Files: `src/cluster/` | 2 nodos QEMU se descubren y comparten FS read-only.

### Architectural Issues (Low)

* [ ] **AI-2. Consolidate legacy syscall wrappers** | Prereqs: — | Files: `src/syscall/mod.rs`
  - **Descripcion:** Tras la migracion a Ob, varias syscalls legacy son wrappers finos que podrian eliminarse. `handler_readfile` (RAX 11) y `handler_writefile` (RAX 12) ya estan en None. `handler_mkdir`/`handler_unlink`/`handler_rmdir`/`handler_rename` (RAX 25-28) ya estan en None. Sin embargo, `handler_open` (RAX 10), `handler_readdir` (RAX 8), `handler_pipe` (RAX 5) siguen activos como wrappers de Ob.
  - **Decision:** Mantener las syscalls legacy activas por compatibilidad con binarios antiguos. No eliminar hasta que todos los binarios conocidos usen exclusivamente Ob API (v1.0).
  - **Severidad:** BAJA — deuda técnica controlada

* [ ] **AI-3. ObObjectTable lock granularity** | Prereqs: — | Files: `src/object/mod.rs`
  - **Descripcion:** El `ObObjectTable` usa un unico `spin::Mutex` global. Bajo carga de multiple proceso con operaciones Ob concurrentes (open, query, set, destroy), esto puede convertirse en cuello de botella.
  - **Propuesta:** Migrar a lock striping (16 locks, hash de ObId para elegir lock) o a una `RwLock` para operaciones de solo lectura vs escritura. Evaluar si es necesario tras medir contention real.
  - **Severidad:** BAJA — optimizacion, evaluar tras medir contention

### Registry features (Low)

* [ ] **B2.2 Z6. Registry transaction journal | NT: Hive LOG (.LOG1/.LOG2)** | Prereqs: B2.1 | Files: `src/cm/journal.rs`
  - **Descripcion:** Write-Ahead Log (WAL) para cada hive. Sigue el modelo NT de `.LOG` / `.LOG1` / `.LOG2`. Recovery al boot: replay del log si seq numbers no coinciden.
  - **Severidad:** MEDIA

* [ ] **B2.3 Z6. Multi-Hive Architecture | NT: SYSTEM/SOFTWARE/SECURITY/DEFAULT hives** | Prereqs: B2.1 | Files: `src/cm/hive.rs`, `src/cm/manager.rs`
  - **Descripcion:** Multiples hives bajo `\Registry` con independencia de carga, persistencia y recovery.
  - **Severidad:** MEDIA

* [ ] **B2.4 Z6. Registry Security | NT: SECURITY.HIVE, Key ACLs (NT6)** | Prereqs: B2.1 | Files: `src/cm/security.rs`
  - **Descripcion:** Control de acceso sobre keys registry usando NT6 Security Reference Monitor (SID + ACL + SeAccessCheck).
  - **Severidad:** MEDIA

* [ ] **B2.5 Z6. Registry notification + load/unload | NT: RegNotifyChangeKeyValue, NtLoadKey, NtUnloadKey** | Prereqs: B2.1 | Files: `src/cm/notify.rs`
  - **Descripcion:** Key change notifications via Event Bus. Hive load/unload for user profiles.
  - **Severidad:** BAJA

---

## COMPLETED (referencia historica)

El contenido completo de los items completados se ha movido a:
[IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md)

Incluye:
- Fase 1 (v0.40-v0.45)
- v0.47 (Networking TCP/IP)
- X7 (Object Manager unification)
- B9 (Shell Ring 0 → Ring 3 migration)
- OBF-01..06, OBF-09 (Fase 1 Objectification)
- A5.3 (AHCI NCQ)
- A4.4 (Input subsystem redesign)
- B4.5 (Virtual terminals)
- AI-5 (Libneodos-nxl modularization)
- Fase 2 Ob: Timer, Semaphore, Section (OBF-10..12)


---

## REFERENCE (analytical content, tables, design docs)

### Matriz de Problemas Arquitectonicos

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


### Architectural Initiatives

> Los items checklist (AI-1 a AI-5) estan listados en sus secciones de prioridad correspondientes. Esta seccion solo contiene el contexto analitico.

Las siguientes iniciativas arquitectonicas son cambios transversales que afectan a multiples subsistemas y requieren coordinacion.

* [ ] **AI-1. Clean up ObInfoClass/ObSetInfoClass enums** | Prereqs: — | Files: `src/object/types.rs`
  - **Descripcion:** El handler `handler_ob_query_info` soporta info classes 0-16, pero el enum `ObInfoClass` en `types.rs` solo define hasta 14 (KeyboardLayout). Similarmente, `ObSetInfoClass` solo define hasta 5 (KeyboardLayout), mientras que el handler soporta clases 4 (ProcessTerminate), 6 (VfsRename), 7 (WriteContent), 8 (SetCwd), 9 (SetVolumeLabel).
  - **Accion:** Anadir `ObInfoClass::ReadContent = 15`, `ObInfoClass::VolumeLabel = 16`, `ObSetInfoClass::ProcessTerminate = 4`, `ObSetInfoClass::VfsRename = 6`, `ObSetInfoClass::WriteContent = 7`, `ObSetInfoClass::SetCwd = 8`, `ObSetInfoClass::SetVolumeLabel = 9`.
  - **Severidad:** MEDIA — enums incompletos no reflejan implementacion real
  - **Tests:** `ob_info_class_variants`, `ob_set_info_class_variants`

* [ ] **AI-2. Consolidate legacy syscall wrappers** | Prereqs: — | Files: `src/syscall/mod.rs`
  - **Descripcion:** Tras la migracion a Ob, varias syscalls legacy son wrappers finos que podrian eliminarse. `handler_readfile` (RAX 11) y `handler_writefile` (RAX 12) ya estan en None. `handler_mkdir`/`handler_unlink`/`handler_rmdir`/`handler_rename` (RAX 25-28) ya estan en None. Sin embargo, `handler_open` (RAX 10), `handler_readdir` (RAX 8), `handler_pipe` (RAX 5) siguen activos como wrappers de Ob.
  - **Decision:** Mantener las syscalls legacy activas por compatibilidad con binarios antiguos. No eliminar hasta que todos los binarios conocidos usen exclusivamente Ob API (v1.0).
  - **Severidad:** BAJA — deuda técnica controlada

* [ ] **AI-3. ObObjectTable lock granularity** | Prereqs: — | Files: `src/object/mod.rs`
  - **Descripcion:** El `ObObjectTable` usa un unico `spin::Mutex` global. Bajo carga de multiple proceso con operaciones Ob concurrentes (open, query, set, destroy), esto puede convertirse en cuello de botella.
  - **Propuesta:** Migrar a lock striping (16 locks, hash de ObId para elegir lock) o a una `RwLock` para operaciones de solo lectura vs escritura. Evaluar si es necesario tras medir contention real.
  - **Severidad:** BAJA — optimizacion, evaluar tras medir contention

* [ ] **AI-4. Standardize error codes between Ob and syscall layer** | Prereqs: — | Files: `src/object/types.rs`, `src/syscall/mod.rs`
  - **Descripcion:** Actualmente `ObError` tiene su propio conjunto de codigos (-1 a -9), y `SyscallError` tiene otro conjunto separado. La capa de syscall traduce entre ellos manualmente. Puede producir discrepancias (e.g., `ObError::NotFound` -> `SyscallError::NoEnt`).
  - **Propuesta:** Unificar en un solo conjunto de codigos de error reutilizado por ambas capas, o anadir un mapping formal verificado por tests.
  - **Severidad:** MEDIA — error mapping manual propenso a bugs
  - **Tests:** `ob_error_syscall_mapping_complete`

### Objectification Roadmap — Plan y Tablas

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

### USR: Sistema de Usuarios NT-style (SAM + UAC + SUDO)

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
| USR-012 | neologon.nxe — login screen | `userbin/neologon/` | ~300 líneas |
| USR-013 | NeoInit → spawn NEOLOGON.NXE | `userbin/neoinit/` | +5 líneas |
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


### System Tools / Administration Suite

> Hoja de ruta del ecosistema de herramientas administrativas de NeoDOS.
> Todas las herramientas son binarios Ring 3 (`.NXE`) que se comunican con el
> kernel exclusivamente a traves de la API Object Manager (Ob) y syscalls foundation.
>
> **Principio:** Ninguna herramienta accede directamente a estructuras internas
> del kernel. Toda la informacion se obtiene via Ob info objects o syscalls publicas.

### Dependencias Arquitectonicas

```
Memory Manager ─── NeoMem
Scheduler     ─── NeoTop, NeoStat
Process Mgr   ─── NeoTask, NeoTop
Registry Mgr  ─── NeoCfg
Object Mgr    ─── NeoTask, NeoDebug
VFS / FS Mgr  ─── NeoFS
Event Bus     ─── NeoLog
Syscall Layer ─── NeoStat
```

---

### NeoMem

| Campo | Valor |
|-------|-------|
| **Estado** | **v0.1 IMPLEMENTED** |
| **Version actual** | v0.1 |
| **Binario** | `neomem.nxe` (reemplaza `mem.nxe`) |
| **Path** | `C:\Programs\neomem.nxe` |
| **Dependencias** | `\Global\Info\Memory` (Ob info object) |
| **Syscalls** | `ob_open`, `ob_query_info(ObInfoClass::Memory=10)` |

**Descripcion:** Herramienta oficial de diagnostico de memoria. Muestra
estadisticas de memoria fisica, kernel heap, memoria de usuario y paginacion.

**Roadmap:**

| Version | Funcionalidad | Estado |
|---------|--------------|--------|
| v0.1 | Estadisticas basicas: fisica, kernel heap, user, paging | **COMPLETADO** |
| v0.2 | Memoria por proceso (query por PID) | Planificado |
| v0.3 | Analisis del heap (fragmentacion, slab por cache) | Planificado |
| v1.0 | Diagnostico avanzado (leaks, histogramas, tendencias) | Planificado |

**Relacion con el kernel:**
- `MemoryStats` struct en `src/memory/mod.rs` (extendido con 15 campos)
- Handler `ob_query_info(class=Memory)` en `src/syscall/mod.rs`
- `\Global\Info\Memory` ObObject creado en `main.rs` (PHASE 3.0)
- Slab allocator stats via `allocator::ALLOCATOR.usage()` en `src/slab.rs`
- Buddy allocator stats via `buddy::free_pages()` / `buddy::total_frames()`

---

### NeoTop

| Campo | Valor |
|-------|-------|
| **Estado** | **v0.1 EXISTENTE** (`neotop.nxe`, migrado a B4.8 como item completado) |
| **Binario** | `neotop.nxe` |
| **Dependencias** | Scheduler, Memory Manager, Ob API |
| **Syscalls** | `ob_enum(\Process)`, `ob_query_info(Process)`, `ob_open(\Global\Info\*)`, `sys_sleep` |

**Descripcion:** Monitor del sistema en tiempo real. Muestra procesos activos,
uso de CPU por core, estadisticas de memoria, drivers cargados.

**Roadmap:**

| Version | Funcionalidad | Estado |
|---------|--------------|--------|
| v0.1 | Lista de procesos, memoria basica, refresco 1s | **EXISTE** |
| v0.2 | CPU por core, grafico de barras, drivers | Planificado |
| v0.3 | Filtros interactivos, ordenacion, modo batch | Planificado |
| v1.0 | Dashboard completo, alertas, exportacion CSV | Planificado |

---

### NeoTask

| Campo | Valor |
|-------|-------|
| **Estado** | **Planificado** |
| **Dependencias** | Process Manager (`src/scheduler/`), Ob API, KWait |
| **Syscalls** | `ob_enum(\Process)`, `ob_query_info(Process)`, `ob_set_info(ProcessTerminate)`, `ob_set_info(ProcessPriority)` |

**Descripcion:** Administrador de procesos. Permite listar, inspeccionar y
controlar procesos del sistema.

**Roadmap:**

| Version | Funcionalidad |
|---------|--------------|
| v0.1 | Listar procesos con PID, PPID, estado, prioridad, threads |
| v0.2 | Informacion detallada: handles, objetos, memoria por proceso |
| v0.3 | Suspender/reanudar procesos, cambiar prioridad |
| v1.0 | Arbol de procesos, dependencias, kill graceful |

**Relacion con el kernel:**
- Enumera procesos via `ob_enum("\Ob\Process")`
- Obtiene detalles via `ob_query_info(fd, Process)`
- Terminal procesos via `ob_set_info(fd, ProcessTerminate)`
- Cambia prioridad via `ob_set_info(fd, ProcessPriority)`

---

### NeoCfg

| Campo | Valor |
|-------|-------|
| **Estado** | **Planificado** (depende de Registry B2.1) |
| **Prerequisitos** | B2.1 (Registry hive database), NT5 (Ob namespace para `\Registry`) |
| **Dependencias** | Registry Manager (`src/cm/`), NT6 Security (SID/ACL) |

**Descripcion:** Administrador de configuracion estilo Registry Editor de
Windows NT. Opera sobre `\Registry\` en el namespace Ob.

**Roadmap:**

| Version | Funcionalidad |
|---------|--------------|
| v0.1 | Lectura de claves y valores del Registry |
| v0.2 | Edicion de valores existentes (REG_SZ, REG_DWORD) |
| v0.3 | Creacion/eliminacion de claves y valores |
| v1.0 | Gestion de seguridad (ACL por clave), backup/restore de hives |

---

### NeoLog

| Campo | Valor |
|-------|-------|
| **Estado** | **Planificado** |
| **Dependencias** | Event Bus (`src/eventbus/`), TraceBuffer (`src/trace.rs`), B1.1 (tracing infrastructure) |
| **Syscalls** | `ob_open(\Global\Info\*)`, `ob_query_info` |

**Descripcion:** Sistema de visualizacion de logs. Lee el buffer de eventos
del kernel y lo presenta filtrado por categoria, severidad y timestamp.

**Roadmap:**

| Version | Funcionalidad |
|---------|--------------|
| v0.1 | Visualizacion de logs del kernel en tiempo real |
| v0.2 | Filtrado por categoria (scheduler, VFS, memory, drivers, security) |
| v0.3 | Niveles de severidad (error, warning, info, debug) |
| v1.0 | Busqueda, exportacion, log persistente a disco |

---

### NeoStat

| Campo | Valor |
|-------|-------|
| **Estado** | **Planificado** |
| **Dependencias** | Syscall dispatch (`src/syscall/`), Scheduler, IPC (`src/pipe.rs`), Ob Manager (`src/object/`) |

**Descripcion:** Estadisticas globales del sistema. Muestra contadores de
rendimiento del kernel: syscalls, scheduler, IPC, Object Manager.

**Roadmap:**

| Version | Funcionalidad |
|---------|--------------|
| v0.1 | Contadores de syscalls por tipo (hits/errors) |
| v0.2 | Estadisticas del scheduler (context switches, CPU time por proceso) |
| v0.3 | Estadisticas IPC (pipes creados, datos transferidos) |
| v1.0 | Dashboard de servicios del kernel |

---

### NeoFS

| Campo | Valor |
|-------|-------|
| **Estado** | **Planificado** |
| **Dependencias** | VFS (`src/vfs/`), NeoDOS FS driver, FAT32 driver, IoStack |
| **Syscalls** | `ob_open`, `ob_query_info`, `ob_enum`, `sys_fsck` |

**Descripcion:** Herramientas del sistema de archivos. Opera via VFS y
proporciona informacion sobre montajes, espacio, y diagnostico.

**Roadmap:**

| Version | Funcionalidad |
|---------|--------------|
| v0.1 | Informacion del filesystem: tipo, total, usado, libre por unidad |
| v0.2 | Listado de montajes activos, puntos de montaje, tipo de FS |
| v0.3 | Diagnostico VFS: cache hits, operaciones por segundo |
| v1.0 | Gestion de discos y particiones (GPT, MBR) |

---

### NeocCtl

| Campo | Valor |
|-------|-------|
| **Estado** | **Planificado** |
| **Dependencias** | Driver runtime, Scheduler, Power management, Ob API |
| **Syscalls** | `ob_open`, `ob_set_info`, `sys_poweroff`, `sys_driver_unload` |

**Descripcion:** Panel de control del sistema. Proporciona interfaz unificada
para administracion del sistema: servicios, apagado, reinicio, configuracion.

**Roadmap:**

| Version | Funcionalidad |
|---------|--------------|
| v0.1 | Apagar, reiniciar, estado del sistema |
| v0.2 | Gestion de servicios: listar, iniciar, detener |
| v0.3 | Monitoreo de salud del sistema |
| v1.0 | Planificacion de tareas, politicas de energia |

---

### NeoDebug

| Campo | Valor |
|-------|-------|
| **Estado** | **Planificado** |
| **Dependencias** | Object Manager, TraceBuffer, B1.1 (tracing), A3.2 (kernel debugger) |
| **Syscalls** | `ob_open`, `ob_enum`, `ob_query_info`, `sys_kobj_enum` |

**Descripcion:** Herramientas avanzadas de desarrollo y depuracion del kernel.

**Roadmap:**

| Version | Funcionalidad |
|---------|--------------|
| v0.1 | Diagnostico de objetos Ob (listar, inspeccionar) |
| v0.2 | Diagnostico de memoria (slab, buddy, heap) |
| v0.3 | Tracing de syscalls y scheduler |
| v1.0 | Debugger integrado, breakpoints, dump de estado |

---

### Resumen del Ecosistema

| Herramienta | Estado v0.46 | Dependencia Principal | Interfaz con Kernel |
|------------|-------------|----------------------|-------------------|
| **NeoMem** | **v0.1 COMPLETADO** | Memory Manager | `\Global\Info\Memory` via Ob |
| **NeoTop** | Existente | Scheduler + Memory | `\Ob\Process` + `\Global\Info\*` |
| **NeoTask** | Planificado | Process Manager | `\Ob\Process` via Ob |
| **NeoCfg** | Planificado (B2.1) | Registry Manager | `\Registry\` via Ob |
| **NeoLog** | Planificado (B1.1) | Event Bus + Trace | `\Global\Info\*` via Ob |
| **NeoStat** | Planificado | Syscall Layer | Syscalls foundation |
| **NeoFS** | Planificado | VFS | `ob_open` + `ob_query_info` |
| **NeoCtl** | Planificado | Driver Runtime | Ob + foundation syscalls |
| **NeoDebug** | Planificado (A3.2) | Object Manager | `ob_enum` + `ob_query_info` |


### Recommended Next Steps

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
| 12 | ~~**Networking (B3.1-B3.2)**~~ | ~~v0.47~~ | ~~VirtIO-net, IRP~~ | ~~3000-5000 lineas~~ **COMPLETADO** |
| 13 | **AHCI NCQ (A5.3)** | v0.46.2 | A2.2, IRP | 900 lineas **COMPLETADO** |
| 14 | **NS-1: Namespace ownership tracking** | v0.48 | — | 3-4 días |
| 15 | **NS-2: Proteger directorios raíz del namespace** | v0.48 | NS-1 | 1-2 días |
| 16 | **FS-1: Dynamic inode allocator** | v0.48 | — | 2-3 días |
| 17 | **FS-2: Dynamic block bitmap** | v0.48 | — | 2-3 días |
| 18 | **FS-4: Eliminar hardcoded sector offsets** | v0.48 | FS-1 | 1 día |
| 19 | **NS-4: e1000 shutdown/cleanup** | v0.48 | — | 1 día |
| 20 | **CAP_NS_WRITE capability** | v0.48 | NS-1 | 1 día |
| 21 | **NS-3: Extender ResourceRegistry** | v0.49 | NS-1 | 1 día |
| 22 | **FS-3: Indirect blocks support** | v0.49 | FS-1, FS-2 | 1-2 días |
| 23 | **FS-5: Basic journaling (WAL)** | v0.49 | FS-1, FS-2 | 1 semana |
| 24 | **FS-6: Metadata checksums** | v0.49 | — | 2-3 días |
| 25 | **Name reservation (DOS names)** | v0.49 | — | 4 horas |
| 26 | **USR-F1: SAM + Token NT** | v0.48 | NT6 (existente) | 900 lineas |
| 27 | **USR-F2: Login + SUDO** | v0.49 | USR-F1 | 1600 lineas |
| 28 | **USR-F3: Hardening + Grupos** | v0.50 | USR-F2 | 600 lineas |
| 29 | **NeoReg transaction journal (B2.2)** | v0.50 | B2.1 | 500-700 lineas |
| 30 | **Shell redirection (B4.3)** | v0.46+ | neoshell | 300-400 lineas |
| 31 | **Registry hive database (B2.1)** | v0.50 | NT5, NT6, IoStack | 2000-3000 lineas |
| 32 | **Kernel debugger (A3.2)** | v0.51+ | A3.1 | 1500-2000 lineas |

### Auditoría de estabilidad (v0.46.7)

**Hallazgo CRÍTICO:** `handler_exit` y `kill_pid` no llamaban `ob_close_object()` para handles no-pipe (archivos, dispositivos, eventos, directorios, objetos Ob). Esto causaba una fuga permanente de referencias ObObject por cada proceso que terminaba con handles abiertos sin cerrar explícitamente. Combinado con la fuga de fd en `resolve_path()` de NeoShell (que abría un fd por comando para verificar existencia de archivo y nunca lo cerraba), el sistema agotaba la tabla ObObject o el heap del kernel después de ~250 comandos, provocando reinicio/apagado inesperado.

### Fixes aplicados (todos verificados con `auto_test.py` 570 tests PASS)

| # | Modulo | Archivo | Fix | Severidad |
|---|--------|---------|-----|-----------|
| 1 | Syscall | `syscall/handlers.rs:308-316` | `else if h.has_ob_object() { ob_close_object(h.object_id) }` en `handler_exit` | CRITICA |
| 2 | Scheduler | `scheduler/mod.rs:629-636` | `else if h.has_ob_object() { ob_close_object(h.object_id) }` en `kill_pid` | CRITICA |
| 3 | NeoShell | `userbin/neoshell/src/main.rs:350` | Cerrar fd en `resolve_path()` tras verificar existencia | CRITICA |
| 4 | Handle | `handle.rs:220` | Prevenir fd overflow `>255` en `alloc_handle` | ALTA |
| 5 | Slab | `slab.rs:119-133` | Detección de double-free en free list | MEDIA |
| 6 | Syscall | `syscall/handlers.rs:569-574` | Cerrar ObObject destino no-pipe en `dup2` | MEDIA |

### Hallazgo CRITICO #2: Crash en `handler_ob_create(Process)` tras 2-3 comandos

**Sintoma:** La maquina virtual se reinicia/apaga tras ejecutar 2-3 comandos consecutivos en NeoShell. Sin mensaje de panic en el log serial (QEMU no vacia el buffer antes de salir).

**Causa raiz:** En QEMU con acelerador TCG, las instrucciones se agrupan en bloques de traduccion (Translation Blocks, TB). Una funcion larga como `handler_ob_create(Process)` puede caber en un solo TB. Los temporizadores pendientes solo se entregan al final del TB. Si un timer interrupt queda pendiente hasta que el codigo entra en `without_interrupts` + `scheduler.lock()` en `add_ring3_process`, el timer handler no puede ejecutarse (interruptos deshabilitados) y el watchdog no se pettea → reset por watchdog. En real hardware esto no ocurre porque los interrupts se entregan en cada instruccion, no al final del TB.

**Fix:** Insertar `rdtsc` al inicio de `handler_ob_create`. En QEMU TCG, `rdtsc` fuerza una salida del TB actual, permitiendo que los interrupts pendientes se entreguen ANTES de que el codigo entre en la seccion critica con interrupts deshabilitados.

| # | Modulo | Archivo | Fix | Severidad |
|---|--------|---------|-----|-----------|
| 7 | Syscall | `syscall/ob.rs:handler_ob_create` | `rdtsc` al inicio fuerza TB exit en QEMU TCG | CRITICA |

### Vulnerabilidades identificadas (no corregidas en este parche)

| # | Archivo:Linea | Hallazgo | Severidad |
|---|--------------|----------|-----------|
| 7 | `syscall/handlers.rs:1685` | `sys_poll` sin `is_user_ptr_valid` — infoleak | CRITICA |
| 8 | `object/mod.rs:285` | TOCTOU en `ob_close_object` entre drop lock y finalize | ALTA |
| 9 | `buddy.rs:232-242` | `mark_used_region` no remueve de free lists | ALTA |
| 10 | `syscall/handlers.rs:523` | `sys_poweroff` sin check de admin | ALTA |
| 11 | `scheduler/mod.rs:984` | `on_timer_tick` no setea per-CPU need_resched | MEDIA |
| 12 | `scheduler/mod.rs:661-711` | `recycle_terminated` no libera recursos fisicos | MEDIA |

### NeoFS Audit & Roadmap (v0.48+)

> Auditoría completa del sistema de archivos NeoFS y la interacción
> Driver/Namespace. Ver documentación completa:
> - [NEOFS_AUDIT.md](NEOFS_AUDIT.md) — Hallazgos detallados
> - [NEOFS_ROADMAP.md](NEOFS_ROADMAP.md) — Roadmap por fases
> - [NEOFS_TESTS.md](NEOFS_TESTS.md) — Propuesta de tests

### Hallazgos Principales

| ID | Problema | Archivos | Severidad | Impacto |
|----|----------|----------|-----------|---------|
| NS-1 | Namespace ownership tracking ausente | `src/object/namespace.rs`, `src/object/mod.rs` | **CRÍTICA** | Cualquier driver/process puede borrar entries de otro |
| NS-2 | Directorios raíz del namespace sin protección | `src/object/namespace.rs` | **CRÍTICA** | Un driver puede crear `\Device\` entries conflictivas |
| NS-3 | ResourceRegistry no trackea Ob entries | `src/drivers/hotreload.rs` | **ALTA** | Hot unload deja entries huérfanas en namespace |
| NS-4 | e1000 no es NEM, no hot-reload, sin cleanup | `src/net/e1000.rs`, `src/net/mod.rs` | **MEDIA** | No hay shutdown path para NIC |
| FS-1 | Inode allocator fijo en 256 | `src/fs/neodos_fs.rs` | **CRÍTICA** | Máximo 256 archivos en todo el FS |
| FS-2 | Block bitmap fijo (320 bytes = 2560 bloques) | `src/fs/neodos_fs.rs` | **CRÍTICA** | Máximo ~10 MB de datos |
| FS-3 | Sin indirect blocks (solo 12 directos) | `src/fs/neodos_fs.rs` | **ALTA** | Archivos limitados a 48 KB |
| FS-4 | Hardcoded sector offsets (data en sector 200) | `src/fs/neodos_fs.rs` | **MEDIA** | Cambiar num_inodes rompe offsets |
| FS-5 | Sin journaling/write-ahead log | `src/fs/` (falta `journal.rs`) | **ALTA** | Crash entre writes → FS inconsistente |
| FS-6 | Sin checksums en metadata | `src/fs/neodos_fs.rs` | **MEDIA** | Corrupción de superblock indetectable |

### Nuevas Tareas Priorizadas (NeoFS vNext)

Items para añadir a la tabla Recommended Next Steps:

| Prioridad | Item | Fase | Dependencias | Esfuerzo |
|-----------|------|------|-------------|----------|
| 5 | **NS-1: Namespace ownership tracking** | v0.48 | — | 3-4 días |
| 6 | **NS-2: Proteger directorios raíz del namespace** | v0.48 | NS-1 | 1-2 días |
| 7 | **FS-1: Dynamic inode allocator** | v0.48 | — | 2-3 días |
| 8 | **FS-2: Dynamic block bitmap** | v0.48 | — | 2-3 días |
| 9 | **NS-3: Extender ResourceRegistry** | v0.49 | NS-1 | 1 día |
| 10 | **FS-3: Indirect blocks support** | v0.49 | FS-1, FS-2 | 1-2 días |
| 11 | **FS-4: Eliminar hardcoded sector offsets** | v0.48 | FS-1 | 1 día |
| 12 | **FS-5: Basic journaling (WAL)** | v0.49 | FS-1, FS-2 | 1 semana |
| 13 | **FS-6: Metadata checksums** | v0.49 | — | 2-3 días |
| 14 | **NS-4: e1000 shutdown/cleanup** | v0.48 | — | 1 día |
| 15 | **CAP_NS_WRITE capability** | v0.48 | NS-1 | 1 día |
| 16 | **Name reservation (DOS names)** | v0.49 | — | 4 horas |

### Tests Planificados (26 nuevos)

| ID | Test | Categoría | Líneas |
|----|------|-----------|--------|
| T1-1 | `inode_create_300` | Inode stress | 50 |
| T1-2 | `inode_reuse_after_delete` | Inode stress | 60 |
| T1-3 | `inode_max_limit` | Inode stress | 30 |
| T1-4 | `inode_collision_check` | Inode stress | 40 |
| T1-5 | `inode_corruption_detect` | Inode stress | 40 |
| T2-1 | `ns_path_long_255` | Namespace | 40 |
| T2-2 | `ns_path_too_long` | Namespace | 20 |
| T2-3 | `ns_deeply_nested_32` | Namespace | 50 |
| T2-4 | `ns_entry_corrupted_0xE5` | Namespace | 30 |
| T2-5 | `ns_entry_corrupted_bad_len` | Namespace | 30 |
| T2-6 | `ns_reserved_name_con` | Namespace | 20 |
| T2-7 | `ns_case_insensitive_unicode` | Namespace | 30 |
| T3-1 | `driver_ns_register_device` | Driver/NS | 40 |
| T3-2 | `driver_ns_name_collision` | Driver/NS | 40 |
| T3-3 | `driver_ns_protected_root` | Driver/NS | 30 |
| T3-4 | `driver_ns_protected_global_info` | Driver/NS | 30 |
| T3-5 | `driver_ns_hot_unload_cleanup` | Driver/NS | 60 |
| T3-6 | `driver_ns_hot_unload_blocks_removed` | Driver/NS | 50 |
| T3-7 | `driver_ns_duplicate_name` | Driver/NS | 30 |
| T3-8 | `driver_ns_cap_required` | Driver/NS | 30 |
| T4-1 | `fs_stress_create_open_close_delete_10k` | Stress | 40 |
| T4-2 | `fs_stress_concurrent_files` | Stress | 60 |
| T4-3 | `ns_stress_1000_entries_namespace` | Stress | 50 |
| T4-4 | `fs_stress_long_path_walk` | Stress | 40 |
| T4-5 | `driver_stress_load_unload_cycle` | Stress | 50 |
| T4-6 | `driver_stress_concurrent_load` | Stress | 40 |


### Auditoría Arquitectónica del VFS de NeoDOS

# Auditoría Arquitectónica del VFS de NeoDOS (2026-06-30)

## Resumen Ejecutivo

El VFS de NeoDOS es ligero y funcional, pero adolece de una **separación incompleta de responsabilidades**. El `FileSystem` trait (`src/fs/vfs.rs`) y el `Vfs` struct son limpios y genéricos, pero el `MountManager` (`src/vfs/mount.rs`) reside en un módulo separado con suscripción al Object Manager que **duplica la funcionalidad de montaje**. El código específico de NeoFS está correctamente encapsulado en `src/fs/neodos_fs.rs`, aunque expone detalles internos (inodos, bitmap) como `pub`. La integración con el Object Manager es funcional pero tiene **ownership confuso**: los handles de archivo crean objetos `ObObject` sin path namespace, y las referencias viven en dos sitios (HandleTable y ObObjectTable). Detectados **4 riesgos arquitectónicos** y **7 items de deuda técnica**.

---

## 1. Arquitectura Actual

### 1.1 Capas del VFS

```
┌──────────────────────────────────────────────┐
│  Syscall Handlers (syscall/ob.rs, handlers.rs) │
├──────────────────────────────────────────────┤
│  Ob open_path()  (object/mod.rs)              │
│    ├─ Namespace lookup (object/namespace.rs)  │
│    └─ VFS resolve_path() (fs/vfs.rs)          │
├──────────────────────────────────────────────┤
│  Vfs struct (fs/vfs.rs)                       │
│    ├─ drives[26]: Box<dyn FileSystem>         │
│    ├─ mounts[8]: sub-mounts dentro de drives  │
│    └─ walk_components(): path traversal       │
├──────────────────────────────────────────────┤
│  FileSystem trait (fs/vfs.rs)                 │
│    ├─ NeoDosFs (fs/neodos_fs.rs)              │
│    ├─ Fat32Driver (drivers/fat32.rs)          │
│    └─ Iso9660Driver (drivers/iso9660.rs)      │
├──────────────────────────────────────────────┤
│  IoStack (vfs/io.rs)                          │
│    ├─ translation LBA (partición)             │
│    ├─ block cache lookup                      │
│    └─ read_sectors/write_sectors              │
├──────────────────────────────────────────────┤
│  BlockDevice trait (drivers/block/)           │
│    ├─ RamDisk / BootAta / BootAhci            │
│    ├─ NVMe / NemBlockDevice                   │
│    └─ read_blocks/write_blocks                │
└──────────────────────────────────────────────┘
```

### 1.2 Módulos VFS

| Módulo | Archivo | Responsabilidad |
|--------|---------|----------------|
| `fs/vfs.rs` | `src/fs/vfs.rs` | Core VFS: `FileSystem` trait, `Vfs` struct con 26 drives, path resolution, mount/unmount |
| `vfs/io.rs` | `src/vfs/io.rs` | `IoStack`: block I/O con traducción de partición, stub de cache L1/L2 |
| `vfs/mount.rs` | `src/vfs/mount.rs` | `MountManager`: mount de alto nivel con namespace Ob, symlinks \DosDevices |
| `vfs/partition.rs` | `src/vfs/partition.rs` | `PartitionInfo`, parsing GPT, búsqueda de particiones por GUID |
| `fs/neodos_fs.rs` | `src/fs/neodos_fs.rs` | Implementación `FileSystem` para NeoFS (+ inodo, bitmap, dir entry internos) |
| `fs/fsck.rs` | `src/fs/fsck.rs` | FSCK específico de NeoFS (lee inodos, bitmap, directorios directamente) |
| `handle.rs` | `src/handle.rs` | `HandleTable`, `HandleEntry` — tabla de handles por proceso, todos referencian ObObject |
| `globals.rs` | `src/globals.rs` | Globales `VFS`, `BLOCK_CACHE`, `PAGE_CACHE`, `BLOCK_DEVICES`, helpers `with_vfs()` |

---

## 2. Fortalezas

### 2.1 `FileSystem` trait bien diseñado
- Trait genérico con 12 métodos, defaults para `NotImplemented` en operaciones opcionales.
- `Send` bound — correcto para kernel multihilo.
- `VfsNode` simple (inode, mode, size) — abstracción limpia.

### 2.2 Vfs struct minimalista
- `drives[26]` — array fijo, O(1) lookup, sin allocaciones dinámicas.
- `mounts[8]` — array fijo con sub-montaje (mount dentro de un drive).
- Path resolution puramente VFS: `walk_components()` con soporte `.`, `..`, traversal de mount points.
- `split_drive()` correcto y simple.

### 2.3 IoStack — capa de I/O unificada
- Traducción LBA partición-relativa → absoluta.
- Cache lookup transparente.
- `with_device()` permite acceso directo cuando el FS lo necesita.

### 2.4 Partition parsing limpio
- GPT parsing sin dependencias externas.
- Búsqueda por GUID de partición.
- Separado en módulo propio.

### 2.5 HandleTable bien encapsulado
- Soporta fds ilimitados (Vec dinámico).
- Sentinel values para stdin/stdout/stderr claros.
- `alloc_handle()` O(n) pero aceptable.

---

## 3. Debilidades y Riesgos

### ⚠ R1: Ownership Confuso — Handles Filesystem vs Object Manager

**Gravedad: ALTA**

Cuando `handler_ob_open()` resuelve `\Global\FileSystem\C:\path`:
1. `ob_open_path()` llama a `vfs.resolve_path()` → obtiene `(drive_idx, VfsNode)`
2. Crea un `ObObject` vía `ob_create_object(ObType::Filesystem, path_str, node.inode, drive_idx, None)`
3. Inserta en namespace con `ob_insert_object(path_str, obj_id)`
4. Crea un `HandleEntry::ob_object(obj_id, ...)` en la handle table del proceso

**Problemas:**
- El `ObObject` se crea **sin `ObOperations`**, por lo que `on_destroy` nunca se llama. Cuando el handle se cierra (`ob_close_object`), se decrementa refcount, pero no se liberan recursos del filesystem (no hay un `close` real en el FS).
- `ob_close_object` destruye el `ObObject` cuando refcount llega a 0, pero el `VfsNode` (inode, mode, size) que se resolvió es una copia — no hay un objeto persisted que represente "archivo abierto" en el VFS.
- **No hay un "file object"** en el sentido NT (un objeto que representa una instancia de apertura con estado). La handle table almacena `ObId` + `offset`, pero el `ObObject` en la tabla global no tiene offset — cada proceso tiene su offset en la `HandleEntry`.
- **Dos procesos que abren el mismo archivo** crean dos `ObObject`s separados en el Object Manager, sin relación entre sí. Esto es correcto pero ineficiente.

### ⚠ R2: Dualidad de Mount Managers

**Gravedad: ALTA**

Existen **DOS sistemas de mount** paralelos:

1. **Vfs::mount/unmount** (`src/fs/vfs.rs:126-147`): monta `Box<dyn FileSystem>` en `drives[letter]`. Array fijo de 26, mount dentro de drive (sub-mount).

2. **MountManager** (`src/vfs/mount.rs`): crea objetos `ObType::MountPoint` en el Object Manager, crea entradas en `\DosDevices\`, inserta symlinks, registra en `\Device\`.

**Problemas:**
- `main.rs` llama a **ambos** para el mismo mount: `vfs.mount('C', Box::new(fs))` + `vfs_mount("\\Device\\NeoDosVolume0", 'C', NeoDosFs)`.
- No hay validación cruzada: se puede montar en `MountManager` sin mount en `Vfs`, o viceversa.
- `vfs_path_to_mount()` busca en `MountManager` por prefijo de path, pero `Vfs::resolve_path()` usa `walk_components()` que busca en `Vfs.mounts[]` — dos tablas de montaje separadas.
- Al hacer `unmount`, `Vfs::unmount()` limpia `drives[idx]` y elimina mounts hijos; `vfs_unmount()` destruye el `ObObject` del MountPoint. Pero **no hay sincronización** entre ambos.

### ⚠ R3: ObCreate File/Directory No Pasa por VFS

**Gravedad: MEDIA**

`handler_ob_create()` (syscall RAX=61):
- Para `ObType::Directory`: llama a `ob_create_object_path()` que registra en Object Manager y namespace. **No llama a `Vfs::mkdir()`**.
- Para pipes: crea pipe y registra.
- Para otros tipos: registra en OM directamente.

El `ObType::Directory` creation registra en el **namespace del Object Manager**, no en el filesystem NeoFS. Un directorio creado vía `ob_create(ObType::Directory, "\\Global\\FileSystem\\C:\\Temp\\newdir")` crea una entrada en el namespace de Ob, pero **no crea un directorio en NeoFS**. Esto es confuso: el path `\Global\FileSystem\C:\...` sugiere que es un path de filesystem, pero la creación es en el namespace Ob, no en el disco.

### ⚠ R4: Cache Fragmentación

**Gravedad: MEDIA**

Hay **tres caches separadas** sin coordinación:

1. **BlockCache** (`buffer/block_cache.rs`): cache de sectores (512B), LRU, usada por NeoFS y FAT32.
2. **PageCache** (`buffer/page_cache.rs`): cache de páginas (4KB), usada por NeoFS para `read_file_to_buf`.
3. **InodeCache** (`fs/neodos_fs.rs:136`): array fijo `[Option<Inode>; 256]`, dentro de `NeoDosFs`.

**Problemas:**
- `InodeCache` vive dentro de `NeoDosFs` y usa `BlockCache` con `partition_base` hardcodeado a `self.abs_lba(0)`.
- `PageCache` es global (`PAGE_CACHE` en globals.rs), pero `NeoDosFs::read_file_to_buf` la usa con `inode_num` como clave — si dos instancias de NeoDosFs (misma partición, drives diferentes) comparten PageCache, pueden mezclar datos.
- No hay `dirty` tracking coordinado entre BlockCache y PageCache.
- `flush_cache_if_needed()` en globals.rs flushea ambos independentemente, pero sin garantía de orden (primero page, luego block).

---

## 4. Separación VFS vs NeoFS

### 4.1 Correctamente Separado

| Concepto | Pertenece a | Archivo |
|----------|-------------|---------|
| `FileSystem` trait | VFS | `fs/vfs.rs:51-80` |
| Path resolution | VFS | `fs/vfs.rs:149-201` |
| Drive table | VFS | `fs/vfs.rs:91-95` |
| Mount sub-mount | VFS | `fs/vfs.rs:84-89, 98-105` |
| `IoStack` | VFS | `vfs/io.rs` |
| `PartitionInfo`/GPT | VFS | `vfs/partition.rs` |
| `VfsError` | VFS | `fs/vfs.rs:9-22` |
| `VfsNode` | VFS | `fs/vfs.rs:37-41` |
| `DirEntry` | VFS | `fs/vfs.rs:46-49` |
| Handle management | VFS/Handle | `handle.rs` |
| `Superblock` | NeoFS | `fs/neodos_fs.rs:13-22` |
| `Inode` | NeoFS | `fs/neodos_fs.rs:72-85` |
| `InodeCache` | NeoFS | `fs/neodos_fs.rs:136-171` |
| `BlockBitmap` | NeoFS | `fs/neodos_fs.rs:29-68` |
| `DirectoryEntry` | NeoFS | `fs/neodos_fs.rs:107-113` |
| `FsError` | NeoFS | `fs/neodos_fs.rs:118-128` |
| Block I/O (read/write sectors) | NeoFS via IoStack | `fs/neodos_fs.rs:182-1169` |
| FSCK | NeoFS | `fs/fsck.rs` |

### 4.2 Infracciones Detectadas

1. **`NeoDosFs` expone métodos de acceso a bloques como `pub`**: `abs_lba()`, `get_inode_block_ptr()`, `inode_data_block_count()`, `find_entry_in_directory()`, etc. son públicos. Deberían ser `pub(crate)` o privados, accedidos solo a través del trait `FileSystem`.

2. **`NeoDosFs::read_file_to_buf()` salta el VFS**: usado directamente por la shell (`DosShell::cat`) y otras partes del kernel, bypassando la capa de handles y VFS.

3. **`FsError` → `VfsError` conversión** manual en `impl FileSystem for NeoDosFs` (línea 1004-1016). Correcto pero frágil — si se añade un nuevo `FsError`, hay que acordarse de mapearlo.

4. **FSCK accede a estructuras internas de NeoFS directamente**: `read_inode()`, `read_superblock()`, `read_dir_entry()` — todo depende del layout de disco de NeoFS. Esto es correcto (FSCK debe ser específico del FS), pero actualmente `fsck.rs` está en `src/fs/` como módulo hermano. Si se añade FSCK para FAT32, habría que duplicar o refactorizar.

---

## 5. Integración con Object Manager

### 5.1 Flujo ObOpen → VFS

```
sys_ob_open (RAX=60)
  → handler_ob_open(ob.rs:110)
    → ob_open_path() (object/mod.rs:322)
      → 1. Namespace lookup (namespace.rs: lookup_path)
      → 2. Si es \Global\FileSystem\:
        → Vfs::resolve_path() (fs/vfs.rs:189)
        → ob_create_object(ObType::Filesystem, path_str, inode, drive, None)
        → namespace::ob_insert_object(path_str, obj_id)
        → ob_reference(obj_id)
      → 3. Crea HandleEntry::ob_object(obj_id, access)
```

### 5.2 Ownership

| Entidad | Propietario | Ciclo de vida |
|---------|-------------|---------------|
| `Vfs` struct | Global static `VFS` | Toda la vida del kernel |
| `Vfs.drives[]` | `Vfs` | Desde mount hasta unmount |
| `Box<dyn FileSystem>` (NeoDosFs) | `Vfs.drives[idx]` | Desde mount hasta unmount |
| `ObObject` (Filesystem) | `OB_TABLE` | Desde `ob_create_object` hasta `ob_close_object` (refcount=0) |
| `HandleEntry.ob_object_id` | `HandleTable` del EPROCESS | Desde alloc_handle hasta close/sys_exit |
| Namespace entry | `OB_NAMESPACE` | Desde insert hasta remove |
| InodeCache | `NeoDosFs` | Toda la vida del FS |
| BlockCache | Global `BLOCK_CACHE` | Toda la vida del kernel |

### 5.3 Riesgos de Ownership

**ObObject creado en ob_open_path NO se destruye al cerrar el handle si hay namespace entry:**
- `ob_open_path()` inserta el ObObject en el namespace con `ob_insert_object()`.
- Cuando se cierra el handle, `HandleEntry::close()` llama `ob_close_object()` que decrementa refcount. Si llega a 0, destruye el ObObject.
- **Pero**: la namespace entry (`\Global\FileSystem\C:\...`) permanece en `OB_NAMESPACE` apuntando a un ObId que ya no existe.
- Siguiente `ob_open_path()` detecta el entry huérfano y lo limpia (línea 336-338: "Remove stale entry"), pero esto es un parche, no un diseño limpio.

**HandleEntry almacena ObId sin verificar validez:**
- Si un ObObject es destruido (refcount=0), cualquier HandleEntry que aún tenga ese ObId queda colgando.
- `HandleEntry::obj_type()` llama `ob_lookup(self.object_id)` que devuelve `None` — el handler debe manejar este caso. Algunos lo hacen, otros no.

---

## 6. Integración con Namespace

### 6.1 Árbol de Namespace

```
\
├── Device\              → contiene dispositivos (Harddisk0, NeoDosVolume0, EspVolume0)
│   ├── NeoDosVolume0
│   ├── EspVolume0
│   ├── Tcp              (objeto de socket)
│   └── Udp              (objeto de socket)
├── DosDevices\          → symlinks a Device\
│   ├── C: → \Device\NeoDosVolume0
│   └── A: → \Device\EspVolume0
├── Global\
│   ├── FileSystem\      → VFS path resolution
│   │   ├── C:\...       (objetos creados dinámicamente por ob_open_path)
│   │   └── A:\...       (objetos creados dinámicamente por ob_open_path)
│   ├── Info\            → objetos de info del sistema
│   │   ├── CpuInfo
│   │   ├── Version
│   │   ├── Memory
│   │   ├── DateTime
│   │   ├── Drives
│   │   ├── Drivers
│   │   └── Keyboard
│   └── Mount\           → MountPoints
│       ├── C:
│       └── A:
├── Driver\              → drivers NEM cargados
├── Ob\                  → Ob jetos internos (Process, Pipe, Thread, etc.)
│   ├── Process\
│   ├── Pipe\
│   ├── Thread\
│   └── ...
├── Registry\            → registry keys
├── Process\             → procesos activos
└── FileSystem\          → filesystems registrados
```

### 6.2 Problemas de Namespace

1. **`\Global\FileSystem\C:\...` mezcla namespace Ob con paths de FS real**. El namespace de Ob es un árbol de objetos kernel, no un filesystem. Mezclar ambos crea ambigüedad: `ob_enum("\Global\FileSystem\C:\")` enumeraría el namespace Ob, no el directorio raíz de NeoFS.

2. **MountPoint objects en `\Global\Mount\` vs `\DosDevices\` vs `\Device\`**: Tres representaciones del mismo mount. Sin sincronización.

3. **No hay `\DosDevices\` consistente**: solo se crean symlinks para C: y A: durante el boot. No hay registro de units adicionales.

---

## 7. Integración con Drivers

### 7.1 Block Devices

- `BLOCK_DEVICES` global: `BlockDeviceManager` con Vec de `Box<dyn BlockDevice>`.
- Drivers NEM se registran vía `hst_register_block_device()` en boot loader (Phase 3.85).
- `IoStack.device_id` es un índice numérico en `BLOCK_DEVICES` — **frágil**: si un dispositivo se desmonta/elimina, los índices cambian y todos los IoStack que referencian ese `device_id` quedan inválidos.

### 7.2 Hot Unload no maneja referencias activas

- `driver_unload` en boot_loader limpia el driver, pero no notifica al VFS ni a los IoStack que referencian sus block devices.
- Si un driver de disco se descarga mientras hay archivos abiertos (handles vivos), los siguientes reads/writes via `IoStack` usarán un índice de dispositivo que ya no es válido.

### 7.3 Falta de abstractión de Driver Registration

- No hay un mecanismo para que un driver notifique al VFS "este device_id ya no es válido".
- `NemBlockDevice` se registra con un `device_id` fijo, pero si se recarga, puede obtener un `device_id` diferente.

---

## 8. Caché

### 8.1 Estado Actual

| Cache | Tipo | Tamaño | Propietario | Política | Dirty tracking |
|-------|------|--------|-------------|----------|----------------|
| BlockCache | Sector (512B) | 64 entradas | Global (`BLOCK_CACHE`) | LRU | Sí |
| PageCache | Página (4KB) | 64 entradas | Global (`PAGE_CACHE`) | LRU | Sí |
| InodeCache | Inode (256B) | 256 entradas | `NeoDosFs` (per-instancia) | Fill-once, nunca invalida | N/A |

### 8.2 Problemas

1. **InodeCache nunca se invalida**: una vez cargado un inodo, permanece en cache aunque otro proceso lo modifique. TOCTOU race potencial.

2. **BlockCache y PageCache duplican datos**: un sector de 512B en BlockCache es parte de una página de 4KB en PageCache. Ambas caches pueden tener el mismo data físico con versiones diferentes.

3. **PageCache usa `inode_num` como clave primaria**: si dos `NeoDosFs` instancias (e.g., C: y D:) tienen archivos con el mismo `inode_num`, habrá colisión. El `PageCache` global no tiene contexto de drive.

4. **No hay write-back coordination**: BlockCache flushea dirty sectors, PageCache flushea dirty pages. Si una página dirty en PageCache contiene sectores también dirty en BlockCache, se escribirán dos veces.

---

## 9. Deuda Técnica

| ID | Descripción | Archivo | Gravedad |
|----|-------------|---------|----------|
| DT-1 | `IoStack` contenidos dentro de `NeoDosFs` crean dependencia directa del FS al dispositivo | `fs/neodos_fs.rs:178` | Media |
| DT-2 | `with_vfs()` + `globals::VFS.lock()` — lock global del VFS, contendedor | `globals.rs:14,25-31` | Alta |
| DT-3 | `NeoDosFs::abs_lba()` recalculado constantemente con `self.io_stack.translate_lba()` | `fs/neodos_fs.rs:182-184` | Baja |
| DT-4 | Hardcodeo de offset de bloques de datos: `200 + (block * 8)` | `fs/neodos_fs.rs:338,339,393,...` | Media |
| DT-5 | `list_directory()`, `read_file()`, `read_file_to_buf()` en `NeoDosFs` son para debug/consola directa, no via VFS | `fs/neodos_fs.rs:319-558` | Baja |
| DT-6 | `KDrive` eliminado del código pero referenciado en AGENTS.md y documentación | — | Baja |
| DT-7 | FSCK (`fs/fsck.rs`) hardcodea layout NeoFS (sector 200, inode table sector 1) | `fs/fsck.rs:32-55` | Media |

---

### VFS Roadmap — Fase 1: Estabilización (Prioridad: CRÍTICA)

*Eliminar riesgos de ownership y dualidad de mounts.*

* [ ] **VFS-1.1. Unificar MountManager** | Prereqs: — | Files: `src/vfs/mount.rs`, `src/fs/vfs.rs`, `src/main.rs`
  - **Descripcion:** Fusionar `Vfs::mount()` con `vfs::mount::vfs_mount()`. Un solo punto de mount/unmount que sincronice Vfs.drives[] + MountPoint creation + \DosDevices symlinks. Actualmente `main.rs` llama a ambos para el mismo mount — eliminar la duplicación.
  - **Severidad:** CRITICO — dos tablas de montaje independientes sin validación cruzada
  - **Tests:** `vfs_mount_dual_sync`, `vfs_mount_unmount_removes_both`

* [ ] **VFS-1.2. Arreglar ownership ObOpen → VFS** | Prereqs: — | Files: `src/object/mod.rs`, `src/handle.rs`
  - **Descripcion:** No crear ObObject persistente en namespace para cada `ob_open_path()`. Usar ObObject efímero (sin namespace entry) para file handles, o crear un "file object" real con ObOperations cuyo `on_destroy` cierre el archivo en el FS subyacente.
  - **Severidad:** CRITICO — namespace entries huérfanos, sin callback de cleanup
  - **Tests:** `vfs_ownership_obid_valid_after_close`, `vfs_ownership_namespace_entry_cleanup`

* [x] **VFS-1.3. Eliminar stale namespace entries** | Prereqs: VFS-1.2 | Files: `src/object/mod.rs`, `src/object/namespace.rs`
  - **Descripcion:** Añadida `ob_remove_by_id()` en namespace que busca y elimina entries por ObId. `ob_destroy_object()` y `ob_close_object()` llaman `ob_remove_by_id()` al destruir. El parche reactivo (línea 336-338 de `object/mod.rs`) queda como safety net.
  - **Severidad:** ALTA — namespace inconsistente ✅
  - **Tests:** `vfs_namespace_cleanup_on_destroy`, `vfs_namespace_cleanup_on_close`, `vfs_namespace_no_orphan_on_close_with_refs`

* [x] **VFS-1.4. HandleTable → ObObject consistency** | Prereqs: — | Files: `src/handle.rs`
  - **Descripcion:** Añadidos `is_valid()` (verifica ObId vivo en Object Manager) e `is_open_and_valid()`. `close()` solo llama `ob_close_object` si `is_valid()`. Corregido `has_ob_object()` que falsamente trataba STDIN/STDOUT como ObObjects. Double-close y stale handles son seguros.
  - **Severidad:** MEDIA — colgar handles puede causar uso-after-free ✅
  - **Tests:** `vfs_ownership_is_valid`, `vfs_ownership_is_valid_after_obj_destroyed`, `vfs_ownership_double_close_safe`, `vfs_ownership_stdio_always_valid`, `vfs_ownership_closed_not_valid`

### VFS Roadmap — Fase 2: Separación de Capas (Prioridad: ALTA)

*VFS puramente genérico, NeoFS puramente específico.*

* [ ] **VFS-2.1. Privatizar métodos de NeoFS** | Prereqs: — | Files: `src/fs/neodos_fs.rs`
  - **Descripcion:** Hacer `abs_lba()`, `find_entry_in_directory()`, `get_inode_block_ptr()`, `inode_data_block_count()`, `directory_byte_span()`, `rebuild_bitmap()` → `pub(crate)` o privados. Solo deben ser accesibles a través del trait `FileSystem`.
  - **Severidad:** ALTA — ruptura de encapsulación, cualquier módulo puede acceder a detalles internos de NeoFS
  - **Tests:** (compilación, no se rompen callers existentes)

* [ ] **VFS-2.2. Refactorizar FSCK** | Prereqs: — | Files: `src/fs/fsck.rs`
  - **Descripcion:** Extraer lógica común de FSCK a un trait `FsckIntegrity` o similar, con implementación para NeoFS. Mover `fs/fsck.rs` a `drivers/fsck_neodos.rs` para que quede junto a su FS. Si se añade FSCK para FAT32, compartir el trait.
  - **Severidad:** MEDIA — FSCK atado a layout de NeoFS, difícil de extender
  - **Tests:** Los 6 tests existentes de FSCK más 2 de integración

* [ ] **VFS-2.3. Eliminar acceso directo a NeoFS desde shell** | Prereqs: — | Files: `src/shell/commands/*.rs`, `src/fs/neodos_fs.rs`
  - **Descripcion:** `DosShell::cat()`, `list_directory()` y otros comandos usan `NeoDosFs` directamente. Deben ir por VFS + handles, no por NeoDosFs directo.
  - **Severidad:** MEDIA — bypass de capa VFS, imposibilita añadir chequeos de seguridad en VFS
  - **Tests:** (funcional — comandos existentes deben seguir funcionando)

* [ ] **VFS-2.4. PageCache con contexto de drive** | Prereqs: — | Files: `src/buffer/page_cache.rs`
  - **Descripcion:** PageCache global usa `inode_num` como clave primaria. Dos instancias de NeoDosFs (C: y D:) con mismo inode_num colisionan. Añadir `drive_idx` a la clave de PageCache.
  - **Severidad:** ALTA — corrupción silenciosa de datos entre drives
  - **Tests:** `vfs_cache_pagecache_drive_context`

### VFS Roadmap — Fase 3: Namespace Consistencia (Prioridad: MEDIA)

* [ ] **VFS-3.1. Separar \Global\FileSystem del Ob namespace** | Prereqs: VFS-1.1 | Files: `src/object/mod.rs`, `src/object/namespace.rs`
  - **Descripcion:** Que `ob_enum("\Global\FileSystem\")` NO enumere el namespace Ob, sino que delegue al VFS para listar directorios reales del filesystem montado.
  - **Severidad:** MEDIA — ambigüedad semántica entre namespace Ob y paths de FS
  - **Tests:** `vfs_namespace_filesystem_isolation`

* [ ] **VFS-3.2. \DosDevices dinámico** | Prereqs: VFS-1.1 | Files: `src/vfs/mount.rs`
  - **Descripcion:** Registrar automáticamente symlinks en `\DosDevices\` para cada nuevo mount. Actualmente solo C: y A: se registran en boot.
  - **Severidad:** BAJA — los mounts adicionales no aparecen en DosDevices
  - **Tests:** `vfs_mount_dosdevices_autoregister`

* [ ] **VFS-3.3. Proteger paths del namespace** | Prereqs: VFS-3.1 | Files: `src/syscall/ob.rs`
  - **Descripcion:** Impedir que `ob_create(ObType::Directory)` cree directorios dentro de `\Global\FileSystem\` — esa ruta debe ser solo para VFS, no para el namespace Ob.
  - **Severidad:** MEDIA — creación de directorios en namespace que parecen de FS pero no son reales
  - **Tests:** `vfs_namespace_protected_paths`

### VFS Roadmap — Fase 4: Drivers y Block Devices (Prioridad: ALTA)

* [ ] **VFS-4.1. Device IDs estables** | Prereqs: — | Files: `src/vfs/io.rs`, `src/drivers/block/mod.rs`
  - **Descripcion:** Usar UUID o nombre simbólico (ej. el nombre del driver + número de serie) para identificar block devices en lugar de índice numérico en Vec. El `IoStack` debe referenciar por nombre, no por índice. Evita invalidación al insertar/eliminar dispositivos.
  - **Severidad:** ALTA — si un dispositivo se elimina, los índices cambian y todos los IoStack quedan inválidos
  - **Tests:** `vfs_iostack_device_id_stable`

* [ ] **VFS-4.2. Hot-unload safety** | Prereqs: VFS-4.1 | Files: `src/drivers/boot_loader/mod.rs`, `src/drivers/driver_runtime.rs`
  - **Descripcion:** Cuando un driver se descarga, notificar al VFS para invalidar IoStacks que referencien sus devices. Marcar NeoDosFs como "stale" si su device se va. Impedir reads/writes adicionales.
  - **Severidad:** ALTA — descarga de driver de disco con archivos abiertos causa uso de device_id inválido
  - **Tests:** `vfs_iostack_stale_device_handling`

* [ ] **VFS-4.3. Refcount de block devices** | Prereqs: VFS-4.1 | Files: `src/drivers/block/mod.rs`
  - **Descripcion:** Llevar contador de referencias a cada block device (cuántos IoStack lo usan). Prevenir unload si refcount > 0.
  - **Severidad:** ALTA — hot unload puede dejar referencias colgadas
  - **Tests:** `driver_stress_load_unload_cycle`

### VFS Roadmap — Fase 5: Caché Unificada (Prioridad: MEDIA)

* [ ] **VFS-5.1. Unificar BlockCache + PageCache** | Prereqs: — | Files: `src/buffer/block_cache.rs`, `src/buffer/page_cache.rs`
  - **Descripcion:** Una sola cache de páginas 4KB con sub-sector dirty tracking. Eliminar duplicación de datos (mismo contenido en ambas caches). Política LRU unificada.
  - **Severidad:** MEDIA — dos caches con datos redundantes, posible incoherencia
  - **Tests:** `vfs_cache_coherency`

* [ ] **VFS-5.2. InodeCache con invalidación** | Prereqs: — | Files: `src/fs/neodos_fs.rs`
  - **Descripcion:** Añadir versión/secuencia en superblock. Invalidar InodeCache cuando versión cambie (otro proceso modificó el inodo). Actualmente la cache nunca invalida — TOCTOU race potencial.
  - **Severidad:** ALTA — stale inode data tras modificación concurrente
  - **Tests:** `vfs_cache_inode_invalidation`

* [ ] **VFS-5.3. Write-back ordenado** | Prereqs: VFS-5.1 | Files: `src/globals.rs`
  - **Descripcion:** Garantizar flush page → flush block en ese orden coordinado. Actualmente ambos flushean independientemente sin orden.
  - **Severidad:** BAJA — posible escritura duplicada, no pérdida de datos
  - **Tests:** `vfs_cache_writeback_order`

### VFS Roadmap — Fase 6: Características (Prioridad: BAJA)

* [ ] **VFS-6.1. Overlay mounts** | Prereqs: VFS-1.1 | Files: `src/fs/vfs.rs`
  - **Descripcion:** Montar un FS sobre otro (capa de solo lectura + escritura). Útil para live CDs, actualizaciones, configuraciones por defecto con override de usuario.
  - **Severidad:** BAJA — feature nueva
  - **Tests:** `vfs_overlay_read_through`, `vfs_overlay_write_copy`

* [ ] **VFS-6.2. Extended attributes VFS** | Prereqs: — | Files: `src/fs/vfs.rs`
  - **Descripcion:** Añadir atributos VFS al trait `FileSystem`: `VfsAttr::ReadOnly`, `VfsAttr::Hidden`, `VfsAttr::System`, `VfsAttr::Archive`. Que coexistan con los atributos específicos de cada FS.
  - **Severidad:** BAJA — feature nueva
  - **Tests:** `vfs_ext_attr_read`, `vfs_ext_attr_write`

* [ ] **VFS-6.3. File notifications via Event Bus** | Prereqs: — | Files: `src/fs/vfs.rs`, `src/eventbus/`
  - **Descripcion:** Emitir eventos de Event Bus para cambios de archivos (crear, borrar, modificar). Permite a drivers y procesos de usuario reaccionar a cambios en el FS.
  - **Severidad:** BAJA — feature nueva
  - **Tests:** `vfs_notify_create`, `vfs_notify_delete`, `vfs_notify_modify`

* [ ] **VFS-6.4. Async VFS operations via IRP** | Prereqs: IRP system estable | Files: `src/fs/vfs.rs`
  - **Descripcion:** Hacer que las operaciones del trait `FileSystem` soporten async via IRP en lugar de solo sync. Permitir lectura/escritura no bloqueante desde el VFS.
  - **Severidad:** BAJA — feature nueva
  - **Tests:** `vfs_async_read`, `vfs_async_write`

### VFS Roadmap — Fase 7: Rendimiento (Prioridad: BAJA)

* [ ] **VFS-7.1. Eliminar lock global de VFS** | Prereqs: — | Files: `src/globals.rs`, `src/fs/vfs.rs`
  - **Descripcion:** Reemplazar `Mutex<Vfs>` con read-write lock o lock-free path resolution. El lock global es contendedor — lecturas concurrentes de directorios diferentes se serializan innecesariamente.
  - **Severidad:** BAJA — optimización
  - **Tests:** `vfs_perf_concurrent_reads`

* [ ] **VFS-7.2. Lookup cache** | Prereqs: — | Files: `src/fs/vfs.rs`
  - **Descripcion:** Cache de resultados de `lookup()` para paths recientes. Evitar recorrer el árbol de directorios en disco repetidamente para paths usados con frecuencia.
  - **Severidad:** BAJA — optimización
  - **Tests:** `vfs_perf_lookup_cache_hit`

* [ ] **VFS-7.3. Path cache** | Prereqs: VFS-7.2 | Files: `src/fs/vfs.rs`
  - **Descripcion:** Cache de `resolve_path()` completa con invalidación por cambio de directorio. Almacenar el resultado (drive_idx, inode) para paths completos.
  - **Severidad:** BAJA — optimización
  - **Tests:** `vfs_perf_path_cache_hit`


## Referencias

- [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md) — invariantes MUST/MUST NOT
- [AGENTS.md](../AGENTS.md) — build, test, convenciones de commit
- [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) — vision a largo plazo v0.40 -> v1.0
- [OBJECT_MANAGER_ARCHITECTURE.md](OBJECT_MANAGER_ARCHITECTURE.md) — diseno completo del Object Manager
- [KERNEL.md](KERNEL.md) — documentacion del kernel
- [NEOFS_AUDIT.md](NEOFS_AUDIT.md) — auditoría NeoFS (2026-06-28)
- [NEOFS_TESTS.md](NEOFS_TESTS.md) — tests propuestos para NeoFS

