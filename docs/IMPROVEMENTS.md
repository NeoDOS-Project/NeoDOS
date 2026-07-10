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
2. Cada item pendiente incluye: ID, archivos, prereqs, tests.
3. Al completar: actualizar `CHANGELOG.md` y mover a `IMPROVEMENTS_COMPLETED.md`.
4. Validar: `cargo build` + `python3 scripts/auto_test.py` + `scripts/check_deps.py`.

---

## PRIORITY OVERVIEW

| ID | Item | Prio | Cat |
|----|------|------|-----|
| **v0.50** | **Registry bugfixes (CM-FIX) + Shell overhaul + NeoFS snapshot syscall** | **HIGH** | milestone |
| CM-FIX | Registry bugfixes (free list, delete_value, unmount flush, iterative delete) | **HIGH** | registry |
| NFSv2-SYSCALL | sys_ob_snapshot (RAX 77) | **HIGH** | fs |
| SH-TOKEN | Shell tokenizer (quoting, pipes, ; separator) | **HIGH** | shell |
| SH-QUOTE | Shell quoting/escaping | **HIGH** | shell |
| SH-REDIR | Shell redirection (>, <, >>) | **HIGH** | shell |
| B4.11 | NeoInit auto-start servicios | **HIGH** | boot |
| AUDIT-32 | 5+ `.expect()` panic paths → Result | **HIGH** | kernel |
| | | | |
| **v0.51** | **NeoFS v2 remaining + Shell + Networking tools** | **MEDIUM** | milestone |
| NFSv2-BTREE | B-tree persistente genérico | MEDIUM | fs |
| NFSv2-FREELIST | Free list allocator | MEDIUM | fs |
| NFSv2-SNAPSHOT | Snapshot table (64 circular) | MEDIUM | fs |
| NFSv2-SHELL | Comandos SNAPSHOT en neoshell | MEDIUM | shell |
| NFSv2-MKFS | Herramienta mkfs.neodos | MEDIUM | fs |
| SH-EDITOR | Shell line editor (ANSI, Ctrl keys, insert) | MEDIUM | shell |
| SH-HISTORY | Shell history persistence | MEDIUM | shell |
| SH-ENV | Shell env expansion (%VAR%) | MEDIUM | shell |
| SH-PIPE | Pipeline wait + exit codes | MEDIUM | shell |
| SH-BATCH | Shell batch scripting (IF, GOTO, FOR) | MEDIUM | shell |
| SH-SEP | Shell semicolon separator | MEDIUM | shell |
| SH-COMPL | Shell completion (filename, path cache) | MEDIUM | shell |
| NET-1.9 | ipconfig.nxe | MEDIUM | net |
| NET-1.10 | ping.nxe | MEDIUM | net |
| B3.4 | NTP client | MEDIUM | net |
| ADM-1 | neotop v0.2 (per-thread CPU, I/O, network) | MEDIUM | admin |
| ADM-2 | neostat (monitor rendimiento histórico) | MEDIUM | admin |
| ADM-4 | neotask (gestor de tareas) | MEDIUM | admin |
| ADM-5 | neocfg (config vía Registry) | MEDIUM | admin |
| ADM-6 | neofs (formatear, label, fsck, stats) | MEDIUM | admin |
| | | | |
| **v0.52** | **VirtIO + Performance + Security** | **MEDIUM** | milestone |
| VIO-ARCH | Virtqueue abstraction + modern PCI transport | **HIGH** | drivers |
| VIO-NET | VirtIO Network (0x1000) | **HIGH** | drivers |
| VIO-9P | VirtIO 9P filesystem (0x1009) | MEDIUM | drivers |
| VIO-BLK2 | VirtIO Block NEM driver | MEDIUM | drivers |
| VIO-INPUT | VirtIO Input (0x1013) | MEDIUM | drivers |
| B6.1 | Zero-copy pipes | MEDIUM | kernel |
| B5.1 | Module signature validation | MEDIUM | security |
| B5.2 | Driver permission enforcement | MEDIUM | security |
| CM-SEC | Registry security (ACL por clave) | MEDIUM | registry |
| CM-DIRTY | Registry per-cell dirty tracking | MEDIUM | registry |
| CM-MULTI | Registry multi-hive (SOFTWARE, SECURITY, DEFAULT) | MEDIUM | registry |
| B4.6 | NeoEdit text editor | MEDIUM | userland |
| B4.7 | Shared library per-process binding | MEDIUM | userland |
| A3.2 | Kernel debugger (KD) | MEDIUM | kernel |
| VFS-2.2 | Refactorizar FSCK | MEDIUM | fs |
| | | | |
| **v0.53** | **Hardening + Multi-hive + Docs** | **LOW** | milestone |
| B5.3 | Secure boot chain | LOW | security |
| CM-WAL | Registry WAL (write-ahead logging) | LOW | registry |
| CM-LIB | Registry libneodos wrappers (7 missing) | LOW | lib |
| CM-REGEDIT | regedit.nxe — registry editor | LOW | admin |
| USR-001..024 | USR Fase 1+2: SAM + Login + SUDO | LOW | security |
| B1.1 | Kernel tracing infrastructure | LOW | kernel |
| B1.2 | NeoTrace system | LOW | kernel |
| ADM-3 | neolog (visor event log) | LOW | admin |
| NET-1.7 | Kernel: nic_id + ephemeral port | LOW | net |
| BUG-NEM-RX | NEM e1000 no recibe paquetes | LOW | drivers |
| B6.2 | Copy-on-write fork | LOW | kernel |
| VFS-3.2 | `\DosDevices` dinámico | LOW | fs |
| VFS-5.3 | Write-back ordenado | LOW | fs |
| VFS-6.1..6.4 | VFS Features (overlay, attr, notifications, async) | LOW | fs |
| VFS-7.1..7.3 | VFS Performance (lock, lookup cache, path cache) | LOW | fs |
| ADM-7 | neoctl: panel de control | LOW | admin |
| ADM-8 | neodebug: frontend KD | LOW | admin |
| ADM-9 | neomem v0.2 | LOW | admin |
| B4.8 | NeoTOP v0.2+ | LOW | admin |
| B4.12 | Compositor 2D | LOW | userland |
| B7.1..B7.6 | Experimental (GUI, TPM, package mgr, etc.) | LOW | xp |
| USR-025..032 | USR Fase 3: Hardening + Grupos | LOW | security |
| DH1 | Actualizar README.md | LOW | docs |
| DH2 | Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md | LOW | docs |
| DH3 | Completar libneodos syscall wrappers | LOW | lib |
| DH-HISTORY | Mantener docs/HISTORY.md actualizado | LOW | docs |
| AI-1 | Completar ObInfoClass/ObSetInfoClass enums | LOW | ob |
| AI-2 | Consolidate legacy syscall wrappers | LOW | syscall |
| AI-3 | ObObjectTable lock granularity | LOW | ob |
| AI-4 | Arreglar TOCTOU race en kobj_register | LOW | ob |
| AUDIT-11 | IPI function duplicates in smp.rs | LOW | cleanup |
| AUDIT-12 | AHCI structs defined twice | LOW | cleanup |
| AUDIT-13 | PCI config access in 7 files | LOW | cleanup |
| AUDIT-14 | HST extern in 8 NEM drivers | LOW | cleanup |
| AUDIT-15 | PAGE_SIZE defined 7 times | LOW | cleanup |
| AUDIT-16 | Error enums overlapping variants | LOW | cleanup |
| AUDIT-17 | User address space constrained (36MB) | LOW | kernel |
| AUDIT-18 | Idle loops without `hlt` | LOW | cleanup |
| AUDIT-19 | Global static mut without sync (40+) | LOW | cleanup |
| AUDIT-20 | Large files: syscall/ob.rs, syscall/handlers.rs | LOW | cleanup |
| AUDIT-21 | Scheduler panics on table full | LOW | cleanup |
| AUDIT-22 | Page cache O(n) linear scans | LOW | cleanup |
| AUDIT-23 | NEM v3 header docs contradict code | LOW | docs |
| AUDIT-24 | libneodos.md: syscall instruction vs int 0x80 | LOW | docs |
| AUDIT-25 | libneodos.md: user.ld base addr wrong | LOW | docs |
| AUDIT-26 | scheduler.md: CpuRunQueue field names wrong | LOW | docs |
| AUDIT-27 | objects.md: SocketRecv class 23 (re-check) | LOW | docs |
| AUDIT-28 | memory.md: kernel_image base wrong | LOW | docs |
| AUDIT-29 | Version mismatch in AGENTS/Cargo/CHANGELOG | LOW | docs |
| PKG-1 | NeoGet v1 (diferido a v0.70) | LOW | xp |
| VIO-CON | VirtIO Console (0x1002) | LOW | drivers |
| VIO-RNG | VirtIO RNG (0x1003) | LOW | drivers |
| VIO-SCSI | VirtIO SCSI (0x100A) | LOW | drivers |
| VIO-GPU | VirtIO GPU (0x1012) | LOW | drivers |
| VIO-VSOCK | VirtIO VSOCK (0x1014) | LOW | drivers |
| VIO-SOUND | VirtIO Sound (0x1015) | LOW | drivers |
| VIO-BALLOON | VirtIO Memory Balloon (0x1004) | LOW | drivers |

