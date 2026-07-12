# NeoDOS — Roadmap

> Items pendientes del roadmap. Los completados están en
> [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md).
>
> Version actual: **v0.50-dev** — SSDT reorganized: RAX 0-59
> Objetivo: v1.0 — executive NT-like arquitectónicamente sólido.
> Leer [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) antes de planificar cambios.
> Fuente de verdad: [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md)

**Próximo milestone: v0.50** (Shell tokenizer + NeoFS snapshot + Power Manager Phase 2)

---

## Execution Rules

1. Una fase no empieza hasta que sus prerequisitos estén marcados **[COMPLETED]**.
2. Cada item pendiente incluye: ID, archivos, prereqs, tests.
3. Al completar: actualizar `CHANGELOG.md` y mover a `IMPROVEMENTS_COMPLETED.md`.
4. Validar: `cargo build` + `cargo run --bin neodev -- test` + `scripts/check_deps.py`.

---

## PRIORITY OVERVIEW

| ID | Item | Prio | Cat |
| ---- | ------ | ------ | ----- |
| **v0.50** | **Shell tokenizer + NeoFS snapshot + Power Phase 2** | **HIGH** | milestone |
| NFSv2-SYSCALL | sys_ob_snapshot (RAX 77) | **HIGH** | fs |
| SH-TOKEN+QUOTE | Shell tokenizer + quoting/escaping | **HIGH** | shell |
| SSDT-DRVUNLOAD | sys_driver_unload (RAX 35) → ob_destroy | **MEDIUM** | kernel |
| SSDT-MIGRATE-DUP2 | sys_dup2 (RAX 22) → Ob API | **LOW** | kernel |
| PM-PHASE2 | Power Manager kernel core (ObType=21, Registry, plan mgmt) | **HIGH** | power |
| AUDIT-32 | 5+ `.expect()` panic paths → Result | **HIGH** | kernel |
| | | | |
| **v0.51** | **NeoFS v2 + Shell Phase 2 + USR-P1 (SAM)** | **MEDIUM** | milestone |
| NFSv2-BTREE | B-tree persistente genérico | MEDIUM | fs |
| NFSv2-FREELIST+SNAP | Free list + Snapshot table (64 circular) | MEDIUM | fs |
| NFSv2-SHELL+MKFS | Shell snapshot cmds + mkfs.neodos | MEDIUM | fs+shell |
| SH-EDITOR+HISTORY | Line editor + history persistence | MEDIUM | shell |
| SH-ENV+PIPE | Env expansion (%VAR%) + pipeline wait | MEDIUM | shell |
| SH-SEP+COMPL+BATCH | Semicolon + completion + batch scripting | MEDIUM | shell |
| USR-P1a | ObType::Session=19 + SAM built-in users | MEDIUM | security |
| USR-P1b | Token: integrity_level + creation_time | MEDIUM | security |
| USR-P1c | SAM persistence to Registry hive | MEDIUM | security |
| USR-P1d | SeAccessCheck: fix empty DACL + group SIDs | MEDIUM | security |
| USR-P1e | ObSetInfoClass::ChangePassword (31) | MEDIUM | security |
| NET-1.9 | ipconfig.nxe | MEDIUM | net |
| NET-1.10 | ping.nxe | MEDIUM | net |
| B3.4 | NTP client | MEDIUM | net |
| ADM-1+2 | neotop v0.2 + neostat | MEDIUM | admin |
| ADM-4 | neotask (gestor de tareas) | MEDIUM | admin |
| ADM-5+6 | neocfg + neofs | MEDIUM | admin |
| | | | |
| **v0.52** | **VirtIO + Sessions + FS Security** | **MEDIUM** | milestone |
| VIO-ARCH | Virtqueue abstraction + modern PCI transport | **HIGH** | drivers |
| VIO-NET | VirtIO Network (0x1000) | **HIGH** | drivers |
| VIO-9P+BLK2+INPUT | VirtIO 9P + Block NEM + Input | MEDIUM | drivers |
| B6.1 | Zero-copy pipes | MEDIUM | kernel |
| USR-P2a | SessionManager + ob_create(Session) | MEDIUM | ob |
| USR-P2b | SessionInfo + SessionLock/Logoff | MEDIUM | ob |
| USR-P2c | TokenInfo (28) + session_id inheritance | MEDIUM | ob |
| USR-P2d | neologon.nxe login binary | MEDIUM | userland |
| USR-P2e | NeoInit spawns neologon | MEDIUM | boot |
| USR-P3a | DirEntryV2: owner_sid field | MEDIUM | fs |
| USR-P3b | VFS permission check function | MEDIUM | fs |
| USR-P3c | Wire VFS checks in syscall handlers | MEDIUM | syscall |
| USR-P3d | Default permissions by extension | MEDIUM | fs |
| VFS-2.2 | Refactorizar FSCK | MEDIUM | fs |
| PM-PHASE3 | Power syscall dispatch + Event Bus types | MEDIUM | power |
| | | | |
| **v0.53** | **Security + Registry + Integrity** | **MEDIUM** | milestone |
| B5.1 | Module signature validation | MEDIUM | security |
| B5.2 | Driver permission enforcement | MEDIUM | security |
| CM-DIRTY | Registry per-cell dirty tracking | MEDIUM | registry |
| CM-MULTI | Registry multi-hive (SOFTWARE, SECURITY, DEFAULT) | MEDIUM | registry |
| USR-P4a | cm/security.rs: Registry ACL module | MEDIUM | registry |
| USR-P4b | Wire sec_desc_cell on key creation | MEDIUM | registry |
| USR-P4c | ACL checks in Cm syscall handlers | MEDIUM | registry |
| USR-P4d | User profile hive auto-mount | MEDIUM | registry |
| USR-P5a | Integrity level in SeAccessCheck | MEDIUM | security |
| USR-P5b | SetIntegrityLevel + IntegrityLevel query | MEDIUM | ob |
| USR-P5c | Privilege enforcement in admin syscalls | MEDIUM | syscall |
| A3.2 | Kernel debugger (KD) | MEDIUM | kernel |
| B4.6 | NeoEdit text editor | MEDIUM | userland |
| B4.7 | Shared library per-process binding | MEDIUM | userland |
| | | | |
| **v0.54** | **Hardening + User commands + Docs + DNS + i18n** | **LOW** | milestone |
| B5.3 | Secure boot chain | LOW | security |
| CM-WAL | Registry WAL (write-ahead logging) | LOW | registry |
| PM-PHASE4 | Service Manager shutdown integration + libneodos wrappers + shell commands | MEDIUM | power |
| CM-LIB | Registry libneodos wrappers (7 missing) | LOW | lib |
| CM-REGEDIT | regedit.nxe — registry editor | LOW | admin |
| USR-P6a | WHOAMI command | LOW | shell |
| USR-P6b | PASSWD command | LOW | shell |
| USR-P6c | WHO + LOGOFF commands | LOW | shell |
| USR-P6d | SU command | LOW | shell |
| USR-P6e | RUNAS command | LOW | shell |
| NET-DNS | DNS resolver (stub resolver + cache) | LOW | net |
| I18N-P1 | i18n runtime (libneodos + NLT format) | LOW | lib |
| I18N-P2 | Migrar NeoShell + NeoInit + apps core a tr!() | LOW | shell |
| I18N-P3 | neolocale tool + archivos .nlt + segundo idioma | LOW | tools |
| B1.1 | Kernel tracing infrastructure | LOW | kernel |
| B1.2 | NeoTrace system | LOW | kernel |
| ADM-3 | neolog (visor event log) | LOW | admin |
| BUG-NEM-RX | NEM e1000 no recibe paquetes | LOW | drivers |
| AUDIT-17 | User address space constrained (36MB) | LOW | kernel |
| B6.2 | Copy-on-write fork | LOW | kernel |
| VFS-3.2 | `\DosDevices` dinámico | LOW | fs |
| VFS-5.3 | Write-back ordenado | LOW | fs |
| VFS-6.1..6.4 | VFS Features (overlay, attr, notifications, async) | LOW | fs |
| VFS-7.1..7.3 | VFS Performance (lock, lookup cache, path cache) | LOW | fs |
| PM-PHASE5 | Power Manager polish: event handlers, async coordination, tests | LOW | power |
| | | | |
| **backlog** | **Low-priority + experimental + cleanup** | **LOW** | |
| DH2 | Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md | LOW | docs |
| DH3 | Completar libneodos syscall wrappers | LOW | lib |
| DH-HISTORY | Mantener docs/HISTORY.md actualizado | LOW | docs |
| AI-1 | Completar ObInfoClass/ObSetInfoClass enums | LOW | ob |
| AI-2 | Consolidate legacy syscall wrappers | LOW | syscall |
| SSDT-FINAL | SSDT audit, cleanup, renumbering (DONE v0.49→v0.50) | **DONE** | syscall |
| AI-3 | ObObjectTable lock granularity | LOW | ob |
| AI-4 | Arreglar TOCTOU race en kobj_register | LOW | ob |
| ADM-7+8+9 | neoctl + neodebug + neomem v0.2 | LOW | admin |
| B4.8 | NeoTOP v0.2+ | LOW | admin |
| B4.12 | Compositor 2D | LOW | userland |
| B7.1..B7.6 | Experimental (GUI, TPM, package mgr, etc.) | LOW | xp |
| PKG-1 | NeoGet v1 (diferido a v0.70) | LOW | xp |
| VIO-CON...BALLOON | VirtIO Console/RNG/SCSI/GPU/VSOCK/Sound/Balloon | LOW | drivers |
| CLEANUP-1..35 | Dead code, duplicates, refactors (see LOW section) | LOW | cleanup |

