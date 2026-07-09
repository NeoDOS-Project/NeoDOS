# NeoDOS — Roadmap

> Items pendientes del roadmap. Los completados están en
> [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md).

> Version actual: **v0.49.0** — Tests: 656 — ABI: v7 — Ob API: RAX 60-76
> Objetivo: v1.0 — executive NT-like arquitectónicamente sólido.
> Leer [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) antes de planificar cambios.
> Fuente de verdad: [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md)

**Próximo milestone: v0.50** (Registry bugfixes + Shell overhaul + NeoFS robustez)

---

## Execution Rules

1. Una fase no empieza hasta que sus prerequisitos estén marcados **[COMPLETED]**.
2. Cada item pendiente incluye: ID, equivalente NT, archivos, prereqs, criterio de aceptación, tests.
3. Al completar un item: actualizar `CHANGELOG.md` y moverlo a `IMPROVEMENTS_COMPLETED.md`.
4. Validar antes de cerrar: `cargo build` en `neodos-kernel/` + `python3 scripts/auto_test.py` + `scripts/check_deps.py`.

### Checklist por item completado

- [ ] Código implementado
- [ ] Tests en `testing.rs` (mínimo 1 por invariante)
- [ ] `auto_test.py` pasa
- [ ] `check_deps.py` pasa
- [ ] `CHANGELOG.md` actualizado
- [ ] `docs/HISTORY.md` actualizado si el cambio es arquitectónico
- [ ] `docs/` actualizado si cambia contrato
- [ ] Movido a `IMPROVEMENTS_COMPLETED.md`

---

## PRIORITY OVERVIEW

| ID | Item | Priority |
|----|------|----------|
| DH3 | ~~Completar libneodos syscall wrappers~~ **[STALE]** | **HIGH** |
| VIO-ARCH | Virtqueue abstraction + modern PCI transport | **HIGH** |
| VIO-NET | VirtIO Network (0x1000) | **HIGH** |
| VIO-9P | VirtIO 9P filesystem (0x1009) | **HIGH** |
| ~~NET-1.5~~ | ~~libneodos SOCKET constants + wrappers~~ **[COMPLETED]** | ~~**HIGH**~~ |
| ~~NET-1.6~~ | ~~Kernel: SocketRecv class 23~~ **[COMPLETED]** | ~~**HIGH**~~ |
| ~~NET-1.8~~ | ~~net.nxl userland network library~~ **[COMPLETED]** | ~~**HIGH**~~ |
| ~~NET-1.15~~ | ~~netcfg.nxe network service~~ **[COMPLETED]** | ~~**HIGH**~~ |
| ~~NET-1.11~~ | ~~dhcp.nxe (userland) — **COMPLETED** como `dhcpd.nxe`: servicio DHCP userspace con DORA completo, lease renewal, APIPA fallback.~~ **[COMPLETED]** | ~~MEDIUM~~ |
| ~~B2.6~~ | ~~Registry defaults in boot~~ **[COMPLETED]** | ~~**HIGH**~~ |
| ~~**B2.7**~~ | ~~**Registry disk persistence**~~ **[COMPLETED]** | ~~**CRITICAL**~~ |
| ~~B4.10~~ | ~~NeoInit Registry-driven config~~ **[COMPLETED]** | ~~**HIGH**~~ |
| SH-ALL | Shell overhaul (quoting, redirection, editor, scripting) | MEDIUM |
| SH-QUOTE | Shell quoting/escaping | MEDIUM |
| SH-REDIR | Shell redirection (>, <, >>) | MEDIUM |
| SH-EDITOR | Shell line editor (ANSI, Ctrl keys, insert) | MEDIUM |
| SH-HISTORY | Shell history persistence | MEDIUM |
| SH-COMPL | Shell completion (filename, path cache) | MEDIUM |
| SH-ENV | Shell env expansion (%VAR%) | MEDIUM |
| SH-PIPE | Pipeline wait + exit codes | MEDIUM |
| SH-BATCH | Shell batch scripting (IF, GOTO, FOR) | MEDIUM |
| SH-TOKEN | Shell tokenizer (quoting, pipes, ; separator) | MEDIUM |
| SH-SEP | Shell semicolon command separator | LOW |
| PKG-1 (P0-P6) | NeoGet v1 — sistema de paquetes `.nxp` (diseño completo, impl diferida a v0.70) | LOW |
| v0.49 | NeoFS v2 (formato NE2 — B-tree, COW, extents, inline data, snapshots) | **HIGH** |
| VFS-3.1 | Separar `\Global\FileSystem` del Ob namespace | MEDIUM |
| VFS-3.3 | Proteger paths del namespace | MEDIUM |
| VFS-5.1 | Unificar BlockCache + PageCache | MEDIUM |
| VFS-5.2 | InodeCache con invalidación | MEDIUM |
| VFS-2.2 | Refactorizar FSCK | MEDIUM |
| ~~VFS-2.3~~ | ~~Eliminar acceso directo a NeoFS desde shell~~ **[COMPLETED]** | ~~MEDIUM~~ |
| ADM-1 | neotop v0.2 (per-thread CPU, I/O stats, network bar) | **HIGH** |
| ADM-2 | neostat (monitor rendimiento histórico) | **HIGH** |
| ADM-3 | neolog (visor event log + EventBus dump) | **HIGH** |
| ADM-4 | neotask (gestor de tareas: kill, priority, spawn) | MEDIUM |
| ADM-5 | neocfg (configuración vía Registry) | MEDIUM |
| ADM-6 | neofs (formatear, label, fsck, stats) | MEDIUM |
| AI-1 | ~~Completar ObInfoClass/ObSetInfoClass enums~~ **[STALE]** | MEDIUM |
| AI-4 | ~~Arreglar TOCTOU race en kobj_register~~ **[STALE]** | MEDIUM |
| B1.1 | Kernel tracing infrastructure | MEDIUM |
| B1.2 | NeoTrace system | MEDIUM |
| ~~B3.3~~ | ~~**DHCP test page fault**~~ **— ~~RESUELTO: DHCP eliminado del kernel.~~ **[COMPLETED]** | ~~HIGH~~ |
| ~~NET-1.16~~ | ~~**Kernel DHCP no progresa**~~ **— ~~RESUELTO: DHCP movido a userspace (`dhcpd.nxe`).~~ **[COMPLETED]** | ~~HIGH~~ |
| ~~NET-1.11~~ | ~~**dhcp.nxe (userland)**~~ **— ~~COMPLETED como `dhcpd.nxe`.~~ **[COMPLETED]** | ~~MEDIUM~~ |
| B3.4 | NTP client | MEDIUM |
| B4.3 | Shell redirection (>, <, >>) | MEDIUM |
| B4.6 | NeoEdit text editor | MEDIUM |
| B4.7 | Shared library per-process binding | MEDIUM |
| B4.9 | NeoShell scripting (.BAT) | MEDIUM |
| B4.11 | NeoInit auto-start services | MEDIUM |
| B5.1 | Module signature validation | MEDIUM |
| B5.2 | Driver permission enforcement | MEDIUM |
| B5.3 | Secure boot chain | MEDIUM |
| NET-1.7 | Kernel: nic_id + ephemeral port | MEDIUM |
| NET-1.9 | ipconfig.nxe | MEDIUM |
| NET-1.10 | ping.nxe | MEDIUM |
| NET-1.11 | dhcp.nxe (userland) | MEDIUM |
| NET-1.16 | Kernel DHCP no progresa en userspace | **HIGH** |
| VIO-BLK2 | VirtIO Block NEM driver | MEDIUM |
| VIO-INPUT | VirtIO Input (0x1013) | MEDIUM |
| DH1 | Actualizar README.md | MEDIUM |
| DH2 | Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md | MEDIUM |
| A3.2 | Kernel debugger (KD) | MEDIUM |
| USR-001..024 | USR Fase 1+2: SAM + Login + SUDO | MEDIUM |
| VIO-CON | VirtIO Console (0x1002) | LOW |
| VIO-RNG | VirtIO RNG (0x1003) | LOW |
| VIO-SCSI | VirtIO SCSI (0x100A) | LOW |
| VIO-GPU | VirtIO GPU (0x1012) | LOW |
| VIO-VSOCK | VirtIO VSOCK (0x1014) | LOW |
| VIO-SOUND | VirtIO Sound (0x1015) | LOW |
| VIO-BALLOON | VirtIO Memory Balloon (0x1004) | LOW |
| VFS-3.2 | `\DosDevices` dinámico | LOW |
| VFS-5.3 | Write-back ordenado | LOW |
| VFS-6.1..6.4 | VFS Features (overlay, attr, notifications, async) | LOW |
| VFS-7.1..7.3 | VFS Performance (lock, lookup cache, path cache) | LOW |
| B6.1 | Zero-copy pipes | LOW |
| B6.2 | Copy-on-write fork | LOW |
| ADM-7 | neoctl: panel de control (dispositivos, servicios, drivers) | LOW |
| ADM-8 | neodebug: frontend Ring 3 del kernel debugger | LOW |
| ADM-9 | neomem v0.2: mapa de memoria avanzado | LOW |
| B4.8 | NeoTOP (v0.1 exists, v0.2+) | LOW |
| B4.10 | Compositor 2D | LOW |
| B7.1..B7.6 | Experimental (GUI, TPM, package mgr, TT debug, hotpatch, DFS) | LOW |
| AI-2 | Consolidate legacy syscall wrappers | LOW |
| AI-3 | ObObjectTable lock granularity | LOW |
| | **Registry (ver `docs/design/registry-improvements.md`)** | |
| CM-FIX | Registry bugfixes (free list, value deletion, unmount flush, iterative delete) | **HIGH** |
| CM-SEC | Registry security (ACL por clave, SeAccessCheck) | MEDIUM |
| CM-DIRTY | Registry per-cell dirty tracking + incremental flush | MEDIUM |
| CM-MULTI | Registry multi-hive (SOFTWARE, SECURITY, DEFAULT) | MEDIUM |
| CM-WAL | Registry WAL (write-ahead logging, crash recovery) | MEDIUM |
| CM-LIB | Registry libneodos wrappers (7 missing wrappers) | MEDIUM |
| CM-REGEDIT | regedit.nxe — registry editor | LOW |
| USR-025..032 | USR Fase 3: Hardening + Grupos | LOW |
| v0.50 | Async I/O y Registry (milestone) | LOW |
| v0.51 | ASLR v2 y Benchmarking | LOW |
| v0.52 | Networking completo (UDP, DNS, DHCP) | LOW |
| v0.53 | Rendimiento (zero-copy pipes, COW fork) | LOW |
| v0.54-v0.59 | Documentación y Hardening | LOW |
| v1.0.0 | API estable | LOW |