---

## HIGH

### v0.50: Registry bugfixes + Shell + NeoFS snapshot

#### Registry

* [ ] **CM-FIX. Registry bugfixes** | Prereqs: -- | Files: `src/cm/hive.rs`, `src/cm/mod.rs`, `src/syscall/cm.rs`
  - Fix free list: reemplazar `free_head`/`scan_next_free` por next-fit linear scan con `next_alloc_hint`.
  - Cambiar `cells` de `[Option<Cell>; 2048]` a `Vec<Option<Cell>>` (soft max).
  - Añadir `Hive::delete_value()`: desenlazar de lista de valores, liberar celda.
  - Fix `RegistryDeleteValue` handler: llama a `cm_delete_value()` en vez del hack `REG_NONE`.
  - Fix `cm_unload_hive()`: flush dirty data antes de desmontar.
  - Fix `cm_flush_key()` deadlock: evitar doble adquisición de lock.
  - Reemplazar `delete_key()` recursivo por iterativo con `Vec` stack explícito.
  - **Tests:** `cm_free_list_next_fit`, `cm_delete_value`, `cm_delete_value_persist`, `cm_unmount_flush`, `cm_deep_key_deletion_iterative`, `cm_key_deletion_preserves_siblings`

#### NeoFS v2

* [ ] **NFSv2-SYSCALL. sys_ob_snapshot (RAX 77)** | Prereqs: NFSv2-FILESYSTEM, NFSv2-SNAPSHOT | Files: `src/syscall/ob.rs`, `src/syscall/mod.rs`, `src/object/types.rs`
  - handler_ob_snapshot: CREATE/RESTORE/LIST/PURGE sobre handle del FS raíz.
  - SSDT entry + permission entry. Nuevos ObInfoClass si aplica.
  - **Tests:** `syscall_ob_snapshot_create`, `syscall_ob_snapshot_restore`, `syscall_ob_snapshot_list`

#### Shell (Phase 1 — foundation)