---

## HIGH

### v0.50: Shell tokenizer + NeoFS snapshot + Power Phase 2

#### NeoFS v2

- [ ] **NFSv2-SYSCALL. sys_ob_snapshot (RAX 77)** | Prereqs: NFSv2-BTREE, NFSv2-SNAPSHOT | Files: `src/syscall/ob.rs`, `src/syscall/mod.rs`, `src/object/types.rs`
  - handler_ob_snapshot: CREATE/RESTORE/LIST/PURGE sobre handle del FS raíz.
  - SSDT entry + permission entry. Nuevos ObInfoClass si aplica.
  - **Tests:** `syscall_ob_snapshot_create`, `syscall_ob_snapshot_restore`, `syscall_ob_snapshot_list`

#### Shell (Phase 1 — foundation)

#### Power Manager — Phase 2: Kernel core

- [ ] **PM-PHASE2. Power Manager kernel core** | Prereqs: -- | Files: `src/power/mod.rs` (new), `src/power/plan.rs` (new), `src/power/coordinator.rs` (new), `src/object/types.rs`, `src/cm/mod.rs`, `src/main.rs`
  - Implementar `PowerManager` struct con `POWER_MANAGER: Mutex<PowerManager>` global.
  - `PowerSystemState` enum: Active, ShuttingDown, Rebooting, Suspending, Hibernating, Off.
  - `PowerPlan` + `PowerPolicies`: DisplayTimeout, SleepTimeout, HibernateEnabled, CpuPolicy, LidAction, PowerButtonAction.
  - `PowerManager::load_plan_from_registry(index)`: leer plan activo desde `\Registry\Machine\System\Power\Plans\<Name>\*`.
  - `PowerManager::save_plan_to_registry(index)`: persistir políticas activas.
  - `src/power/coordinator.rs`: `shutdown()` y `reboot()` sin integración con servicios todavía — solo HAL calls.
  - `src/object/types.rs`: añadir `PowerManager = 21` a `ObType`.
  - `src/main.rs`: añadir PHASE 3.883 — crear `\Device\PowerManager` en namespace Ob, inicializar PowerManager.
  - `src/cm/mod.rs`: añadir defaults de Power en `cm_ensure_default_values()`: `ActivePlan=0`, `Plans\Balanced\*`, `Plans\Performance\*`, `Plans\PowerSaver\*`.
  - **Tests:** `pm_init_state_active`, `pm_device_namespace_exists`, `pm_query_plan_defaults`, `pm_set_plan_balanced`, `pm_set_plan_performance`, `pm_set_plan_invalid`, `pm_plan_persists_to_registry`, `pm_set_policy_display_timeout`, `pm_set_policy_invalid_id`, `pm_policy_persists`

#### Kernel Hardening

- [ ] **AUDIT-32. 5+ `.expect()` panic paths → Result<()>** | Files: `src/scheduler/mod.rs:485-487`, `src/main.rs:334`, `src/globals.rs:38`, `src/arch/x64/serial.rs:73`, `src/urn/mod.rs:383`
  - Scheduler slot full, block device missing, serial write failure — all crash the kernel instead of returning `Result`.
  - **Tests:** `scheduler_slot_exhaustion_graceful`, `urn_create_failure_propagated`

---

## MEDIUM

### v0.51: NeoFS v2 remaining + Shell Phase 2 + SAM foundation + Network tools

#### NeoFS v2 (completar implementación)

- [ ] **NFSv2-BTREE. B-tree persistente genérico** | Prereqs: -- | Files: `src/fs/btree.rs`
  - B-tree con orden configurable, nodos 4KB. Operaciones: insert, lookup, delete, walk inorder.
  - COW en escritura: insert/delete crea nuevos nodos hasta la raíz, devuelve nueva root_lba.
  - **Tests:** `btree_insert_lookup`, `btree_delete`, `btree_walk_inorder`, `btree_cow_new_root`, `btree_cow_preserves_old_root`

- [ ] **NFSv2-FREELIST. Free list** | Prereqs: -- | Files: `src/fs/freelist.rs`
  - Lista de regiones libres (start_lba, length). Alocar: first-fit. Liberar: merge con adyacentes.
  - **Tests:** `freelist_alloc_marks_used`, `freelist_free_reclaims`, `freelist_merge_adjacent`, `freelist_multi_node`

- [ ] **NFSv2-SNAPSHOT. Snapshot table** | Prereqs: NFSv2-BTREE | Files: `src/fs/snapshot.rs`
  - Tabla circular de 64 entradas. CREATE copia root_btree_lba actual. RESTORE cambia superblock.
  - **Tests:** `snapshot_create_list`, `snapshot_restore`, `snapshot_circular_64`

- [ ] **NFSv2-MKFS. Herramienta mkfs.neodos** | Prereqs: NFSv2-FILESYSTEM | Files: `userbin/mkfs/` (o script build)
  - Escribir superblock "NE2\0", B-tree raíz vacío, freelist con todo el espacio libre.
  - **Tests:** `mkfs_creates_valid_ne2_superblock`

- [ ] **VFS-2.2. Refactorizar FSCK** | Prereqs: -- | Files: `src/fs/fsck.rs`
  - Extraer lógica común a trait `FsckIntegrity`, mover a `drivers/fsck_neodos.rs`.
  - **Tests:** 6 tests existentes + 2 de integración

#### Shell (Phase 2 — editor, env, pipeline)

- [ ] **SH-EDITOR+HISTORY. Shell line editor + history** | Prereqs: -- | Files: `userbin/neoshell/src/editor.rs`, `userbin/neoshell/src/history.rs`
  - Reemplaza readline() con `LineEditor`: posicionamiento ANSI, Ctrl-A/E/K/U/R, Insert.
  - Ring buffer dinámico, persistencia en `C:\System\neoshell.hst`.
  - **Tests:** `editor_basic_input`, `editor_backspace`, `editor_ctrl_k`, `editor_history_search`, `history_persistence_save_load`

- [ ] **SH-ENV+PIPE. Shell env expansion + pipeline** | Prereqs: SH-TOKEN+QUOTE | Files: `userbin/neoshell/src/env.rs`, `userbin/neoshell/src/pipeline.rs`
  - Post-tokenization pass: reemplaza `%VARNAME%` con valor de `EnvStore`.
  - Pipeline espera a todos los procesos vía `ob_wait`, recolecta exit codes.
  - **Tests:** `env_simple_expansion`, `env_unknown_var`, `pipeline_simple_wait`, `pipeline_three_stage`, `pipeline_exit_code_report`

- [ ] **SH-SEP+COMPL+BATCH. Separator + completion + scripting** | Prereqs: SH-TOKEN+QUOTE, SH-REDIR, SH-ENV+PIPE | Files: `userbin/neoshell/src/tokenizer.rs`, `userbin/neoshell/src/completion.rs`, `userbin/neoshell/src/batch.rs`
  - Token `Semicolon` en tokenizer. Completion engine con PATH cache.
  - Intérprete batch: `ECHO`, `SET`, `IF EXIST/ERRORLEVEL`, `GOTO :label`, `CALL`, `FOR %%F`, `SHIFT`, `REM`, `@`, `PAUSE`.
  - **Tests:** `semicolon_two_commands`, `completion_command_prefix`, `bat_echo_set`, `bat_if_goto`, `bat_call_subroutine`, `bat_for_loop`, `bat_shift_args`, `bat_pause_resume`

