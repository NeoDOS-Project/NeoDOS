# NeoDOS — Roadmap Local

> **Fuente única de verdad:** GitHub Issues
> Este archivo es la lista de ideas local. La IA o `scripts/sync-roadmap.sh`
> convierten cada ítem en una GitHub Issue.

## Formato

```
- **ID**: Título `prioridad` `etiqueta1` `etiqueta2` `hito`
  Descripción del ítem.

  state: open|closed        (opcional, defecto open)
```

---

## Implementadas (issues cerradas en GitHub)

### v0.50.2 — Hostname

- **HN**: System hostname persistence with Registry `priority/medium` `area/kernel` `type/feature` `v0.50 — Consolidation`
  ObInfoClass::Hostname, ObSetInfoClass::SetHostname, libneodos wrappers,
  hostname.nxe binary, ipconfig integration, Registry persistence.
  state: closed

### v0.50.1 — Power Manager Phase 2

- **PM-PHASE2**: Power Manager kernel core `priority/high` `area/power` `type/feature` `v0.50 — Consolidation`
  PowerManager struct, PowerPlan/PowerPolicies, Registry persistence,
  shutdown/reboot coordination, ObType::PowerManager.
  state: closed

### v0.50 — Audits & Cleanup

- **AUDIT-33**: Boot/init hardening `priority/critical` `area/kernel` `type/security` `v0.50 — Consolidation`
  Panic→graceful halt, Registry tolerance, service fallback.
  state: closed

- **CM-FIX**: Registry bugfixes `priority/high` `area/registry` `type/bug` `v0.50 — Consolidation`
  Free list fix, soft max cells, delete_value, deadlock fix, iterative delete_key.
  state: closed

### v0.49 — Service Manager & NeoFS v2 & Network Userland

- **SM-001**: Service Manager `priority/high` `area/kernel` `type/feature` `v0.50 — Consolidation`
  ObType::Service, service state machine, dependencies, Registry backend.
  state: closed

- **NET-1.5..1.15**: Networking userland `priority/high` `area/net` `type/feature`
  libneodos socket wrappers, net.nxl, netcfg, ipconfig, dhcp.
  state: closed

---

## Pendientes

### v0.51 — NeoFS v2 + Shell Phase 2 + SAM

#### M1.1 — NeoFS v2 Completion

- **NFSv2-BTREE**: B-tree persistente genérico (COW) `priority/high` `area/fs` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Implementar B-tree con copy-on-write para NeoFS v2.
  Inserción, borrado y búsqueda O(log n) con nodos de 4KB.
  state: open

- **NFSv2-FREELIST**: Free list + merge adyacentes `priority/high` `area/fs` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Gestión de espacio libre con merge de bloques adyacentes.
  state: open

- **NFSv2-SNAPSHOT**: Snapshot table (64 circular) `priority/high` `area/fs` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Tabla de snapshots circular con 64 entradas. Depende de NFSv2-BTREE.
  Dependencies: NFSv2-BTREE
  state: open

- **NFSv2-MKFS**: mkfs.neodos tool `priority/medium` `area/fs` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Herramienta para formatear particiones como NeoFS v2.
  Dependencies: NFSv2-FREELIST
  state: open

- **VFS-2.2**: Refactorizar FSCK a trait `priority/medium` `area/fs` `type/refactor` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  state: open

#### M1.2 — Shell Phase 2

- **SH-EDITOR+HISTORY**: Shell line editor + history `priority/high` `area/shell` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  LineEditor ANSI con Ctrl-A/E/K/U/R, Insert.
  Ring buffer dinámico con persistencia en C:\\System\\neoshell.hst.
  state: open

- **SH-ENV+PIPE**: Shell env expansion + pipeline `priority/high` `area/shell` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  %VARNAME% expansion, pipeline con ob_wait, exit codes.
  Dependencies: SH-TOKEN+QUOTE
  state: open