* [ ] **SH-TOKEN. Shell tokenizer** | Prereqs: -- | Files: `userbin/neoshell/src/tokenizer.rs`
  - Diseño en `docs/design/shell-improvements.md`. Tokenizer state machine para pipes, redirects, quoting.
  - **Tests:** `tokenizer_pipe`, `tokenizer_redirect`, `tokenizer_quoted_arg`

* [ ] **SH-QUOTE. Shell quoting/escaping** | Prereqs: -- | Files: `userbin/neoshell/src/tokenizer.rs`
  - `"..."` (expande %VAR%), `'...'` (literal), `^` escape, `%%` literal percent.
  - **Tests:** `tokenizer_double_quotes`, `tokenizer_single_quotes_literal`, `tokenizer_escape_char`, `tokenizer_unmatched_quote`

* [ ] **SH-REDIR. Shell redirection (>, <, >>, 2>)** | Prereqs: SH-TOKEN | Files: `userbin/neoshell/src/redir.rs`, `userbin/neoshell/src/tokenizer.rs`
  - Tokenizer parsea `>`, `>>`, `<`, `2>`. Antes del spawn: abrir archivo target via `ob_open`/`ob_create`, `dup2` sobre el fd, spawn.
  - **Tests:** `redirect_stdout_to_file`, `redirect_stdin_from_file`, `redirect_append`, `redirect_stderr`, `redirect_file_not_found`, `redirect_permission_denied`

#### Boot

* [ ] **B4.11. NeoInit: auto-start de servicios** | Prereqs: B4.10 (NeoInit Registry config, completed) | Files: `userbin/neoinit/`
  - Leer AutoStartServices desde Registry, spawn_detached() para cada uno.
  - **Tests:** Registry con servicio prueba, verificar spawn

#### Kernel Hardening

* [ ] **AUDIT-32. 5+ `.expect()` panic paths → Result<()>** | Files: `src/scheduler/mod.rs:485-487`, `src/main.rs:334`, `src/globals.rs:38`, `src/arch/x64/serial.rs:73`, `src/urn/mod.rs:383`
  - Scheduler slot full, block device missing, serial write failure — all crash the kernel instead of returning `Result`.
  - **Tests:** `scheduler_slot_exhaustion_graceful`, `urn_create_failure_propagated`

---

## MEDIUM

### v0.51: NeoFS v2 remaining + Shell Phase 2 + Networking tools

#### NeoFS v2 (completar implementación)

* [ ] **NFSv2-BTREE. B-tree persistente genérico** | Prereqs: -- | Files: `src/fs/btree.rs`
  - B-tree con orden configurable, nodos 4KB. Operaciones: insert, lookup, delete, walk inorder.
  - COW en escritura: insert/delete crea nuevos nodos hasta la raíz, devuelve nueva root_lba.
  - **Tests:** `btree_insert_lookup`, `btree_delete`, `btree_walk_inorder`, `btree_cow_new_root`, `btree_cow_preserves_old_root`

* [ ] **NFSv2-FREELIST. Free list** | Prereqs: -- | Files: `src/fs/freelist.rs`
  - Lista de regiones libres (start_lba, length). Alocar: first-fit. Liberar: merge con adyacentes.
  - **Tests:** `freelist_alloc_marks_used`, `freelist_free_reclaims`, `freelist_merge_adjacent`, `freelist_multi_node`

* [ ] **NFSv2-SNAPSHOT. Snapshot table** | Prereqs: NFSv2-BTREE | Files: `src/fs/snapshot.rs`
  - Tabla circular de 64 entradas. CREATE copia root_btree_lba actual. RESTORE cambia superblock.
  - **Tests:** `snapshot_create_list`, `snapshot_restore`, `snapshot_circular_64`

* [ ] **NFSv2-MKFS. Herramienta mkfs.neodos** | Prereqs: NFSv2-FILESYSTEM | Files: `userbin/mkfs/` (o script build)
  - Escribir superblock "NE2\0", B-tree raíz vacío, freelist con todo el espacio libre.
  - **Tests:** `mkfs_creates_valid_ne2_superblock`

* [ ] **VFS-2.2. Refactorizar FSCK** | Prereqs: -- | Files: `src/fs/fsck.rs`
  - Extraer lógica común a trait `FsckIntegrity`, mover a `drivers/fsck_neodos.rs`.
  - **Tests:** 6 tests existentes + 2 de integración

#### Shell (Phase 2 — editor, env, pipeline)

* [ ] **SH-EDITOR. Shell line editor (ANSI)** | Prereqs: -- | Files: `userbin/neoshell/src/editor.rs`
  - Reemplaza readline() con `LineEditor`: posicionamiento ANSI, Ctrl-A/E (home/end), Ctrl-K (kill), Ctrl-U (clear), Ctrl-R (history search), Insert toggle.
  - **Tests:** `editor_basic_input`, `editor_backspace`, `editor_left_right`, `editor_home_end`, `editor_ctrl_k`, `editor_history_search`

* [ ] **SH-HISTORY. Shell history persistence** | Prereqs: SH-EDITOR | Files: `userbin/neoshell/src/history.rs`
  - Ring buffer dinámico, persistencia en `C:\System\neoshell.hst`.
  - **Tests:** `history_add_retrieve`, `history_prev_next`, `history_persistence_save_load`, `history_max_entries`

* [ ] **SH-ENV. Shell env expansion (%VAR%)** | Prereqs: SH-QUOTE | Files: `userbin/neoshell/src/env.rs`
  - Post-tokenization pass: reemplaza `%VARNAME%` con valor de `EnvStore`.
  - **Tests:** `env_simple_expansion`, `env_multiple_expansion`, `env_unknown_var`, `env_literal_percent`, `env_in_redirect_target`