#### Networking — Userland tools

- [ ] **NET-1.9. ipconfig.nxe** | Prereqs: -- | Files: `userbin/ipconfig/` (new)
  - `IPCONFIG [/ALL]` — interfaces, MAC, IP, gateway, DNS, stats.
  - **Tests:** integración

- [ ] **NET-1.10. ping.nxe** | Prereqs: -- | Files: `userbin/ping/` (new)
  - `PING <host> [/n count] [/w ms]`. Socket raw ICMP echo request.
  - **Tests:** ping a QEMU host

- [ ] **B3.4. NTP client** | Prereqs: -- | Files: `src/net/ntp.rs`
  - Cliente NTP (RFC 5905, SNTP simplificado). Sincroniza RTC del sistema.
  - **Tests:** `ntp_request_parse_response`, `ntp_offset_calculation`

#### Admin Tools (Fase 1 — Monitorización + Control)

- [ ] **ADM-1. neotop v0.2** | Prereqs: -- | Files: `userbin/neotop/`
  - Añadir per-thread CPU, I/O stats, network bar.
  - **Tests:** `neotop_v0.2_cpu_io_network`

- [ ] **ADM-2. neostat** | Prereqs: -- | Files: `userbin/neostat/`
  - Terminal dashboard: CPU%, memoria, disco, red. Muestreo periódico 1s.
  - **Tests:** `neostat_displays_all_gauges`

- [ ] **ADM-4. neotask** | Prereqs: -- | Files: `userbin/neotask/`
  - Listar procesos, matar, cambiar prioridad, crear proceso.
  - **Tests:** `neotask_kill_pid`, `neotask_set_priority`, `neotask_spawn`

- [ ] **ADM-5. neocfg (Panel de Control)** | Prereqs: -- | Files: `userbin/neocfg/` (new), `scripts/build.sh`, `scripts/create_ne2_image.py`, `docs/design/neocfg-design.md`
  - Aplicación Ring 3 .NXE: panel de control modular que consume exclusivamente APIs públicas de libneodos.
  - `CfgModule` trait: cada subsistema (System, Keyboard, About, Power, Locale) implementa interfaz común.
  - `ui/menu.rs`: renderizado de menús con navegación por teclado (↑↓, Enter, Esc, 1-9).
  - `ui/dialog.rs`: diálogos de entrada, confirmación, mensajes informativos.
  - Módulo System (solo lectura): version, memory, cpu, drives, processes, services via `ob_query_info`.
  - Módulo Keyboard: listar/cambiar layouts via `ob_set_info(KeyboardLayout=5)`, `ob_query_info(KeyboardLayout=14)`.
  - Módulo About: version strings via `ob_query_info(Version=8)`.
  - Módulo Power (stub): mensaje "not available" hasta PM-PHASE2 completado.
  - Módulo Locale (stub): mensaje "not available" hasta I18N-P1 completado.
  - Todos los textos visibles via `tr!()` macro (i18n desde el diseño).
  - Preparado para GUI: la lógica en `modules/` se reutiliza sin cambios.
  - **Tests:** `neocfg_menu_navigation`, `neocfg_system_info`, `neocfg_keyboard_set_layout`, `neocfg_about_version`, `neocfg_stubs_no_crash`, `neocfg_i18n_all_keys_present`, `neocfg_no_direct_registry_access`

- [ ] **ADM-6. neofs** | Prereqs: -- | Files: `userbin/neofs/`
  - Estadísticas de volumen, correr fsck, cambiar label, listar montajes.
  - **Tests:** `neofs_fsck_drive`, `neofs_format_volume`, `neofs_label_roundtrip`

#### Security — USR-P1: SAM foundation

> Diseño completo: `docs/design/users-security-design.md`. Modelo NT-like: SAM + Token + Session + ACL.
> No Unix uid/gid — usar SID (ya existente en `src/security/`).
> No nuevas syscalls — todo via Ob API (RAX 60-66) con ObType::Session=19 e info classes nuevas.
> Cada paso es pequeño, testeable, y mantiene backward compatibility.

- [ ] **USR-P1a. ObType::Session + SAM built-in users** | Prereqs: -- | Files: `src/object/types.rs`, `src/main.rs`, `src/security/mod.rs`
  - Add `Session = 19` to ObType enum
  - Create built-in users (Administrator S-1-5-21-500, Guest S-1-5-21-501) in `init_security()`
  - Verify SAM entries exist after boot
  - **Tests:** `usr_type_session_exists`, `usr_builtin_admin_created`, `usr_builtin_guest_created`

- [ ] **USR-P1b. Token: add integrity_level + creation_time** | Prereqs: USR-P1a | Files: `src/security/token.rs`, `src/security/mod.rs`
  - Add `IntegrityLevel` enum (Untrusted=0, Low=1, Medium=2, High=3, System=4)
  - Add `integrity_level: IntegrityLevel` and `creation_time: u64` fields to Token
  - Update `new_admin()` → integrity_level=System, `new_user()` → integrity_level=Medium
  - **Tests:** `usr_token_admin_system_il`, `usr_token_user_medium_il`, `usr_token_creation_time_set`

- [ ] **USR-P1c. SAM persistence to Registry hive** | Prereqs: USR-P1a | Files: `src/security/sam.rs`
  - Implement `sam_save(path)` — serialize SAM to `\Registry\Machine\SAM` via VFS (binary magic `SAM\0`, version 2)
  - Implement `sam_load(path)` — deserialize from VFS
  - Wire save on user create/delete/password change
  - Wire load at boot in `init_security()`
  - **Tests:** `usr_sam_save_load_roundtrip`, `usr_sam_persist_across_reboot`, `usr_sam_save_on_user_create`

- [ ] **USR-P1d. SeAccessCheck: fix empty DACL + group SID checking** | Prereqs: USR-P1b | Files: `src/security/access.rs`, `src/security/acl.rs`
  - Fix empty DACL: empty ACL = deny all (match NT behavior)
  - Add group SID checking: iterate `token.groups` in addition to `token.sid` during ACL evaluation
  - Keep admin bypass intact
  - **Tests:** `usr_se_access_empty_dacl_denies`, `usr_se_access_group_sid_allowed`, `usr_se_access_group_sid_denied`, `usr_se_access_admin_bypass`

- [ ] **USR-P1e. ObSetInfoClass::ChangePassword syscall handler** | Prereqs: USR-P1c | Files: `src/object/types.rs`, `src/syscall/ob.rs`
  - Add `ChangePassword = 31` to ObSetInfoClass
  - Handler validates old password hash, updates SAM with new password hash
  - Returns EAUTH if old password doesn't match
  - **Tests:** `usr_change_password_ok`, `usr_change_password_wrong_old`, `usr_change_password_then_login`

### v0.52: VirtIO + Sessions + FS Security

#### VirtIO Driver Roadmap

> VIO-ARCH es prerrequisito transversal para VIO-NET, VIO-9P, VIO-BLK2, VIO-INPUT.

- [ ] **VIO-ARCH. Virtqueue abstraction + modern PCI transport** | Prereqs: A2.1 | Files: `src/virtio/` (new)
  - Capa base: virtqueue split vring 1.0, legacy I/O BAR + modern MMIO BAR (VirtIO 1.0+),
    feature negotiation, indirect descriptors, MSI-X + interrupciones (poll fallback), PCI discovery.
  - **Tests:** `vio_virtqueue_alloc_free`, `vio_virtqueue_submit_chain`, `vio_virtqueue_poll_completion`,
    `vio_modern_bar_detect`, `vio_feature_negotiation`, `vio_msix_configure`

- [ ] **VIO-NET. VirtIO Network (0x1000)** | Prereqs: VIO-ARCH | Files: `src/net/virtio_net.rs` or `drivers/virtio-net/` (NEM)
  - 1 RX + 1 TX virtqueue, mergeable RX buffers, checksum offload, MAC desde config space,
    link status polling, legacy + modern transport. Se integra con `src/net/nic.rs`.
  - **Tests:** `vio_net_probe`, `vio_net_send_recv`, `vio_net_mac_config`

- [ ] **VIO-9P. VirtIO 9P (0x1009)** | Prereqs: VIO-ARCH | Files: `drivers/virtio-9p/` (NEM), `src/fs/9p.rs`
  - Filesystem 9P2000.L sobre VirtIO para compartir directorios host-huésped.
  - **Tests:** `vio_9p_version_attach`, `vio_9p_walk_open_read`, `vio_9p_write_close`