- **SH-SEP+COMPL+BATCH**: Separator + completion + scripting `priority/medium` `area/shell` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Semicolon token, completion engine, batch interpreter (ECHO, SET, IF, GOTO, CALL, FOR).
  Dependencies: SH-TOKEN+QUOTE, SH-REDIR, SH-ENV+PIPE
  state: open

#### M1.3 — SAM Foundation + Network Tools

- **USR-P1a**: ObType::Session + SAM built-in users `priority/high` `area/security` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Session=19 en ObType, built-in Administrator/Guest, SAM entries.
  state: open

- **USR-P1b**: Token: add integrity_level + creation_time `priority/high` `area/security` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  IntegrityLevel enum, Token fields, admin=System IL, user=Medium IL.
  Dependencies: USR-P1a
  state: open

- **USR-P1c**: SAM persistence to Registry hive `priority/medium` `area/security` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  sam_save/sam_load via VFS, wire on user create/delete/password change.
  Dependencies: USR-P1a
  state: open

- **USR-P1d**: SeAccessCheck: fix empty DACL + group SIDs `priority/high` `area/security` `type/bug` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Empty ACL=deny all, iterate token.groups in ACL evaluation.
  Dependencies: USR-P1b
  state: open

- **USR-P1e**: ObSetInfoClass::ChangePassword `priority/medium` `area/security` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  ChangePassword=31, validates old password, updates SAM hash.
  Dependencies: USR-P1c
  state: open

- **NET-1.9**: ipconfig.nxe `priority/medium` `area/net` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  IPCONFIG /ALL: interfaces, MAC, IP, gateway, DNS, stats.
  state: open

- **NET-1.10**: ping.nxe `priority/medium` `area/net` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  PING host [/n count] [/w ms] via raw ICMP echo request.
  state: open

- **B3.4**: NTP client `priority/low` `area/net` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  SNTP simplificado (RFC 5905), sincronización RTC.
  state: open

- **ADM-1**: neotop v0.2 `priority/medium` `area/tools` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Per-thread CPU, I/O stats, network bar.
  state: open

- **ADM-2**: neostat `priority/medium` `area/tools` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Terminal dashboard: CPU%, memoria, disco, red.
  state: open

- **ADM-4**: neotask `priority/medium` `area/tools` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Listar procesos, matar, cambiar prioridad, crear proceso.
  state: open

- **ADM-5**: neocfg (Panel de Control) `priority/medium` `area/tools` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Panel de control modular con CfgModule trait. Módulos: System, Keyboard, About, Power (stub), Locale (stub).
  state: open

- **ADM-6**: neofs `priority/medium` `area/tools` `type/feature` `v0.51 — NeoFS v2 + Shell Phase 2 + SAM`
  Estadísticas de volumen, fsck, cambiar label, listar montajes.
  state: open

### v0.52 — VirtIO + Sessions + FS Security

#### M1.4 — VirtIO Architecture

- **VIO-ARCH**: Virtqueue abstraction + modern PCI transport `priority/high` `area/virtio` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  Split vring 1.0, legacy I/O + modern MMIO BAR, feature negotiation, MSI-X.
  Dependencies: A2.1
  state: open

- **VIO-NET**: VirtIO Network (0x1000) `priority/high` `area/virtio` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  1 RX + 1 TX virtqueue, mergeable RX buffers, checksum offload.
  Dependencies: VIO-ARCH
  state: open

- **VIO-BLK2**: VirtIO Block NEM driver `priority/high` `area/virtio` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  Reemplazar BOOT_DRIVER por NEM standalone. Hotplug multi-dispositivo.
  Dependencies: VIO-ARCH
  state: open

- **VIO-9P**: VirtIO 9P filesystem `priority/medium` `area/virtio` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  Filesystem 9P2000.L para compartir directorios host-huésped.
  Dependencies: VIO-ARCH
  state: open