---

## CRITICAL

---

## HIGH

### VirtIO Driver Roadmap

> **Estado actual:** Solo VirtIO Block (0x1001) como BOOT_DRIVER inline, PCI legacy I/O BAR,
> virtqueue manual sin abstracción reusable, polling síncrono.
> **Prerrequisito transversal:** VIO-ARCH debe completarse antes de los drivers específicos.

* [ ] **VIO-ARCH. Virtqueue abstraction + modern PCI transport** | Prereqs: A2.1 | Files: `src/virtio/` (new)
  - Capa base reutilizable: virtqueue split vring 1.0, legacy I/O BAR + modern MMIO BAR (VirtIO 1.0+),
    feature negotiation, múltiples virtqueues, indirect descriptors, MSI-X + interrupciones (poll fallback), PCI discovery genérica.
  - **Tests:** `vio_virtqueue_alloc_free`, `vio_virtqueue_submit_chain`, `vio_virtqueue_poll_completion`,
    `vio_modern_bar_detect`, `vio_feature_negotiation`, `vio_msix_configure`

* [ ] **VIO-NET. VirtIO Network (0x1000)** | Prereqs: VIO-ARCH | Files: `src/net/virtio_net.rs` or `drivers/virtio-net/` (NEM)
  - 1 RX + 1 TX virtqueue, mergeable RX buffers, checksum offload, MAC desde config space,
    link status polling, legacy + modern transport. Se integra con `src/net/nic.rs` (trait `NetworkInterface`).
  - **Tests:** `vio_net_probe`, `vio_net_send_recv`, `vio_net_mac_config`

* [ ] **VIO-9P. VirtIO 9P (0x1009)** | Prereqs: VIO-ARCH | Files: `drivers/virtio-9p/` (NEM), `src/fs/9p.rs`
  - Filesystem 9P2000.L sobre VirtIO para compartir directorios host-huésped.
    Operaciones: Tversion, Tattach, Twalk, Topen, Tread, Twrite, Tclunk. Montable vía VFS.
  - **Tests:** `vio_9p_version_attach`, `vio_9p_walk_open_read`, `vio_9p_write_close`

### Networking — Userland tools

* [x] **NET-1.5. libneodos: SOCKET constants + wrappers** | Prereqs: NET-1 F4 | Files: `libneodos/src/syscall.rs`
  - Añadir `ob_type::SOCKET = 18`, `ObInfoClass::SocketInfo`..`SocketRecv`, `ObSetInfoClass::SocketConnect`..`SocketClose`.
    Wrappers: `ob_socket_create/connect/bind/listen/send/recv/close`.
  - **Tests:** compilación, no se rompen callers

* [x] **NET-1.6. Kernel: ObInfoClass::SocketRecv (class 23)** | Prereqs: NET-1 F4 | Files: `src/object/types.rs`, `src/syscall/ob.rs`
  - Handler `ob_query_info` copia `socket.recv_buf` a usuario. Si no hay datos, `-EAGAIN`.
  - **Tests:** `ob_query_info_socket_recv`

* [x] **NET-1.8. net.nxl: userland network library** | Prereqs: NET-1.5/1.6 | Files: `libnet/` (new)
  - NXL slot 3 (`0x1e0c0000`). API: interface_count/get_interface_info/get_stats,
    socket_create/bind/connect/listen/send/recv/close, set_ip/set_gateway.
  - **Tests:** unitarios parsing (mock syscalls)

* [x] **NET-1.15. netcfg.nxe: network service** | Prereqs: NET-1.8, B2.6 | Files: `userbin/netcfg/` (new)
  - Servicio auto-iniciado: carga net.nxl, lee Registry, ejecuta DHCP si toca, aplica IP, persiste resultado.
  - **Tests:** netcfg con DHCP simulado

### Registry

* [x] **B2.6. Valores Registry por defecto en boot** | Prereqs: B2.1 | Files: `src/main.rs`, `src/cm/mod.rs`
  - En Phase 3.881, crear `CurrentControlSet\Services\NeoInit\DefaultShell`,
    `Network\Interfaces\0\DHCPEnabled=1`, etc. Solo si no existen.
  - **Tests:** `cm_default_values_created`

* [ ] **CM-FIX. Registry bugfixes (free list, delete_value, unmount flush, iterative delete)** | Prereqs: -- | Files: `src/cm/hive.rs`, `src/cm/mod.rs`, `src/syscall/cm.rs`
  - Diseño completo en `docs/design/registry-improvements.md` sección 2.1.
  - Fix free list: reemplazar `free_head`/`scan_next_free` roto por next-fit linear scan con `next_alloc_hint`.
  - Cambiar `cells` de `[Option<Cell>; 2048]` a `Vec<Option<Cell>>` (soft max).
  - Añadir `Hive::delete_value()`: desenlazar de lista de valores, liberar celda.
  - Fix `RegistryDeleteValue` handler: llama a `cm_delete_value()` en vez del hack `REG_NONE`.
  - Fix `cm_unload_hive()`: flush dirty data antes de desmontar.
  - Fix `cm_flush_key()` deadlock: evitar doble adquisición de lock.
  - Reemplazar `delete_key()` recursivo por iterativo con `Vec` stack explícito.
  - **Tests:** `cm_free_list_next_fit`, `cm_delete_value`, `cm_delete_value_persist`, `cm_unmount_flush`, `cm_deep_key_deletion_iterative`, `cm_key_deletion_preserves_siblings`

* [ ] **CM-SEC. Registry security (ACL por clave, SeAccessCheck)** | Prereqs: CM-FIX | Files: `src/cm/security.rs` (new), `src/cm/mod.rs`, `src/syscall/cm.rs`
  - Diseño en `docs/design/registry-improvements.md` sección 2.2.
  - Nuevo archivo `src/cm/security.rs` con `cm_check_access()`, `cm_ensure_security()`, `cm_inherit_security()`.
  - En creación de clave: asignar SecurityCell con owner=SID del token, DACL por defecto.
  - En apertura/consulta/escritura/borrado: `SeAccessCheck(token, sec_desc, requested_access)`.
  - Herencia: clave hija copia o referencia la SecurityCell del padre.
  - Admin bypass: token admin accede a cualquier clave.
  - **Tests:** `cm_sec_key_creation_assigns_owner`, `cm_sec_access_granted`, `cm_sec_access_denied`, `cm_sec_inheritance_parent_child`, `cm_sec_explicit_set_via_ob`, `cm_sec_admin_bypass`

* [ ] **CM-DIRTY. Registry per-cell dirty tracking + incremental flush** | Prereqs: CM-FIX | Files: `src/cm/hive.rs`, `src/cm/cache.rs`, `src/cm/mod.rs`
  - Diseño en `docs/design/registry-improvements.md` sección 2.3.
  - Añadir `dirty_cells: BitVec` a Hive (1 bit por slot).
  - `slot_mut()` marca el bit dirty; `serialize_dirty()` escribe solo celdas sucias.
  - Wire `CellCache` existente en `slot()`/`slot_mut()` para evitar linear scan.
  - `cm_flush_key()` usa dirty tracking para flush incremental.
  - **Tests:** `cm_dirty_cell_set_on_write`, `cm_dirty_cleared_after_flush`, `cm_dirty_serialize_only_dirty`, `cm_dirty_full_flush_roundtrip`, `cm_dirty_hive_flag`

* [ ] **CM-MULTI. Registry multi-hive (SOFTWARE, SECURITY, DEFAULT)** | Prereqs: CM-FIX | Files: `src/cm/mod.rs`
  - Diseño en `docs/design/registry-improvements.md` sección 2.4.
  - Montar SOFTWARE hive en `\Registry\Machine\Software` (app/user settings).
  - Montar SECURITY hive en `\Registry\Machine\Security` (políticas, SID cache).
  - Montar DEFAULT hive en `\Registry\User\.Default` (user defaults).
  - Cada hive crea su directorio raíz en namespace Ob durante el mount.
  - **Tests:** `cm_multi_software_mounted`, `cm_multi_hive_isolation`, `cm_multi_cross_hive_path_fails`, `cm_multi_unload_reload`

* [ ] **CM-WAL. Registry WAL (write-ahead logging, crash recovery)** | Prereqs: CM-FIX | Files: `src/cm/wal.rs` (new), `src/cm/mod.rs`
  - Diseño en `docs/design/registry-improvements.md` sección 2.5.
  - Nuevo archivo `src/cm/wal.rs` con `wal_log()`, `wal_replay()`, `wal_checkpoint()`.
  - Cada mutación escribe entrada WAL a `C:\System\Registry\<name>.wal` + fsync antes de aplicar a hive.
  - En mount: si existe `.wal`, hacer replay antes de cargar `.hiv`.
  - Checkpoint: tras flush exitoso, serializar hive, truncar `.wal`.
  - Checksum débil `wrapping_add` reemplazado por CRC32 (NEOHv2).
  - **Tests:** `cm_wal_created_on_mutation`, `cm_wal_replay_on_load`, `cm_wal_truncated_after_flush`, `cm_wal_power_loss_recovery`, `cm_wal_empty_noop`

* [ ] **CM-LIB. Registry libneodos wrappers** | Prereqs: CM-FIX | Files: `libneodos/src/syscall.rs`
  - Diseño en `docs/design/registry-improvements.md` sección 2.6.
  - Añadir 7 wrappers faltantes: `sys_cm_create_key`, `sys_cm_delete_key`, `sys_cm_enum_key`, `sys_cm_enum_value`, `sys_cm_flush_key`, `sys_cm_load_hive`, `sys_cm_unload_hive`.
  - **Tests:** `cm_lib_create_key_wrapper`, `cm_lib_enum_key_wrapper`, `cm_lib_enum_value_wrapper`, `cm_lib_flush_key_wrapper`

