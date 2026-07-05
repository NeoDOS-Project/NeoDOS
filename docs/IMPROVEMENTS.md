# NeoDOS — Roadmap

> Items pendientes del roadmap. Los completados están en
> [IMPROVEMENTS_COMPLETED.md](IMPROVEMENTS_COMPLETED.md).

> Version actual: **v0.48.6** — Tests: 537 — ABI: v7 — Ob API: RAX 60-66
> Objetivo: v1.0 — executive NT-like arquitectónicamente sólido.
> Leer [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) antes de planificar cambios.
> Fuente de verdad: [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md)

**Próximo milestone: v0.49** (NeoFS robustez — indirect blocks, journaling, checksums)

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
| ~~B2.6~~ | ~~Registry defaults in boot~~ **[COMPLETED]** | ~~**HIGH**~~ |
| ~~**B2.7**~~ | ~~**Registry disk persistence**~~ **[COMPLETED]** | ~~**CRITICAL**~~ |
| ~~B4.10~~ | ~~NeoInit Registry-driven config~~ **[COMPLETED]** | ~~**HIGH**~~ |
| PKG-1 (P0-P6) | NeoGet v1 — sistema de paquetes `.nxp` (diseño completo, impl diferida a v0.70) | LOW |
| v0.49 | NeoFS robustez (indirect blocks, journaling, checksums) | MEDIUM |
| VFS-3.1 | Separar `\Global\FileSystem` del Ob namespace | MEDIUM |
| VFS-3.3 | Proteger paths del namespace | MEDIUM |
| VFS-5.1 | Unificar BlockCache + PageCache | MEDIUM |
| VFS-5.2 | InodeCache con invalidación | MEDIUM |
| VFS-2.2 | Refactorizar FSCK | MEDIUM |
| VFS-2.3 | Eliminar acceso directo a NeoFS desde shell | MEDIUM |
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
| B3.3 | **DHCP test page fault (CR2=0x0) when sending DISCOVER via NEM e1000** — exposed by static array layout changes. Test `dhcp_discover_offer_sequence` needs investigation: likely null pointer or DMA buffer alignment in NEM e1000 driver. Currently commented out in `src/net/dhcp.rs`. | HIGH |
| NET-1.16 | **Kernel DHCP no progresa en userspace** — `build_dhcp_packet()` usa `Vec` (heap alloc) y `nic_send_packet()` toma spinlocks, inseguro desde timer IRQ. `dhcp_tick()` en idle loop no corre porque threads siempre están Ready. Solución: refactorizar `build_dhcp_packet()` a buffer fijo, proteger `DHCP_CLIENT` con `Mutex`, llamar `dhcp_tick()` desde timer IRQ. Actualmente netcfg asigna APIPA. | HIGH |
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
| B2.2..B2.5 | Registry features (journal, multi-hive, security, notification) | LOW |
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

### v0.49 — NeoFS robustez

* [ ] **v0.49. NeoFS robustez** | Prereqs: v0.48 | Files: `src/fs/neodos_fs.rs`
  - Indirect blocks (FS-3), journaling (FS-5), checksums (FS-6), ResourceRegistry extendido (NS-3), DOS name reservation
  - **Tests:** `fs_indirect_blocks`, `fs_journal_replay`, `fs_checksum_verify`

### VFS Fase 3: Namespace Consistencia

* [ ] **VFS-3.1. Separar `\Global\FileSystem` del Ob namespace** | Prereqs: VFS-1.1 | Files: `src/object/mod.rs`, `src/object/namespace.rs`
  - `ob_enum("\Global\FileSystem\")` debe delegar al VFS, no al namespace Ob.
  - **Tests:** `vfs_namespace_filesystem_isolation`