- **VIO-INPUT**: VirtIO Input `priority/medium` `area/virtio` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  Teclado, ratón, tablet vía VirtIO.
  Dependencies: VIO-ARCH
  state: open

#### M1.5 — Sessions + FS Security

- **USR-P2a**: SessionManager global + ObCreate(Session) `priority/high` `area/security` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  SESSION_MANAGER global, handler sys_ob_create(Session), auto-path.
  Dependencies: USR-P1a
  state: open

- **USR-P2b**: ObInfoClass::SessionInfo + SessionLock/Logoff `priority/high` `area/security` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  SessionInfo=24, SessionLock=28, SessionLogoff=29.
  Dependencies: USR-P2a
  state: open

- **USR-P2c**: TokenInfo + inheritance with session_id `priority/medium` `area/security` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  TokenInfo=28, child inherits session_id from parent.
  Dependencies: USR-P2a
  state: open

- **USR-P2d**: neologon.nxe login binary `priority/high` `area/security` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  Login prompt, SAM credential validation via sys_ob_create(Session).
  Dependencies: USR-P2b, USR-P2c
  state: open

- **USR-P2e**: NeoInit spawns neologon instead of shell `priority/high` `area/security` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  NeoInit Phase 4 spawns neologon, DefaultAutoLogin support.
  Dependencies: USR-P2d
  state: open

- **USR-P3a**: DirEntryV2 owner_sid field `priority/high` `area/fs` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  Add owner_sid to DirEntryV2, backward compat.
  Dependencies: USR-P1b
  state: open

- **USR-P3b**: VFS permission checking function `priority/high` `area/fs` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  check_vfs_access() con token, mode, owner_sid.
  Dependencies: USR-P1d, USR-P3a
  state: open

- **USR-P3c**: Wire VFS checks in syscall handlers `priority/high` `area/fs` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  Check READ/WRITE/EXECUTE en ob_open, ob_create, ob_destroy, ob_set_info.
  Dependencies: USR-P3b
  state: open

- **USR-P3d**: Default permissions by extension `priority/medium` `area/fs` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  .NEM/.SYS → admin-only, .NXE/.NXL → world r+x.
  Dependencies: USR-P3c
  state: open

#### M1.6 — Power Phase 3 + Zero-copy

- **PM-PHASE3**: Power syscall dispatch + Event Bus types `priority/medium` `area/power` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  Dispatch handlers 37-42, Event Bus types 19-26, shutdown coordination.
  Dependencies: PM-PHASE2
  state: open

- **B6.1**: Zero-copy pipes `priority/medium` `area/kernel` `type/feature` `v0.52 — VirtIO + Sessions + FS Security`
  Pipes sin copia de datos entre procesos.
  state: open

### v0.53 — Registry Phase 2 + Integrity + Signing

- **CM-DIRTY**: Registry per-cell dirty tracking `priority/high` `area/registry` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  dirty_cells BitVec, serialize_dirty escribe solo celdas sucias.
  state: open

- **CM-MULTI**: Registry multi-hive `priority/high` `area/registry` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  Montar SOFTWARE, SECURITY, DEFAULT hives.
  state: open

- **USR-P4a**: Registry ACL checking module `priority/high` `area/security` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  cm_check_access() reusing SeAccessCheck.
  Dependencies: USR-P1d
  state: open

- **USR-P4b**: Wire sec_desc_cell on key creation `priority/high` `area/registry` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  Inherit parent sec_desc_cell or create default.
  Dependencies: USR-P4a
  state: open

- **USR-P4c**: ACL checks in Cm syscall handlers `priority/high` `area/registry` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  Wire cm_check_access in all Registry handlers.
  Dependencies: USR-P4b
  state: open

- **USR-P4d**: User profile hive auto-mount `priority/medium` `area/registry` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  Auto-mount \\Registry\\User\\{sid} on session creation.
  Dependencies: USR-P4c
  state: open