* [ ] **SH-PIPE. Pipeline wait + exit codes** | Prereqs: SH-TOKEN | Files: `userbin/neoshell/src/pipeline.rs`
  - Pipeline espera a todos los procesos vía `ob_wait`, recolecta exit codes, reporta errores.
  - **Tests:** `pipeline_simple_wait`, `pipeline_three_stage`, `pipeline_exit_code_report`, `pipeline_empty_cmd_error`

* [ ] **SH-SEP. Shell semicolon command separator (`;`)** | Prereqs: SH-TOKEN | Files: `userbin/neoshell/src/tokenizer.rs`
  - Token `Semicolon` en tokenizer. `execute_line` divide en comandos por `;` y ejecuta secuencialmente.
  - **Tests:** `semicolon_two_commands`, `semicolon_with_redirect`, `semicolon_mixed_with_pipe`

* [ ] **SH-COMPL. Shell completion** | Prereqs: -- | Files: `userbin/neoshell/src/completion.rs`
  - Completion engine con PATH cache (TTL), filename completion para paths con `\` o `/`.
  - **Tests:** `completion_command_prefix`, `completion_filename`, `completion_path_cache_hit`, `completion_no_matches`

* [ ] **SH-BATCH. Shell batch scripting (.BAT)** | Prereqs: SH-QUOTE, SH-REDIR, SH-ENV, SH-SEP | Files: `userbin/neoshell/src/batch.rs`
  - Intérprete batch: `ECHO`, `SET`, `IF EXIST/ERRORLEVEL`, `GOTO :label`, `CALL`, `FOR %%F`, `SHIFT`, `REM`, `@`, `PAUSE`.
  - **Tests:** `bat_echo_set`, `bat_if_goto`, `bat_call_subroutine`, `bat_for_loop`, `bat_shift_args`, `bat_pause_resume`

#### Networking — Userland tools

* [ ] **NET-1.9. ipconfig.nxe** | Prereqs: NET-1.8 | Files: `userbin/ipconfig/` (new)
  - `IPCONFIG [/ALL]` — interfaces, MAC, IP, gateway, DNS, stats.
  - **Tests:** integración

* [ ] **NET-1.10. ping.nxe** | Prereqs: NET-1.8 | Files: `userbin/ping/` (new)
  - `PING <host> [/n count] [/w ms]`. Socket raw ICMP echo request.
  - **Tests:** ping a QEMU host

* [ ] **B3.4. NTP client** | Prereqs: -- | Files: `src/net/ntp.rs`
  - Cliente NTP (RFC 5905, SNTP simplificado). Sincroniza RTC del sistema.
  - **Tests:** `ntp_request_parse_response`, `ntp_offset_calculation`

#### Admin Tools (Fase 1 — Monitorización + Control)

* [ ] **ADM-1. neotop v0.2** | Prereqs: -- | Files: `userbin/neotop/`
  - Añadir per-thread CPU, I/O stats, network bar.
  - **Tests:** `neotop_v0.2_cpu_io_network`

* [ ] **ADM-2. neostat** | Prereqs: -- | Files: `userbin/neostat/`
  - Terminal dashboard: CPU%, memoria, disco, red. Muestreo periódico 1s.
  - **Tests:** `neostat_displays_all_gauges`

* [ ] **ADM-4. neotask** | Prereqs: -- | Files: `userbin/neotask/`
  - Listar procesos, matar, cambiar prioridad, crear proceso.
  - **Tests:** `neotask_kill_pid`, `neotask_set_priority`, `neotask_spawn`

* [ ] **ADM-5. neocfg** | Prereqs: B2.6 | Files: `userbin/neocfg/`
  - Navegación de árbol del Registry: `ls`, `cd`, `cat`, `set`, `delete`, `create`.
  - **Tests:** `neocfg_read_write_key`, `neocfg_enum_key_value`

* [ ] **ADM-6. neofs** | Prereqs: -- | Files: `userbin/neofs/`
  - Estadísticas de volumen, correr fsck, cambiar label, listar montajes.
  - **Tests:** `neofs_fsck_drive`, `neofs_format_volume`, `neofs_label_roundtrip`

### v0.52: VirtIO + Performance + Security

#### VirtIO Driver Roadmap

> VIO-ARCH es prerrequisito transversal para VIO-NET, VIO-9P, VIO-BLK2, VIO-INPUT.

* [ ] **VIO-ARCH. Virtqueue abstraction + modern PCI transport** | Prereqs: A2.1 | Files: `src/virtio/` (new)
  - Capa base: virtqueue split vring 1.0, legacy I/O BAR + modern MMIO BAR (VirtIO 1.0+),
    feature negotiation, indirect descriptors, MSI-X + interrupciones (poll fallback), PCI discovery.
  - **Tests:** `vio_virtqueue_alloc_free`, `vio_virtqueue_submit_chain`, `vio_virtqueue_poll_completion`,
    `vio_modern_bar_detect`, `vio_feature_negotiation`, `vio_msix_configure`

* [ ] **VIO-NET. VirtIO Network (0x1000)** | Prereqs: VIO-ARCH | Files: `src/net/virtio_net.rs` or `drivers/virtio-net/` (NEM)
  - 1 RX + 1 TX virtqueue, mergeable RX buffers, checksum offload, MAC desde config space,
    link status polling, legacy + modern transport. Se integra con `src/net/nic.rs`.
  - **Tests:** `vio_net_probe`, `vio_net_send_recv`, `vio_net_mac_config`

* [ ] **VIO-9P. VirtIO 9P (0x1009)** | Prereqs: VIO-ARCH | Files: `drivers/virtio-9p/` (NEM), `src/fs/9p.rs`
  - Filesystem 9P2000.L sobre VirtIO para compartir directorios host-huésped.
  - **Tests:** `vio_9p_version_attach`, `vio_9p_walk_open_read`, `vio_9p_write_close`

* [ ] **VIO-BLK2. VirtIO Block NEM driver** | Prereqs: VIO-ARCH | Files: `drivers/virtio-blk/` (new, NEM SYSTEM)
  - Reemplazar BOOT_DRIVER inline por NEM standalone. Hotplug multi-dispositivo. MSI-X con DPC.
  - **Tests:** `vio_blk_probe`, `vio_blk_read_write`, `vio_blk_multi_device`

* [ ] **VIO-INPUT. VirtIO Input (0x1013)** | Prereqs: VIO-ARCH | Files: `drivers/virtio-input/` (NEM)
  - Teclado, ratón, tablet vía VirtIO. Integración con `src/input/manager.rs`.
  - **Tests:** `vio_input_key_event`, `vio_input_abs_event`, `vio_input_multi_device`

#### Performance

* [ ] **B6.1. Zero-copy pipes** | Prereqs: -- | Files: `src/pipe.rs`
  - Pipes sin copia de datos entre procesos.
  - **Tests:** `pipe_zero_copy_throughput`

#### Security

* [ ] **B5.1. Module signature validation** | Prereqs: NT6 | Files: `src/drivers/loader.rs`
  - Validación criptográfica de módulos `.nem` antes de cargar.
  - **Tests:** `nem_signature_valid_accepts`, `nem_signature_invalid_rejects`, `nem_signature_tamper_detected`

* [ ] **B5.2. Driver permission enforcement** | Prereqs: NT6.3, B5.1 | Files: `src/drivers/caps.rs`
  - Cruzar capacidad declarada del driver con token del proceso y ACL del objeto.
  - **Tests:** `driver_caps_allow_admin`, `driver_caps_deny_user`, `driver_caps_acl_intersection`

#### Registry (Phase 2 — security + dirty tracking)

* [ ] **CM-SEC. Registry security (ACL por clave)** | Prereqs: CM-FIX | Files: `src/cm/security.rs` (new), `src/cm/mod.rs`, `src/syscall/cm.rs`
  - Nuevo `src/cm/security.rs` con `cm_check_access()`, `cm_ensure_security()`, `cm_inherit_security()`.
  - Admin bypass: token admin accede a cualquier clave.
  - **Tests:** `cm_sec_key_creation_assigns_owner`, `cm_sec_access_granted`, `cm_sec_access_denied`, `cm_sec_inheritance_parent_child`, `cm_sec_admin_bypass`

* [ ] **CM-DIRTY. Registry per-cell dirty tracking + incremental flush** | Prereqs: CM-FIX | Files: `src/cm/hive.rs`, `src/cm/cache.rs`, `src/cm/mod.rs`
  - `dirty_cells: BitVec` (1 bit por slot). `slot_mut()` marca dirty; `serialize_dirty()` escribe solo celdas sucias.
  - **Tests:** `cm_dirty_cell_set_on_write`, `cm_dirty_cleared_after_flush`, `cm_dirty_serialize_only_dirty`, `cm_dirty_full_flush_roundtrip`

* [ ] **CM-MULTI. Registry multi-hive** | Prereqs: CM-FIX | Files: `src/cm/mod.rs`
  - Montar SOFTWARE, SECURITY, DEFAULT hives. Cada hive crea su directorio raíz en namespace Ob.
  - **Tests:** `cm_multi_software_mounted`, `cm_multi_hive_isolation`, `cm_multi_cross_hive_path_fails`, `cm_multi_unload_reload`

#### Userland

* [ ] **B4.6. NeoEdit text editor** | Prereqs: A4.7, B4.4 | Files: `userbin/neoedit/`
  - Editor de texto modal Ring 3. Usa `ob_open` + `ob_query_info(ReadContent)` / `ob_set_info(WriteContent)`.
  - **Tests:** `neoedit_open_display`, `neoedit_edit_save`, `neoedit_scroll`

* [ ] **B4.7. Shared library per-process binding** | Prereqs: sys_loadlib | Files: `src/elf.rs`, `libneodos/`
  - Evolucionar NXL slots globales a binding per-process. Cada EPROCESS mantiene su tabla de NXLs.
  - **Tests:** `nxl_per_process_isolation`, `nxl_unload_on_exit`, `nxl_version_coexistence`

#### Kernel

* [ ] **A3.2. Kernel debugger (KD)** | Prereqs: A3.1 | Files: `src/debugger/mod.rs`
  - INT3 breakpoints, hardware watchpoints (DR0-DR3), GDB remote protocol stub via serial.
  - **Tests:** `kd_breakpoint_set_and_hit`, `kd_breakpoint_invalid_addr`, `kd_watchpoint_write_detect`, `kd_register_snapshot`, `kd_gdb_protocol_qSupported`

---

## LOW

### v0.53+: Hardening + Multi-hive + Docs + Cleanup

#### Registry (Phase 3 — WAL + lib wrappers)

* [ ] **CM-WAL. Registry WAL (write-ahead logging, crash recovery)** | Prereqs: CM-FIX | Files: `src/cm/wal.rs` (new), `src/cm/mod.rs`
  - Cada mutación escribe entrada WAL a `C:\System\Registry\<name>.wal` + fsync antes de aplicar a hive.
  - En mount: si existe `.wal`, hacer replay antes de cargar `.hiv`.
  - **Tests:** `cm_wal_created_on_mutation`, `cm_wal_replay_on_load`, `cm_wal_truncated_after_flush`, `cm_wal_power_loss_recovery`

* [ ] **CM-LIB. Registry libneodos wrappers** | Prereqs: CM-FIX | Files: `libneodos/src/syscall.rs`
  - Añadir 7 wrappers: `sys_cm_create_key`, `sys_cm_delete_key`, `sys_cm_enum_key`, `sys_cm_enum_value`, `sys_cm_flush_key`, `sys_cm_load_hive`, `sys_cm_unload_hive`.
  - **Tests:** `cm_lib_create_key_wrapper`, `cm_lib_enum_key_wrapper`, `cm_lib_enum_value_wrapper`, `cm_lib_flush_key_wrapper`

* [ ] **CM-REGEDIT. regedit.nxe** | Prereqs: CM-LIB | Files: `userbin/regedit/` (new)
  - Navegación de árbol, crear/borrar claves, set/query valores, flush manual.
  - **Tests:** `regedit_browse_tree`, `regedit_create_delete_key`, `regedit_set_query_value`, `regedit_flush`

#### Security (USR)

* [ ] **USR-001..024. USR Fase 1+2: SAM + Login + SUDO** | Prereqs: NT6 | Files: multiples
  - Ver `docs/security.md`. F1: SAM + Token NT. F2: Login + SUDO.
  - **Tests:** `sam_create_user`, `sam_authenticate`, `sudo_spawn_as_user`

* [ ] **B5.3. Secure boot chain** | Prereqs: B5.1 | Files: `neodos-bootloader/`, `src/boot/secure.rs`
  - Verificación encadenada bootloader → kernel → drivers.
  - **Tests:** `secure_boot_kernel_verified`, `secure_boot_driver_verified`, `secure_boot_fail_closed`

#### Tracing

* [ ] **B1.1. Kernel tracing infrastructure** | Prereqs: A2.4 | Files: `src/trace/mod.rs`
  - TraceBuffer con trace points registrables dinámicamente, filtrado por categoría/nivel, dump via serial con timestamps HPET.
  - **Tests:** `trace_register_dynamic_point`, `trace_filter_by_category`, `trace_dump_serial_format`

* [ ] **B1.2. NeoTrace system** | Prereqs: B1.1 | Files: `userbin/neotrace/`
  - Comando `NEOTRACE` con subcomandos START/STOP/DUMP/FILTER.
  - **Tests:** `neotrace_start_stop_toggle`, `neotrace_dump_output`

#### Admin (Fase 3 — Avanzado)

* [ ] **ADM-3. neolog** | Prereqs: B1.1 | Files: `userbin/neolog/`
  - Visor de event log del kernel + EventBus. Filtro por categoría/nivel/timestamp.
  - **Tests:** `neolog_eventbus_dump`, `neolog_trace_filter`

* [ ] **ADM-7. neoctl** | Files: `userbin/neoctl/`
* [ ] **ADM-8. neodebug** | Files: `userbin/neodebug/`
* [ ] **ADM-9. neomem v0.2** | Files: `userbin/neomem/`
* [ ] **B4.8. NeoTOP v0.2+** | Files: `userbin/neotop/`
* [ ] **B4.12. Compositor 2D** | Files: `userbin/compositor/`

#### Networking

* [ ] **NET-1.7. Kernel: nic_id + ephemeral port** | Prereqs: NET-1 F4 | Files: `src/syscall/ob.rs`, `src/net/socket.rs`
  - Asignar NIC por defecto y puerto efímero (49152-65535) si no especificado.
  - **Tests:** `socket_auto_port_assign`

* [ ] **BUG-NEM-RX. NEM e1000 driver no recibe paquetes** | Files: `drivers/e1000/src/lib.rs`, `neodos-kernel/src/drivers/nem/net_bridge.rs`
  - `e1000_poll()` nunca detecta paquetes entrantes (bit DD no seteado). Workaround: `default_nic_id()` prefiere kernel e1000.

#### Kernel

* [ ] **B6.2. Copy-on-write fork** | Prereqs: -- | Files: `src/memory/cow.rs`, `src/syscall.rs`
* [ ] **AUDIT-17. User address space constrained (USER_LIMIT=36MB)** | Files: `src/arch/x64/paging.rs`
* [ ] **AUDIT-48. Fixed 16 KB kernel stack with no guard page** | Files: `src/scheduler/mod.rs:21`
* [ ] **AUDIT-34. No RAII IRQL guard — 15+ manual raise/lower** | Files: `src/scheduler/mod.rs`
* [ ] **AUDIT-33. `BIN_BUF` global static mut not re-entrant** | Files: `src/syscall/handlers.rs:79`
* [ ] **AUDIT-47. Non-reentrant IRP pool with wraparound overwrite** | Files: `src/irp/mod.rs:13-14`

#### Cleanup (quick wins — refactors and dead code)

* [ ] **CLEANUP-1. Dead code mask `#![allow(dead_code)]` in main.rs + globals.rs** | Files: `src/main.rs:9`, `src/globals.rs:1`
  - Remove `#[allow(dead_code)]`, fix revealed dead items. Merged from AUDIT-30/AUDIT-82.
  - **Tests:** (compile-only)

