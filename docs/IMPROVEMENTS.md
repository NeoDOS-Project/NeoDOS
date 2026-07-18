# NeoDOS — Plan de Implementación Detallado

> **Versión del proyecto:** v0.50.2 | **Tests:** 665 (kernel) | **ABI:** v8 | **SSDT:** RAX 0–59
>
> Este documento contiene el detalle granular de cada tarea: archivos,
> prerrequisitos, tests y descripción técnica.
>
> Para la visión general, fases, milestones y prioridades, consultar
> **[ROADMAP.md](../ROADMAP.md)** (documento maestro).
>
> Documentos relacionados:
> - [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) — Visión a largo plazo
> - [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md) — Invariantes MUST/MUST NOT

---

## Execution Rules

1. Una fase no empieza hasta que sus prerrequisitos están marcados **[COMPLETED]**.
2. Cada item pendiente incluye: ID, archivos, prereqs, tests.
3. Al completar: actualizar `CHANGELOG.md` y mover a `IMPROVEMENTS_COMPLETED.md`.
4. Validar: `cargo build` + `cargo run --bin neodev -- test` + `scripts/check_deps.py`.

---

## Fase 0: Consolidación (v0.50)

### M0.1 — Shell tokenizer + Power Phase 2 + Hardening

#### Kernel Hardening

- [ ] **AUDIT-32. 5+ `.expect()` panic paths → Result<()>** | Files: `src/scheduler/mod.rs:485-487`, `src/main.rs:334`, `src/globals.rs:38`, `src/arch/x64/serial.rs:73`, `src/urn/mod.rs:383`
  - Scheduler slot full, block device missing, serial write failure — all crash the kernel instead of returning `Result`.
  - **Tests:** `scheduler_slot_exhaustion_graceful`, `urn_create_failure_propagated`

- [ ] **AUDIT-34. Low-level syscall/interrupt validation** | Prereqs: -- | Files: `src/arch/x64/idt.rs`, `src/arch/x64/cpu_local.rs`, `src/syscall/mod.rs`
  - Añadir guardas de ABI, estado de interrupciones y resched para rutas críticas.
  - **Tests:** `syscall_invalid_arg_returns_error`, `interrupt_state_consistency`, `preempt_reschedule_guard`

- [ ] **AUDIT-35. Registry persistence hardening (flush atómico + recovery)** | Prereqs: CM-FIX | Files: `src/cm/mod.rs`, `src/cm/hive.rs`
  - **Tests:** `cm_atomic_flush`, `cm_recovery_on_corrupt_hive`

- [ ] **AUDIT-36. Userland build/linker pipeline** | Prereqs: -- | Files: `userbin/**`, `libneodos/`, `user.ld` | Repo: `NeoDev`
  - Normalizar entrypoint `_start`, linker scripts y empaquetado de `.NXE`.
  - **Tests:** `userbin_link_smoke`, `neoinit_shell_spawn_smoke`

- [ ] **AUDIT-37. Suite de tests de integración boot/registry/shell** | Prereqs: -- | Files: `src/testing.rs`
  - **Tests:** `boot_to_shell_integration`, `registry_persist_across_reboot`, `shell_command_execution_flow`

---

## Fase 1: Kernel Maduro (v0.51–v0.55)

### M1.1 — NeoFS v2 Completion (v0.51)

### M1.2 — Shell Phase 2 (v0.51)

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

### M1.3 — SAM Foundation + Network Tools (v0.51)

#### Security — USR-P1: SAM foundation

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

#### Admin Tools — Phase 1

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
  - Aplicación Ring 3 .NXE: panel de control modular.
  - `CfgModule` trait: cada subsistema (System, Keyboard, About, Power, Locale) implementa interfaz común.
  - `ui/menu.rs`, `ui/dialog.rs`.
  - Módulo System (solo lectura), Keyboard, About, Power (stub), Locale (stub).
  - Todos los textos visibles via `tr!()` macro (i18n desde el diseño).
  - **Tests:** `neocfg_menu_navigation`, `neocfg_system_info`, `neocfg_keyboard_set_layout`, `neocfg_about_version`, `neocfg_stubs_no_crash`, `neocfg_i18n_all_keys_present`, `neocfg_no_direct_registry_access`

- [ ] **ADM-6. neofs** | Prereqs: -- | Files: `userbin/neofs/`
  - Estadísticas de volumen, correr fsck, cambiar label, listar montajes.
  - **Tests:** `neofs_fsck_drive`, `neofs_format_volume`, `neofs_label_roundtrip`

### M1.4 — VirtIO Architecture (v0.52)

- [ ] **VIO-ARCH. Virtqueue abstraction + modern PCI transport** | Prereqs: A2.1 | Files: `src/virtio/` (new)
  - Capa base: virtqueue split vring 1.0, legacy I/O BAR + modern MMIO BAR (VirtIO 1.0+),
    feature negotiation, indirect descriptors, MSI-X + interrupciones (poll fallback), PCI discovery.
  - **Tests:** `vio_virtqueue_alloc_free`, `vio_virtqueue_submit_chain`, `vio_virtqueue_poll_completion`,
    `vio_modern_bar_detect`, `vio_feature_negotiation`, `vio_msix_configure`

- [ ] **VIO-NET. VirtIO Network (0x1000)** | Prereqs: VIO-ARCH | Files: `src/net/virtio_net.rs` or `drivers/virtio-net/` (NEM)
  - 1 RX + 1 TX virtqueue, mergeable RX buffers, checksum offload, MAC desde config space,
    link status polling, legacy + modern transport. Se integra con `src/net/nic.rs`.
  - **Tests:** `vio_net_probe`, `vio_net_send_recv`, `vio_net_mac_config`

- [ ] **VIO-BLK2. VirtIO Block NEM driver** | Prereqs: VIO-ARCH | Files: `drivers/virtio-blk/` (new, NEM SYSTEM)
  - Reemplazar BOOT_DRIVER inline por NEM standalone. Hotplug multi-dispositivo. MSI-X con DPC.
  - **Tests:** `vio_blk_probe`, `vio_blk_read_write`, `vio_blk_multi_device`

- [ ] **VIO-9P. VirtIO 9P (0x1009)** | Prereqs: VIO-ARCH | Files: `drivers/virtio-9p/` (NEM), `src/fs/9p.rs`
  - Filesystem 9P2000.L sobre VirtIO para compartir directorios host-huésped.
  - **Tests:** `vio_9p_version_attach`, `vio_9p_walk_open_read`, `vio_9p_write_close`

- [ ] **VIO-INPUT. VirtIO Input (0x1013)** | Prereqs: VIO-ARCH | Files: `drivers/virtio-input/` (NEM)
  - Teclado, ratón, tablet vía VirtIO. Integración con `src/input/manager.rs`.
  - **Tests:** `vio_input_key_event`, `vio_input_abs_event`, `vio_input_multi_device`

### M1.5 — Sessions + FS Security (v0.52)

#### Security — USR-P2: Sessions

- [ ] **USR-P2a. SessionManager global + ObCreate(Session)** | Prereqs: USR-P1a | Files: `src/globals.rs`, `src/scheduler/mod.rs`, `src/syscall/ob.rs`
  - Add `SESSION_MANAGER: Mutex<SessionManager>` global
  - Handler for `sys_ob_create(Session)` — allocates session_id (1-based)
  - Auto-path: `\Session\{session_id}` in namespace
  - **Tests:** `usr_session_create_alloc_id`, `usr_session_namespace_path`, `usr_session_create_then_query`

- [ ] **USR-P2b. ObInfoClass::SessionInfo + ObSetInfoClass::SessionLock/Logoff** | Prereqs: USR-P2a | Files: `src/object/types.rs`, `src/syscall/ob.rs`
  - Add `SessionInfo = 24` to ObInfoClass
  - Add `SessionLock = 28` and `SessionLogoff = 29` to ObSetInfoClass
  - **Tests:** `usr_session_query_info`, `usr_session_lock_state`, `usr_session_logoff_cleans_up`

- [ ] **USR-P2c. TokenInfo + Token inheritance with session_id** | Prereqs: USR-P2a | Files: `src/object/types.rs`, `src/syscall/ob.rs`, `src/scheduler/mod.rs`
  - Add `TokenInfo = 28` to ObInfoClass
  - Modify `add_ring3_process()`: child inherits `session_id` from parent's token
  - **Tests:** `usr_token_info_query`, `usr_process_inherits_session_id`

- [ ] **USR-P2d. neologon.nxe: login binary** | Prereqs: USR-P2b, USR-P2c | Files: `userbin/neologon/` (new), `libneodos/src/syscall.rs`
  - Prints login prompt, reads username + password
  - Calls `sys_ob_create(Session)` → kernel validates SAM credentials
  - **Tests:** `usr_neologon_login_ok`, `usr_neologon_login_bad_password`, `usr_neologon_max_attempts`

- [ ] **USR-P2e. NeoInit spawns neologon instead of shell** | Prereqs: USR-P2d | Files: `userbin/neoinit/`
  - NeoInit Phase 4: spawn `C:\Programs\neologon.nxe` instead of shell directly
  - If DefaultAutoLogin is set: auto-login as that user
  - **Tests:** `usr_neoinit_spawns_neologon`, `usr_auto_login_from_registry`