- **USR-P5a**: Integrity level in SeAccessCheck `priority/high` `area/security` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  If process_IL < object_IL, deny WRITE/DELETE.
  Dependencies: USR-P1b
  state: open

- **USR-P5b**: SetIntegrityLevel + IntegrityLevel info classes `priority/medium` `area/security` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  SetIntegrityLevel=32 (can only lower), IntegrityLevel=27.
  Dependencies: USR-P5a
  state: open

- **USR-P5c**: Privilege enforcement in admin syscalls `priority/high` `area/security` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  Wire has_privilege() in driver_unload, cm_load_hive, cm_unload_hive.
  Dependencies: USR-P1b
  state: open

- **B5.1**: Module signature validation `priority/high` `area/security` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  Validación criptográfica de módulos .nem antes de cargar.
  state: open

- **B5.2**: Driver permission enforcement `priority/high` `area/security` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  Cruzar capacidad declarada del driver con token del proceso.
  Dependencies: B5.1
  state: open

- **A3.2**: Kernel debugger (KD) GDB stub `priority/medium` `area/kernel` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  INT3 breakpoints, hardware watchpoints, GDB remote protocol via serial.
  state: open

- **B4.6**: NeoEdit text editor `priority/low` `area/tools` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  Editor de texto modal Ring 3.
  state: open

- **B4.7**: Shared library per-process binding `priority/medium` `area/kernel` `type/feature` `v0.53 — Registry Phase 2 + Integrity + Signing`
  Evolucionar NXL slots globales a binding per-process.
  state: open

### v0.54 — Power Phase 4 + User Commands + DNS

- **PM-PHASE4**: Power shutdown coordination + libneodos + shell `priority/medium` `area/power` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  ServiceManager::stop_all(), libneodos wrappers, shell REBOOT/POWEROFF.
  Dependencies: PM-PHASE3
  state: open

- **USR-P6a**: WHOAMI command `priority/low` `area/shell` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  Dependencies: USR-P2c
  state: open

- **USR-P6b**: PASSWD command `priority/low` `area/shell` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  Dependencies: USR-P2d
  state: open

- **USR-P6c**: WHO + LOGOFF commands `priority/low` `area/shell` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  Dependencies: USR-P2b
  state: open

- **USR-P6d**: SU command `priority/low` `area/shell` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  Dependencies: USR-P2d, USR-P2e
  state: open

- **USR-P6e**: RUNAS command `priority/low` `area/shell` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  Dependencies: USR-P6d
  state: open

- **NET-DNS**: DNS resolver (stub + cache) `priority/medium` `area/net` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  Dependencies: NET-1.9
  state: open

- **B1.1**: Kernel tracing infrastructure `priority/medium` `area/kernel` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  TraceBuffer, trace points, filtered dump with timestamps.
  state: open

- **B1.2**: NeoTrace system `priority/low` `area/tools` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  Dependencies: B1.1
  state: open

- **ADM-3**: neolog `priority/low` `area/tools` `type/feature` `v0.54 — Power Phase 4 + User Commands + DNS`
  Visor de event log del kernel + EventBus.
  Dependencies: B1.1
  state: open

### v0.55 — WAL + Secure Boot + VFS Advanced

- **CM-WAL**: Registry WAL (write-ahead logging) `priority/medium` `area/registry` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  WAL entries with fsync, replay on mount, power loss recovery.
  Dependencies: CM-DIRTY
  state: open

- **CM-LIB**: Registry libneodos wrappers `priority/low` `area/registry` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  7 wrappers: cm_create_key, cm_delete_key, cm_enum_key, cm_enum_value, cm_flush_key, cm_load_hive, cm_unload_hive.
  state: open

- **CM-REGEDIT**: regedit.nxe `priority/low` `area/registry` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  Dependencies: CM-LIB
  state: open

- **B5.3**: Secure boot chain `priority/low` `area/security` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  Verificación encadenada bootloader → kernel → drivers.
  Dependencies: B5.1
  state: open