* [ ] **CLEANUP-2. Unused macros + functions + enum variants + constants** | Files: multiple
  - Merged from AUDIT-31. Remove `with_current!`, `trace_irq_enter!`/`trace_irq_exit!`, `register_tests()` (virtio), `with_cache`, `nic_get_mask`, `socket_next_accept_id`, `pipe_peek_read_closed`, `clear`/`segment_count`, `ObError::TableFull`, `ObType::EventBus`, `PIT_HZ`.
  - **Tests:** verify build

* [ ] **CLEANUP-3. AUDIT-35. virtio::register_tests() orphaned** | Files: `src/virtio/mod.rs:35`
  - `register_tests()` defined but never called from `testing.rs`. Add call.
  - **Tests:** Add call to `virtio::register_tests()` in `testing.rs`

* [ ] **CLEANUP-4. AUDIT-51. unregister_all() does nothing** | Files: `src/drivers/nem/driver.rs:92-98`
* [ ] **CLEANUP-5. AUDIT-55/77. ABI validation duplicated** | Files: `src/drivers/abi/mod.rs:50-80`, `src/drivers/nem/policy.rs:27-57`
  - `abi::negotiate()` and `policy::validate_abi()` implement same three checks. Make `validate_abi()` delegate to `negotiate()`.

* [ ] **CLEANUP-6. AUDIT-56. Dual mount managers** | Files: `src/fs/vfs.rs:84-95`, `src/vfs/mount.rs:38-123`
* [ ] **CLEANUP-7. AUDIT-58. Error constants duplicated libneodos/libneodos-nxl** | Files: `libneodos/src/syscall.rs:3-17`, `libneodos-nxl/src/error.rs:4-18`
* [ ] **CLEANUP-8. AUDIT-59. 10+ enums with manual `to_str()` instead of `Display`** | Files: multiple
* [ ] **CLEANUP-9. AUDIT-60. iso9660.rs dead filesystem driver** | Files: `src/drivers/iso9660.rs`
* [ ] **CLEANUP-10. AUDIT-61. debugger/mod.rs GDB stub dead code** | Files: `src/debugger/mod.rs`
* [ ] **CLEANUP-11. AUDIT-62. kbd_layout.rs never compiled** | Files: `src/drivers/nem/drivers/kbd_layout.rs`
* [ ] **CLEANUP-12. AUDIT-63. 23 dead functions** | Files: multiple (see AUDIT-63 description)
* [ ] **CLEANUP-13. AUDIT-64. PageCacheLevel unused variants** | Files: `src/vfs/io.rs:9`
* [ ] **CLEANUP-14. AUDIT-65. Dead struct CryptoContext** | Files: `src/vfs/io.rs:16`
* [ ] **CLEANUP-15. AUDIT-50/80. `lazy_static!` → `LazyLock`** | Files: multiple (27 usages)
* [ ] **CLEANUP-16. AUDIT-72. net/mod.rs monolithic protocol dispatch** | Files: `src/net/mod.rs:68-197`
* [ ] **CLEANUP-17. AUDIT-73. Storage probe hardcoded to 4 drivers** | Files: `src/drivers/storage_manager.rs:2-5`
* [ ] **CLEANUP-18. AUDIT-74. SPSC ring buffer triplicated** | Files: `src/work_queue.rs`, `src/input/vt.rs`, `src/arch/x64/cpu_local.rs`
* [ ] **CLEANUP-19. AUDIT-75. 27 fixed-size arrays across kernel** | Files: multiple
* [ ] **CLEANUP-20. AUDIT-76. Network unsafe pointer casts (9×)** | Files: `src/net/mod.rs`
* [ ] **CLEANUP-21. AUDIT-78. kernel_stack_trace fixed crash buffers** | Files: `src/crash/mod.rs:34,66,70`
* [ ] **CLEANUP-22. AUDIT-79. from_u8/from_u16 → TryFrom** | Files: `src/drivers/nem/mod.rs:46-98`
* [ ] **CLEANUP-23. AUDIT-83. TOCTOU in storage probe** | Files: `src/drivers/storage_manager.rs`
* [ ] **CLEANUP-24. AUDIT-11. IPI function duplicates** | Files: `src/arch/x64/smp.rs`
* [ ] **CLEANUP-25. AUDIT-12. AHCI structs defined twice** | Files: `src/drivers/boot_ahci.rs`, `drivers/ahci/src/lib.rs`
* [ ] **CLEANUP-26. AUDIT-13. PCI config access in 7 files** | Files: `src/drivers/pci.rs`, `drivers/*/src/lib.rs`
* [ ] **CLEANUP-27. AUDIT-14. HST extern in 8 NEM drivers** | Files: `drivers/*/src/lib.rs`
* [ ] **CLEANUP-28. AUDIT-15. PAGE_SIZE defined 7 times** | Files: multiple
* [ ] **CLEANUP-29. AUDIT-16. Error enums overlapping variants** | Files: `src/fs/vfs.rs`, `src/fs/neodos_fs.rs`, `src/drivers/fat32.rs`, `src/drivers/iso9660.rs`
* [ ] **CLEANUP-30. AUDIT-18. Idle loops without `hlt`** | Files: `src/main.rs`, `src/hal/raw/cpu.rs`
* [ ] **CLEANUP-31. AUDIT-19. Global static mut without sync (40+)** | Files: multiple
* [ ] **CLEANUP-32. AUDIT-20. Split syscall/ob.rs (2280 lines) + handlers.rs (1771)** | Files: `src/syscall/ob.rs`, `src/syscall/handlers.rs`
* [ ] **CLEANUP-33. AUDIT-21. Scheduler panics on table full** | Files: `src/scheduler/mod.rs`
* [ ] **CLEANUP-34. AUDIT-22. Page cache O(n) linear scans** | Files: `src/buffer/page_cache.rs`
* [ ] **CLEANUP-35. AUDIT-49. 10 inconsistent name buffer sizes** | Files: multiple