#### Security — USR-P3: FS Security

- [ ] **USR-P3a. DirEntryV2: add owner_sid field** | Prereqs: USR-P1b | Files: `src/fs/neodos_dir.rs`, `src/fs/neodos_v2.rs`
  - Add `owner_sid: Sid` to DirEntryV2
  - Backward compat: read old NE2 without owner_sid → assign default
  - **Tests:** `usr_direntry_owner_sid_written`, `usr_direntry_backward_compat_read`, `usr_direntry_default_owner`

- [ ] **USR-P3b. VFS permission checking function** | Prereqs: USR-P1d, USR-P3a | Files: `src/fs/vfs.rs`
  - Add `check_vfs_access(token, mode, owner_sid, desired) -> bool`
  - **Tests:** `usr_vfs_check_owner_rw`, `usr_vfs_check_other_ro`, `usr_vfs_check_admin_bypass`

- [ ] **USR-P3c. Wire VFS permission checks in syscall handlers** | Prereqs: USR-P3b | Files: `src/syscall/ob.rs`
  - Check READ/WRITE/EXECUTE in ob_open, ob_create, ob_destroy, ob_set_info
  - **Tests:** `usr_vfs_open_read_ok`, `usr_vfs_open_write_denied`, `usr_vfs_create_in_own_dir`, `usr_vfs_delete_own_file_ok`, `usr_vfs_delete_other_file_denied`

- [ ] **USR-P3d. Default permissions by extension** | Prereqs: USR-P3c | Files: `src/fs/vfs.rs`
  - On `create()`: apply default PERM_* bits based on file extension
  - .NEM/.SYS → admin-only, .NXE/.NXL → world r+x, etc.
  - **Tests:** `usr_default_perm_nem_readonly`, `usr_default_perm_nxe_rx`, `usr_default_perm_dir_full`

### M1.6 — Power Phase 3 + Zero-copy (v0.52)

- [ ] **PM-PHASE3. Power syscall dispatch + Event Bus types** | Prereqs: PM-PHASE2 | Files: `src/syscall/ob.rs`, `src/eventbus/mod.rs`, `src/abi_freeze.rs`, `src/power/event.rs` (new)
  - Dispatch en `handler_ob_set_info` para clases 37-42 (PowerShutdown, PowerReboot, PowerSuspend, etc.)
  - Dispatch en `handler_ob_query_info` para clases 32-34 (PowerPlanInfo, PowerStatus, PowerSystemState)
  - Tipos 19-26 en Event Bus: `EVENT_SHUTDOWN_PHASE2`, `EVENT_SUSPEND`, `EVENT_RESUME`, etc.
  - **Tests:** `pm_shutdown_transition_state`, `pm_shutdown_dispatches_event`, `pm_shutdown_flushes_hives`, `pm_shutdown_second_call_busy`, `pm_event_power_button_triggers_action`, `pm_event_lid_close_triggers_action`

- [ ] **B6.1. Zero-copy pipes** | Prereqs: -- | Files: `src/pipe.rs`
  - Pipes sin copia de datos entre procesos.
  - **Tests:** `pipe_zero_copy_throughput`

### M1.7 — Registry Phase 2 + Integrity Levels (v0.53)

#### Registry — Phase 2

- [ ] **CM-DIRTY. Registry per-cell dirty tracking + incremental flush** | Prereqs: -- | Files: `src/cm/hive.rs`, `src/cm/cache.rs`, `src/cm/mod.rs`
  - `dirty_cells: BitVec` (1 bit por slot). `serialize_dirty()` escribe solo celdas sucias.
  - **Tests:** `cm_dirty_cell_set_on_write`, `cm_dirty_cleared_after_flush`, `cm_dirty_serialize_only_dirty`, `cm_dirty_full_flush_roundtrip`

- [ ] **CM-MULTI. Registry multi-hive** | Prereqs: -- | Files: `src/cm/mod.rs`
  - Montar SOFTWARE, SECURITY, DEFAULT hives.
  - **Tests:** `cm_multi_software_mounted`, `cm_multi_hive_isolation`, `cm_multi_cross_hive_path_fails`, `cm_multi_unload_reload`

#### Security — USR-P4: Registry ACL

- [ ] **USR-P4a. cm/security.rs: Registry ACL checking module** | Prereqs: USR-P1d | Files: `src/cm/security.rs` (new), `src/cm/mod.rs`
  - New file: `cm_check_access(token, sec_desc, desired_access) -> bool`
  - Reuses `SeAccessCheck`
  - If key has no sec_desc_cell: default — admin full, user read-only
  - **Tests:** `usr_cm_sec_check_admin_full`, `usr_cm_sec_check_user_readonly`, `usr_cm_sec_default_sd`

- [ ] **USR-P4b. Wire sec_desc_cell on key creation** | Prereqs: USR-P4a | Files: `src/cm/hive.rs`
  - On `KeyCell` creation: inherit parent's `sec_desc_cell` or create default
  - **Tests:** `usr_cm_key_inherits_parent_sec`, `usr_cm_key_default_sec_when_no_parent`

- [ ] **USR-P4c. ACL checks in Cm syscall handlers** | Prereqs: USR-P4b | Files: `src/syscall/cm.rs` or `src/syscall/ob.rs`
  - Wire `cm_check_access()` in all Registry handlers
  - **Tests:** `usr_cm_open_key_admin`, `usr_cm_create_key_user_denied`, `usr_cm_delete_key_admin_only`

- [ ] **USR-P4d. User profile hive auto-mount** | Prereqs: USR-P4c | Files: `src/cm/mod.rs`
  - On session creation: auto-mount `\Registry\User\{sid}` hive
  - **Tests:** `usr_cm_user_hive_mounted_on_login`, `usr_cm_user_hive_has_defaults`

#### Security — USR-P5: Integrity levels

- [ ] **USR-P5a. Integrity level in SeAccessCheck** | Prereqs: USR-P1b | Files: `src/security/access.rs`
  - Extend `SeAccessCheck`: if `process_IL < object_IL`, deny WRITE/DELETE (allow READ)
  - **Tests:** `usr_il_medium_read_high_ok`, `usr_il_medium_write_high_denied`, `usr_il_system_bypass`

- [ ] **USR-P5b. SetIntegrityLevel + IntegrityLevel info classes** | Prereqs: USR-P5a | Files: `src/object/types.rs`, `src/syscall/ob.rs`
  - Add `SetIntegrityLevel = 32` to ObSetInfoClass (can only lower)
  - Add `IntegrityLevel = 27` to ObInfoClass
  - **Tests:** `usr_il_drop_from_high_to_medium`, `usr_il_raise_denied`, `usr_il_query_current`

- [ ] **USR-P5c. Privilege enforcement in admin syscalls** | Prereqs: USR-P1b | Files: `src/syscall/permission.rs`, `src/syscall/ob.rs`
  - Wire `token.has_privilege(bit)` in driver_unload, cm_load_hive, cm_unload_hive
  - **Tests:** `usr_priv_admin_has_all`, `usr_priv_user_has_change_notify`, `usr_priv_driver_unload_denied_for_user`

### M1.8 — Module Signing + KD + Shared Libraries (v0.53)

#### Security — Module signing

- [ ] **B5.1. Module signature validation** | Prereqs: -- | Files: `src/drivers/loader.rs`
  - Validación criptográfica de módulos `.nem` antes de cargar.
  - **Tests:** `nem_signature_valid_accepts`, `nem_signature_invalid_rejects`, `nem_signature_tamper_detected`

- [ ] **B5.2. Driver permission enforcement** | Prereqs: B5.1 | Files: `src/drivers/caps.rs`
  - Cruzar capacidad declarada del driver con token del proceso y ACL del objeto.
  - **Tests:** `driver_caps_allow_admin`, `driver_caps_deny_user`, `driver_caps_acl_intersection`

#### Kernel debugger

- [ ] **A3.2. Kernel debugger (KD)** | Prereqs: A3.1 | Files: `src/debugger/mod.rs`
  - INT3 breakpoints, hardware watchpoints (DR0-DR3), GDB remote protocol stub via serial.
  - **Tests:** `kd_breakpoint_set_and_hit`, `kd_breakpoint_invalid_addr`, `kd_watchpoint_write_detect`, `kd_register_snapshot`, `kd_gdb_protocol_qSupported`

#### Userland

- [ ] **B4.6. NeoEdit text editor** | Prereqs: A4.7, B4.4 | Files: `userbin/neoedit/`
  - Editor de texto modal Ring 3. Usa `ob_open` + `ob_query_info(ReadContent)` / `ob_set_info(WriteContent)`.
  - **Tests:** `neoedit_open_display`, `neoedit_edit_save`, `neoedit_scroll`

- [ ] **B4.7. Shared library per-process binding** | Prereqs: sys_loadlib | Files: `src/elf.rs`, `libneodos/`
  - Evolucionar NXL slots globales a binding per-process.
  - **Tests:** `nxl_per_process_isolation`, `nxl_unload_on_exit`, `nxl_version_coexistence`