* [ ] **CM-REGEDIT. regedit.nxe — registry editor** | Prereqs: CM-LIB | Files: `userbin/regedit/` (new)
  - Diseño en `docs/design/registry-improvements.md` sección 2.6.
  - Navegación de árbol: `REGEDIT <path>` muestra subclaves y valores.
  - Crear/borrar claves: `REGEDIT /CREATE <path>`, `REGEDIT /DELETE <path>`.
  - Set/query valores: `REGEDIT /SET <path> <name> <type> <value>`, `REGEDIT /QUERY <path> <name>`.
  - Flush manual: `REGEDIT /FLUSH <path>`.
  - **Tests:** `regedit_browse_tree`, `regedit_create_delete_key`, `regedit_set_query_value`, `regedit_flush`

### NeoFS v2 (formato "NE2\0")

> **Diseño completo:** `docs/neofs_v2_design.md`
> **Filosofía:** B-tree por directorio, COW (sin journal), extents, inline data, CRC32 de archivo completo, 64 snapshots circulares.
> **No retrocompatible** con NeoFS v1 ("NEOD"). Partición nueva con formato limpio.

* [ ] **NFSv2-BTREE. B-tree persistente genérico** | Prereqs: -- | Files: `src/fs/btree.rs` (nuevo)
  - B-tree con orden configurable, nodos 4KB. Operaciones: insert, lookup, delete, walk inorder.
  - COW en escritura: insert/delete crea nuevos nodos hasta la raíz, devuelve nueva root_lba.
  - Serialización a/desde bloques 4KB con CRC32 por nodo.
  - **Tests:** `btree_insert_lookup`, `btree_delete`, `btree_walk_inorder`, `btree_cow_new_root`, `btree_cow_preserves_old_root`

* [ ] **NFSv2-FREELIST. Free list** | Prereqs: -- | Files: `src/fs/freelist.rs` (nuevo)
  - Lista de regiones libres (start_lba, length). Alocar: first-fit. Liberar: merge con adyacentes.
  - Múltiples nodos si hay muchas regiones (encadenados por next_lba).
  - **Tests:** `freelist_alloc_marks_used`, `freelist_free_reclaims`, `freelist_merge_adjacent`, `freelist_multi_node`

* [ ] **NFSv2-SNAPSHOT. Snapshot table** | Prereqs: NFSv2-BTREE | Files: `src/fs/snapshot.rs` (nuevo)
  - Tabla circular de 64 entradas. CREATE copia root_btree_lba actual. RESTORE cambia superblock.
  - PURGE: vacía tabla (no reclama bloques — GC lazy en alloc_block).
  - **Tests:** `snapshot_create_list`, `snapshot_restore`, `snapshot_circular_64`

* [x] **NFSv2-FILESYSTEM. FileSystem trait impl** | Prereqs: NFSv2-BTREE, NFSv2-FREELIST, NFSv2-SNAPSHOT | Files: `src/fs/neodos_v2.rs`, `src/fs/mod.rs`
  - NeoDosFsV2 con superblock "NE2\0", B-tree directory, extents, inline data, COW.
  - read/lookup/readdir/stat: lookup en B-tree, verificar CRC32.
  - write/create/mkdir: COW: alocar bloques, escribir, nuevo nodo B-tree, nueva raíz.
  - remove_file: eliminar entrada del B-tree. remove_dir: eliminar subárbol completo.
  - rename: modificar nombre en DirEntry, actualizar B-tree.
  - DEL instantáneo (solo B-tree), RD recursivo siempre funciona.
  - Superblock: root_lba, root_version (se incrementa en cada escritura), timestamps.
  - mode bits: R,W,X,S,D (sin DOS attributes, sin 8.3, sin owner/group).
  - Archivos <208 bytes: inline (0 lecturas de disco).
  - **Tests:** `neofs_v2_create_file`, `neofs_v2_write_read_roundtrip`, `neofs_v2_cow_root_version_inc`, `neofs_v2_cow_preserves_old_data`, `neofs_v2_delete_removes_entry`, `neofs_v2_rmdir_recursive`, `neofs_v2_checksum_verify`, `neofs_v2_inline_small_file`, `neofs_v2_large_file_extents`, `neofs_v2_dir_10k_entries`, `neofs_v2_rename_file`, `neofs_v2_mkdir_lookup`, `neofs_v2_write_power_loss_safe`

* [x] **NFSv2-FSCK. fsck para formato NE2** | Prereqs: NFSv2-FILESYSTEM | Files: `src/fs/fsck.rs` (reescribir)
  - Verificar checksum del superblock. Walk completo del B-tree verificando CRC32 de cada nodo.
  - Verificar que freelist + used_blocks = total_blocks. Opción --deep para checksums de datos.
  - Modo repair: reconstruir freelist desde B-tree walk.
  - **Tests:** `neofs_v2_fsck_clean`, `neofs_v2_fsck_corrupt_btree`

* [ ] **NFSv2-SYSCALL. sys_ob_snapshot (RAX 77)** | Prereqs: NFSv2-FILESYSTEM, NFSv2-SNAPSHOT | Files: `src/syscall/ob.rs`, `src/syscall/mod.rs`, `src/object/types.rs`
  - handler_ob_snapshot: CREATE/RESTORE/LIST/PURGE sobre handle del FS raíz.
  - SSDT entry + permission entry. Nuevos ObInfoClass si aplica.
  - **Tests:** `syscall_ob_snapshot_create`, `syscall_ob_snapshot_restore`, `syscall_ob_snapshot_list`

* [ ] **NFSv2-SHELL. Comandos SNAPSHOT para neoshell** | Prereqs: NFSv2-SYSCALL | Files: `userbin/neoshell/`, `libneodos/`
  - `SNAPSHOT LIST`, `SNAPSHOT CREATE`, `SNAPSHOT RESTORE <id>`, `SNAPSHOT PURGE`.
  - Wrappers en libneodos. Posible NXL con lógica de snapshot.
  - **Tests:** `snapshot_shell_create_restore`

* [ ] **NFSv2-MKFS. Herramienta mkfs.neodos (formato NE2)** | Prereqs: NFSv2-FILESYSTEM | Files: `userbin/mkfs/` (o script build)
  - Escribir superblock "NE2\0", B-tree raíz vacío, freelist con todo el espacio libre.
  - Integrar en scripts/build.sh para generar imagen de disco con formato NE2.
  - **Tests:** `mkfs_creates_valid_ne2_superblock`

### System Bootstrap (NeoInit)

* [x] **B4.10. NeoInit: leer Registry para config** | Prereqs: B2.6 | Files: `userbin/neoinit/`
  - NeoInit lee DefaultShell, AutoStartServices, EnableVT, WaitForNetwork desde
    `\Registry\Machine\System\CurrentControlSet\Services\NeoInit`. Eliminar paths hardcodeados.
  - **Tests:** boot con Registry, verificar shell spawn

### Admin Tools (Fase 1 — Monitorización)

> **Base disponible:** `neomem.nxe` v0.1, `neotop.nxe` v0.1 ya existen.
> Ob API (RAX 60-66), info objects `\Global\Info\*`, `\Process` enum, console.nxl.
> Patrón: `sys_ob_open` → `sys_ob_query_info` → `sys_close`. Shell descubre `.NXE` por `PATH`.

* [ ] **ADM-1. neotop v0.2: añadir per-thread CPU, I/O stats, network bar** | Prereqs: -- | Files: `userbin/neotop/`
  - Por proceso: CPU ticks, bytes I/O leídos/escritos, conexiones de red activas.
    Barra de red con tráfico RX/TX por NIC desde `\Global\Info\NicInfo`.
  - **Tests:** `neotop_v0.2_cpu_io_network`

* [ ] **ADM-2. neostat: monitor de rendimiento histórico** | Prereqs: -- | Files: `userbin/neostat/`
  - Terminal dashboard: CPU% (idle/busy), memoria (total/usada/libre), disco (lectura/escritura),
    red (RX/TX). Muestreo periódico con `sys_sleep_ex` + refresco cada 1s.
  - **Tests:** `neostat_displays_all_gauges`

* [ ] **ADM-3. neolog: visor de event log del kernel** | Prereqs: B1.1 | Files: `userbin/neolog/`
  - Dump del trace buffer del kernel + eventos EventBus. Filtro por categoría/nivel/timestamp.
    Timestamps con HPET. Salida paginada (more-like).
  - **Tests:** `neolog_eventbus_dump`, `neolog_trace_filter`


---

## MEDIUM

### v0.49 — NeoFS v2 (formato NE2)

> Ver sección **NeoFS v2 (formato "NE2\0")** en HIGH para la lista completa de items.
> Este hito sustituye el anterior v0.49 (indirect blocks, journaling, checksums) por el rediseño completo NeoFS v2.

### VFS Fase 3: Namespace Consistencia

* [x] **VFS-3.1. Separar `\Global\FileSystem` del Ob namespace** | Prereqs: VFS-1.1 | Files: `src/object/mod.rs`, `src/object/namespace.rs`
  - `ob_enum("\Global\FileSystem\")` debe delegar al VFS, no al namespace Ob.
  - **Tests:** `vfs_namespace_filesystem_isolation`