- **PM-PHASE5**: Power Manager polish + full tests `priority/low` `area/power` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  Async coordination, Event Bus integration, complete test suite.
  Dependencies: PM-PHASE4
  state: open

- **VFS-3.2**: \\DosDevices dinámico `priority/low` `area/fs` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  state: open

- **VFS-5.3**: Write-back ordenado `priority/low` `area/fs` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  state: open

- **VFS-6.1**: Overlay mounts `priority/low` `area/fs` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  state: open

- **VFS-6.2**: Extended attributes `priority/low` `area/fs` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  state: open

- **VFS-6.3**: File notifications via Event Bus `priority/low` `area/fs` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  state: open

- **VFS-6.4**: Async VFS via IOCP `priority/low` `area/fs` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  state: open

- **VFS-7.1**: Eliminar lock global de VFS `priority/low` `area/fs` `type/refactor` `v0.55 — WAL + Secure Boot + VFS Advanced`
  state: open

- **VFS-7.2**: Lookup cache `priority/low` `area/fs` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  state: open

- **VFS-7.3**: Path cache `priority/low` `area/fs` `type/feature` `v0.55 — WAL + Secure Boot + VFS Advanced`
  state: open

### v0.56 — NXE/NXP Ecosystem

- **NXE-ECO-12**: NXE metadata auto-generation `priority/medium` `area/tools` `type/feature` `v0.56 — NXE/NXP Ecosystem`
  Dependencies: NXE-ECO-1
  state: open

- **NXE-ECO-13**: \\Resource\\ virtual Ob namespace `priority/medium` `area/kernel` `type/feature` `v0.56 — NXE/NXP Ecosystem`
  Dependencies: NXE-ECO-5
  state: open

- **NXE-ECO-14**: NXE header validation `priority/low` `area/kernel` `type/feature` `v0.56 — NXE/NXP Ecosystem`
  state: open

- **NXE-ECO-15**: Digital signature verification infrastructure `priority/low` `area/security` `type/feature` `v0.56 — NXE/NXP Ecosystem`
  state: open

- **I18N-P4**: format_str() con placeholders {0} `priority/medium` `area/i18n` `type/feature` `v0.56 — NXE/NXP Ecosystem`
  Dependencies: I18N-P1
  state: open

- **I18N-P5**: i18n_available_locales() `priority/low` `area/i18n` `type/feature` `v0.56 — NXE/NXP Ecosystem`
  Dependencies: I18N-P1
  state: open

- **I18N-P6**: Per-user locale (Registry) `priority/low` `area/i18n` `type/feature` `v0.56 — NXE/NXP Ecosystem`
  Dependencies: USR-P1, I18N-P1
  state: open

### v0.57 — Executive Manager

- **EXEC-CM**: Configuration Manager `priority/high` `area/executive` `type/feature` `v0.57 — Executive Manager`
  Consolidación Registry + boot settings, CurrentControlSet, hardware profiles.
  Dependencies: CM-MULTI
  state: open

- **EXEC-SM**: Session Manager `priority/high` `area/executive` `type/feature` `v0.57 — Executive Manager`
  Gestión completa de sesiones, integración con Service Manager.
  Dependencies: USR-P2a
  state: open

- **EXEC-OM**: Object Namespace Manager `priority/medium` `area/executive` `type/feature` `v0.57 — Executive Manager`
  Per-process namespace virtualization, virtual /dev /proc /sys dirs.
  state: open

- **EXEC-PM**: Power Manager final `priority/medium` `area/executive` `type/feature` `v0.57 — Executive Manager`
  Power Manager como servicio Executive completo.
  Dependencies: PM-PHASE5
  state: open

### v0.58 — Official Tools