### M1.9 — Power Phase 4 + User Commands + DNS (v0.54)

- [ ] **PM-PHASE4. Power Manager: shutdown coordination + libneodos + shell** | Prereqs: PM-PHASE3 | Files: `src/services/mod.rs`, `libneodos/src/power.rs` (new), `libneodos/src/syscall.rs`, `userbin/neoshell/`
  - `ServiceManager::stop_all()` en orden inverso de dependencias
  - libneodos wrappers: `power_shutdown()`, `power_reboot()`, `power_suspend()`, etc.
  - Shell commands: `REBOOT` built-in, `POWEROFF` migrado
  - **Tests:** `pm_service_manager_stop_all_order`, `pm_service_manager_stop_all_timeout`, `pm_lib_get_plan`, `pm_lib_set_plan`, `pm_lib_reboot`, `pm_lib_shutdown`

#### Security — USR-P6: User commands

- [ ] **USR-P6a. WHOAMI command** | Prereqs: USR-P2c | Files: `userbin/neoshell/`
  - **Tests:** `usr_whoami_prints_username`, `usr_whoami_shows_sid`

- [ ] **USR-P6b. PASSWD command** | Prereqs: USR-P2d | Files: `userbin/neoshell/`
  - **Tests:** `usr_passwd_change_ok`, `usr_passwd_wrong_old`, `usr_passwd_mismatch_confirm`

- [ ] **USR-P6c. WHO + LOGOFF commands** | Prereqs: USR-P2b | Files: `userbin/neoshell/`
  - **Tests:** `usr_who_lists_sessions`, `usr_logoff_terminates_shell`

- [ ] **USR-P6d. SU command** | Prereqs: USR-P2d, USR-P2e | Files: `userbin/neoshell/`
  - **Tests:** `usr_su_correct_password`, `usr_su_wrong_password`, `usr_su_spawns_as_target`

- [ ] **USR-P6e. RUNAS command** | Prereqs: USR-P6d | Files: `userbin/neoshell/`
  - **Tests:** `usr_runas_admin_command`, `usr_runas_user_denied_without_password`

#### Networking

#### Tracing

- [ ] **B1.1. Kernel tracing infrastructure** | Prereqs: -- | Files: `src/trace/mod.rs`
  - TraceBuffer, trace points registrables, filtrado por categoría/nivel, dump serial con timestamps.
  - **Tests:** `trace_register_dynamic_point`, `trace_filter_by_category`, `trace_dump_serial_format`

- [ ] **B1.2. NeoTrace system** | Prereqs: B1.1 | Files: `userbin/neotrace/`
  - **Tests:** `neotrace_start_stop_toggle`, `neotrace_dump_output`

#### Admin — Phase 3

- [ ] **ADM-3. neolog** | Prereqs: B1.1 | Files: `userbin/neolog/`
  - Visor de event log del kernel + EventBus.
  - **Tests:** `neolog_eventbus_dump`, `neolog_trace_filter`

### M1.10 — Registry WAL + Secure Boot + VFS Advanced (v0.55)

#### Registry — Phase 3

- [ ] **CM-WAL. Registry WAL (write-ahead logging, crash recovery)** | Prereqs: CM-DIRTY | Files: `src/cm/wal.rs` (new), `src/cm/mod.rs`
  - Cada mutación escribe entrada WAL a `C:\System\Registry\<name>.wal` + fsync antes de aplicar.
  - En mount: si existe `.wal`, hacer replay antes de cargar `.hiv`.
  - **Tests:** `cm_wal_created_on_mutation`, `cm_wal_replay_on_load`, `cm_wal_truncated_after_flush`, `cm_wal_power_loss_recovery`

- [ ] **CM-LIB. Registry libneodos wrappers** | Prereqs: -- | Files: `libneodos/src/syscall.rs`
  - Añadir 7 wrappers: `sys_cm_create_key`, `sys_cm_delete_key`, `sys_cm_enum_key`, `sys_cm_enum_value`, `sys_cm_flush_key`, `sys_cm_load_hive`, `sys_cm_unload_hive`.
  - **Tests:** `cm_lib_create_key_wrapper`, `cm_lib_enum_key_wrapper`, `cm_lib_enum_value_wrapper`, `cm_lib_flush_key_wrapper`

- [ ] **CM-REGEDIT. regedit.nxe** | Prereqs: CM-LIB | Files: `userbin/regedit/` (new)
  - **Tests:** `regedit_browse_tree`, `regedit_create_delete_key`, `regedit_set_query_value`, `regedit_flush`

#### Security

- [ ] **B5.3. Secure boot chain** | Prereqs: B5.1 | Files: `neodos-bootloader/`, `src/boot/secure.rs`
  - Verificación encadenada bootloader → kernel → drivers.
  - **Tests:** `secure_boot_kernel_verified`, `secure_boot_driver_verified`, `secure_boot_fail_closed`

#### Power Manager — Phase 5

- [ ] **PM-PHASE5. Power Manager polish: async coordination, full tests** | Prereqs: PM-PHASE4 | Files: `src/power/coordinator.rs`, `src/power/event.rs`, `src/power/mod.rs`, `docs/power-manager.md`
  - Completar coordinación asíncrona: shutdown con timeout por servicio, force-kill fallback
  - Integración Event Bus completa (EVENT_POWER_BUTTON, EVENT_LID_CLOSE/OPEN)
  - **Tests:** completar los 25 tests del diseño original

#### VFS — remaining features

- [ ] **VFS-3.2. `\DosDevices` dinámico** | Files: `src/vfs/mount.rs`
- [ ] **VFS-5.3. Write-back ordenado (flush page → flush block)** | Files: `src/globals.rs`
- [ ] **VFS-6.1. Overlay mounts** | Files: `src/fs/vfs.rs`
- [ ] **VFS-6.2. Extended attributes VFS** | Files: `src/fs/vfs.rs`
- [ ] **VFS-6.3. File notifications via Event Bus** | Files: `src/fs/vfs.rs`, `src/eventbus/`
- [ ] **VFS-6.4. Async VFS operations via IOCP (not IRP)** | Files: `src/fs/vfs.rs`
- [ ] **VFS-7.1. Eliminar lock global de VFS** | Files: `src/globals.rs`, `src/fs/vfs.rs`
- [ ] **VFS-7.2. Lookup cache** | Files: `src/fs/vfs.rs`
- [ ] **VFS-7.3. Path cache** | Files: `src/fs/vfs.rs`

### M1.11 — Font Manager (v0.55)

- [ ] **FONT-P1. Font Manager core + PSF provider** | Prereqs: -- | Files: `src/font/mod.rs` (new), `src/font/provider.rs` (new), `src/font/psf.rs` (new), `src/font/cache.rs` (new), `src/font/embedded.rs` (new)
  - FontProvider trait, PSF v1/v2 format parser, format detection (magic bytes).
  - FontMetrics/Glyph/FontHandle types, FontRegistry with `Mutex` protection.
  - Embedded PSF v2 font (replaces current `src/font.rs` const array) for boot-time fallback.
  - `font_render_glyph()` with `put_pixel` callback (framebuffer-agnostic).
  - **Tests:** `font_detect_psf2_magic`, `font_detect_random_data`, `font_parse_psf2_valid`, `font_parse_psf2_corrupt_version`, `font_glyph_by_index_ascii`, `font_glyph_missing_codepoint`, `font_metrics_matches_header`, `font_register_provider`, `font_no_providers_registered`