- [ ] **VIO-BLK2. VirtIO Block NEM driver** | Prereqs: VIO-ARCH | Files: `drivers/virtio-blk/` (new, NEM SYSTEM)
  - Reemplazar BOOT_DRIVER inline por NEM standalone. Hotplug multi-dispositivo. MSI-X con DPC.
  - **Tests:** `vio_blk_probe`, `vio_blk_read_write`, `vio_blk_multi_device`

- [ ] **VIO-INPUT. VirtIO Input (0x1013)** | Prereqs: VIO-ARCH | Files: `drivers/virtio-input/` (NEM)
  - Teclado, ratón, tablet vía VirtIO. Integración con `src/input/manager.rs`.
  - **Tests:** `vio_input_key_event`, `vio_input_abs_event`, `vio_input_multi_device`

#### Performance

- [ ] **B6.1. Zero-copy pipes** | Prereqs: -- | Files: `src/pipe.rs`
  - Pipes sin copia de datos entre procesos.
  - **Tests:** `pipe_zero_copy_throughput`

#### Security — USR-P2: Sessions

- [ ] **USR-P2a. SessionManager global + ObCreate(Session)** | Prereqs: USR-P1a | Files: `src/globals.rs`, `src/scheduler/mod.rs`, `src/syscall/ob.rs`
  - Add `SESSION_MANAGER: Mutex<SessionManager>` global with `sessions: Vec<Option<Session>>`
  - Handler for `sys_ob_create(Session)` — allocates session_id (1-based), creates Session struct
  - Auto-path: `\Session\{session_id}` in namespace
  - Session struct: `{ session_id, user_sid, token, login_time, state, vt_num, process_count }`
  - **Tests:** `usr_session_create_alloc_id`, `usr_session_namespace_path`, `usr_session_create_then_query`

- [ ] **USR-P2b. ObInfoClass::SessionInfo + ObSetInfoClass::SessionLock/Logoff** | Prereqs: USR-P2a | Files: `src/object/types.rs`, `src/syscall/ob.rs`
  - Add `SessionInfo = 24` to ObInfoClass: returns session_id, user_sid, state, login_time, process_count
  - Add `SessionLock = 28` and `SessionLogoff = 29` to ObSetInfoClass
  - SessionLock: sets state=Locked, blocks input on associated VT
  - SessionLogoff: terminates all processes in session, frees session slot
  - **Tests:** `usr_session_query_info`, `usr_session_lock_state`, `usr_session_logoff_cleans_up`

- [ ] **USR-P2c. TokenInfo + Token inheritance with session_id** | Prereqs: USR-P2a | Files: `src/object/types.rs`, `src/syscall/ob.rs`, `src/scheduler/mod.rs`
  - Add `TokenInfo = 28` to ObInfoClass: returns sid, is_admin, groups, privileges, integrity_level, session_id
  - Modify `add_ring3_process()`: child inherits `session_id` from parent's token
  - **Tests:** `usr_token_info_query`, `usr_process_inherits_session_id`

- [ ] **USR-P2d. neologon.nxe: login binary** | Prereqs: USR-P2b, USR-P2c | Files: `userbin/neologon/` (new), `libneodos/src/syscall.rs`
  - New user binary: `userbin/neologon/` with `_start()` entry
  - Prints login prompt, reads username + password
  - Calls `sys_ob_create(Session)` → kernel validates SAM credentials
  - On success: spawns shell (`C:\Programs\neoshell.nxe`) within the new session
  - On failure: prints error, re-prompts (max 3 attempts)
  - libneodos wrappers: `session_create()`, `session_lock()`, `session_logoff()`, `change_password()`
  - **Tests:** `usr_neologon_login_ok`, `usr_neologon_login_bad_password`, `usr_neologon_max_attempts`

- [ ] **USR-P2e. NeoInit spawns neologon instead of shell** | Prereqs: USR-P2d | Files: `userbin/neoinit/`
  - NeoInit Phase 4: spawn `C:\Programs\neologon.nxe` instead of shell directly
  - neologon handles the shell spawn after authentication
  - If DefaultAutoLogin is set in Registry: auto-login as that user (skip prompt)
  - **Tests:** `usr_neoinit_spawns_neologon`, `usr_auto_login_from_registry`

#### Security — USR-P3: FS Security

- [ ] **USR-P3a. DirEntryV2: add owner_sid field** | Prereqs: USR-P1b | Files: `src/fs/neodos_dir.rs`, `src/fs/neodos_v2.rs`
  - Add `owner_sid: Sid` to DirEntryV2 (serialized size grows 128→136 bytes)
  - Superblock flag `FEATURE_OWNER_SID`: indicates extended dir entries
  - Backward compat: read old NE2 without owner_sid → assign default S-1-5-21-0-0-0-1000
  - Write new dir entries with current process token.sid as owner
  - **Tests:** `usr_direntry_owner_sid_written`, `usr_direntry_backward_compat_read`, `usr_direntry_default_owner`

- [ ] **USR-P3b. VFS permission checking function** | Prereqs: USR-P1d, USR-P3a | Files: `src/fs/vfs.rs`
  - Add `check_vfs_access(token, mode, owner_sid, desired) -> bool` to VFS
  - Logic: owner check → group member check → other check
  - Admin bypass: admin always granted
  - Uses existing PERM_R/W/X/D bits from `mode` field
  - **Tests:** `usr_vfs_check_owner_rw`, `usr_vfs_check_other_ro`, `usr_vfs_check_admin_bypass`

- [ ] **USR-P3c. Wire VFS permission checks in syscall handlers** | Prereqs: USR-P3b | Files: `src/syscall/ob.rs`
  - `handler_ob_open` (VFS paths): check READ/WRITE/EXECUTE against file mode + owner
  - `handler_ob_create` (VFS mkdir/create): check WRITE against parent dir
  - `handler_ob_destroy` (VFS unlink): check DELETE against file
  - `handler_ob_set_info(VfsRename)`: check WRITE+DELETE against file
  - Returns `ObError::AccessDenied` on failure
  - **Tests:** `usr_vfs_open_read_ok`, `usr_vfs_open_write_denied`, `usr_vfs_create_in_own_dir`, `usr_vfs_delete_own_file_ok`, `usr_vfs_delete_other_file_denied`

- [ ] **USR-P3d. Default permissions by extension** | Prereqs: USR-P3c | Files: `src/fs/vfs.rs`
  - On `create()`: apply default PERM_* bits based on file extension
  - .NEM → PERM_R|PERM_S (admin-only read, system file)
  - .SYS → PERM_R|PERM_S (admin-only read, system file)
  - .NXE → PERM_R|PERM_X (world-readable+executable)
  - .NXL → PERM_R|PERM_X (world-readable+executable)
  - .CFG/.INI → PERM_R|PERM_W (user config)
  - (other) → PERM_R|PERM_W (user files)
  - Directories → PERM_R|PERM_W|PERM_X|PERM_D
  - **Tests:** `usr_default_perm_nem_readonly`, `usr_default_perm_nxe_rx`, `usr_default_perm_dir_full`

#### Power Manager — Phase 3: Syscall dispatch + Event Bus

- [ ] **PM-PHASE3. Power syscall dispatch + Event Bus types** | Prereqs: PM-PHASE2 | Files: `src/syscall/ob.rs`, `src/eventbus/mod.rs`, `src/abi_freeze.rs`, `src/power/event.rs` (new)
  - `src/syscall/ob.rs`: añadir dispatch en `handler_ob_set_info` para clases 37-42 (PowerShutdown, PowerReboot, PowerSuspend, PowerHibernate, PowerSetPlan, PowerSetPolicy). Admin check para shutdown/reboot/set-plan.
  - `src/syscall/ob.rs`: añadir dispatch en `handler_ob_query_info` para clases 32-34 (PowerPlanInfo, PowerStatus, PowerSystemState). Sin admin check.
  - `src/eventbus/mod.rs`: añadir tipos 19-26: `EVENT_SHUTDOWN_PHASE2`, `EVENT_SUSPEND`, `EVENT_RESUME`, `EVENT_POWER_BUTTON`, `EVENT_LID_CLOSE`, `EVENT_LID_OPEN`, `EVENT_BATTERY_LOW`, `EVENT_POWER_SOURCE_CHANGE`.
  - `src/abi_freeze.rs`: validar nuevos tipos no estén en rango 0-15.
  - `src/power/event.rs`: handlers para `EVENT_POWER_BUTTON` → ejecutar `PowerButtonAction`, `EVENT_LID_CLOSE` → ejecutar `LidAction`.
  - **Tests:** `pm_shutdown_transition_state`, `pm_shutdown_dispatches_event`, `pm_shutdown_flushes_hives`, `pm_shutdown_second_call_busy`, `pm_event_power_button_triggers_action`, `pm_event_lid_close_triggers_action`