- **TOOL-NEODEV-LEGACY**: Eliminar scripts heredados `priority/high` `area/tools` `type/cleanup` `v0.58 — Official Tools`
  Eliminar build.sh, qemu-debug.sh, auto_test.py, create_ne2_image.py.
  Dependencies: TOOL-NEODEV
  state: open

- **TOOL-NEOCFG**: neocfg completar módulos `priority/medium` `area/tools` `type/feature` `v0.58 — Official Tools`
  Completar módulos Power y Locale (actualmente stubs).
  Dependencies: ADM-5
  state: open

- **TOOL-ADM**: Admin tools completion `priority/medium` `area/tools` `type/feature` `v0.58 — Official Tools`
  neomem v0.2, neotop v0.2+, neotask completo, neolog.
  state: open

- **TOOL-NXE**: NXE tools completion `priority/medium` `area/tools` `type/feature` `v0.58 — Official Tools`
  Completar nxeinfo, nxpkg, nxdump, nxres, nxlocale, nxverify.
  state: open

### v0.59 — Installation & Bootstrap

- **INSTALL-NEOFS**: Creación de NeoFS desde cero `priority/high` `area/install` `type/feature` `v0.59 — Installation & Bootstrap`
  Dependencies: NFSv2-MKFS
  state: open

- **INSTALL-BOOTSTRAP**: Bootstrap inicial `priority/high` `area/install` `type/feature` `v0.59 — Installation & Bootstrap`
  Bootloader detecta instalación vs arranque normal.
  state: open

- **INSTALL-NXE**: install.nxe `priority/high` `area/install` `type/feature` `v0.59 — Installation & Bootstrap`
  Asistente interactivo: disco, particionar, formatear, copiar sistema.
  state: open

- **INSTALL-CONFIG**: Configuración inicial `priority/high` `area/install` `type/feature` `v0.59 — Installation & Bootstrap`
  Crear usuario admin, configurar keyboard, locale.
  Dependencies: USR-P1
  state: open

- **INSTALL-PACKAGES**: Despliegue de paquetes base `priority/medium` `area/install` `type/feature` `v0.59 — Installation & Bootstrap`
  Copiar NXP base, registrar servicios.
  state: open

### v0.60 — NLT i18n + Regional Formats

- **I18N-P7**: NLT compression (LZSS/LZ4) `priority/low` `area/i18n` `type/feature` `v0.60 — NLT i18n + Regional Formats`
  Dependencies: I18N-P1
  state: open

- **I18N-P8**: UTF-16 support `priority/low` `area/i18n` `type/feature` `v0.60 — NLT i18n + Regional Formats`
  Dependencies: I18N-P1
  state: open

- **I18N-P9**: Pluralization `priority/low` `area/i18n` `type/feature` `v0.60 — NLT i18n + Regional Formats`
  Dependencies: I18N-P1
  state: open

- **I18N-P10**: Regional formats (dates, currencies) `priority/low` `area/i18n` `type/feature` `v0.60 — NLT i18n + Regional Formats`
  Dependencies: I18N-P1
  state: open

- **I18N-P11**: RTL/bidi support `priority/low` `area/i18n` `type/feature` `v0.60 — NLT i18n + Regional Formats`
  Dependencies: I18N-P1
  state: open

- **I18N-P12**: Digital signatures in NLT `priority/low` `area/i18n` `type/feature` `v0.60 — NLT i18n + Regional Formats`
  Dependencies: I18N-P1
  state: open

### v0.61 – v0.69 — Security & Stability

- **SEC-AUDIT-FULL**: Auditoría de seguridad completa `priority/critical` `area/security` `type/security` `v0.61 — Security Hardening`
  Revisión de todas las syscalls, accesos a memoria, validación de punteros.
  state: open

- **SEC-FUZZ-SYSCALLS**: Fuzzing de syscalls (0–77) `priority/critical` `area/security` `type/security` `v0.61 — Security Hardening`
  Fuzzing automatizado con argumentos aleatorios.
  state: open