- [ ] **FONT-P2. ObType::Font + namespace + ObInfoClass/ObSetInfoClass** | Prereqs: FONT-P1 | Files: `src/font/mod.rs`, `src/object/types.rs`, `src/syscall/handlers.rs`, `src/object/mod.rs`
  - Add `ObType::Font = 23` (kernel-created only).
  - Add `ObInfoClass::FontMetrics = 38`, `FontGlyph = 39`.
  - Add `ObSetInfoClass::FontLoad = 48`, `FontSetDefault = 49` (admin-only).
  - Create `\Font\` namespace directory with `Default` symlink.
  - ObOperations for Font (free buffer on destroy).
  - **Tests:** `font_load_from_path`, `font_load_nonexistent_path`, `font_load_unsupported_format`, `font_load_not_admin`, `font_set_default`, `font_set_default_not_admin`, `font_destroy_releases_memory`, `font_metrics_ob_query`, `font_metrics_on_invalid_type`

- [ ] **FONT-P3. Console integration** | Prereqs: FONT-P2 | Files: `src/console.rs`, `src/font/mod.rs`, `src/main.rs`
  - Replace `font::FONT_WIDTH` / `font::FONT_HEIGHT` / `font::draw_char()` with Font Manager API.
  - Console resolves glyphs via `font_get_glyph(default_font_id, codepoint)`.
  - Boot sequence: init Font Manager in Phase 3 (embedded fallback), switch to disk font after VFS ready.
  - Dynamic console dimensions based on font metrics (remove `VGA_WIDTH`/`VGA_HEIGHT` fixed constants dependency).
  - **Tests:** `console_uses_font_manager`, `console_fallback_font_embedded`, `console_switch_font_dynamic`

- [ ] **FONT-P4. Registry configuration** | Prereqs: FONT-P2 | Files: `scripts/gen_system_hiv.py`
  - Add `Services\FontManager\DefaultFont = "Terminus"` (REG_SZ).
  - Add `Services\FontManager\FontPath = "\System\Fonts"` (REG_SZ).
  - **Tests:** `font_registry_keys_exist`

- [ ] **FONT-P5. NeoDev integration + default PSF font** | Prereqs: FONT-P1 | Files: `tools/fonts/default.psf` (new) | Repo: `NeoDev`
  - Add `fonts: Vec<String>` to NeoDev Config.
  - Font validation stage: check PSF magic, report metrics.
  - Copy `.psf` fonts to `C:\System\Fonts\` in disk image.
  - Generate `fonts.list` manifest.
  - Add `tools/fonts/default.psf` (Terminus 8x16 or equivalent).
  - **Tests:** `neodev_validate_valid_psf`, `neodev_validate_invalid_file`, `neodev_font_copied_to_image`

- [ ] **FONT-P6. Eliminar font.rs + build_font.py** | Prereqs: FONT-P3 | Files: `neodos-kernel/src/font.rs`, `neodos-kernel/build_font.py`, `neodos-kernel/src/console.rs`
  - Remove old `font.rs` (const array with `draw_char()`).
  - Remove `build_font.py` (manual OTF-to-Rust generator).
  - Purge all remaining direct references to `font::FONT_WIDTH` / `font::FONT_HEIGHT` / `font::draw_char()`.
  - **Tests:** `console_embedded_font_matches_old` (pixel-for-pixel identical comparison) -- debe pasar con el nuevo Font Manager.

---

## Fase 2: Ecosistema de Usuario (v0.56–v0.60)

### M2.1 — NXE/NXP Ecosystem Completion (v0.56)

#### NXE/NXP — Phase 2

- [ ] **NXE-ECO-12. NXE metadata auto-generation in build pipeline** | Prereqs: NXE-ECO-1 | Files: `libneodos/user.ld` | Repo: `NeoDev`
  - Generación automática de metadatos en build.rs de cada proyecto NXE.
  - Wire en NeoDev: inject .note.neodos después de cargo build.
  - **Tests:** `nxe_metadata_elf_section_exists`, `nxe_metadata_tlv_roundtrip`

- [ ] **NXE-ECO-13. `\Resource\<app>\` virtual Ob namespace** | Prereqs: NXE-ECO-5 | Files: `neodos-kernel/src/object/mod.rs`
  - Exponer recursos de aplicación como namespace Ob virtual.
  - **Tests:** `res_open_roundtrip`

- [ ] **NXE-ECO-14. NXE file header validation in kernel (size, type)** | Files: `src/elf.rs`
  - Validación de cabecera NXE al cargar (tamaño, tipo de ejecutable).
  - **Tests:** `nxe_header_validation_size`, `nxe_header_validation_type`

- [ ] **NXE-ECO-15. Digital signature verification infrastructure** | Files: `src/security/signature.rs`
  - Infraestructura para verificación de firmas digitales en NXE/NXP.
  - **Tests:** `signature_verify_valid`, `signature_verify_tampered`

#### i18n — Migration

- [x] **I18N-P2. Migrar apps core a tr_id!()** | Prereqs: I18N-P1 | Files: `userbin/neoshell/`, `userbin/neoinit/`, `userbin/corehelp/`, `userbin/coredir/`, `userbin/corecopy/`, `userbin/kill/`, `userbin/ps/`
  - Migrar todas las apps existentes de `tr!()` (no-op) a `tr_id!(IDS_CONSTANT)`.
  - **Tests:** (integración)

- [ ] **I18N-P4. format_str() con placeholders {0}** | Prereqs: I18N-P1 | Files: `libneodos/src/i18n.rs`
  - Reemplazo de `{0}`, `{1}` en strings traducidos. Buffer de stack de 256 bytes.
  - **Tests:** `i18n_format_simple`, `i18n_format_multiple_args`, `i18n_format_missing_args`

- [ ] **I18N-P5. i18n_available_locales()** | Prereqs: I18N-P1 | Files: `libneodos/src/i18n.rs`
  - Enumerar directorios en `C:\System\Locale\` para listar idiomas disponibles.
  - **Tests:** `i18n_available_locales_returns_list`

- [ ] **I18N-P6. Per-user locale (Registry)** | Prereqs: USR-P1 | Files: `libneodos/src/i18n.rs`
  - `\Registry\User\{sid}\Control\Locale\Language` con prioridad sobre sistema.

### M2.2 — Executive Manager (v0.57)

- [ ] **EXEC-CM. Configuration Manager** | Prereqs: CM-MULTI | Files: `src/cm/config_mgr.rs` (new)
  - Consolidación de Registry + boot settings en un Configuration Manager.
  - Gestión de CurrentControlSet, perfiles de hardware.
  - **Tests:** `cm_config_mgr_current_set`, `cm_config_mgr_profile_switch`

- [ ] **EXEC-SM. Session Manager** | Prereqs: USR-P2a | Files: `src/session/` (new)
  - Gestión completa de sesiones de usuario (login, logout, lock, switch).
  - Integración con Service Manager para sesiones por usuario.
  - **Tests:** `session_create_destroy`, `session_switch_user`

- [ ] **EXEC-OM. Object Namespace Manager** | Prereqs: -- | Files: `src/object/namespace.rs`
  - Virtualización de namespace por proceso (per-process view).
  - Directorios /dev, /proc, /sys virtuales por proceso.
  - **Tests:** `namespace_per_process_isolation`, `namespace_virtual_dirs`

- [ ] **EXEC-PM. Power Manager final** | Prereqs: PM-PHASE5 | Files: `src/power/coordinator.rs`
  - Power Manager como servicio Executive completo con políticas, planos, eventos.
  - **Tests:** `exec_power_manager_policies`, `exec_power_manager_events`

### M2.3 — Herramientas Oficiales (v0.58)

- [x] **TOOL-NEODEV. NeoDev v2** | Prereqs: -- | Files: `tools/neodev/`
  - Build, Image, ISO, Run, Test, QEMU + VirtualBox backends.
  - Auto-descubrimiento de proyectos.
  - Sustitución completa de scripts heredados (build.sh, qemu-debug.sh, auto_test.py,
    create_ne2_image.py, create_gpt_image.py).
  - Extraído a repositorio independiente: `github.com/NeoDOS-Project/NeoDev`.
  - Comandos CLI: `neodev build|run|test|image|clean|list|config|vm`.

- [x] **TOOL-NEODEV-EXTRACT. NeoDev standalone repo** | Prereqs: TOOL-NEODEV, TOOL-NEODEV-VBOX | Files: `tools/neodev/`
  - NeoDev extraído de `tools/neodev/` a repositorio independiente.
  - Dependencias de ruta hardcodeadas reemplazadas por `--neodos-path`, `NEODOS_PATH`, y auto-detección.
  - Sistema de configuración multi-capa (global, proyecto, CLI, entorno).
  - Documentación propia: README, CHANGELOG, LICENSE, CONTRIBUTING, docs/*.md.
  - improvements.md con 20 mejoras identificadas.
  - `neodev.toml` sigue siendo compatible en el proyecto NeoDOS.

- [x] **TOOL-NEODEV-VBOX. VirtualBox backend** | Prereqs: TOOL-NEODEV | Files: `tools/neodev/src/vmm/vbox.rs`
  - Backend VirtualBox completo: crear VM, iniciar, detener, reset, estado, importar VDI.
  - Arquitectura `HypervisorBackend` trait con factory `create_backend()`.
  - QEMU extraído a `tools/neodev/src/vmm/qemu.rs`.
  - CLI: `--backend qemu|virtualbox` en run/test, `neodev vm start|stop|reset|status|create|delete`.
  - Config generalizada con `[vm]` section en `neodev.toml`.
  - Test runner backend-agnostic (QEMU + VirtualBox).
  - `scripts/vbox-setup.sh` eliminado.

- [x] **TOOL-NEODEV-DHCP. DHCP Integration Test** | Prereqs: TOOL-NEODEV-VBOX | Files: `tools/neodev/src/test_.rs`, `tools/neodev/src/main.rs`, `tools/neodev/src/image.rs`, `scripts/gen_system_hiv.py`, `userbin/dhcptest/`, `userbin/neoinit/src/main.rs`, `userbin/ipconfig/src/main.rs`
  - Prueba DHCP automatizada usando VirtualBox Bridge Mode.
  - `neodev dhcp --backend virtualbox` — subcomando dedicado.
  - `userbin/dhcptest/` binario NXE con DORA embebido, validación y display.
  - Detección inteligente de interfaz bridge (Ethernet > Wi-Fi, carrier check, IP check).
  - Validaciones: IP != 0, no APIPA, máscara, gateway, DNS, lease time.
  - Marcadores `DHCPTEST_PASSED` / `DHCPTEST_FAILED` / `DHCPTEST_COMPLETE`.
  - `gen_system_hiv.py` flags `--enable-tests` y `--enable-network-test`.
  - `ipconfig.nxe` mejorado con máscara, gateway, DNS, origen DHCP, lease time.
  - `EnableNetworkTest` registry key para arranque condicional de dhcptest.
  - Logging detallado: selección de interfaz, DORA completo, validaciones, ipconfig.

- [ ] **TOOL-NEODEV-LEGACY. Eliminar scripts heredados** | Prereqs: TOOL-NEODEV | Files: `scripts/`
  - Eliminar build.sh, qemu-debug.sh, auto_test.py, create_ne2_image.py, etc.
  - Documentar migración en CHANGELOG.md.

- [ ] **TOOL-NEOCFG. neocfg completar módulos** | Prereqs: ADM-5 | Files: `userbin/neocfg/`
  - Completar módulos Power y Locale (actualmente stubs).

- [ ] **TOOL-ADM. Herramientas de administración** | Prereqs: -- | Files: `userbin/neomem/`, `userbin/neotop/`, `userbin/neotask/`, `userbin/neolog/`
  - neomem v0.2, neotop v0.2+, neotask completo, neolog.

- [ ] **TOOL-NXE. Herramientas NXE** | Prereqs: NXE-ECO-2/3/4/7/8/9 | Files: `tools/nxeinfo/`, `tools/nxpkg/`, `tools/nxdump/`, `userbin/nxres/`, `userbin/nxlocale/`, `userbin/nxverify/`
  - Completar todos los modos, flags, y comportamiento.

### M2.4 — Instalación y Bootstrap (v0.59)

- [ ] **INSTALL-NEOFS. Creación de NeoFS desde cero** | Prereqs: NFSv2-MKFS | Files: `userbin/install/`
  - Crear partición GPT, formatear como NE2, crear estructura de directorios base.

- [ ] **INSTALL-BOOTSTRAP. Bootstrap inicial** | Prereqs: INSTALL-NEOFS | Files: `neodos-bootloader/`
  - Bootloader que detecta instalación vs. arranque normal.
  - Si no hay instalación: lanzar install.nxe.

- [ ] **INSTALL-NXE. install.nxe** | Prereqs: INSTALL-NEOFS | Files: `userbin/install/src/main.rs`
  - Asistente interactivo: seleccionar disco, particionar, formatear, copiar sistema.
  - Configuración inicial: teclado, idioma, contraseña admin.

- [ ] **INSTALL-CONFIG. Configuración inicial** | Prereqs: USR-P1 | Files: `userbin/install/src/config.rs`
  - Crear usuario admin, configurar keyboard layout, locale.

- [ ] **INSTALL-PACKAGES. Despliegue de paquetes base** | Prereqs: NXP-ECO | Files: `userbin/install/src/packages.rs`
  - Copiar NXP base a la instalación, registrar servicios.

### M2.5 — NLT i18n + Regional Formats (v0.60)

- [ ] **I18N-P7. Compresión NLT** | Prereqs: I18N-P1 | Files: `tools/nltc/`, `libneodos/src/i18n.rs`
  - Flag `NLT_FLAG_COMPRESSED` (0x0001) para compresión LZSS/LZ4 de StringData.

- [ ] **I18N-P8. UTF-16 support** | Prereqs: I18N-P1 | Files: `libneodos/src/i18n.rs`
  - Flag en header para elegir UTF-8/UTF-16 en StringData.

- [ ] **I18N-P9. Pluralización** | Prereqs: I18N-P1 | Files: `libneodos/src/i18n.rs`
  - Sistema de plurales: `IDS_FILE_0 = "0 files"`, `IDS_FILE_1 = "1 file"`, `IDS_FILE_N = "{0} files"`.

- [ ] **I18N-P10. Formatos regionales** | Prereqs: I18N-P1 | Files: `libneodos/src/i18n.rs`
  - Fechas, monedas, calendarios desde archivos NLT de sistema.

- [ ] **I18N-P11. Soporte RTL/bidi** | Prereqs: I18N-P1 | Files: `libneodos/src/i18n.rs`
  - Flag `NLT_FLAG_RTL` (0x0002) + consulta Registry Layout para espejar GUI.

- [ ] **I18N-P12. Firmas digitales en NLT** | Prereqs: I18N-P1 | Files: `libneodos/src/i18n.rs`
  - Campo reservado en header + flag `NLT_FLAG_SIGNED` para verificar integridad.

---

## Fase 3: Seguridad y Estabilidad (v0.61–v0.69)

### M3.1 — Security Hardening (v0.61–v0.62)

- [ ] **SEC-AUDIT-FULL. Auditoría de seguridad completa** | Prereqs: -- | Files: `docs/SECURITY_AUDIT.md` (new)
  - Revisión de todas las syscalls, accesos a memoria, validación de punteros.

- [ ] **SEC-FUZZ-SYSCALLS. Fuzzing de syscalls (0–77)** | Prereqs: -- | Files: `tools/fuzzer/` (new)
  - Fuzzing automatizado de todas las syscalls con argumentos aleatorios.

- [ ] **SEC-FUZZ-DRIVERS. Fuzzing de interfaz HST** | Prereqs: -- | Files: `tools/fuzzer/drivers/`
  - Fuzzing de las exportaciones HST de drivers NEM (hst_log, hst_read_io, etc.).

- [ ] **SEC-ASLR-V2. ASLR v2: pila aleatoria + heap aleatorio** | Prereqs: ASLR v1 | Files: `src/arch/x64/paging.rs`, `src/elf.rs`
  - Posición aleatoria de la pila Ring 3 y del heap de usuario.

- [ ] **SEC-ASLR-V3. ASLR v3: full randomization** | Prereqs: SEC-ASLR-V2 | Files: `src/arch/x64/paging.rs`
  - Randomización completa: ELF + stack + heap + mmap.

- [ ] **SEC-NX. Non-executable stack enforcement** | Prereqs: -- | Files: `src/arch/x64/paging.rs`
  - Marcar páginas de pila como no ejecutables (NX bit).

- [ ] **SEC-NX-HEAP. Non-executable heap enforcement** | Prereqs: -- | Files: `src/arch/x64/paging.rs`
  - Marcar páginas de heap como no ejecutables.

### M3.2 — Performance (v0.63)

- [ ] **PERF-SCHED-LOCKFREE. Scheduler lock-free** | Prereqs: -- | Files: `src/scheduler/mod.rs`
  - Per-CPU run queues con operaciones lock-free.

- [ ] **PERF-SLAB-NUMA. Per-CPU heaps NUMA-aware** | Prereqs: -- | Files: `src/allocator.rs`
  - Slab allocator con conocimiento de nodos NUMA.

- [ ] **PERF-BENCH-SUITE. Benchmarking suite automática** | Prereqs: -- | Files: `tools/bench/`
  - Suite de benchmarks para medir rendimiento del kernel.

- [ ] **PERF-PGO. Profile-guided optimization** | Prereqs: PERF-BENCH-SUITE | Files: `build.rs`, `Cargo.toml`
  - Optimización guiada por perfiles de ejecución real.

### M3.3 — Documentación y Test Coverage (v0.64–v0.65)

- [ ] **DOCS-API-COMPLETE. Documentación completa de API** | Prereqs: -- | Files: `docs/syscalls.md`, `docs/libneodos.md`, `docs/drivers.md`
  - Documentar todas las syscalls, wrappers libneodos, y API de drivers NEM.

- [ ] **DOCS-SUBSYSTEMS. Documentación de subsistemas** | Prereqs: -- | Files: `docs/*.md`
  - Completar docs de todos los subsistemas kernel.

- [ ] **DOCS-TUTORIALS. Tutoriales** | Prereqs: DOCS-API-COMPLETE | Files: `docs/tutorials/`
  - Escribir driver NEM, crear app Ring 3, contribuir al proyecto.

- [ ] **TEST-COVERAGE-80. Coverage >80%** | Prereqs: -- | Files: `src/testing.rs`
  - Alcanzar >80% de cobertura de líneas en todo el kernel.

- [ ] **TEST-COVERAGE-95. Coverage >95%** | Prereqs: TEST-COVERAGE-80 | Files: `src/testing.rs`
  - Alcanzar >95% de cobertura.

### M3.4 — Bugfixes y Hardening (v0.66–v0.69)

- [ ] **AUDIT-FUZZ-ROUND2. Segunda ronda de fuzzing** | Prereqs: SEC-FUZZ-SYSCALLS
- [ ] **BUG-ALL. Corrección de todos los bugs detectados**
- [ ] **HARDEN-STATIC-BUFS. Eliminar buffers estáticos globales** | Files: `src/syscall/mod.rs`, `src/crash/mod.rs`
- [ ] **HARDEN-OOB. Auditoría de bounds checking en syscalls**
- [ ] **ABI-FREEZE-FINAL. Congelación final de ABI para v1.0**

---

## Fase 4: v1.0 — Primera API Estable

- [ ] **V1.0-RELEASE. Release v1.0.0**
- [ ] **V1.0-ABI-FROZEN. Todas las interfaces congeladas**
- [ ] **V1.0-DOCS. Documentación de release**
- [ ] **V1.0-TESTS. Suite completa (800+)**
- [ ] **V1.0-NXE-COMPAT. Binarios compilados contra ABI final**

---

## Deuda Técnica (transversal)

### TD.1 — Arrays Fijos Residuales

Verificar que no queden arrays de tamaño fijo en el kernel tras la migración a
`Vec<T>` y `Slab<T>` completada en v0.41.

### TD.3 — Static Buffers Globales

| Tarea | Archivos | Prioridad |
|-------|----------|-----------|
| Eliminar BIN_BUF[65536] | `src/syscall/mod.rs` | ALTA |
| Eliminar CMD_BUF[65536] | `src/syscall/mod.rs` | ALTA |
| Eliminar buffers fijos en crash dump | `src/crash/mod.rs` | MEDIA |
| Eliminar buffers fijos en serial | `src/arch/x64/serial.rs` | BAJA |

### TD.4 — Cleanup (CLEANUP-1..35)

Agrupados en paquetes de trabajo:

<details>
<summary>CLEANUP-DEADCODE (2 items)</summary>

- [ ] **CLEANUP-2. Unused macros + functions + enum variants + constants** | Files: multiple
- [ ] **CLEANUP-12. 23 dead functions** | Files: multiple
</details>

<details>
<summary>CLEANUP-DUPLICATES (10 items)</summary>

- [ ] **CLEANUP-5. ABI validation duplicated** | Files: `src/drivers/abi/mod.rs:50-80`, `src/drivers/nem/policy.rs:27-57`
- [ ] **CLEANUP-6. Dual mount managers** | Files: `src/fs/vfs.rs:84-95`, `src/vfs/mount.rs:38-123`
- [ ] **CLEANUP-7. Error constants duplicated libneodos/libneodos-nxl** | Files: `libneodos/src/syscall.rs:3-17`, `libneodos-nxl/src/error.rs:4-18`
- [ ] **CLEANUP-15. `lazy_static!` → `LazyLock`** | Files: multiple (27 usages)
- [ ] **CLEANUP-16. net/mod.rs monolithic protocol dispatch** | Files: `src/net/mod.rs:68-197`
- [ ] **CLEANUP-18. SPSC ring buffer triplicated** | Files: `src/work_queue.rs`, `src/input/vt.rs`, `src/arch/x64/cpu_local.rs`
- [ ] **CLEANUP-24. IPI function duplicates** | Files: `src/arch/x64/smp.rs`
- [ ] **CLEANUP-25. AHCI structs defined twice** | Files: `src/drivers/boot_ahci.rs`, `drivers/ahci/src/lib.rs`
- [ ] **CLEANUP-26. PCI config access in 7 files** | Files: `src/drivers/pci.rs`, `drivers/*/src/lib.rs`
- [ ] **CLEANUP-27. HST extern in 8 NEM drivers** | Files: `drivers/*/src/lib.rs`
- [ ] **CLEANUP-28. PAGE_SIZE defined 7 times** | Files: multiple
</details>

<details>
<summary>CLEANUP-REFACTOR (11 items)</summary>

- [ ] **CLEANUP-8. 10+ enums with manual `to_str()` instead of `Display`** | Files: multiple
- [ ] **CLEANUP-17. Storage probe hardcoded to 4 drivers** | Files: `src/drivers/storage_manager.rs:2-5`
- [ ] **CLEANUP-19. 27 fixed-size arrays across kernel** | Files: multiple
- [ ] **CLEANUP-20. Network unsafe pointer casts (9×)** | Files: `src/net/mod.rs`
- [ ] **CLEANUP-21. kernel_stack_trace fixed crash buffers** | Files: `src/crash/mod.rs:34,66,70`
- [ ] **CLEANUP-22. from_u8/from_u16 → TryFrom** | Files: `src/drivers/nem/mod.rs:46-98`
- [ ] **CLEANUP-23. TOCTOU in storage probe** | Files: `src/drivers/storage_manager.rs`
- [ ] **CLEANUP-29. Error enums overlapping variants** | Files: `src/fs/vfs.rs`, `src/fs/neodos_fs.rs`, `src/drivers/fat32.rs`, `src/drivers/iso9660.rs`
- [ ] **CLEANUP-30. Idle loops without `hlt`** | Files: `src/main.rs`, `src/hal/raw/cpu.rs`
- [ ] **CLEANUP-31. Global static mut without sync (40+)** | Files: multiple
- [ ] **CLEANUP-32. Split syscall/ob.rs (2280 lines) + handlers.rs (1771)** | Files: `src/syscall/ob.rs`, `src/syscall/handlers.rs`
- [ ] **CLEANUP-33. Scheduler panics on table full** | Files: `src/scheduler/mod.rs`
- [ ] **CLEANUP-34. Page cache O(n) linear scans** | Files: `src/buffer/page_cache.rs`
- [ ] **CLEANUP-35. 10 inconsistent name buffer sizes** | Files: multiple
</details>

### TD.5 — Object Manager Consolidation

- [ ] **AI-1. Completar ObInfoClass/ObSetInfoClass enums** | Files: `libneodos/src/syscall.rs`
- [ ] **AI-2. Consolidate legacy syscall wrappers** | Files: `src/syscall/mod.rs`
- [ ] **AI-3. ObObjectTable lock granularity (lock striping)** | Files: `src/object/mod.rs`
- [ ] **AI-4. Arreglar TOCTOU race en kobj_register** | Files: `src/object/mod.rs`

### NET.ARP — ARP Reliability Improvements
- [ ] **ARP-1. Volatile reads for e1000 RX descriptor status** | Files: `neodos-kernel/src/net/e1000.rs` | Prioridad: Media | Complejidad: Baja

  - The e1000 `poll_packet()` reads `desc.status` without `read_volatile`. On real hardware or certain emulators, the compiler may optimize away the DMA-coherent memory read. Use `core::ptr::addr_of!` with `read_volatile` for descriptor status and length.
  - **Nota:** Se añadió `core::sync::atomic::fence(Ordering::Release)` como barrera de memoria entre la escritura del descriptor y el doorbell TDT/RDT. Pendiente: cambiar la lectura de `desc.status` a `read_volatile` para evitar optimizaciones del compilador.

  - **Justificación:** DHCP works without it (broadcast packets are continuously polled), but ARP replies (single unicast frame) may be missed if the compiler caches the descriptor read.
- [ ] **ARP-2. Refactor ARP resolution out of icmp_ping()** | Files: `neodos-kernel/src/net/icmp.rs`, `neodos-kernel/src/net/arp.rs` | Prioridad: Media | Complejidad: Media
  - The `icmp_ping()` function contains duplicated ARP resolution logic inline. The existing `arp_resolve()` function is fire-and-forget (returns None immediately). Add a new `arp_resolve_blocking(target_ip, timeout_us)` that sends the request and waits for the reply with a timeout.
  - **Justificación:** Elimina duplicación, centraliza la lógica ARP, facilita mantener consistencia entre todos los clientes que necesiten resolución ARP.

- [ ] **ARP-3. Pending packet queue for concurrent ARP resolutions** | Files: `neodos-kernel/src/net/arp.rs` | Prioridad: Baja | Complejidad: Alta
  - Currently, if multiple threads attempt ARP resolution simultaneously, each sends its own ARP request. Implement a pending queue: thread A sends ARP request, thread B detects the pending resolution and waits on the same entry.
  - **Justificación:** Reduce tráfico ARP en la red y evita respuestas duplicadas.

### TD.6 — Estabilización ABI

- [ ] **SSDT-DRVUNLOAD. Migrar sys_driver_unload a Ob API** | Files: `src/syscall/mod.rs`, `src/drivers/hotreload.rs`, `userbin/loadnem/`
  - Asegurar drivers en `\Driver\` namespace Ob.
  - Usar `ob_destroy()` para unload en lugar de syscall legacy.
  - Eliminar `handler_driver_unload` del SSDT.

---

## Fixed (v0.50)

- [x] **NET-ICMP-CKSUM. ICMP echo request checksum endianness** | Files: `neodos-kernel/src/net/icmp.rs`
  - `icmp_ping()` almacenaba el checksum ICMP en little-endian en lugar de network byte order. El receptor (Linux, routers) validaba y descartaba el paquete. DHCP funcionaba (UDP checksum bien) pero ICMP nunca recibía respuesta. Afectaba a QEMU y VirtualBox por igual; en QEMU user-mode el bug pasaba desapercibido porque QEMU no valida checksums ICMP.

- [x] **NET-ROUTE. `next_hop_mac()` hardcoded a 10.0.2.0/24** | Files: `neodos-kernel/src/net/nic.rs`
  - La función usaba una máscara fija `0xFFFFFF00` comparando con `0x0A000200` (10.0.2.0/24, subnet de QEMU). Cambiado a usar la máscara real de la NIC. `icmp_ping()` también ignoraba el gateway para destinos fuera de subred.

- [x] **NET-E1000-BARRIER. Missing memory barriers en e1000 TX/RX** | Files: `neodos-kernel/src/net/e1000.rs`
  - Las escrituras a descriptores TX podían ser reordenadas respecto al doorbell TDT. En VirtualBox (emulación multi-thread), el hardware leía descriptores stale. Añadido `core::sync::atomic::fence(Ordering::Release)` entre escritura de descriptor y actualización de TDT/RDT.

- [x] **NET-E1000-RA. MAC address no programada en RA register** | Files: `neodos-kernel/src/net/e1000.rs`
  - El registro RA (Receive Address) no se inicializaba explícitamente con la MAC. Aunque RCTL_UPE acepta todo unicast, algunos emuladores (VirtualBox) requieren RA[0] para filtrar correctamente.

- [x] **NET-VBOX-PROMISC. VirtualBox bridge sin promiscuous mode** | Files: `tools/neodev/src/vmm/vbox.rs`
  - Añadido `--nicpromisc1 allow-all` para que VirtualBox bridge acepte todo el tráfico.

- [x] **NET-DEFAULT-BRIDGED. NeoDev default network mode cambiado a bridged** | Files: `tools/neodev/src/vmm/mod.rs`, `tools/neodev/src/config.rs`
  - Default backend: virtualbox. Default network: bridged (DHCP desde router físico).

## Bugs Conocidos

- [x] **BUG-NEM-RX. NEM e1000 driver no recibe paquetes** | Files: `drivers/e1000/src/lib.rs`, `neodos-kernel/src/drivers/nem/net_bridge.rs`
  - Causa raíz: `probe_e1000()` llamaba a `init_e1000_hw(mmio)` antes de establecer `MMIO_BASE`, por lo que todos los registros se escribían a dirección 0 en lugar del BAR MMIO del e1000. El hardware nunca se configuraba.
  - Fix: `init_e1000_hw` ahora establece `MMIO_BASE` al inicio. Añadidas fences de memoria (`Release`) antes de doorbell TX/RX. Verificación de retorno de `hst_virt_to_phys`.
  - Kernel e1000 driver (`src/net/e1000.rs`) eliminado completamente. Solo `e1000.nem` gestiona el hardware.

---

## VirtIO (baja prioridad)

- [ ] **VIO-CON. VirtIO Console (0x1002)** | Files: `drivers/virtio-console/`
- [ ] **VIO-RNG. VirtIO RNG (0x1003)** | Files: `drivers/virtio-rng/`
- [ ] **VIO-SCSI. VirtIO SCSI (0x100A)** | Files: `drivers/virtio-scsi/`
- [ ] **VIO-GPU. VirtIO GPU (0x1012)** | Files: `drivers/virtio-gpu/`
- [ ] **VIO-VSOCK. VirtIO VSOCK (0x1014)** | Files: `drivers/virtio-vsock/`
- [ ] **VIO-SOUND. VirtIO Sound (0x1015)** | Files: `drivers/virtio-sound/`
- [ ] **VIO-BALLOON. VirtIO Memory Balloon (0x1004)** | Files: `drivers/virtio-balloon/`

---

## PATH y Entorno (v0.50+)

- [ ] **PATH-REG-SET. Persistir SET PATH en el Registry** | Prioridad: Media | Complejidad: Baja | Impacto: Bajo
  - Cuando el usuario ejecuta `SET PATH=...`, persistir el valor en `\Registry\Machine\System\CurrentControlSet\Control\Session Manager\Environment\PATH`.
  - Requiere re-exportar `sys_cm_set_value` en `libneodos/src/lib.rs`.
  - **Archivos:** `userbin/neoshell/src/shell.rs`, `libneodos/src/lib.rs`.
  - **Tests:** `shell_set_path_persists_to_registry`, `shell_restart_preserves_custom_path`.

- [ ] **PATH-REG-ENV. Entorno de sistema completo desde Registry** | Prioridad: Baja | Complejidad: Media | Impacto: Medio
  - Leer todas las variables de entorno desde `Session Manager\Environment` del Registry (no solo PATH).
  - Permitir variables como `TEMP`, `PROMPT`, `PATHEXT`.
  - Sincronizar cambios de `SET` con el Registry.
  - **Archivos:** `userbin/neoshell/src/shell.rs`, `scripts/gen_system_hiv.py`.
  - **Tests:** `shell_env_from_registry`, `shell_env_sync_bidirectional`.

- [ ] **COREHELP-PATH. corehelp descubre comandos en todos los directorios del PATH** | Prioridad: Media | Complejidad: Baja | Impacto: Bajo
  - `cmd_show_detail` actualmente busca en todos los directorios del PATH (implementado).
  - `cmd_list_all` actualmente enumera `C:\Programs` para leer los metadatos `::HELP::` de cada NXE.
  - Debería leer los NXE desde el directorio donde se encontraron originalmente (no hardcodear `C:\Programs`).
  - **Archivos:** `userbin/corehelp/src/main.rs`.
  - **Tests:** `corehelp_lists_tools_from_system_tools`, `corehelp_detail_finds_in_system_tools`.

---

## Experimental (post-1.0)

- [ ] **B7.1. Full GUI system** | Files: `userbin/gui/`
- [ ] **B7.2. Advanced secure boot (TPM)** | Files: `src/boot/tpm.rs`
- [ ] **B7.3. Package manager (NeoStore)** | Files: `userbin/neopkg/`
- [ ] **B7.4. Time-travel debugging** | Files: `src/debugger/timetravel.rs`
- [ ] **B7.5. Live kernel patching** | Files: `src/patch/mod.rs`
- [ ] **B7.6. Distributed NeoDOS nodes** | Files: `src/cluster/`
- [ ] **B6.2. Copy-on-write fork** | Files: `src/memory/cow.rs`, `src/syscall.rs` (NOTA: contradice modelo NT)

---

## i18n/NLT (post-migración)

- [ ] **I18N-NXRESOURCE. nxres no implementa visualización de contenido de recurso** | Prioridad: Baja | Complejidad: Media | Impacto: Bajo
  - El subcomando `<resource>` de nxres muestra "not yet implemented".
  - Requiere implementar lectura de contenido de recursos desde el paquete NXE/NXP.
  - **Archivos:** `userbin/nxres/src/main.rs`.

- [ ] **I18N-NLT-KBD. Traducción de nombres de modificadores de teclado** | Prioridad: Baja | Complejidad: Baja | Impacto: Bajo
  - Los nombres de modificadores (LCTRL, LSHIFT, etc.) y LEDs se muestran en inglés en neokey.
  - Podrían traducirse mediante NLT si se desea localización completa.
  - **Archivos:** `userbin/neokey/src/main.rs`, `data/locale/*/neokey.toml`.

- [ ] **I18N-PLURALS. Sistema de pluralización para NLT** | Prioridad: Baja | Complejidad: Alta | Impacto: Arquitectónico
  - Actualmente NLT no soporta pluralización. Los mensajes como "X file(s)" requieren manejo de plurales por idioma.
  - Requiere extensión del formato NLTv2 (nuevo flag, tabla de plurales).
  - Diferimiento a v0.60+.

- [ ] **I18N-TEST-AUTOMATION. Tests automatizados de cambio de idioma** | Prioridad: Media | Complejidad: Media | Impacto: Medio
  - No existen tests que verifiquen que todos los mensajes cambian correctamente al cambiar de idioma.
  - Requiere framework de test en QEMU con cambio de locale y captura de salida.
  - **Archivos:** `tests/`, `tools/neodev/src/test_.rs`.

## Documentation backlog

- [ ] **DH2. Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md** | Files: `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`
- [ ] **DH3. Completar libneodos syscall wrappers** | Files: `libneodos/src/syscall.rs`
- [ ] **DH-HISTORY. Mantener docs/HISTORY.md** | Files: `docs/HISTORY.md`

## Hostname Infrastructure

Improvements detected during hostname implementation:

- [ ] **HN-01. Dynamic hostname change without reboot** | Prioridad: Alta | Complejidad: Media | Impacto: Arquitectonico
  - Implement `sys_set_hostname()` notification so the DHCP client can update DNS dynamically
  - Notify network services (DHCP, DNS, NetBIOS) on hostname change via event bus or registry notification.
  - **Archivos:** `neodos-kernel/src/syscall/ob.rs`, `libnet-nxl/src/main.rs`, `userbin/dhcpd/src/main.rs`

- [ ] **HN-02. DHCP Option 12 (Host Name)** | Prioridad: Alta | Complejidad: Media | Impacto: Red
  - Send the system hostname in DHCP DISCOVER/REQUEST packets via Option 12.
  - **Archivos:** `userbin/dhcpd/src/main.rs`, `libnet-nxl/src/main.rs`

- [ ] **HN-03. DNS dynamic update on hostname/IP change** | Prioridad: Media | Complejidad: Alta | Impacto: Red
  - Auto-register hostname in DNS when IP changes or on boot.
  - **Archivos:** `userbin/dhcpd/src/main.rs`, `libnet/src/lib.rs`

- [ ] **HN-04. NetBIOS name service (NBNS) integration** | Prioridad: Baja | Complejidad: Alta | Impacto: Red
  - Register hostname as NetBIOS name on the local network.
  - **Archivos:** New module `neodos-kernel/src/net/netbios.rs`

- [ ] **HN-05. Hostname validation rules** | Prioridad: Media | Complejidad: Baja | Impacto: Estabilidad
  - RFC 952/1123 hostname validation: max 63 chars, alphanumeric + hyphen, no leading/trailing hyphen.
  - **Archivos:** `neodos-kernel/src/syscall/ob.rs`

---

## Organization & Ecosystem (NeoDOS-Project)

Future improvements for the GitHub organization and project ecosystem:

- [ ] **ORG-TEAMS. Create GitHub teams with proper permissions** | Prioridad: Baja | Complejidad: Baja | Impacto: Alto
  - Create teams: `core` (admin), `contributors` (write), `drivers` (maintain), `docs` (maintain).
  - **Archivos:** Organización NeoDOS-Project en GitHub.

- [ ] **ORG-RULES. Add branch protection rules** | Prioridad: Baja | Complejidad: Baja | Impacto: Alto
  - Require PR reviews for `main` branch, require CI passing, require linear history.
  - **Archivos:** Organización NeoDOS-Project en GitHub.

- [ ] **ORG-CODEOWNERS. Add CODEOWNERS file** | Prioridad: Baja | Complejidad: Baja | Impacto: Medio
  - Define ownership for kernel, drivers, bootloader, docs, tools.
  - **Archivos:** `.github/CODEOWNERS`

- [ ] **ORG-DEPENDABOT. Configure Dependabot** | Prioridad: Baja | Complejidad: Baja | Impacto: Medio
  - Enable dependency updates for Rust (Cargo), GitHub Actions.
  - **Archivos:** `.github/dependabot.yml`

- [ ] **ORG-PROJECTS. Migrate to GitHub Projects v2** | Prioridad: Baja | Complejidad: Media | Impacto: Alto
  - Set up kanban boards for roadmap tracking, sprint planning.
  - **Archivos:** Organización NeoDOS-Project en GitHub.

- [ ] **ORG-DISCUSSIONS. Enable GitHub Discussions** | Prioridad: Baja | Complejidad: Baja | Impacto: Medio
  - Categories: Announcements, General, Q&A, Ideas, Show and tell.
  - **Archivos:** Repositorio NeoDOS en GitHub.

- [ ] **ORG-SPONSORS. Configure GitHub Sponsors** | Prioridad: Muy Baja | Complejidad: Baja | Impacto: Alto
  - Enable sponsors button for financial support.

- [ ] **ORG-PACKAGES. Enable GitHub Package Registry** | Prioridad: Baja | Complejidad: Media | Impacto: Medio
  - Publish NXE/NXL/NEM artifacts as packages for distribution.

- [ ] **ORG-RELEASES. Automate releases with GitHub Actions** | Prioridad: Baja | Complejidad: Media | Impacto: Alto
  - Auto-tag, auto-build, auto-release on version bumps.

- [ ] **ORG-PAGES. Set up GitHub Pages site** | Prioridad: Baja | Complejidad: Media | Impacto: Alto
  - Documentation site for NeoDOS with mdBook or similar.

- [ ] **ORG-METRICS. Track community health metrics** | Prioridad: Muy Baja | Complejidad: Baja | Impacto: Bajo
  - Enable insights, contributor graphs, dependency graphs.

---

## Repository Architecture (Multi-Repo Separation)

Future improvements for splitting the monorepo into multiple repositories.
See [REPOSITORY_ARCHITECTURE.md](REPOSITORY_ARCHITECTURE.md) for full analysis.

- [ ] **REPO-SEP-001. Extract NeoDev to separate repo** | Prioridad: Baja | Complejidad: Media | Impacto: Alto
  - `neodev/` is fully standalone (host Rust tool, no kernel deps).
  - Move to own repo with independent versioning and release cycle.
  - **Post-v1.0** — not urgent, benefits appear when external contributors use the tool.

- [ ] **REPO-SEP-002. Extract NeoTools to separate repo** | Prioridad: Baja | Complejidad: Baja | Impacto: Medio
  - `nxdump`, `nxeinfo`, `nxpkg` are standalone analysis tools.
  - Group as workspace in `NeoTools` repo.
  - **Post-v1.0** — useful for OS format analysis without cloning kernel.

- [ ] **REPO-SEP-003. Extract NeoMCP to separate repo** | Prioridad: Baja | Complejidad: Baja | Impacto: Medio
  - `scripts/mcp_server/` is a standalone Python application.
  - **Post-v1.0** — extract when MCP server needs its own CI/deployment.

- [ ] **REPO-SEP-004. Extract NeoTranslations to separate repo** | Prioridad: Baja | Complejidad: Media | Impacto: Alto
  - `data/locale/` + `tools/nltc/` + `scripts/gen_nlt*.py`
  - Enables community translation contributions without kernel access.
  - **Post-v1.0** — valuable for internationalization community.

- [ ] **REPO-SEP-005. Extract NeoDOS-LSP to separate repo** | Prioridad: Baja | Complejidad: Baja | Impacto: Medio
  - `neodos-lsp/` is fully standalone LSP server.
  - **Post-v1.0** — extract when LSP has stable features and external users.

- [ ] **REPO-SEP-006. Move docs/ to NeoDocs repo** | Prioridad: Muy Baja | Complejidad: Media | Impacto: Bajo
  - Documentation references kernel code directly, creating sync burden.
  - **Post-v1.0** — only if docs are stable and external contributions increase.

- [ ] **REPO-SEP-007. Re-evaluate drivers/ separation at v1.0** | Prioridad: Muy Baja | Complejidad: Alta | Impacto: Alto
  - NEM ABI v8 is stable. Drivers compile independently.
  - **Re-evaluate at v1.0** when ABI is frozen. Not before.

- [ ] **REPO-SEP-008. Extract NeoTools to separate repo** | Prioridad: Alta | Complejidad: Baja | Impacto: Medio
  - Move `tools/nxeinfo`, `tools/nxpkg`, `tools/nxdump` to `NeoDOS-Project/NeoTools`.
  - Standalone host tools, no kernel dependencies. Cargo workspace with 3 crates.
  - CI: build, lint, smoke tests. Releases independent from kernel.
  - See `docs/AUDIT_REPORT.md` §7.1 for migration plan.

- [ ] **REPO-SEP-009. Extract NeoDOS-LSP to separate repo** | Prioridad: Alta | Complejidad: Baja | Impacto: Medio
  - Move `neodos-lsp/` to `NeoDOS-Project/NeoDOS-LSP`.
  - Standalone LSP server with 15+ host Rust dependencies. Zero kernel deps.
  - CI: build, lint, unit tests. Useful for any NeoDOS development setup.
  - See `docs/AUDIT_REPORT.md` §7.1 for migration plan.

- [ ] **REPO-SEP-010. Extract NeoMCP to separate repo** | Prioridad: Media | Complejidad: Baja | Impacto: Bajo
  - Move `scripts/mcp_server/` to `NeoDOS-Project/NeoMCP`.
  - Python MCP server, completely independent of kernel.
  - CI: ruff, pytest. Can evolve at own pace.
  - See `docs/AUDIT_REPORT.md` §7.1 for migration plan.

- [ ] **REPO-SEP-011. Define NEM ABI manifest for driver publication** | Prioridad: Media | Complejidad: Media | Impacto: Alto
  - Create YAML/JSON manifest declaring: ABI version, host service table, capabilities, required kernel version.
  - Prerequisite for NeoDrivers separation post-v1.0.
  - See `docs/AUDIT_REPORT.md` §5.2 for specification.

- [ ] **REPO-SEP-012. Create versioned kernel ABI manifest** | Prioridad: Media | Complejidad: Media | Impacto: Alto
  - Declare syscall SSDT table, ObInfoClass enums, NXL ABI, boot protocol version.
  - Enables external tools (NeoTools, LSP, SDK) to verify compatibility.
  - See `docs/AUDIT_REPORT.md` §5.2 for specification.

- [ ] **REPO-SEP-013. Publish libneodos as standalone crate on crates.io (post-v1.0)** | Prioridad: Baja | Complejidad: Media | Impacto: Alto
  - Extract libneodos as published crate for third-party app development.
  - Blocking: syscall ABI freeze (v1.0), application demand.
  - See `docs/AUDIT_REPORT.md` §3.2 (NeoSDK analysis).

- [ ] **REPO-SEP-014. Add independent CI for drivers/ within monorepo** | Prioridad: Media | Complejidad: Baja | Impacto: Medio
  - Each driver builds independently. Add CI matrix to verify all compile.
  - Prepare for eventual NeoDrivers separation.
  - See `docs/AUDIT_REPORT.md` §3.3 (NeoDrivers analysis).

- [ ] **REPO-SEP-015. Freeze NLT format for NeoTranslations separation** | Prioridad: Media | Complejidad: Alta | Impacto: Alto
  - Complete I18N-P7..P12 (compression, UTF-16, pluralization, RTL) before freezing.
  - Blocking: community translation contributions via NeoTranslations repo.
  - See `docs/AUDIT_REPORT.md` §3.10 (NeoTranslations analysis).

---

## Referencias

- [ROADMAP.md](../ROADMAP.md) — Visión general, fases, milestones, prioridades
- [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md) — Invariantes MUST/MUST NOT
- [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) — Visión a largo plazo v0.40 → v4.x
- [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md) — Items completados
- [CHANGELOG.md](../CHANGELOG.md) — Historial de cambios por versión