### v0.53: Security + Registry + Integrity

#### Security — Module signing + driver permissions

- [ ] **B5.1. Module signature validation** | Prereqs: NT6 | Files: `src/drivers/loader.rs`
  - Validación criptográfica de módulos `.nem` antes de cargar.
  - **Tests:** `nem_signature_valid_accepts`, `nem_signature_invalid_rejects`, `nem_signature_tamper_detected`

- [ ] **B5.2. Driver permission enforcement** | Prereqs: NT6.3, B5.1 | Files: `src/drivers/caps.rs`
  - Cruzar capacidad declarada del driver con token del proceso y ACL del objeto.
  - **Tests:** `driver_caps_allow_admin`, `driver_caps_deny_user`, `driver_caps_acl_intersection`

#### Registry (Phase 2 — dirty tracking + multi-hive)

- [ ] **CM-DIRTY. Registry per-cell dirty tracking + incremental flush** | Prereqs: -- | Files: `src/cm/hive.rs`, `src/cm/cache.rs`, `src/cm/mod.rs`
  - `dirty_cells: BitVec` (1 bit por slot). `slot_mut()` marca dirty; `serialize_dirty()` escribe solo celdas sucias.
  - **Tests:** `cm_dirty_cell_set_on_write`, `cm_dirty_cleared_after_flush`, `cm_dirty_serialize_only_dirty`, `cm_dirty_full_flush_roundtrip`

- [ ] **CM-MULTI. Registry multi-hive** | Prereqs: -- | Files: `src/cm/mod.rs`
  - Montar SOFTWARE, SECURITY, DEFAULT hives. Cada hive crea su directorio raíz en namespace Ob.
  - **Tests:** `cm_multi_software_mounted`, `cm_multi_hive_isolation`, `cm_multi_cross_hive_path_fails`, `cm_multi_unload_reload`

> **Nota:** Registry ACL security (CM-SEC) integrado con USR-P4a/4b/4c.

#### Security — USR-P4: Registry ACL

- [ ] **USR-P4a. cm/security.rs: Registry ACL checking module** | Prereqs: USR-P1d | Files: `src/cm/security.rs` (new), `src/cm/mod.rs`
  - New file: `cm_check_access(token, sec_desc, desired_access) -> bool`
  - Reuses `SeAccessCheck` from security subsystem
  - If key has no sec_desc_cell: default — admin full, user read-only
  - Helper: `cm_default_sec_desc(creator_sid)` — creates SD with creator as owner
  - **Tests:** `usr_cm_sec_check_admin_full`, `usr_cm_sec_check_user_readonly`, `usr_cm_sec_default_sd`

- [ ] **USR-P4b. Wire sec_desc_cell on key creation** | Prereqs: USR-P4a | Files: `src/cm/hive.rs`
  - On `KeyCell` creation: inherit parent's `sec_desc_cell` or create default via `cm_default_sec_desc()`
  - Store `sec_desc_cell` as index to a Security cell in the hive
  - Serialize/deserialize Security cells in NEOH format
  - **Tests:** `usr_cm_key_inherits_parent_sec`, `usr_cm_key_default_sec_when_no_parent`

- [ ] **USR-P4c. ACL checks in Cm syscall handlers** | Prereqs: USR-P4b | Files: `src/syscall/cm.rs` or `src/syscall/ob.rs` (Registry handlers)
  - Wire `cm_check_access()` in: open_key, create_key, delete_key, set_value, delete_value, enum_key, enum_value
  - Returns `ObError::AccessDenied` if check fails
  - **Tests:** `usr_cm_open_key_admin`, `usr_cm_create_key_user_denied`, `usr_cm_delete_key_admin_only`

- [ ] **USR-P4d. User profile hive auto-mount** | Prereqs: USR-P4c | Files: `src/cm/mod.rs`
  - On session creation: auto-mount `\Registry\User\{sid}` hive
  - Profile hive stored at `C:\Users\{username}\ntuser.hiv`
  - Default values: Environment\PATH, Console\colors, etc.
  - **Tests:** `usr_cm_user_hive_mounted_on_login`, `usr_cm_user_hive_has_defaults`

#### Security — USR-P5: Integrity levels

- [ ] **USR-P5a. Integrity level in SeAccessCheck** | Prereqs: USR-P1b | Files: `src/security/access.rs`
  - Extend `SeAccessCheck`: if `process_IL < object_IL`, deny WRITE/DELETE (allow READ)
  - Add `integrity_level` field to `SecurityDescriptor` (default=Medium)
  - Admin bypass: SYSTEM integrity level always passes
  - **Tests:** `usr_il_medium_read_high_ok`, `usr_il_medium_write_high_denied`, `usr_il_system_bypass`