- **SEC-FUZZ-DRIVERS**: Fuzzing de interfaz HST `priority/high` `area/security` `type/security` `v0.61 — Security Hardening`
  Fuzzing de exportaciones HST de drivers NEM.
  state: open

- **SEC-ASLR-V2**: ASLR v2 (stack + heap) `priority/high` `area/security` `type/security` `v0.61 — Security Hardening`
  Posición aleatoria de pila Ring 3 y heap de usuario.
  Dependencies: ASLR v1
  state: open

- **SEC-ASLR-V3**: ASLR v3 (full randomization) `priority/medium` `area/security` `type/security` `v0.62 — Security Hardening II`
  ELF + stack + heap + mmap randomization.
  Dependencies: SEC-ASLR-V2
  state: open

- **SEC-NX**: Non-executable stack `priority/high` `area/security` `type/security` `v0.62 — Security Hardening II`
  NX bit en páginas de pila.
  state: open

- **SEC-NX-HEAP**: Non-executable heap `priority/high` `area/security` `type/security` `v0.62 — Security Hardening II`
  NX bit en páginas de heap.
  state: open

- **PERF-SCHED-LOCKFREE**: Scheduler lock-free `priority/high` `area/kernel` `type/perf` `v0.63 — Performance`
  Per-CPU run queues con operaciones lock-free.
  state: open

- **PERF-SLAB-NUMA**: Per-CPU heaps NUMA-aware `priority/medium` `area/kernel` `type/perf` `v0.63 — Performance`
  state: open

- **PERF-BENCH-SUITE**: Benchmarking suite `priority/medium` `area/kernel` `type/perf` `v0.63 — Performance`
  state: open

- **PERF-PGO**: Profile-guided optimization `priority/low` `area/kernel` `type/perf` `v0.63 — Performance`
  Dependencies: PERF-BENCH-SUITE
  state: open

- **DOCS-API-COMPLETE**: Documentación completa de API `priority/critical` `area/meta` `type/docs` `v0.64 — Documentation & Test Coverage`
  Syscalls, libneodos, drivers NEM.
  state: open

- **DOCS-SUBSYSTEMS**: Documentación de subsistemas `priority/high` `area/meta` `type/docs` `v0.64 — Documentation & Test Coverage`
  state: open

- **DOCS-TUTORIALS**: Tutorials `priority/high` `area/meta` `type/docs` `v0.64 — Documentation & Test Coverage`
  Dependencies: DOCS-API-COMPLETE
  state: open

- **TEST-COVERAGE-80**: Coverage >80% `priority/critical` `area/meta` `type/test` `v0.64 — Documentation & Test Coverage`
  state: open

- **TEST-COVERAGE-95**: Coverage >95% `priority/high` `area/meta` `type/test` `v0.65 — Documentation & Test Coverage II`
  Dependencies: TEST-COVERAGE-80
  state: open

### v1.0 — First Stable API

- **V1.0-RELEASE**: Release v1.0.0 `priority/critical` `area/meta` `type/release` `v1.0 — First Stable API`
  state: open

- **V1.0-ABI-FROZEN**: Todas las interfaces congeladas `priority/critical` `area/meta` `type/release` `v1.0 — First Stable API`
  Syscalls, NEM, eventos, capacidades.
  state: open

- **V1.0-DOCS**: Documentación de release `priority/critical` `area/meta` `type/release` `v1.0 — First Stable API`
  Changelog, guía de migración.
  Dependencies: DOCS-API-COMPLETE
  state: open

- **V1.0-TESTS**: Suite completa (800+) `priority/critical` `area/meta` `type/release` `v1.0 — First Stable API`
  Dependencies: TEST-COVERAGE-95
  state: open

- **V1.0-NXE-COMPAT**: Binarios contra ABI final `priority/high` `area/meta` `type/release` `v1.0 — First Stable API`
  Dependencies: V1.0-ABI-FROZEN
  state: open