#### Documentation

* [x] **AUDIT-23. NEM v3 header docs contradict code** | Files: `docs/ARCHITECTURE.md`, `docs/drivers.md`, `src/nem/mod.rs`
  - Fixed offset table (added padding row at 26, corrected all subsequent offsets). Rewrote `drivers.md` table to match actual `NemHeaderV3` struct.
* [x] **AUDIT-24. libneodos.md: syscall instruction vs int 0x80** | Files: `docs/libneodos.md`, `libneodos/src/syscall.rs`
  - Changed "syscall instruction" → "int 0x80" in doc.
* [x] **AUDIT-25. libneodos.md: user.ld base addr wrong** | Files: `docs/libneodos.md`, `userbin/*/user.ld`
  - Changed "placing code at 0x400000" → "linking at address 0; runtime loads at 0x400000".
* [x] **AUDIT-26. scheduler.md: CpuRunQueue field names wrong** | Files: `docs/scheduler.md`, `src/arch/x64/cpu_local.rs`
  - Fixed field names (head/tail → head_idx/tail_idx), added missing `count: u16`.
* [x] **AUDIT-27. objects.md: SocketRecv class 23 (re-check)** | Files: `docs/objects.md`
  - Already correct — SocketRecv=23 consistent everywhere.
* [x] **AUDIT-28. memory.md: kernel_image base wrong** | Files: `docs/memory.md`, `neodos-kernel/kernel.ld`
  - Already fixed in prior audit.