* [ ] **VFS-3.3. Proteger paths del namespace** | Prereqs: VFS-3.1 | Files: `src/syscall/ob.rs`
  - Impedir `ob_create(ObType::Directory)` dentro de `\Global\FileSystem\`.
  - **Tests:** `vfs_namespace_protected_paths`

### VFS Fase 5: Caché Unificada

* [ ] **VFS-5.1. Unificar BlockCache + PageCache** | Prereqs: -- | Files: `src/buffer/block_cache.rs`, `src/buffer/page_cache.rs`
  - Una sola cache de páginas 4KB con sub-sector dirty tracking. Política LRU unificada.
  - **Tests:** `vfs_cache_coherency`

* [ ] **VFS-5.2. InodeCache con invalidación** | Prereqs: -- | Files: `src/fs/neodos_fs.rs`
  - Añadir versión/secuencia en superblock. Invalidar InodeCache cuando cambie.
  - **Tests:** `vfs_cache_inode_invalidation`

### VFS Fase 2 (cont.)

* [ ] **VFS-2.2. Refactorizar FSCK** | Prereqs: -- | Files: `src/fs/fsck.rs`
  - Extraer lógica común a trait `FsckIntegrity`, mover a `drivers/fsck_neodos.rs`.
  - **Tests:** 6 tests existentes + 2 de integración

* [ ] **VFS-2.3. Eliminar acceso directo a NeoFS desde shell** | Prereqs: -- | Files: `src/shell/commands/*.rs`, `src/fs/neodos_fs.rs`
  - Comandos shell deben ir por VFS + handles, no por NeoDosFs directo.
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

* [ ] **NET-1.11. dhcp.nxe (userland)** | Prereqs: NET-1.8 | Files: `userbin/dhcp/` (new)
  - `DHCP [/RENEW] [/RELEASE]`. DHCP via UDP socket. Persiste en Registry.
  - **Tests:** servidor DHCP simulado

* [ ] **NET-1.16. Kernel DHCP no progresa en userspace** | Prereqs: NET-1.8 | Files: `src/net/dhcp.rs`, `src/arch/x64/idt.rs`
  - `build_dhcp_packet()` usa `Vec` (heap allocation) → inseguro desde timer IRQ.
  - `nic_send_packet()` toma `NIC_REGISTRY.lock()` (spinlock) → deadlock desde IRQ.
  - `dhcp_tick()` en idle loop nunca corre porque threads userspace siempre están Ready.
  - **Solución:** buffer fijo en `build_dhcp_packet()`, `Mutex` para `DHCP_CLIENT`, llamar `dhcp_tick()` desde timer IRQ.
  - **Tests:** `dhcp_discover_offer_sequence`, netcfg obtiene IP real vía DHCP.

* [ ] **B3.4. NTP client** | Prereqs: B3.2 | Files: `src/net/ntp.rs`
  - Cliente NTP (RFC 5905, SNTP simplificado). Sincroniza RTC del sistema.
  - **Tests:** `ntp_request_parse_response`, `ntp_offset_calculation`

### Userland

* [ ] **B4.3. Shell redirection (>, <, >>)** | Prereqs: A4.7 | Files: `userbin/neoshell/`
  - Parser detecta tokens `>` (write), `>>` (append), `<` (read). Usa `sys_dup2`.
  - **Tests:** `redirect_stdout_to_file`, `redirect_stdin_from_file`, `redirect_append`

* [ ] **B4.6. NeoEdit text editor** | Prereqs: A4.7, B4.4 | Files: `userbin/neoedit/`
  - Editor de texto modal Ring 3. Usa `ob_open` + `ob_query_info(ReadContent)` / `ob_set_info(WriteContent)`.
  - **Tests:** `neoedit_open_display`, `neoedit_edit_save`, `neoedit_scroll`

* [ ] **B4.7. Shared library per-process binding** | Prereqs: sys_loadlib | Files: `src/elf.rs`, `libneodos/`
  - Evolucionar NXL slots globales a binding per-process. Cada EPROCESS mantiene su tabla de NXLs.
  - **Tests:** `nxl_per_process_isolation`, `nxl_unload_on_exit`, `nxl_version_coexistence`

* [ ] **B4.9. NeoShell scripting (.BAT)** | Prereqs: B4.1, B4.2, B4.3 | Files: `userbin/neoshell/`
  - Intérprete batch: `ECHO`, `SET`, `IF`, `GOTO`, `CALL`, `FOR`, `REM`, `@`.
  - **Tests:** `bat_echo_set`, `bat_if_goto`, `bat_call_subroutine`, `bat_for_loop`

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

---

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
| AUDIT-27 | docs/objects.md: SocketRecv class 23 documented but does not exist in kernel enum | `docs/objects.md`, `src/object/types.rs` |
| AUDIT-28 | docs/memory.md: kernel_image base says 0x100000, actual load address is 0x4000000 | `docs/memory.md`, `neodos-kernel/kernel.ld` |
| AUDIT-29 | Version mismatch: AGENTS.md says v0.48.6, kernel Cargo.toml says 0.48.0 | `AGENTS.md`, `neodos-kernel/Cargo.toml` |
| AI-2 | Consolidate legacy syscall wrappers | `src/syscall/mod.rs` |
| AI-3 | ObObjectTable lock granularity (lock striping) | `src/object/mod.rs` |
| B2.2 | Registry transaction journal (WAL) | `src/cm/journal.rs` |
| B2.3 | Multi-Hive Architecture | `src/cm/hive.rs`, `src/cm/manager.rs` |
| B2.4 | Registry Security (ACL por clave) | `src/cm/security.rs` |
| B2.5 | Registry notification + load/unload | `src/cm/notify.rs` |
| USR-025..032 | USR Fase 3: runas, secedit, groups, MIC enforcement | `userbin/runas/`, `userbin/secedit/`, `src/security/` |

### Milestones (LOW)

| ID | Item | Prereqs |
|----|------|---------|
| v0.50 | Async I/O (IOCP v1), Registry hive features, DHCP | v0.49 |
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

### NeoFS Audit
Full audit in [NEOFS_AUDIT.md](NEOFS_AUDIT.md), roadmap in [NEOFS_ROADMAP.md](NEOFS_ROADMAP.md),
test plan in [NEOFS_TESTS.md](NEOFS_TESTS.md).

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