* [x] **VFS-3.3. Proteger paths del namespace** | Prereqs: VFS-3.1 | Files: `src/syscall/ob.rs`
  - Impedir `ob_create(ObType::Directory)` dentro de `\Global\FileSystem\`.
  - **Tests:** `vfs_namespace_protected_paths`

### VFS Fase 5: Caché Unificada

* [x] **VFS-5.1. Unificar BlockCache + PageCache** | Prereqs: -- | Files: `src/buffer/block_cache.rs`, `src/buffer/page_cache.rs`
  - **[COMPLETED en v0.49 — BlockCache eliminado, PageCache unificado como única capa de caché]**
  - **Tests:** `vfs_cache_coherency`

* [x] **VFS-5.2. InodeCache con invalidación** | Prereqs: -- | Files: `src/fs/neodos_fs.rs`
  - **[COMPLETED en v0.49 — InodeCache.version + check_version() implementados]**
  - **Tests:** `vfs_cache_inode_invalidation`

### VFS Fase 2 (cont.)

* [ ] **VFS-2.2. Refactorizar FSCK** | Prereqs: -- | Files: `src/fs/fsck.rs`
  - Extraer lógica común a trait `FsckIntegrity`, mover a `drivers/fsck_neodos.rs`.
  - **Tests:** 6 tests existentes + 2 de integración

* [x] **VFS-2.3. Eliminar acceso directo a NeoFS desde shell** | Prereqs: -- | Files: `src/shell/commands/*.rs`, `src/fs/neodos_fs.rs`
  - Comandos shell deben ir por VFS + handles, no por NeoDosFs directo. **[COMPLETED en v0.48.x — migración completa a syscalls Ob (RAX 60-66)]**
  - **Tests:** (funcional — comandos existentes deben seguir funcionando)


### Tracing & Observability

* [ ] **B1.1. Kernel tracing infrastructure** | Prereqs: A2.4 | Files: `src/trace/mod.rs`
  - Ampliar TraceBuffer con trace points registrables dinámicamente, filtrado por categoría/nivel,
    dump formateado via serial con timestamps HPET.
  - **Tests:** `trace_register_dynamic_point`, `trace_filter_by_category`, `trace_dump_serial_format`

* [ ] **B1.2. NeoTrace system** | Prereqs: B1.1 | Files: `userbin/neotrace/`
  - Comando `NEOTRACE` con subcomandos START/STOP/DUMP/FILTER.
  - **Tests:** `neotrace_start_stop_toggle`, `neotrace_dump_output`

### Networking (remaining)

* [ ] **NET-1.7. Kernel: nic_id + puerto efímero en socket_create** | Prereqs: NET-1 F4 | Files: `src/syscall/ob.rs`, `src/net/socket.rs`
  - Asignar NIC por defecto y puerto efímero (49152-65535) si no especificado.
  - **Tests:** `socket_auto_port_assign`

* [ ] **NET-1.9. ipconfig.nxe** | Prereqs: NET-1.8 | Files: `userbin/ipconfig/` (new)
  - `IPCONFIG [/ALL]` — interfaces, MAC, IP, gateway, DNS, stats.
  - **Tests:** integración

* [ ] **NET-1.10. ping.nxe** | Prereqs: NET-1.8 | Files: `userbin/ping/` (new)
  - `PING <host> [/n count] [/w ms]`. Socket raw ICMP echo request.
  - **Tests:** ping a QEMU host

* [x] **NET-1.11. dhcpd.nxe (userland DHCP service)** | Prereqs: NET-1.8 | Files: `userbin/dhcpd/` (new)
  - Servicio DHCP userspace: DORA completo sobre socket UDP, lease renewal, APIPA fallback, persistencia en Registry.
  - Kernel DHCP eliminado (`src/net/dhcp.rs`).
  - **Tests:** DORA sequence via QEMU user-mode DHCP.

* [ ] **BUG-NEM-RX: NEM e1000 driver no recibe paquetes** | Files: `drivers/e1000/src/lib.rs`, `neodos-kernel/src/drivers/nem/net_bridge.rs`
  - El driver NEM e1000 envia DISCOVER OK, pero `e1000_poll()` nunca detecta paquetes entrantes (bit DD no seteado).
  - Con kernel e1000 funciona correctamente (DORA completo). Workaround: `default_nic_id()` prefiere kernel e1000.
  - **Causa posible:** `hst_virt_to_phys()` devuelve dir fisica incorrecta para RX descriptor ring, o la coherencia de cache falla.

* [ ] **B3.4. NTP client** | Prereqs: B3.2 | Files: `src/net/ntp.rs`
  - Cliente NTP (RFC 5905, SNTP simplificado). Sincroniza RTC del sistema.
  - **Tests:** `ntp_request_parse_response`, `ntp_offset_calculation`

### Userland

* [ ] **SH-REDIR. Shell redirection (>, <, >>, 2>)** | Prereqs: tokenizer, editor | Files: `userbin/neoshell/src/redir.rs`, `userbin/neoshell/src/tokenizer.rs`
  - Diseño completo en `docs/design/shell-improvements.md` sección 2.6.
  - Tokenizer parsea `>`, `>>`, `<`, `2>` como tokens de redirección.
  - Antes del spawn: abrir archivo target via `ob_open`/`ob_create`, `dup2` sobre el fd correspondiente, spawn.
  - **Tests:** `redirect_stdout_to_file`, `redirect_stdin_from_file`, `redirect_append`, `redirect_stderr`, `redirect_file_not_found`, `redirect_permission_denied`

* [ ] **B4.6. NeoEdit text editor** | Prereqs: A4.7, B4.4 | Files: `userbin/neoedit/`
  - Editor de texto modal Ring 3. Usa `ob_open` + `ob_query_info(ReadContent)` / `ob_set_info(WriteContent)`.
  - **Tests:** `neoedit_open_display`, `neoedit_edit_save`, `neoedit_scroll`

* [ ] **B4.7. Shared library per-process binding** | Prereqs: sys_loadlib | Files: `src/elf.rs`, `libneodos/`
  - Evolucionar NXL slots globales a binding per-process. Cada EPROCESS mantiene su tabla de NXLs.
  - **Tests:** `nxl_per_process_isolation`, `nxl_unload_on_exit`, `nxl_version_coexistence`

* [ ] **SH-BATCH. NeoShell scripting (.BAT)** | Prereqs: SH-QUOTE, SH-REDIR, SH-ENV, SH-SEP | Files: `userbin/neoshell/src/batch.rs`
  - Diseño completo en `docs/design/shell-improvements.md` sección 2.10.
  - Intérprete batch: `ECHO`, `SET`, `IF EXIST/ERRORLEVEL`, `GOTO :label`, `CALL`, `FOR %%F`, `SHIFT`, `REM`, `@`, `PAUSE`.
  - **Tests:** `bat_echo_set`, `bat_if_goto`, `bat_call_subroutine`, `bat_for_loop`, `bat_shift_args`, `bat_pause_resume`

* [ ] **SH-QUOTE. Shell quoting/escaping** | Prereqs: -- | Files: `userbin/neoshell/src/tokenizer.rs`
  - Diseño en `docs/design/shell-improvements.md` sección 2.5.
  - Tokenizer state machine: `"..."` (expande %VAR%), `'...'` (literal), `^` escape, `%%` literal percent.
  - **Tests:** `tokenizer_double_quotes`, `tokenizer_single_quotes_literal`, `tokenizer_escape_char`, `tokenizer_unmatched_quote`

* [ ] **SH-EDITOR. Shell line editor (ANSI)** | Prereqs: -- | Files: `userbin/neoshell/src/editor.rs`
  - Diseño en `docs/design/shell-improvements.md` sección 2.7.
  - Reemplaza readline() actual con `LineEditor`: posicionamiento ANSI real, Ctrl-A/E (home/end), Ctrl-K (kill), Ctrl-U (clear), Ctrl-R (history search), Insert toggle.
  - **Tests:** `editor_basic_input`, `editor_backspace`, `editor_left_right`, `editor_home_end`, `editor_ctrl_k`, `editor_history_search`

* [ ] **SH-HISTORY. Shell history persistence** | Prereqs: SH-EDITOR | Files: `userbin/neoshell/src/history.rs`
  - Diseño en `docs/design/shell-improvements.md` sección 2.8.
  - History manager propio del shell (no console.nxl). Ring buffer dinámico, persistencia en `C:\System\neoshell.hst`.
  - **Tests:** `history_add_retrieve`, `history_prev_next`, `history_persistence_save_load`, `history_max_entries`

* [ ] **SH-ENV. Shell env expansion (%VAR%)** | Prereqs: SH-QUOTE | Files: `userbin/neoshell/src/env.rs`
  - Diseño en `docs/design/shell-improvements.md` sección 2.9.
  - Post-tokenization pass: reemplaza `%VARNAME%` con valor de `EnvStore`. `%%` → literal `%`. Error si variable no existe.
  - **Tests:** `env_simple_expansion`, `env_multiple_expansion`, `env_unknown_var`, `env_literal_percent`, `env_in_redirect_target`

* [ ] **SH-PIPE. Pipeline wait + exit codes** | Prereqs: SH-TOKEN | Files: `userbin/neoshell/src/pipeline.rs`
  - Diseño en `docs/design/shell-improvements.md` sección 2.1.
  - Pipeline espera a todos los procesos vía `ob_wait`, recolecta exit codes, reporta errores.
  - **Tests:** `pipeline_simple_wait`, `pipeline_three_stage`, `pipeline_exit_code_report`, `pipeline_empty_cmd_error`

* [ ] **SH-SEP. Shell semicolon command separator (`;`)** | Prereqs: SH-TOKEN | Files: `userbin/neoshell/src/tokenizer.rs`
  - Token `Semicolon` en tokenizer. `execute_line` divide en comandos por `;` y ejecuta secuencialmente.
  - **Tests:** `semicolon_two_commands`, `semicolon_with_redirect`, `semicolon_mixed_with_pipe`

* [ ] **SH-COMPL. Shell completion (filename, path cache)** | Prereqs: -- | Files: `userbin/neoshell/src/completion.rs`
  - Diseño en `docs/design/shell-improvements.md` sección 2.10.
  - Completion engine con PATH cache (TTL), filename completion para paths con `\` o `/`, thread-safe (sin mutable statics).
  - **Tests:** `completion_command_prefix`, `completion_filename`, `completion_path_cache_hit`, `completion_no_matches`

* [ ] **B4.11. NeoInit: auto-start de servicios** | Prereqs: B4.10 | Files: `userbin/neoinit/`
  - Leer AutoStartServices desde Registry, spawn_detached() para cada uno.
  - **Tests:** Registry con servicio prueba, verificar spawn

### Admin Tools (Fase 2 — Control)

* [ ] **ADM-4. neotask: gestor de tareas** | Prereqs: -- | Files: `userbin/neotask/`
  - Listar procesos con PID/PPID/prioridad/hilos/estado desde `\Process`.
    Matar (`sys_kill` RAX 52), cambiar prioridad (`sys_set_priority` RAX 51),
    crear proceso (`sys_ob_create Process`). Confirmación antes de kill.
  - **Tests:** `neotask_kill_pid`, `neotask_set_priority`, `neotask_spawn`

* [ ] **ADM-5. neocfg: configuración del sistema vía Registry** | Prereqs: B2.6 | Files: `userbin/neocfg/`
  - Navegación de árbol del Registry: `ls`, `cd`, `cat` sobre claves/valores.
    Editar: `set <key> <type> <value>`, `delete <key>`, `create <key>`.
    Usa `sys_cm_open_key`/`create_key`/`query_value`/`set_value`/`enum_key`/`delete_key` (RAX 67-74).
  - **Tests:** `neocfg_read_write_key`, `neocfg_enum_key_value`

* [ ] **ADM-6. neofs: utilidad de filesystem** | Prereqs: -- | Files: `userbin/neofs/`
  - Mostrar estadísticas de volumen desde `ObInfoClass::Drives`.
    Correr `sys_fsck` (RAX 55), cambiar label (`sys_set_volume_label` RAX 54).
    Listar puntos de montaje desde `ObInfoClass::Drives`.
  - **Tests:** `neofs_fsck_drive`, `neofs_format_volume`, `neofs_label_roundtrip`

### Security

* [ ] **B5.1. Module signature validation** | Prereqs: NT6 | Files: `src/drivers/loader.rs`
  - Validación criptográfica de módulos `.nem` antes de cargar.
  - **Tests:** `nem_signature_valid_accepts`, `nem_signature_invalid_rejects`, `nem_signature_tamper_detected`

* [ ] **B5.2. Driver permission enforcement** | Prereqs: NT6.3, B5.1 | Files: `src/drivers/caps.rs`
  - Cruzar capacidad declarada del driver con token del proceso y ACL del objeto.
  - **Tests:** `driver_caps_allow_admin`, `driver_caps_deny_user`, `driver_caps_acl_intersection`

* [ ] **B5.3. Secure boot chain** | Prereqs: B5.1 | Files: `neodos-bootloader/`, `src/boot/secure.rs`
  - Verificación encadenada bootloader → kernel → drivers.
  - **Tests:** `secure_boot_kernel_verified`, `secure_boot_driver_verified`, `secure_boot_fail_closed`

### VirtIO (MEDIUM)

* [ ] **VIO-BLK2. VirtIO Block NEM driver** | Prereqs: VIO-ARCH | Files: `drivers/virtio-blk/` (new, NEM SYSTEM)
  - Reemplazar BOOT_DRIVER inline por NEM standalone. Hotplug multi-dispositivo. MSI-X con DPC.
  - **Tests:** `vio_blk_probe`, `vio_blk_read_write`, `vio_blk_multi_device`

* [ ] **VIO-INPUT. VirtIO Input (0x1013)** | Prereqs: VIO-ARCH | Files: `drivers/virtio-input/` (NEM)
  - Teclado, ratón, tablet vía VirtIO. Integración con `src/input/manager.rs`.
  - **Tests:** `vio_input_key_event`, `vio_input_abs_event`, `vio_input_multi_device`

### USR: Sistema de Usuarios

> Ver USR-001..032 en la tabla de prioridades. Diseño detallado: [docs/security.md](security.md)

| Fase | Items | Dependencias |
|------|-------|-------------|
| F1: SAM + Token NT | USR-001..011 | NT6 existente |
| F2: Login + SUDO | USR-012..024 | Fase 1 |
| F3: Hardening + Grupos | USR-025..032 | Fase 2 |


### Kernel Debugger

* [ ] **A3.2. Kernel debugger (KD)** | Prereqs: A3.1 | Files: `src/debugger/mod.rs`
  - Debugger residente: INT3 breakpoints, hardware watchpoints (DR0-DR3), pause/resume,
    shell commands (DEBUG BREAK/WATCH/CONTINUE/REG/MEM/STACK/SCHED),
    GDB remote protocol stub via serial.
  - **Tests:** `kd_breakpoint_set_and_hit`, `kd_breakpoint_invalid_addr`, `kd_watchpoint_write_detect`,
    `kd_register_snapshot`, `kd_gdb_protocol_qSupported` (5 tests)

### 2026-07-04 Audit: Architectural & API Consistency

* [x] **AUDIT-1. Kernel: Registry info classes not handled** | Files: `src/syscall/ob.rs`, `src/object/types.rs`
  - `ObInfoClass::RegistryKey (21)` and `::RegistryValue (22)` both fall to `_ => Inval` in `handler_ob_query_info`.
  - `ObSetInfoClass::RegistryCreateKey (23)`, `::RegistryDeleteKey (24)`, `::RegistrySetValue (25)`, `::RegistryDeleteValue (26)` all fall to `_ => Inval` in `handler_ob_set_info`.
  - **Tests:** `ob_query_info_registry_key_value`, `ob_set_info_registry_operations`

* [x] **AUDIT-2. libneodos ObInfoClass/ObSetInfoClass out of sync with kernel** | Files: `libneodos/src/syscall.rs`
  - `ObInfoClass` missing 6 variants: `SocketInfo(17)`, `SocketAddr(18)`, `TcpStatus(19)`, `NicInfo(20)`, `RegistryKey(21)`, `RegistryValue(22)`.
  - Naming mismatch: variant 7 is `Cpu` in libneodos vs `CpuInfo` in kernel (same value, different name).
  - `ObSetInfoClass` only defines 11 of 27 constants — missing `Security(3)`, `TimerStart(10)`, `TimerCancel(11)`, `SemaphoreRelease(12)`, `SectionMapView(13)`, `SectionUnmapView(14)`, `SetProcessVt(17)`, and all Socket/Registry set classes.
  - `libneodos::ObInfoClass` missing `#[derive(Debug, Clone, Copy, PartialEq, Eq)]` — kernel has all four.
  - `sys_ob_set_info` takes raw `u32` instead of `ObSetInfoClass` enum — no type safety for userspace.
  - `ob_file_create`/`ob_file_delete` hardcode `15u64`/`16u64` instead of using named constants.
  - **Tests:** `libneodos_ob_info_class_completeness`, `libneodos_ob_set_info_class_completeness`

* [x] **AUDIT-3. Two active mount systems (fs/vfs.rs + vfs/mount.rs)** | Files: `src/fs/vfs.rs`, `src/vfs/mount.rs`
  - `MAX_MOUNTS` is 8 in `fs/vfs.rs` vs 16 in `vfs/mount.rs` — inconsistency for same concept.
  - Drives must be registered in **both** systems; `vfs_mount_filesystem()` calls both `vfs.mount()` AND `MountManager::mount()`, creating risk of inconsistency.
  - **Tests:** `mount_dual_system_consistency`

* [x] **AUDIT-4. Low DPC integration: only dispatched from 2 call sites, no test coverage** | Files: `src/dpc/mod.rs`
  - `dpc_dispatch_pending()` exists at line 176 and is called from `idt.rs:663` and `syscall/mod.rs:240`, but has no test coverage and no DPC queue overflow handling.

* [ ] **AUDIT-5. Dead code: processes.rs vestigial demo code** | Files: `src/processes.rs`
  - `proc_a()`/`proc_b()`/`proc_c()`/`proc_d()` — 4 functions that only print "A", "B", "C", "D" in infinite loops. Zero external references. Vestigial from early prototyping.

* [x] **AUDIT-6. Dead code: slab_container.rs `Slab<T>` unused in production** | Files: `src/slab_container.rs`
  - Removed: `mod slab_container;` from `main.rs`, deleted file, removed test registration from `testing.rs`.

* [x] **AUDIT-7. Dead code: Unused IRP functions** | Files: `src/irp/mod.rs`
  - Removed: `irp_set_chain`, `irp_block_current`, `irp_submit_and_wait`, `irp_sync_read`, `irp_sync_write`, `chain_next` field. Updated docs.

* [x] **AUDIT-8. Dead code: Unused EventBus methods** | Files: `src/eventbus/mod.rs`
  - Removed: `push_event_high`, `push_event_with_dyn_payload`, `pop_priority`, `unregister_handler_by_name`, `queue_available`, `handler_count`, `next_event_id`, `high_queue_available`, `pending_dyn_payloads`, free `register_handler`, free `dispatch_pending`, `DynPayloadEntry`, `dyn_payloads` field, `cleanup_dyn_payload`, `#![allow(dead_code)]`. Kept `push_event_priority`/`register_handler_v2`/`dispatch_one` (transitively used by public API).

* [x] **AUDIT-9. Kernel link address discrepancy in docs** | Files: `docs/ARCHITECTURE.md`, `docs/memory.md`
  - `ARCHITECTURE.md:113` says kernel loads at `0x200000`; `kernel.ld:4` says `. = 0x4000000` (64 MB).
  - `memory.md:79` says kernel_image at `0x100000` — also wrong.

* [x] **AUDIT-10. ObSetInfoClass::Security explicitly unimplemented** | Files: `src/syscall/ob.rs`
  - Handler for `Security=3` exists but returns `NoSys` at line 1543 — code exists but does nothing.

### 2026-07-07 Audit: Comprehensive Project Review (dead code, docs staleness, architecture)

* [ ] **AUDIT-30. Dead code mask: `#![allow(dead_code)]` in main.rs + globals.rs** | Files: `src/main.rs:9`, `src/globals.rs:1`
  - Both files suppress all dead-code warnings for the entire kernel crate. Removing them would reveal additional dead items.
  - **Tests:** (requires no behavioral change — compile-only check)

* [ ] **AUDIT-31. Unused macros + functions + enum variants + constants** | Files: multiple
  - Unused macros: `with_current!` (`scheduler/mod.rs:332`), `trace_irq_enter!`/`trace_irq_exit!` (`trace.rs:130,140`)
  - Unused functions: `register_tests()` in `virtio/mod.rs` (tests silently skipped), `with_cache`/`with_page_cache` in `globals.rs:33/42`, `nic_get_mask` in `net/nic.rs:183`, `socket_next_accept_id` in `net/socket.rs:243` (stub), `pipe_peek_read_closed` in `object/pipe.rs:294`, `clear`/`segment_count` in `scheduler/address_space.rs:120/124`
  - Unused enum variants: `ObError::TableFull` (`object/types.rs:70`), `ObType::EventBus` (`object/types.rs:15`)
  - Unused constant: `PIT_HZ` in `boot_benchmark.rs:21`
  - **Tests:** Remove unused items, verify build

* [ ] **AUDIT-32. 5+ `.expect()` panic paths in production code** | Files: `src/scheduler/mod.rs:485-487`, `src/main.rs:334`, `src/globals.rs:38`, `src/arch/x64/serial.rs:73`, `src/urn/mod.rs:383`
  - Scheduler slot full panics (`EPROCESS table full`, `KTHREAD table full`), block device missing, block cache uninitialized, serial write failure, URN object creation — all crash the kernel instead of returning `Result`.
  - **Tests:** `scheduler_slot_exhaustion_graceful`, `urn_create_failure_propagated`

* [ ] **AUDIT-33. `BIN_BUF` global static mut not re-entrant** | Files: `src/syscall/handlers.rs:79`
  - `BIN_BUF: [u8; 65536]` is a global static mut shared by `sys_exec` and `sys_loadlib`. Two concurrent threads calling exec simultaneously will corrupt each other's binary buffer. Should be per-call heap allocated.
  - **Tests:** `concurrent_exec_no_race`

* [ ] **AUDIT-34. No RAII IRQL guard — 15+ manual raise/lower boilerplate in scheduler** | Files: `src/scheduler/mod.rs`
  - Pattern `raise_irql(DISPATCH_LEVEL); ... lower_irql(old_irql)` repeated 15+ times. Missing `IrqlGuard` type implementing `Drop` could prevent RAII violations and reduce code.
  - **Tests:** `irql_guard_restores_on_drop`, `irql_guard_nested`

* [ ] **AUDIT-35. virtio::register_tests() orphaned — tests silently skipped** | Files: `src/virtio/mod.rs:35`
  - `register_tests()` is defined but never called from `testing.rs`. All virtio tests silently excluded from test suite.
  - **Tests:** Add call to `virtio::register_tests()` in `testing.rs`



* [ ] **AUDIT-47. Non-reentrant IRP pool with wraparound overwrite** | Files: `src/irp/mod.rs:13-14`
  - Pool index = `id % IRP_POOL_SIZE` (mod 64). With monotonic ID counter, a slow IRP could be silently overwritten by a new one when the counter wraps.
  - **Tests:** `irp_pool_wraparound_no_overwrite`

* [ ] **AUDIT-48. Fixed 16 KB kernel stack with no guard page** | Files: `src/scheduler/mod.rs:21`
  - `KERNEL_STACK_SIZE = 16384` with no guard page. Deep call chains risk silent stack overflow (syscall → VFS → FS → block cache → IRP → driver → completion).
  - **Tests:** `kernel_stack_deep_call_overflow_safe`

* [ ] **AUDIT-49. 10 inconsistent fixed-size name buffer sizes** | Files: see below
  - No unified naming policy: names truncated at 8, 24, 32, 128, 248, 255, or 260 bytes depending on context (`object/mod.rs:38`=128, `object/types.rs:184`=32, `object/namespace.rs:79`=24, `syscall/ob.rs:24`=32, `syscall/handlers.rs:21`=260, `drivers/driver_runtime.rs:138`=8, `fs/neodos_fs.rs:237`=248, `nxl.rs:20`=24).
  - **Tests:** `name_buf_truncation_no_panic`

* [ ] **AUDIT-50. 27 `lazy_static!` should migrate to `once_cell`/`LazyLock`** | Files: multiple
  - `lazy_static!` crate is in maintenance mode. 27 usages across kernel should migrate to `once_cell::sync::Lazy` or `std::sync::LazyLock`.
  - **Tests:** (no behavioral change — refactor only)

* [ ] **AUDIT-51. drivers/nem/driver.rs `unregister_all()` does nothing** | Files: `src/drivers/nem/driver.rs:92-98`
  - Loop drains handler list but body is empty — comment says "a full implementation would store the function pointer alongside the name."
  - **Tests:** `nem_unregister_all_purges_handlers`

* [x] **AUDIT-52. Two cache implementations with different APIs** | Files: `src/buffer/block_cache.rs`, `src/buffer/page_cache.rs`
  - BlockCache (512B sectors, LRU linear scan) vs PageCache (4KB pages, LRU double-linked list + hash table). **[COMPLETED en v0.49 — BlockCache eliminado, PageCache unificado]**

### 2026-07-09 Audit: Comprehensive Project Review (dead code, duplicates, docs, architecture)

* [ ] **AUDIT-53. 5× duplicate `crc32()` implementation** | Files: `src/fs/neodos_fs.rs:12`, `src/fs/neodos_io.rs:156`, `src/fs/snapshot.rs:122`, `src/fs/freelist.rs:174`, `src/fs/btree.rs:96`
  - Five byte-for-byte identical CRC32 implementations across the filesystem subsystem. `neodos_io.rs:173` already comments "Re-export crc32 from neodos_fs". Move to shared `fs/crc32.rs` utility module.
  - **Tests:** `crc32_single_implementation`

* [ ] **AUDIT-54. GPT parsing duplicated** | Files: `src/drivers/gpt.rs`, `src/vfs/partition.rs`
  - `read_u64_le`/`read_u32_le`/`read_sector_from_dev` helper functions and GPT partition loop logic are copy-pasted between `drivers/gpt.rs` and `vfs/partition.rs`. Consolidate into `drivers/gpt.rs` and re-export from `vfs/partition.rs`.
  - **Tests:** `gpt_parse_consistent`

* [ ] **AUDIT-55. ABI validation duplicated** | Files: `src/drivers/abi/mod.rs:50-80`, `src/drivers/nem/policy.rs:27-57`
  - `abi::negotiate()` (returns `NegotiationResult`) and `policy::validate_abi()` (returns `Result<(), &str>`) implement the same three overlapping-window checks. Have `validate_abi()` delegate to `abi::negotiate()`.
  - **Tests:** `abi_validation_single_code_path`

* [ ] **AUDIT-56. Dual mount managers (Vfs.mounts + MountManager)** | Files: `src/fs/vfs.rs:84-95`, `src/vfs/mount.rs:38-123`
  - `MountManager` in `vfs/mount.rs` and `Vfs.mounts` in `fs/vfs.rs` coexist with separate `MAX_MOUNTS` values (16 vs 8). Both must be updated for every mount/unmount operation, creating inconsistency risk. Merge into single mount manager.
  - **Tests:** `mount_single_manager`

* [ ] **AUDIT-57. MODE_DIR/MODE_FILE constants defined 3×** | Files: `src/fs/vfs.rs:43-44`, `src/fs/neodos_dir.rs:26-27`, `src/fs/neodos_fs.rs:209-210`
  - `MODE_DIR = 0x40` and `MODE_FILE = 0x80` defined identically in 3 files. Define once in `fs/vfs.rs` (the FileSystem trait module) and import everywhere.
  - **Tests:** (compile-only)

* [ ] **AUDIT-58. Error constants duplicated between libneodos and libneodos-nxl** | Files: `libneodos/src/syscall.rs:3-17`, `libneodos-nxl/src/error.rs:4-18`
  - Same 15 error constants (`EINVAL` through `EBUSY`) with same values defined in both crates. Also `ret()`/`ret_unit()` helpers duplicated with slightly different signatures. Share via common crate.
  - **Tests:** (compile-only)

* [ ] **AUDIT-59. 10+ enums with manual `to_str()` instead of `Display`** | Files: multiple (`panic_classification.rs`, `nem/mod.rs`, `object/types.rs`, `vfs/mount.rs`, `urn/mod.rs`, `drivers/driver_runtime.rs`, `drivers/abi/mod.rs`)
  - Every enum defines `pub fn to_str(self) -> &'static str` with a full match. This is a Rust anti-pattern. Replace all with `impl fmt::Display` or derive macros (`strum`, `Display` derive).
  - **Tests:** (compile-only)

* [ ] **AUDIT-60. iso9660.rs dead filesystem driver** | Files: `src/drivers/iso9660.rs`
  - Full ISO9660 filesystem driver (`Iso9660Driver` + `FileSystem` impl) declared in `drivers/mod.rs` but never imported or instantiated anywhere. Zero callers. Remove or register with VFS.
  - **Tests:** (remove dead code, verify build)

* [ ] **AUDIT-61. debugger/mod.rs GDB stub dead code** | Files: `src/debugger/mod.rs`
  - `gdb_main()` implemented but never called from anywhere. Documented in `IMPROVEMENTS.md` as future A3.2 work. Decide: either remove or add as boot-time feature gate.
  - **Tests:** (behavioral — no-op removal)

* [ ] **AUDIT-62. drivers/nem/drivers/kbd_layout.rs never compiled** | Files: `src/drivers/nem/drivers/kbd_layout.rs`
  - Keyboard layout tables file exists in directory but directory has **no `mod.rs`**, so this file is never included in compilation. Either add `mod` declaration or remove the file.
  - **Tests:** (compile-only)

* [ ] **AUDIT-63. 23 dead functions across kernel** | Files: multiple
  - Dead functions found: `signal_device_event()` (`drivers/mod.rs:51`), `read_bar64()`/`map_bar_mmio()` (`drivers/pci.rs:182/192`), `find_neodos_partition()` (`drivers/gpt.rs:35`), `wait_for_key()`/`read_scancode()` (`drivers/ps2.rs:129/139`), `set_rx_permissions()`/`set_rw_permissions()`/`iter_isolated_regions()`/`format_isolation_info()` (`drivers/isolation.rs:338/345/443/581`), `print_ahci_debug()`/`set_ahci_debug_enabled()` (`boot_benchmark.rs:233/429`), `IoStack::acquire_ref()`/`release_ref()`/`mark_stale()`/`with_device()` (`vfs/io.rs:50/56/62/137`), `find_partitions_by_type()`/`read_u64_le()`/`read_u32_le()` (`vfs/partition.rs:51/32/37`), `vfs_mount()`/`vfs_unmount()`/`vfs_get_mount()`/`vfs_unmount_filesystem()`/`vfs_path_to_mount()` (`vfs/mount.rs:129/133/137/174/193`), `write_indirect_block_all()` (`fs/neodos_fs.rs:386`), `reserve_journal_area()` (`fs/journal.rs:415`).
  - **Tests:** Remove each, verify build

* [ ] **AUDIT-64. `PageCacheLevel` unused enum variants** | Files: `src/vfs/io.rs:9`
  - `PageCacheLevel::L2`, `L3`, `L4` variants never used — only `L1` is referenced. Remove dead variants.
  - **Tests:** (compile-only)

* [ ] **AUDIT-65. Dead struct `CryptoContext`** | Files: `src/vfs/io.rs:16`
  - `pub struct CryptoContext {}` — empty struct with zero callers outside own file. Remove.
  - **Tests:** (compile-only)

* [ ] **AUDIT-66. ARCHITECTURE_SOURCE_OF_TRUTH.md Event struct layout wrong** | Files: `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md:379-390`
  - Documents Event as `source: u8`, `timestamp: u32`, `flags: u16` with no `driver_target` field. Actual code has `source: EventSource` (u32), `timestamp: u64`, `flags: u32`, plus `driver_target: u32` present. This is the **governance document** — must match code exactly.
  - **Tests:** (docs fix only)

* [ ] **AUDIT-67. boot.md KERNEL_VERSION_CODE at v0.10.5** | Files: `docs/boot.md:100`
  - `KERNEL_VERSION_CODE = (10 << 8) | 5 = 0x0A05` corresponds to kernel v0.10.5. Current version is v0.49.0. Update constant to match actual version.
  - **Tests:** (docs fix only)

* [ ] **AUDIT-68. roadmap.md version says v0.48** | Files: `docs/roadmap.md:3`
  - "Current: **v0.48**" should be **v0.49.0**. v0.48 focus items (NeoFS stability NS-1/2, FS-1/2/4) are all completed.
  - **Tests:** (docs fix only)

* [ ] **AUDIT-69. Test count outdated in testing.md and ARCHITECTURAL_VISION.md** | Files: `docs/testing.md:5`, `docs/ARCHITECTURAL_VISION.md:96,778`
  - `testing.md` says "537+ tests across 50+ suites", `ARCHITECTURAL_VISION.md` says "537 tests". Current count is **656** tests. Update all stale counts.
  - **Tests:** (docs fix only)

* [ ] **AUDIT-70. filesystem.md structs missing checksum/version fields** | Files: `docs/filesystem.md:13-79`
  - Superblock (missing `version: u32`), Inode (missing `checksum: u32`), DirectoryEntry (missing `checksum: u8`) all lack fields added in v0.49 FS-6. Update struct layouts.
  - Also: `BLOCK_CACHE` still referenced (removed in v0.49 VFS-5.1).
  - **Tests:** (docs fix only)

* [ ] **AUDIT-71. syscalls.md missing Socket and Registry info classes** | Files: `docs/syscalls.md:284-296`
  - `sys_ob_query_info` docs only mention classes 15, 16 — missing Socket info classes (17-23) and Registry info classes (21-22). `sys_ob_set_info` docs only list classes 4-14 — missing Socket (18-22) and Registry (23-27) set classes.
  - Also: `sys_ob_create` type list missing `Socket=18`.
  - **Tests:** (docs fix only)

* [ ] **AUDIT-72. net/mod.rs monolithic protocol dispatch** | Files: `src/net/mod.rs:68-197`
  - 130-line `net_handle_incoming_packet()` function chains `if/else` on `eth_hdr.is_arp()` / `is_ipv4()`, with all protocol decode logic inlined. Contains 9 `unsafe` pointer casts for header deserialization. Replace with `ProtocolHandler` trait + `HashMap<EtherType, Box<dyn ProtocolHandler>>`.
  - **Tests:** `net_protocol_handler_register`, `net_protocol_dispatch_eth`

* [ ] **AUDIT-73. Storage probe hardcoded to 4 concrete drivers** | Files: `src/drivers/storage_manager.rs:2-5`
  - `init_storage()` directly imports `BootAta`, `BootAhci`, `NvmeDriver`, `VirtIoBlk`. Adding a new storage driver requires modifying this file. Should use PCI vendor/device ID → probe function registry.
  - **Tests:** `storage_probe_registry_add_driver`, `storage_probe_auto_discovery`

* [ ] **AUDIT-74. SPSC ring buffer triplicated** | Files: `src/work_queue.rs`, `src/input/vt.rs` (VtInputQueue), `src/arch/x64/cpu_local.rs` (CpuRunQueue)
  - Three independent implementations of lock-free single-producer single-consumer ring buffer with atomic head/tail. Extract into generic `RingBuf<T, const CAP: usize>` in a shared utility module.
  - **Tests:** `ringbuf_push_pop`, `ringbuf_overflow`, `ringbuf_empty_full`

* [ ] **AUDIT-75. 27 fixed-size arrays across kernel** | Files: multiple
  - Fixed-size arrays identified: `VT_COUNT=4`/`VT_QUEUE_SIZE=4096` (`input/vt.rs`), `MAX_NICS=4`/`MAX_SOCKETS=64`/`MAX_TCP_CONNECTIONS=32` (`net/types.rs`), `NXL_SLOT_COUNT=8` (`nxl.rs`), `MAX_ISOLATED_DRIVERS=16` (`drivers/isolation.rs`), `OB_NAME_LEN=128` (`object/types.rs`), `USER_LIMIT=36MB`/`USER_SLOT_COUNT=32` (`arch/x64/paging.rs`), `BIN_BUF=65536` (`syscall/handlers.rs`), `MAX_BLOCK_DEVICES=8` (`drivers/block.rs`), `drives=[Option;26]` (`fs/vfs.rs`), `MAX_SUBDIR_MOUNTS=8` (`vfs/vfs.rs`). Many should be dynamic `Vec` or growable structures.
  - See also: AUDIT-49 (name buffer sizes), AUDIT-33 (BIN_BUF), AUDIT-48 (kernel stack).
  - **Tests:** per-item migration tests

* [ ] **AUDIT-76. Network unsafe pointer casts (9 occurrences)** | Files: `src/net/mod.rs`
  - Pattern `unsafe { &*(packet.as_ptr().add(N) as *const T) }` used 9× to deserialize raw Ethernet/ARP/IPv4/UDP/TCP/ICMP headers. Use `#[repr(packed)]` structs with safe conversion or a parser combinator.
  - **Tests:** `net_header_safe_deserialize_eth`, `net_header_safe_deserialize_ip`

* [ ] **AUDIT-77. Dual ABI validation code paths** | Files: `src/drivers/abi/mod.rs:50-80`, `src/drivers/nem/policy.rs:27-57`
  - `abi::negotiate()` returns `NegotiationResult` with `Compatible/Incompatible`; `policy::validate_abi()` returns `Result<(), &str>` with identical window checks. Consolidate: `validate_abi()` should call `abi::negotiate()` internally.
  - **Tests:** `abi_validation_single_code_path`

* [ ] **AUDIT-78. `kernel_stack_trace` uses fixed crash buffers** | Files: `src/crash/mod.rs:34,66,70`
  - `stack_trace: [u64; 32]`, `pml4: [u64; 512]`, `trace_events: [CrashTraceEvent; 128]` — large fixed arrays in crash dump struct. Make variable-length with header + offset table.
  - **Tests:** `crash_dump_variable_length`

* [ ] **AUDIT-79. from_u8/from_u16 pattern should use `TryFrom`** | Files: `src/drivers/nem/mod.rs:46-98`
  - `DriverCategory::from_u8`, `NemDriverType::from_u8`, `NemDriverType::from_u16` — manual match-based conversions. Replace with `impl TryFrom<u8/u16>` or `strum::FromRepr`.
  - **Tests:** (compile-only)

* [ ] **AUDIT-80. `lazy_static!` still at 27 usages — migrate to `LazyLock`** | Files: multiple
  - AUDIT-50 flagged 27 `lazy_static!` usages. Still all present. `lazy_static!` crate is in maintenance mode. `std::sync::LazyLock` is stable (Rust 1.80+).
  - **Tests:** (compile-only refactor)

* [ ] **AUDIT-81. `proc_a/b/c/d()` in processes.rs still vestigial** | Files: `src/processes.rs`
  - AUDIT-5 from 2026-07-04 audit remains unaddressed: 4 functions (`proc_a`/`proc_b`/`proc_c`/`proc_d`) that only print letters in infinite loops. Zero external references. Vestigial prototyping code.
  - **Tests:** Remove, verify build

* [ ] **AUDIT-82. `#![allow(dead_code)]` mask still present** | Files: `src/main.rs:9`, `src/globals.rs:1`
  - AUDIT-30 flagged both files suppressing all dead-code warnings for the entire kernel crate. Still present. Remove and fix revealed dead items.
  - **Tests:** (compile-only — remove allow, fix warnings)

* [ ] **AUDIT-83. Toctou in storage device enumeration** | Files: `src/drivers/storage_manager.rs`
  - Storage probe iterates PCI bus for storage devices but has no synchronization if drivers load/unload concurrently during probe. Add `StorageRegistry` with probe lock.
  - **Tests:** `storage_probe_concurrent_safe`---

## LOW

| ID | Item | Files |
|----|------|-------|
| VIO-CON | VirtIO Console (0x1002) | `drivers/virtio-console/` |
| VIO-RNG | VirtIO RNG (0x1003) | `drivers/virtio-rng/` |
| VIO-SCSI | VirtIO SCSI (0x100A) | `drivers/virtio-scsi/` |
| VIO-GPU | VirtIO GPU (0x1012) | `drivers/virtio-gpu/` |
| VIO-VSOCK | VirtIO VSOCK (0x1014) | `drivers/virtio-vsock/` |
| VIO-SOUND | VirtIO Sound (0x1015) | `drivers/virtio-sound/` |
| VIO-BALLOON | VirtIO Memory Balloon (0x1004) | `drivers/virtio-balloon/` |
| VFS-3.2 | `\DosDevices` dinámico | `src/vfs/mount.rs` |
| ADM-7 | neoctl: panel de control (dispositivos, servicios, drivers) | `userbin/neoctl/` |
| ADM-8 | neodebug: frontend Ring 3 del kernel debugger | `userbin/neodebug/` |
| ADM-9 | neomem v0.2: mapa de memoria avanzado (page tables, procesos) | `userbin/neomem/` |
| VFS-5.3 | Write-back ordenado (flush page → flush block) | `src/globals.rs` |
| VFS-6.1 | Overlay mounts | `src/fs/vfs.rs` |
| VFS-6.2 | Extended attributes VFS | `src/fs/vfs.rs` |
| VFS-6.3 | File notifications via Event Bus | `src/fs/vfs.rs`, `src/eventbus/` |
| VFS-6.4 | Async VFS operations via IRP | `src/fs/vfs.rs` |
| VFS-7.1 | Eliminar lock global de VFS | `src/globals.rs`, `src/fs/vfs.rs` |
| VFS-7.2 | Lookup cache | `src/fs/vfs.rs` |
| VFS-7.3 | Path cache | `src/fs/vfs.rs` |
| B6.1 | Zero-copy pipes | `src/pipe.rs` |
| B6.2 | Copy-on-write fork | `src/memory/cow.rs`, `src/syscall.rs` |
| B4.8 | NeoTOP v0.2+ | `userbin/neotop/` |
| B4.10 | Compositor 2D | `userbin/compositor/` |
| B7.1 | Full GUI system | `userbin/gui/` |
| B7.2 | Advanced secure boot (TPM) | `src/boot/tpm.rs` |
| B7.3 | Package manager | `userbin/neopkg/` |
| B7.4 | Time-travel debugging | `src/debugger/timetravel.rs` |
| B7.5 | Live kernel patching | `src/patch/mod.rs` |
| B7.6 | Distributed NeoDOS nodes | `src/cluster/` |
| AUDIT-11 | Dead code: IPI function duplicates in smp.rs (send_ipi, send_ipi_all, send_ipi_all_excl_self — identical to ipi.rs) | `src/arch/x64/smp.rs` |
| AUDIT-12 | Duplicate: AHCI structs defined twice (PrdtEntry, CmdTableInner, CmdHeader, CmdList, RecvFis identical in kernel and NEM driver) | `src/drivers/boot_ahci.rs`, `drivers/ahci/src/lib.rs` |
| AUDIT-13 | Duplicate: PCI config access functions in 7 different files | `src/drivers/pci.rs`, `drivers/*/src/lib.rs` (7 files) |
| AUDIT-14 | Duplicate: HST extern declarations in 8 NEM drivers (shared HAL crate would eliminate boilerplate) | `drivers/*/src/lib.rs` (8 files) |
| AUDIT-15 | Duplicate: PAGE_SIZE = 4096 defined 7 times across kernel | `src/memory/mod.rs`, `src/memory/buddy.rs`, `src/hal/x64/mem.rs`, `src/arch/x64/paging.rs`, `src/drivers/virtio_blk.rs`, `src/drivers/nvme.rs`, `drivers/virtio-blk/src/lib.rs` |
| AUDIT-16 | Multiple error enums with overlapping variants (VfsError, FsError, Fat32Error, Iso9660Error all share NotFound/NotADirectory/NotAFile) | `src/fs/vfs.rs`, `src/fs/neodos_fs.rs`, `src/drivers/fat32.rs`, `src/drivers/iso9660.rs` |
| AUDIT-17 | User address space severely constrained (USER_LIMIT=36MB, MAX_BIN_SIZE=64KB) | `src/arch/x64/paging.rs` |
| AUDIT-18 | Idle loops in main.rs/processes.rs use `loop {}` without `hlt` — burns 100% CPU | `src/main.rs`, `src/processes.rs`, `src/hal/raw/cpu.rs` |
| AUDIT-19 | Global static mut without synchronization (40+ instances; usermode.rs 8 globals risky for SMP) | Multiple files |
| AUDIT-20 | Large files should be split: syscall/ob.rs (2280 lines), syscall/handlers.rs (1771 lines) | `src/syscall/ob.rs`, `src/syscall/handlers.rs` |
| AUDIT-21 | Scheduler panics on table full (`expect("EPROCESS table full")`, `expect("KTHREAD table full")`) should return errors | `src/scheduler/mod.rs` |
| AUDIT-22 | Page cache uses 8+ O(n) linear scans across 128 slots (`for i in 0..DEFAULT_CACHE_SIZE`) instead of O(1) linked-list | `src/buffer/page_cache.rs` |
| AUDIT-23 | docs/ARCHITECTURE.md and docs/drivers.md describe different NEM v3 header layouts, both contradicting actual `NemHeaderV3` struct | `docs/ARCHITECTURE.md`, `docs/drivers.md`, `src/nem/mod.rs` |
| AUDIT-24 | docs/libneodos.md: claims syscall instruction is used, actual code uses int 0x80 | `docs/libneodos.md`, `libneodos/src/syscall.rs` |
| AUDIT-25 | docs/libneodos.md: claims user.ld places code at 0x400000, actual user.ld links at 0 | `docs/libneodos.md`, `userbin/*/user.ld` |
| AUDIT-26 | docs/scheduler.md: CpuRunQueue field names wrong (head/tail vs head_idx/tail_idx), missing count field | `docs/scheduler.md`, `src/arch/x64/cpu_local.rs` |
| AUDIT-27 | docs/objects.md: SocketRecv class 23 documented and DOES exist in kernel enum (re-check; this may be resolved) | `docs/objects.md`, `src/object/types.rs` |
| AUDIT-28 | docs/memory.md: kernel_image base says 0x100000, actual load address is 0x4000000 | `docs/memory.md`, `neodos-kernel/kernel.ld` |
| AUDIT-29 | Version mismatch: AGENTS.md says v0.48.7, kernel Cargo.toml says 0.48.0, CHANGELOG says v0.48.9 | `AGENTS.md`, `neodos-kernel/Cargo.toml`, `CHANGELOG.md` |
| DH-HISTORY | Mantener `docs/HISTORY.md` actualizado con hitos arquitectónicos | `docs/HISTORY.md` |
| AI-2 | Consolidate legacy syscall wrappers | `src/syscall/mod.rs` |
| AI-3 | ObObjectTable lock granularity (lock striping) | `src/object/mod.rs` |
| CM-FIX | Registry bugfixes (free list, value deletion, unmount flush, iterative delete) | `src/cm/hive.rs`, `src/cm/mod.rs`, `src/syscall/cm.rs` |
| CM-SEC | Registry security (ACL por clave, SeAccessCheck) | `src/cm/security.rs` (new), `src/cm/mod.rs`, `src/syscall/cm.rs` |
| CM-DIRTY | Registry per-cell dirty tracking + incremental flush | `src/cm/hive.rs`, `src/cm/cache.rs`, `src/cm/mod.rs` |
| CM-MULTI | Registry multi-hive (SOFTWARE, SECURITY, DEFAULT) | `src/cm/mod.rs` |
| CM-WAL | Registry WAL (write-ahead logging, crash recovery) | `src/cm/wal.rs` (new), `src/cm/mod.rs` |
| CM-LIB | Registry libneodos wrappers (7 missing wrappers) | `libneodos/src/syscall.rs` |
| CM-REGEDIT | regedit.nxe — registry editor | `userbin/regedit/` (new) |
| USR-025..032 | USR Fase 3: runas, secedit, groups, MIC enforcement | `userbin/runas/`, `userbin/secedit/`, `src/security/` |

### Milestones (LOW)

| ID | Item | Prereqs |
|----|------|---------|
| v0.50 | Registry bugfixes + security + multi-hive, Shell overhaul, NeoFS robustez | v0.49 |
| v0.51 | ASLR v2 (stack/heap random), PGO, Benchmarking suite, NTP | v0.50 |
| v0.52 | UDP, DNS, TFTP/NFS básico | v0.51 |
| v0.53 | Per-CPU heaps, scheduler lock-free, zero-copy pipes, COW fork | v0.52 |
| v0.54-v0.59 | Documentación API, test coverage >95%, fuzzing, signatures | v0.53 |
| v1.0.0 | Primera API estable. Todo lo anterior COMPLETED. | v0.54-v0.59 |

---

## REFERENCE — Design docs and removed content

### USR System (SAM + Login + SUDO)
Replaced with reference in MEDIUM section. Full design: [docs/security.md](security.md).

### System Tools (Admin Suite)
- **DONE:** `neomem.nxe` v0.1, `neotop.nxe` v0.1.
- **En roadmap:** ADM-1..3 (HIGH, Fase 1 Monitorización), ADM-4..6 (MEDIUM, Fase 2 Control),
  ADM-7..9 (LOW, Fase 3 Avanzado). Ver secciones correspondientes arriba.

### Objectification Roadmap
Mostly completed. See [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md) for:
- OBF-01..12 (Fase 1 + Fase 2 Ob: Thread, Timer, Semaphore, Section)
- X7 (Object Manager unification — handles, KOBJ, URN, security)
- All 16 ObTypes defined, 7 Ob syscalls (RAX 60-66)

### QEMU Bridge Infrastructure
- **scripts/setup-network.sh** — One-time setup: creates `neodos0` bridge via NetworkManager,
  registers with `qemu-bridge-helper`, configures NAT (nftables/iptables), enables IP forwarding,
  adds user to `kvm` group. After setup, QEMU runs without `sudo`.
- **scripts/qemu-debug.sh** — Updated: `--bridge` flag uses `qemu-bridge-helper` (SUID root)
  for runtime TAP creation. Falls back to SLiRP if bridge not found. `--tap` flag for raw TAP.
- **docs/qemu-setup.md** — Full documentation: architecture, security analysis, distribution
  differences, troubleshooting, comparison of all approaches.
- **Design decision:** `qemu-bridge-helper` (SUID root, per-bridge ACL in bridge.conf) chosen
  over `setcap cap_net_admin+ep` (broad, hard to audit) and raw TAP (per-session setup).

### NeoFS Audit
NeoFS v1 is obsolete and has been removed. See [neofs_v2_design.md](neofs_v2_design.md) for the current native filesystem format.
### VFS Architecture Audit
Detailed findings in the VFS sections above. Key risks (R1-R4) resolved or tracked as VFS-2.*/VFS-5.* items.

### Stability Audit (v0.46.7)
All critical fixes applied: handle leaks in `handler_exit`/`kill_pid`, fd leak in `resolve_path()`,
fd overflow prevention, slab double-free detection, `rdtsc` workaround for QEMU TCG timer delivery.
See `CHANGELOG.md` for details.

---

## See also

- `docs/` for full subsystem design docs (architecture, kernel, objects, syscalls, scheduler, memory, drivers, filesystem, registry, security, shell, IPC, network, testing, HAL, interrupts)
- `skills/` for task checklists (build, syscalls, object-manager, scheduler, memory, shell, drivers, filesystem, testing, review, documentation, release)
- [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md) — invariants MUST/MUST NOT
- [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) — long-term strategy v0.40→v1.0
- [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md) — completed roadmap items