* [x] **AUDIT-29. Version mismatch AGENTS/Cargo/CHANGELOG** | Files: `AGENTS.md`, `neodos-kernel/Cargo.toml`, `CHANGELOG.md`
  - Fixed: `Cargo.toml` bumped from 0.48.0 → 0.49.0 to match AGENTS/CHANGELOG.
* [x] **DH1. Actualizar README.md** | Files: `README.md`
  - Updated version badge to v0.49.0, test count to 656.
* [ ] **DH2. Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md** | Files: `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`
* [ ] **DH-HISTORY. Mantener docs/HISTORY.md** | Files: `docs/HISTORY.md`

#### Object Manager / Syscalls

* [ ] **AI-1. Completar ObInfoClass/ObSetInfoClass enums** | Files: `libneodos/src/syscall.rs`
* [ ] **AI-2. Consolidate legacy syscall wrappers** | Files: `src/syscall/mod.rs`
* [ ] **AI-3. ObObjectTable lock granularity (lock striping)** | Files: `src/object/mod.rs`
* [ ] **AI-4. Arreglar TOCTOU race en kobj_register** | Files: `src/object/mod.rs`

#### VFS (remaining)

* [ ] **VFS-3.2. `\DosDevices` dinámico** | Files: `src/vfs/mount.rs`
* [ ] **VFS-5.3. Write-back ordenado (flush page → flush block)** | Files: `src/globals.rs`
* [ ] **VFS-6.1. Overlay mounts** | Files: `src/fs/vfs.rs`
* [ ] **VFS-6.2. Extended attributes VFS** | Files: `src/fs/vfs.rs`
* [ ] **VFS-6.3. File notifications via Event Bus** | Files: `src/fs/vfs.rs`, `src/eventbus/`
* [ ] **VFS-6.4. Async VFS operations via IRP** | Files: `src/fs/vfs.rs`
* [ ] **VFS-7.1. Eliminar lock global de VFS** | Files: `src/globals.rs`, `src/fs/vfs.rs`
* [ ] **VFS-7.2. Lookup cache** | Files: `src/fs/vfs.rs`
* [ ] **VFS-7.3. Path cache** | Files: `src/fs/vfs.rs`