- [ ] **USR-P5b. SetIntegrityLevel + IntegrityLevel info classes** | Prereqs: USR-P5a | Files: `src/object/types.rs`, `src/syscall/ob.rs`
  - Add `SetIntegrityLevel = 32` to ObSetInfoClass — can only lower IL, never raise
  - Add `IntegrityLevel = 27` to ObInfoClass — query object/process IL
  - Handler validates: new_IL < current_IL (can't raise), returns Inval if try to raise
  - **Tests:** `usr_il_drop_from_high_to_medium`, `usr_il_raise_denied`, `usr_il_query_current`

- [ ] **USR-P5c. Privilege enforcement in admin syscalls** | Prereqs: USR-P1b | Files: `src/syscall/permission.rs`, `src/syscall/ob.rs`
  - Wire `token.has_privilege(bit)` in admin-only syscalls: driver_unload, cm_load_hive, cm_unload_hive
  - Token filtering: `new_admin()` → all 12 privileges; `new_user()` → only SE_CHANGE_NOTIFY
  - **Tests:** `usr_priv_admin_has_all`, `usr_priv_user_has_change_notify`, `usr_priv_driver_unload_denied_for_user`

#### Kernel

- [ ] **A3.2. Kernel debugger (KD)** | Prereqs: A3.1 | Files: `src/debugger/mod.rs`
  - INT3 breakpoints, hardware watchpoints (DR0-DR3), GDB remote protocol stub via serial.
  - **Tests:** `kd_breakpoint_set_and_hit`, `kd_breakpoint_invalid_addr`, `kd_watchpoint_write_detect`, `kd_register_snapshot`, `kd_gdb_protocol_qSupported`

#### Userland

- [ ] **B4.6. NeoEdit text editor** | Prereqs: A4.7, B4.4 | Files: `userbin/neoedit/`
  - Editor de texto modal Ring 3. Usa `ob_open` + `ob_query_info(ReadContent)` / `ob_set_info(WriteContent)`.
  - **Tests:** `neoedit_open_display`, `neoedit_edit_save`, `neoedit_scroll`

- [ ] **B4.7. Shared library per-process binding** | Prereqs: sys_loadlib | Files: `src/elf.rs`, `libneodos/`
  - Evolucionar NXL slots globales a binding per-process. Cada EPROCESS mantiene su tabla de NXLs.
  - **Tests:** `nxl_per_process_isolation`, `nxl_unload_on_exit`, `nxl_version_coexistence`

---

## LOW

### v0.54+: Hardening + Multi-hive + Docs + Cleanup

#### Registry (Phase 3 — WAL + lib wrappers)

- [ ] **CM-WAL. Registry WAL (write-ahead logging, crash recovery)** | Prereqs: -- | Files: `src/cm/wal.rs` (new), `src/cm/mod.rs`
  - Cada mutación escribe entrada WAL a `C:\System\Registry\<name>.wal` + fsync antes de aplicar a hive.
  - En mount: si existe `.wal`, hacer replay antes de cargar `.hiv`.
  - **Tests:** `cm_wal_created_on_mutation`, `cm_wal_replay_on_load`, `cm_wal_truncated_after_flush`, `cm_wal_power_loss_recovery`

- [ ] **CM-LIB. Registry libneodos wrappers** | Prereqs: -- | Files: `libneodos/src/syscall.rs`
  - Añadir 7 wrappers: `sys_cm_create_key`, `sys_cm_delete_key`, `sys_cm_enum_key`, `sys_cm_enum_value`, `sys_cm_flush_key`, `sys_cm_load_hive`, `sys_cm_unload_hive`.
  - **Tests:** `cm_lib_create_key_wrapper`, `cm_lib_enum_key_wrapper`, `cm_lib_enum_value_wrapper`, `cm_lib_flush_key_wrapper`

- [ ] **CM-REGEDIT. regedit.nxe** | Prereqs: CM-LIB | Files: `userbin/regedit/` (new)
  - Navegación de árbol, crear/borrar claves, set/query valores, flush manual.
  - **Tests:** `regedit_browse_tree`, `regedit_create_delete_key`, `regedit_set_query_value`, `regedit_flush`

#### Power Manager — Phase 4: Service Manager + libneodos + shell

- [ ] **PM-PHASE4. Shutdown coordination + libneodos wrappers + shell commands** | Prereqs: PM-PHASE3 | Files: `src/services/mod.rs`, `libneodos/src/power.rs` (new), `libneodos/src/syscall.rs`, `userbin/neoshell/`
  - `src/services/mod.rs`: añadir `ServiceManager::stop_all()` — itera servicios en orden inverso de dependencias, para cada uno llama `stop_service()` con timeout.
  - `ServiceManager::stop_all()` es llamado por `PowerCoordinator::shutdown()` antes de `EVENT_SHUTDOWN_PHASE2`.
  - `libneodos/src/power.rs`: wrapper `power_shutdown()`, `power_reboot()`, `power_suspend()`, `power_hibernate()`, `power_get_active_plan()`, `power_set_active_plan()`, `power_set_policy()`.
  - Tipos públicos: `PowerPlanInfo`, `PowerSystemStatus`, `PowerPolicyUpdate`.
  - Internamente: `ob_open("\Device\PowerManager")` → cache fd → `ob_set_info`/`ob_query_info`.
  - `userbin/neoshell/`: añadir `REBOOT` built-in. Migrar `POWEROFF` a llamar `libneodos::power_shutdown()`.
  - **Tests:** `pm_service_manager_stop_all_order`, `pm_service_manager_stop_all_timeout`, `pm_lib_get_plan`, `pm_lib_set_plan`, `pm_lib_reboot`, `pm_lib_shutdown`

#### Power Manager — Phase 5: Polish + event-driven coordination + tests

- [ ] **PM-PHASE5. Power Manager polish: async coordination, full test suite** | Prereqs: PM-PHASE4 | Files: `src/power/coordinator.rs`, `src/power/event.rs`, `src/power/mod.rs`, `docs/power-manager.md`
  - Completar coordinación asíncrona en `coordinator.rs`: shutdown con timeout por servicio, fallback force-kill.
  - Integrar `EVENT_POWER_BUTTON` con `PowerManager` para ejecutar `power_button_action` desde Event Bus.
  - Integrar `EVENT_LID_CLOSE`/`EVENT_LID_OPEN` para ejecutar `lid_action`.
  - Integración con `\Global\Info\Power` para consultas vía `ob_query_info`.
  - Completar suite de tests (25 tests del diseño original):
    - Inicialización: `pm_init_state_active`, `pm_device_namespace_exists`, `pm_query_plan_defaults`, `pm_capabilities_from_fadt`
    - Planes: `pm_set_plan_balanced`, `pm_set_plan_performance`, `pm_set_plan_invalid`, `pm_plan_persists_to_registry`
    - Políticas: `pm_set_policy_display_timeout`, `pm_set_policy_invalid_id`, `pm_policy_persists`, `pm_policy_restored_on_plan_switch`
    - Shutdown: `pm_shutdown_transition_state`, `pm_shutdown_dispatches_event`, `pm_shutdown_flushes_hives`, `pm_shutdown_second_call_busy`
    - Eventos: `pm_event_power_button_triggers_action`, `pm_event_lid_close_triggers_action`
    - HAL/ACPI: tests de `pm_fadt_*`, `pm_hal_*`, `pm_lib_*`
  - **Tests:** completar los 25 tests + integración en QEMU

#### Security — USR-P6: User commands

- [ ] **USR-P6a. WHOAMI command** | Prereqs: USR-P2c | Files: `userbin/neoshell/`
  - New built-in or .NXE: queries `TokenInfo` via `ob_query_info(process_fd, 28)`
  - Extracts SID → SAM lookup → prints `username [SID]`
  - **Tests:** `usr_whoami_prints_username`, `usr_whoami_shows_sid`

- [ ] **USR-P6b. PASSWD command** | Prereqs: USR-P2d | Files: `userbin/neoshell/`
  - Prompts for old password, new password, confirm new password
  - Calls `ob_set_info(session_fd, ChangePassword, buffer)`
  - Prints success or error message
  - **Tests:** `usr_passwd_change_ok`, `usr_passwd_wrong_old`, `usr_passwd_mismatch_confirm`

- [ ] **USR-P6c. WHO + LOGOFF commands** | Prereqs: USR-P2b | Files: `userbin/neoshell/`
  - `WHO`: `ob_enum(\Session\)` → for each session, query SessionInfo → print user + since
  - `LOGOFF`: `ob_set_info(session_fd, SessionLogoff)` → kernel terminates all session processes
  - **Tests:** `usr_who_lists_sessions`, `usr_logoff_terminates_shell`

- [ ] **USR-P6d. SU command** | Prereqs: USR-P2d, USR-P2e | Files: `userbin/neoshell/`
  - `SU <username>`: prompts for target user's password
  - On auth: spawns new shell in target user's session
  - Uses existing `sys_ob_create(Process)` with attrs encoding target token
  - **Tests:** `usr_su_correct_password`, `usr_su_wrong_password`, `usr_su_spawns_as_target`

- [ ] **USR-P6e. RUNAS command** | Prereqs: USR-P6d | Files: `userbin/neoshell/`
  - `RUNAS [/USER:admin] <command>`: spawns command with different token
  - Requires target user's password (or admin consent)
  - **Tests:** `usr_runas_admin_command`, `usr_runas_user_denied_without_password`

- [ ] **B5.3. Secure boot chain** | Prereqs: B5.1 | Files: `neodos-bootloader/`, `src/boot/secure.rs`
  - Verificación encadenada bootloader → kernel → drivers.
  - **Tests:** `secure_boot_kernel_verified`, `secure_boot_driver_verified`, `secure_boot_fail_closed`

#### Networking

- [ ] **NET-DNS. DNS resolver (stub resolver + cache)** | Prereqs: NET-1.9 | Files: `src/net/dns.rs`, `libnet/`
  - Stub resolver: consulta UDP a servidor DNS (puerto 53), parsea respuestas (A, AAAA, CNAME, MX).
  - Caché local con TTL (hasta 64 entradas, expiración por timer).
  - Integración con libnet: `dns_resolve(hostname) -> Ipv4Addr`.
  - Servidores DNS desde Registry (`HKLM\Network\Interfaces\0\DnsServer`), configurable via ipconfig.
  - **Tests:** `dns_parse_a_response`, `dns_parse_cname_chain`, `dns_cache_hit_ttl`, `dns_cache_expiry`, `dns_resolve_localhost`, `dns_server_from_registry`

#### i18n — Internacionalización

> Diseño completo: `docs/design/i18n-design.md`. Formato NLT (Neodos Language Table), runtime en libneodos.
> El kernel NO traduce. Las aplicaciones traducen vía `tr!("clave")` → `i18n_get()`. Fallback: es-ES → es → en-US → clave literal.

- [ ] **I18N-P1. Runtime i18n en libneodos + formato NLT** | Prereqs: -- | Files: `libneodos/src/i18n.rs` (new), `libneodos/src/lib.rs`, `libneodos/src/macros.rs`, `neodos-kernel/src/cm/mod.rs`
  - Nuevo `libneodos/src/i18n.rs`: `NltTable`, `i18n_get()`, `i18n_load()`, `i18n_init()`, `tr!()` macro.
  - Formato NLT: magic `NLT\0`, version=1, offsets u32, búsqueda O(n), zero-copy.
  - `i18n_init()`: lee `\Registry\Machine\...\Control\Locale\Language` del Registry.
  - `i18n_load("app")`: busca `C:\System\Locale\{locale}\{app}.nlt` con cadena de fallback.
  - `tr!("clave")`: si no encuentra traducción, devuelve la clave literal (nunca panic).
  - Kernel: añadir valor `Language` = `"en-US"` por defecto en Registry.
  - **Tests:** `i18n_parse_nlt_valid`, `i18n_get_exact_match`, `i18n_get_missing_returns_key`, `i18n_fallback_chain`, `i18n_load_app_not_found`

- [ ] **I18N-P2. Migrar NeoShell + NeoInit + apps core** | Prereqs: I18N-P1 | Files: `userbin/neoshell/`, `userbin/neoinit/`, `userbin/corehelp/`, `userbin/coredir/`, `userbin/corecopy/`, `userbin/kill/`, `userbin/ps/`
  - Añadir `i18n_init()` + `i18n_load("app_name")` al inicio de cada main().
  - Reemplazar ~72 strings hardcoded por `tr!("clave")`.
  - Claves con convención jerárquica: `error.bad_command`, `prompt.suffix`, `status.running`.
  - Los `b"\r\n"` y caracteres de control se mantienen separados (no se traducen).
  - **Tests:** integración — boot con locale=en-US, verificar mensajes en inglés.

- [ ] **I18N-P3. neolocale tool + archivos .nlt + segundo idioma** | Prereqs: I18N-P2 | Files: `tools/neolocale/` (new), `locale/en-US/*.nlt`, `locale/es-ES/*.nlt`, `scripts/create_ne2_image.py`
  - Nueva herramienta `neolocale.nxe`: validate, check, diff, create, stats.
  - Crear archivos `.nlt` para en-US (inglés por defecto).
  - Crear archivos `.nlt` para es-ES (español, segundo idioma).
  - Integrar `.nlt` en la imagen de disco via `scripts/create_ne2_image.py`.
  - **Tests:** `neolocale_validate_valid`, `neolocale_check_missing`, `i18n_switch_locale_runtime`

- [ ] **BUG-NEM-RX. NEM e1000 driver no recibe paquetes** | Files: `drivers/e1000/src/lib.rs`, `neodos-kernel/src/drivers/nem/net_bridge.rs`
  - `e1000_poll()` nunca detecta paquetes entrantes (bit DD no seteado). Workaround: `default_nic_id()` prefiere kernel e1000.

#### Tracing

- [ ] **B1.1. Kernel tracing infrastructure** | Prereqs: A2.4 | Files: `src/trace/mod.rs`
  - TraceBuffer con trace points registrables dinámicamente, filtrado por categoría/nivel, dump via serial con timestamps HPET.
  - **Tests:** `trace_register_dynamic_point`, `trace_filter_by_category`, `trace_dump_serial_format`

- [ ] **B1.2. NeoTrace system** | Prereqs: B1.1 | Files: `userbin/neotrace/`
  - Comando `NEOTRACE` con subcomandos START/STOP/DUMP/FILTER.
  - **Tests:** `neotrace_start_stop_toggle`, `neotrace_dump_output`

#### Admin (Fase 3 — Avanzado)

- [ ] **ADM-3. neolog** | Prereqs: B1.1 | Files: `userbin/neolog/`
  - Visor de event log del kernel + EventBus. Filtro por categoría/nivel/timestamp.
  - **Tests:** `neolog_eventbus_dump`, `neolog_trace_filter`

- [ ] **ADM-7. neoctl** | Files: `userbin/neoctl/`
- [ ] **ADM-8. neodebug** | Files: `userbin/neodebug/`
- [ ] **ADM-9. neomem v0.2** | Files: `userbin/neomem/`
- [ ] **B4.8. NeoTOP v0.2+** | Files: `userbin/neotop/`
- [ ] **B4.12. Compositor 2D** | Files: `userbin/compositor/`

#### Kernel

- [ ] **B6.2. Copy-on-write fork** | Prereqs: -- | Files: `src/memory/cow.rs`, `src/syscall.rs`
- [ ] **AUDIT-17. User address space constrained (USER_LIMIT=36MB)** | Files: `src/arch/x64/paging.rs`
- [ ] **AUDIT-48. Fixed 16 KB kernel stack with no guard page** | Files: `src/scheduler/mod.rs:21`
- [ ] **AUDIT-34. No RAII IRQL guard — 15+ manual raise/lower** | Files: `src/scheduler/mod.rs`
- [ ] **AUDIT-33. `BIN_BUF` global static mut not re-entrant** | Files: `src/syscall/handlers.rs:79`
- [ ] **AUDIT-47. Non-reentrant IRP pool with wraparound overwrite** | Files: `src/irp/mod.rs:13-14`

#### VFS (remaining)

- [ ] **VFS-3.2. `\DosDevices` dinámico** | Files: `src/vfs/mount.rs`
- [ ] **VFS-5.3. Write-back ordenado (flush page → flush block)** | Files: `src/globals.rs`
- [ ] **VFS-6.1. Overlay mounts** | Files: `src/fs/vfs.rs`
- [ ] **VFS-6.2. Extended attributes VFS** | Files: `src/fs/vfs.rs`
- [ ] **VFS-6.3. File notifications via Event Bus** | Files: `src/fs/vfs.rs`, `src/eventbus/`
- [ ] **VFS-6.4. Async VFS operations via IRP** | Files: `src/fs/vfs.rs`
- [ ] **VFS-7.1. Eliminar lock global de VFS** | Files: `src/globals.rs`, `src/fs/vfs.rs`
- [ ] **VFS-7.2. Lookup cache** | Files: `src/fs/vfs.rs`
- [ ] **VFS-7.3. Path cache** | Files: `src/fs/vfs.rs`

#### Cleanup (quick wins — refactors and dead code)

- [ ] **CLEANUP-1. Dead code mask `#![allow(dead_code)]` in main.rs + globals.rs** | Files: `src/main.rs:9`, `src/globals.rs:1`
  - Remove `#[allow(dead_code)]`, fix revealed dead items. Merged from AUDIT-30/AUDIT-82.
  - **Tests:** (compile-only)

- [ ] **CLEANUP-2. Unused macros + functions + enum variants + constants** | Files: multiple
  - Merged from AUDIT-31. Remove `with_current!`, `trace_irq_enter!`/`trace_irq_exit!`, `register_tests()` (virtio), `with_cache`, `nic_get_mask`, `socket_next_accept_id`, `pipe_peek_read_closed`, `clear`/`segment_count`, `ObError::TableFull`, `ObType::EventBus`, `PIT_HZ`.
  - **Tests:** verify build

- [ ] **CLEANUP-3. AUDIT-35. virtio::register_tests() orphaned** | Files: `src/virtio/mod.rs:35`
  - `register_tests()` defined but never called from `testing.rs`. Add call.
  - **Tests:** Add call to `virtio::register_tests()` in `testing.rs`

- [ ] **CLEANUP-4. AUDIT-51. unregister_all() does nothing** | Files: `src/drivers/nem/driver.rs:92-98`
- [ ] **CLEANUP-5. AUDIT-55/77. ABI validation duplicated** | Files: `src/drivers/abi/mod.rs:50-80`, `src/drivers/nem/policy.rs:27-57`
  - `abi::negotiate()` and `policy::validate_abi()` implement same three checks. Make `validate_abi()` delegate to `negotiate()`.

- [ ] **CLEANUP-6. AUDIT-56. Dual mount managers** | Files: `src/fs/vfs.rs:84-95`, `src/vfs/mount.rs:38-123`
- [ ] **CLEANUP-7. AUDIT-58. Error constants duplicated libneodos/libneodos-nxl** | Files: `libneodos/src/syscall.rs:3-17`, `libneodos-nxl/src/error.rs:4-18`
- [ ] **CLEANUP-8. AUDIT-59. 10+ enums with manual `to_str()` instead of `Display`** | Files: multiple
- [ ] **CLEANUP-9. AUDIT-60. iso9660.rs dead filesystem driver** | Files: `src/drivers/iso9660.rs`
- [ ] **CLEANUP-10. AUDIT-61. debugger/mod.rs GDB stub dead code** | Files: `src/debugger/mod.rs`
- [ ] **CLEANUP-11. AUDIT-62. kbd_layout.rs never compiled** | Files: `src/drivers/nem/drivers/kbd_layout.rs`
- [ ] **CLEANUP-12. AUDIT-63. 23 dead functions** | Files: multiple (see AUDIT-63 description)
- [ ] **CLEANUP-13. AUDIT-64. PageCacheLevel unused variants** | Files: `src/vfs/io.rs:9`
- [ ] **CLEANUP-14. AUDIT-65. Dead struct CryptoContext** | Files: `src/vfs/io.rs:16`
- [ ] **CLEANUP-15. AUDIT-50/80. `lazy_static!` → `LazyLock`** | Files: multiple (27 usages)
- [ ] **CLEANUP-16. AUDIT-72. net/mod.rs monolithic protocol dispatch** | Files: `src/net/mod.rs:68-197`
- [ ] **CLEANUP-17. AUDIT-73. Storage probe hardcoded to 4 drivers** | Files: `src/drivers/storage_manager.rs:2-5`
- [ ] **CLEANUP-18. AUDIT-74. SPSC ring buffer triplicated** | Files: `src/work_queue.rs`, `src/input/vt.rs`, `src/arch/x64/cpu_local.rs`
- [ ] **CLEANUP-19. AUDIT-75. 27 fixed-size arrays across kernel** | Files: multiple
- [ ] **CLEANUP-20. AUDIT-76. Network unsafe pointer casts (9×)** | Files: `src/net/mod.rs`
- [ ] **CLEANUP-21. AUDIT-78. kernel_stack_trace fixed crash buffers** | Files: `src/crash/mod.rs:34,66,70`
- [ ] **CLEANUP-22. AUDIT-79. from_u8/from_u16 → TryFrom** | Files: `src/drivers/nem/mod.rs:46-98`
- [ ] **CLEANUP-23. AUDIT-83. TOCTOU in storage probe** | Files: `src/drivers/storage_manager.rs`
- [ ] **CLEANUP-24. AUDIT-11. IPI function duplicates** | Files: `src/arch/x64/smp.rs`
- [ ] **CLEANUP-25. AUDIT-12. AHCI structs defined twice** | Files: `src/drivers/boot_ahci.rs`, `drivers/ahci/src/lib.rs`
- [ ] **CLEANUP-26. AUDIT-13. PCI config access in 7 files** | Files: `src/drivers/pci.rs`, `drivers/*/src/lib.rs`
- [ ] **CLEANUP-27. AUDIT-14. HST extern in 8 NEM drivers** | Files: `drivers/*/src/lib.rs`
- [ ] **CLEANUP-28. AUDIT-15. PAGE_SIZE defined 7 times** | Files: multiple
- [ ] **CLEANUP-29. AUDIT-16. Error enums overlapping variants** | Files: `src/fs/vfs.rs`, `src/fs/neodos_fs.rs`, `src/drivers/fat32.rs`, `src/drivers/iso9660.rs`
- [ ] **CLEANUP-30. AUDIT-18. Idle loops without `hlt`** | Files: `src/main.rs`, `src/hal/raw/cpu.rs`
- [ ] **CLEANUP-31. AUDIT-19. Global static mut without sync (40+)** | Files: multiple
- [ ] **CLEANUP-32. AUDIT-20. Split syscall/ob.rs (2280 lines) + handlers.rs (1771)** | Files: `src/syscall/ob.rs`, `src/syscall/handlers.rs`
- [ ] **CLEANUP-33. AUDIT-21. Scheduler panics on table full** | Files: `src/scheduler/mod.rs`
- [ ] **CLEANUP-34. AUDIT-22. Page cache O(n) linear scans** | Files: `src/buffer/page_cache.rs`
- [ ] **CLEANUP-35. AUDIT-49. 10 inconsistent name buffer sizes** | Files: multiple

#### Documentation

- [ ] **DH2. Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md** | Files: `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`
- [ ] **DH3. Completar libneodos syscall wrappers** | Files: `libneodos/src/syscall.rs`
- [ ] **DH-HISTORY. Mantener docs/HISTORY.md** | Files: `docs/HISTORY.md`

#### Object Manager / Syscalls

- [ ] **AI-1. Completar ObInfoClass/ObSetInfoClass enums** | Files: `libneodos/src/syscall.rs`
- [ ] **AI-2. Consolidate legacy syscall wrappers** | Files: `src/syscall/mod.rs`
- [ ] **AI-3. ObObjectTable lock granularity (lock striping)** | Files: `src/object/mod.rs`
- [ ] **AI-4. Arreglar TOCTOU race en kobj_register** | Files: `src/object/mod.rs`

#### VirtIO (low priority)

- [ ] **VIO-CON. VirtIO Console (0x1002)** | Files: `drivers/virtio-console/`
- [ ] **VIO-RNG. VirtIO RNG (0x1003)** | Files: `drivers/virtio-rng/`
- [ ] **VIO-SCSI. VirtIO SCSI (0x100A)** | Files: `drivers/virtio-scsi/`
- [ ] **VIO-GPU. VirtIO GPU (0x1012)** | Files: `drivers/virtio-gpu/`
- [ ] **VIO-VSOCK. VirtIO VSOCK (0x1014)** | Files: `drivers/virtio-vsock/`
- [ ] **VIO-SOUND. VirtIO Sound (0x1015)** | Files: `drivers/virtio-sound/`
- [ ] **VIO-BALLOON. VirtIO Memory Balloon (0x1004)** | Files: `drivers/virtio-balloon/`

#### Experimental

- [ ] **B7.1. Full GUI system** | Files: `userbin/gui/`
- [ ] **B7.2. Advanced secure boot (TPM)** | Files: `src/boot/tpm.rs`
- [ ] **B7.3. Package manager** | Files: `userbin/neopkg/`
- [ ] **B7.4. Time-travel debugging** | Files: `src/debugger/timetravel.rs`
- [ ] **B7.5. Live kernel patching** | Files: `src/patch/mod.rs`
- [ ] **B7.6. Distributed NeoDOS nodes** | Files: `src/cluster/`
- [ ] **PKG-1. NeoGet v1 (diferido a v0.70)** | Files: (design only)

---

## Milestones

| Versión | Enfoque | Estado |
| --------- | --------- | -------- |
| v0.50 | Shell tokenizer + NeoFS snapshot syscall + Power Phase 2 + Kernel hardening | **PRÓXIMO** |
| v0.51 | NeoFS v2 remaining (B-tree, freelist, snapshot, mkfs), Shell Phase 2 (editor, env, pipeline, batch), USR-P1 (SAM foundation), Network tools, Admin tools | planned |
| v0.52 | VirtIO (ARCH+NET), Sessions (USR-P2), FS security (USR-P3), Zero-copy pipes | planned |
| v0.53 | Module sig validation, Registry dirty+multihive, Registry ACL (USR-P4), Integrity levels (USR-P5), KD, NeoEdit | planned |
| v0.54 | Secure boot, WAL, lib wrappers, User commands (USR-P6), DNS resolver, Tracing, User address space, Docs, Power Phases 4+5 | backlog |
| v0.55+ | Cleanup (dead code, duplicates, refactors), Backlog items | backlog |

---

## SSDT — Pending Migrations (v0.50+)

### SSDT-DRVUNLOAD: Migrate sys_driver_unload → Ob API

**Current state:** `sys_driver_unload` (RAX 35 / was 58) is a name-based legacy
syscall that unloads a NEM driver by name string. It goes through
`drivers::hotreload::unload_driver(&name, force)`.

**Problem:** The Ob API provides `ob_destroy(fd)` for object destruction, but
driver unloading is name-based, not fd-based. The driver is not registered as
a namespace object in the current implementation — it's only in the driver
registry.

**Proposed architecture:**

1. Ensure all loaded NEM drivers are discoverable as Ob namespace objects
   under `\Driver\` (e.g. `\Driver\PS2MOUSE`).
2. Change `loadnem.nxe` to:
   - Load: `ob_create("\Driver\<name>", DRIVER, ...)` — already done
   - Unload: `ob_open("\Driver\<name>", ...)` + `ob_destroy(fd)` — NOT done
3. Remove `handler_driver_unload` and the `sys_driver_unload` wrapper.

**Impact:**

- `loadnem.nxe`: must be updated to use Ob API for unload
- `libneodos`: remove `sys_driver_unload` wrapper
- Kernel: remove `handler_driver_unload` from SSDT
- Drivers: must register in Ob namespace on load

**Steps:**

1. [ ] Add driver namespace registration in `load_nem_driver()`
2. [ ] Update `loadnem.nxe` to use `ob_destroy()` for unload
3. [ ] Remove `handler_driver_unload` from SSDT
4. [ ] Remove `sys_driver_unload` from libneodos

### SSDT-MIGRATE-DUP2: Migrate sys_dup2 → Ob handle duplication

**Current state:** `sys_dup2` (RAX 22 / was 6) duplicates a file descriptor
slot within the process handle table.

**Analysis:** Dup2 is inherently process-local (handle table manipulation).
It could be exposed as `ob_set_info(HandleDuplicate)` on a process object,
but this adds complexity with no immediate benefit. The current implementation
is simple and efficient.

**Veredict:** Keep as-is. Not a candidate for Ob migration.

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