#### VirtIO (low priority)

* [ ] **VIO-CON. VirtIO Console (0x1002)** | Files: `drivers/virtio-console/`
* [ ] **VIO-RNG. VirtIO RNG (0x1003)** | Files: `drivers/virtio-rng/`
* [ ] **VIO-SCSI. VirtIO SCSI (0x100A)** | Files: `drivers/virtio-scsi/`
* [ ] **VIO-GPU. VirtIO GPU (0x1012)** | Files: `drivers/virtio-gpu/`
* [ ] **VIO-VSOCK. VirtIO VSOCK (0x1014)** | Files: `drivers/virtio-vsock/`
* [ ] **VIO-SOUND. VirtIO Sound (0x1015)** | Files: `drivers/virtio-sound/`
* [ ] **VIO-BALLOON. VirtIO Memory Balloon (0x1004)** | Files: `drivers/virtio-balloon/`

#### Experimental

* [ ] **B7.1. Full GUI system** | Files: `userbin/gui/`
* [ ] **B7.2. Advanced secure boot (TPM)** | Files: `src/boot/tpm.rs`
* [ ] **B7.3. Package manager** | Files: `userbin/neopkg/`
* [ ] **B7.4. Time-travel debugging** | Files: `src/debugger/timetravel.rs`
* [ ] **B7.5. Live kernel patching** | Files: `src/patch/mod.rs`
* [ ] **B7.6. Distributed NeoDOS nodes** | Files: `src/cluster/`
* [ ] **PKG-1. NeoGet v1 (diferido a v0.70)** | Files: (design only)

---

## Milestones

| Versión | Enfoque | Estado |
|---------|---------|--------|
| v0.50 | Registry bugfixes, Shell Phase 1, NeoFS snapshot syscall | **PRÓXIMO** |
| v0.51 | NeoFS v2 remaining (B-tree, freelist, snapshot, mkfs), Shell Phase 2, Networking tools | planned |
| v0.52 | VirtIO, Performance (zero-copy pipes), Security (sig validation, Registry ACL) | planned |
| v0.53+ | Hardening, Multi-hive, Documentation, Cleanup | backlog |

---

## REFERENCE — Design docs and removed content

### Objectification Roadmap
Mostly completed. See [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md) for:
- OBF-01..12 (Fase 1 + Fase 2 Ob: Thread, Timer, Semaphore, Section)
- X7 (Object Manager unification — handles, KOBJ, URN, security)
- All 16 ObTypes defined, 7 Ob syscalls (RAX 60-66)

### QEMU Bridge Infrastructure
- **scripts/setup-network.sh** — Creates `neodos0` bridge via NetworkManager
- **scripts/qemu-debug.sh** — `--bridge` flag uses `qemu-bridge-helper` (SUID root)
- **docs/qemu-setup.md** — Full documentation

---

## See also

- `docs/` for full subsystem design docs
- `skills/` for task checklists
- [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md)
- [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md)
- [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md)
