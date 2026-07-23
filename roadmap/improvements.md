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

- **SM-001**: Service Manager `priority/high` `area/kernel` `type/feature` `v0.49 — Services & Tools`
  ObType::Service, service state machine, dependencies, Registry backend.
  state: closed

- **NET-1.5..1.15**: Networking userland `priority/high` `area/net` `type/feature` `v0.49 — Services & Tools`
  libneodos socket wrappers, net.nxl, netcfg, ipconfig, dhcp.
  state: closed

### v0.50.0-dev — Driver Manager, Logging, B-tree COW

- **LOG**: Configurable kernel logging system `priority/medium` `area/kernel` `type/feature` `v0.50 — Consolidation`
  Log module with 5 levels, 46 subsystems, compile-time + runtime filtering.
  state: closed

- **DRVMGR**: Driver Manager con carga selectiva `priority/high` `area/drivers` `type/feature` `v0.50 — Consolidation`
  Device discovery, manifest-based matching, selective driver loading.
  state: closed

- **NFSv2-BTREE**: B-tree persistente genérico (COW) `priority/high` `area/fs` `type/feature` `v0.50 — Consolidation`
  B-tree COW con operaciones atómicas y journaling de metadatos para NeoFS.
  state: closed

- **BUG-NEOINIT-DUP**: NeoInit duplicate spawn fix `priority/high` `area/shell` `type/bug` `v0.50 — Consolidation`
  Race condition donde NeoInit podía spawnear el shell dos veces.
  state: closed

- **GIT-SSOT**: GitHub Issues Source of Truth `priority/medium` `area/meta` `type/tooling` `v0.50 — Consolidation`
  scripts/sync-roadmap.sh para sincronizar roadmap local con GitHub Issues.
  state: closed

- **CONSOLE-API**: Progress bar/spinner API unificada `priority/low` `area/shell` `type/refactor` `v0.50 — Consolidation`
  console.nxl refactorizado con API común para barras de progreso y spinners.
  state: closed

- **SCRIPTS-RUST**: Port de scripts a Rust `priority/medium` `area/build` `type/refactor` `v0.50 — Consolidation`
  gen-hiv, crashdump, check-deps migrados de Python a Rust.
  state: closed

- **NEODEV-EXTRACT**: NeoDev extraído a repositorio independiente `priority/medium` `area/tools` `type/refactor` `v0.50 — Consolidation`
  NeoDev separado a github.com/NeoDOS-Project/NeoDev, CLI unificada.
  state: closed

### v0.50.0 — NXE/NXP Ecosystem + i18n NLTv2

- **NXE-ECO**: NXE metadata note (.note.neodos) `priority/medium` `area/kernel` `type/feature` `v0.50 — Consolidation`
  ELF note section con metadatos TLV para ejecutables NXE.
  state: closed

- **NXEINFO**: nxeinfo tool `priority/medium` `area/tools` `type/feature` `v0.50 — Consolidation`
  Inspector de ejecutables NXE: metadata, headers, sections, JSON.
  state: closed

- **NXPKG**: nxpkg tool `priority/medium` `area/tools` `type/feature` `v0.50 — Consolidation`
  Creador/gestor de paquetes NXP: create, extract, list, info, verify.
  state: closed

- **NXDUMP**: nxdump tool `priority/medium` `area/tools` `type/feature` `v0.50 — Consolidation`
  Volcado técnico de ELF/NXE/NEM: hex, ELF structures, relocations, strings.
  state: closed

- **LIB-RES**: libneodos Resource API `priority/medium` `area/kernel` `type/feature` `v0.50 — Consolidation`
  res_open, res_read, res_size para acceso a recursos de aplicaciones NXP.
  state: closed

- **I18N-EXT**: libneodos i18n extensions `priority/medium` `area/i18n` `type/feature` `v0.50 — Consolidation`
  i18n_set_app_name, i18n_load_from_package, i18n_available_locales, i18n_format.
  state: closed

- **NXRES**: nxres resource explorer `priority/low` `area/tools` `type/feature` `v0.50 — Consolidation`
  Explorador de recursos Ring 3: list, cat, locale.
  state: closed

- **NXLOCALE**: nxlocale language manager `priority/low` `area/tools` `type/feature` `v0.50 — Consolidation`
  Gestor de idiomas Ring 3: list, current, set, stats, show.
  state: closed

- **NXVERIFY**: nxverify integrity verifier `priority/low` `area/tools` `type/feature` `v0.50 — Consolidation`
  Verificador de integridad Ring 3: file, app, all, package (CRC32).
  state: closed

- **I18N-P1**: Runtime i18n NLTv2 + IDs numéricos `priority/high` `area/i18n` `type/feature` `v0.50 — Consolidation`
  NLTv2 con IDs numéricos u32, búsqueda binaria, tr_id!() macro.
  state: closed

- **I18N-P2**: Migrar apps core a tr_id!() `priority/medium` `area/i18n` `type/feature` `v0.50 — Consolidation`
  35 nuevos paquetes NLT × 3 idiomas migrados de tr!() a tr_id!().
  state: closed

- **I18N-P3a**: nltc compiler `priority/medium` `area/tools` `type/feature` `v0.50 — Consolidation`
  Compilador TOML → NLTv2: compile, check, generate-ids, generate-rust, scaffold.
  state: closed

- **I18N-P3b**: TOML sources + NLTv2 binaries `priority/medium` `area/i18n` `type/feature` `v0.50 — Consolidation`
  14 fuentes TOML (7 en-US + 7 es-ES) compiladas a NLTv2.
  state: closed

- **I18N-P3c**: neolocale tool (NLTv2) `priority/low` `area/tools` `type/feature` `v0.50 — Consolidation`
  neolocale actualizado: validate, stats, diff, check, create para NLTv2.
  state: closed

- **I18N-P3d**: Integración NeoDev + disco `priority/medium` `area/build` `type/feature` `v0.50 — Consolidation`
  compile_nlt_files() en build, NLTs incluidos en imagen de disco.
  state: closed

### v0.50.1 — Power Phase 2, Snapshot, ROADMAP.md

- **SNAP-48**: sys_ob_snapshot (RAX 48) `priority/high` `area/fs` `type/feature` `v0.50 — Consolidation`
  Nueva syscall para operaciones de snapshot NeoFS: CREATE, RESTORE, LIST, PURGE.
  state: closed

- **L2CACHE**: L2 write-back cache `priority/medium` `area/fs` `type/feature` `v0.50 — Consolidation`
  IoStack write_sectors cachea escrituras single-sector via PAGE_CACHE.
  state: closed

- **NET-1.7**: Socket ephemeral port `priority/medium` `area/net` `type/feature` `v0.50 — Consolidation`
  Asignar NIC por defecto y puerto efímero (49152-65535) si no especificado.
  state: closed

- **SH-REDIR**: Shell redirection `priority/high` `area/shell` `type/feature` `v0.50 — Consolidation`
  Redirección Shell (>, <, >>, 2>) con tokenizer y dup2 previo al spawn.
  state: closed

- **I18N-ALL**: i18n completa de todos los User-Bin `priority/medium` `area/i18n` `type/feature` `v0.50 — Consolidation`
  42/42 User-Bin usando NLT + tr_id!() para mensajes visibles.
  state: closed

- **NET-NODELOCK**: Network deadlock fix `priority/high` `area/net` `type/bug` `v0.50 — Consolidation`
  net_handle_incoming_packet recibe &mut dyn NetworkInterface directamente.
  state: closed

- **NET-GARP**: Gratuitous ARP `priority/low` `area/net` `type/feature` `v0.50 — Consolidation`
  send_gratuitous_arp() llamado automáticamente en nic_set_ip().
  state: closed

- **NET-COUNTERS**: Network counters `priority/low` `area/net` `type/feature` `v0.50 — Consolidation`
  Per-protocol packet/byte counters (RX/TX/ARP/ICMP) con dump periódico.
  state: closed

### v0.49.2 — NeoKBD + ACPI Power Management

- **KBD-PHASE1**: NeoKBD kernel module `priority/high` `area/kernel` `type/feature` `v0.49 — Services & Tools`
  ObType::KeyboardDevice, layout engine, Unicode, dead keys, hotkeys, Registry config.
  state: closed

- **KBD-PHASE2**: ps2kbd simplification + kbdcompile `priority/medium` `area/drivers` `type/feature` `v0.49 — Services & Tools`
  ps2kbd simplificado (~150 lines menos), tools/kbdcompile/ para .klc → .kbd.
  state: closed

- **ADM-NEOKEY**: neokey CLI utility `priority/low` `area/tools` `type/feature` `v0.49 — Services & Tools`
  Reemplaza keyb.nxe: NEOKEY show/layout/repeat/delay/leds.
  state: closed

- **PM-PHASE1**: HAL ACPI primitives `priority/high` `area/power` `type/feature` `v0.49 — Services & Tools`
  ACPI reboot/FADT/S5 primitives con chain de fallback.
  state: closed

### v0.49.1 — NeoDev, PowerManager Ob, Console

- **NEODEV**: NeoDev unified development tool `priority/high` `area/tools` `type/feature` `v0.49 — Services & Tools`
  Build, image, run, test, clean en Rust. Reemplaza build.sh, qemu-debug.sh, auto_test.py.
  state: closed

- **PWR-OB**: PowerManager Ob object `priority/medium` `area/power` `type/feature` `v0.49 — Services & Tools`
  ObType::PowerManager(21) en \System\PowerManager. RAX 42 sys_poweroff eliminado.
  state: closed

- **SYS-GETPID-RM**: sys_getpid eliminado, migrado a Ob `priority/medium` `area/kernel` `type/refactor` `v0.49 — Services & Tools`
  RAX 3 eliminado del SSDT. Usar ob_open + ob_query_info(ProcessId).
  state: closed

- **SYS-FSCK-RM**: sys_fsck eliminado, migrado a Ob `priority/medium` `area/kernel` `type/refactor` `v0.49 — Services & Tools`
  RAX 55 eliminado. Usar ob_query_info(FsckStatus) / ob_set_info(FsckRepair).
  state: closed

- **CONSOLE-256**: Console ANSI 256-color + truecolor `priority/low` `area/kernel` `type/feature` `v0.49 — Services & Tools`
  ESC[38;5;Nm/48;5;Nm y ESC[38;2;R;G;Bm parseados en kernel console.
  state: closed

### v0.49.0 — Service Manager, VFS Hardening, NET Userland

- **VFS-1.1**: Unificar MountManager `priority/high` `area/fs` `type/refactor` `v0.49 — Services & Tools`
  MountManager unificado en VFS.
  state: closed

- **VFS-1.2**: Arreglar ownership ObOpen → VFS `priority/high` `area/fs` `type/bug` `v0.49 — Services & Tools`
  Ownership correcto entre handles Ob y VFS.
  state: closed

- **VFS-1.3**: Stale namespace entry cleanup `priority/high` `area/fs` `type/bug` `v0.49 — Services & Tools`
  ob_remove_by_id() elimina entries huérfanas al destruir ObObject.
  state: closed

- **VFS-1.4**: HandleTable → ObObject consistency `priority/high` `area/fs` `type/bug` `v0.49 — Services & Tools`
  is_valid(), close() guardado, has_ob_object() bugfix.
  state: closed

- **VFS-2.1**: Privatizar NeoFS methods `priority/medium` `area/fs` `type/refactor` `v0.49 — Services & Tools`
  5 métodos cambiados de pub a pub(crate).
  state: closed

- **VFS-2.4**: PageCache drive context `priority/medium` `area/fs` `type/feature` `v0.49 — Services & Tools`
  drive_id en clave PageCache para evitar colisiones entre FS.
  state: closed

- **VFS-4.1**: Device IDs estables `priority/medium` `area/fs` `type/feature` `v0.49 — Services & Tools`
  register() escanea slots libres, índices estables, find_by_name().
  state: closed

- **VFS-4.2**: Hot-unload safety `priority/medium` `area/fs` `type/feature` `v0.49 — Services & Tools`
  IoStack.stale flag, operaciones fallan si stale.
  state: closed

- **VFS-4.3**: Refcount block devices `priority/medium` `area/fs` `type/feature` `v0.49 — Services & Tools`
  refcounts[], acquire/release, remove() protegido.
  state: closed

- **OB-FIX-001**: Socket object_id fix `priority/high` `area/net` `type/bug` `v0.49 — Services & Tools`
  SocketConnect/SocketBind usaban entry.offset roto. Corregido a ob_lookup.
  state: closed

### v0.48.x — NeoFS Stability, DHCP, Registry Persistence

- **DHCLIENT**: DHCP client `priority/high` `area/net` `type/feature` `v0.48 — NeoFS & Registry`
  DHCP Discover/Offer/Request/Ack, arranque automático.
  state: closed

- **REG-HIVE**: Registry hive database `priority/high` `area/registry` `type/feature` `v0.48 — NeoFS & Registry`
  Registry con formato NEOH, celdas, free list, persistencia.
  state: closed

- **REG-PERSIST**: Registry disk persistence `priority/high` `area/registry` `type/feature` `v0.48 — NeoFS & Registry`
  cm_flush_key, dirty tracking, serialización VFS.
  state: closed

- **NET-1-F1-F4**: Ethernet/UDP/ARP/ICMP builders `priority/high` `area/net` `type/feature` `v0.48 — NeoFS & Registry`
  Ethernet, UDP, ARP builders, ICMP Port Unreachable, TCP handshake real.
  state: closed

### v0.47.0 — TCP/IP Stack

- **NET-TCP**: TCP/IP stack completo `priority/high` `area/net` `type/feature` `v0.47 — Networking`
  Stack completo: Ethernet, ARP, IPv4, ICMP, UDP, TCP, e1000 NIC NEM.
  state: closed

### v0.46.x — AHCI NCQ, Timer/Semaphore/Section Objects

- **AHCI-NCQ**: AHCI Native Command Queuing `priority/medium` `area/drivers` `type/feature` `v0.46 — Kernel Objects`
  NCQ con 32 slots, tag-based dispatch, fallback a legacy DMA EXT.
  state: closed

- **OBF-10**: Timer Object `priority/medium` `area/kernel` `type/feature` `v0.46 — Kernel Objects`
  ObType::Timer=15: oneshot/periodic, set, cancel, wait.
  state: closed

- **OBF-11**: Semaphore Object `priority/medium` `area/kernel` `type/feature` `v0.46 — Kernel Objects`
  ObType::Semaphore=14: create, release, wait.
  state: closed

- **OBF-12**: Section Object `priority/medium` `area/kernel` `type/feature` `v0.46 — Kernel Objects`
  ObType::Section=17: create, map_view, unmap.
  state: closed

### v0.44.x — Object Manager Unificado

- **X7**: Object Manager (Ob) unificado `priority/high` `area/object-manager` `type/feature` `v0.44 — Object Manager`
  16 ObTypes, 7 syscalls (RAX 60-66), handles, namespace, seguridad. 28 binarios migrados.
  state: closed

- **OBF-01..06, OBF-09**: Fase 1 Objectification `priority/medium` `area/object-manager` `type/feature` `v0.44 — Object Manager`
  ObInfoClass, ObSetInfoClass, ObType::Thread, ob_create/wait/set_info para Thread.
  state: closed

### v0.43.x — Shell Migration, Input, Virtual Terminals

- **B9**: Shell command migration Ring 0 → Ring 3 `priority/high` `area/shell` `type/refactor` `v0.43 — Shell & Input`
  14+ comandos migrados a .NXE Ring 3. Ring 0 solo mantiene RUN y CRASH.
  state: closed

- **A4.4**: Input subsystem redesign `priority/high` `area/kernel` `type/feature` `v0.43 — Shell & Input`
  InputManager, 4 VT queues, per-VT input routing, Alt+F1-F4 switching.
  state: closed

- **B4.5**: Virtual terminals `priority/high` `area/kernel` `type/feature` `v0.43 — Shell & Input`
  Console state save/restore per VT, framebuffer shadow redraw.
  state: closed

### v0.40.x — SMP Fixes, FSCK, AI-5

- **CB1**: Fix WAIT_PID static mut SMP-unsafe `priority/critical` `area/kernel` `type/bug` `v0.40 — Foundation`
  WAIT_PID migrado de static mut a AtomicU32.
  state: closed

- **CB2**: Fix ISOLATED_REGIONS static mut `priority/critical` `area/kernel` `type/bug` `v0.40 — Foundation`
  ISOLATED_REGIONS migrado a Mutex<[...]>.
  state: closed

- **CB3**: Fix NXL_REGISTRY static mut `priority/high` `area/kernel` `type/bug` `v0.40 — Foundation`
  NXL_REGISTRY migrado a Mutex<[...]>.
  state: closed

- **NFSv2-FSCK**: fsck para NE2 `priority/high` `area/fs` `type/feature` `v0.40 — Foundation`
  Verificación CRC32 de superblock y B-tree, reconstrucción de freelist.
  state: closed

- **AI-5**: libneodos-nxl modularizado `priority/low` `area/kernel` `type/refactor` `v0.40 — Foundation`
  libneodos-nxl/src/ ya usa módulos separados.
  state: closed

- **NET-E1000-NEM-REGRESSION**: Fix e1000 DHCP/ARP regression `priority/high` `area/net` `type/bug` `v0.40 — Foundation`
  MMIO_BASE no establecido antes de init_e1000_hw. Código kernel e1000 eliminado.
  state: closed

- **NET-DNS**: DNS resolver + nslookup `priority/medium` `area/net` `type/feature` `v0.40 — Foundation`
  Stub resolver UDP, caché 64 entradas, nslookup.nxe, DHCP option 6.
  state: closed

### v0.39 — Security & PCIe

- **NT6-SRM**: NT6 Security Reference Monitor `priority/high` `area/security` `type/feature` `v0.39 — Security & PCIe`
  SID, Access Token, ACL/ACE, SeAccessCheck, admin vs user token.
  state: closed

- **PCIe-ECAM**: PCIe ECAM config space `priority/high` `area/drivers` `type/feature` `v0.39 — Security & PCIe`
  MMIO ECAM based on ACPI MCFG, dual path ECAM/PIO, BAR utilities.
  state: closed

- **IOAPIC-MSI**: I/O APIC + MSI-X `priority/high` `area/kernel` `type/feature` `v0.39 — Security & PCIe`
  I/O APIC init from MADT, ISA IRQ routing, PIC disable, MSI-X per-entry config.
  state: closed

- **ANSI-TERM**: ANSI terminal emulator `priority/medium` `area/kernel` `type/feature` `v0.39 — Security & PCIe`
  ANSI escape parser, 16-color palette, UTF-8, box-drawing glyphs.
  state: closed

- **URN-NS**: Unified Resource Namespace (URN) `priority/medium` `area/kernel` `type/feature` `v0.39 — Security & PCIe`
  neodos:// scheme, URN open/read/write/seek, 11 tests.
  state: closed

- **KDRIVE**: Virtual K: drive `priority/medium` `area/kernel` `type/feature` `v0.39 — Security & PCIe`
  Virtual drive exposing processes, drivers, memory stats as read-only files.
  state: closed

### v0.37 — Ring 3 Shell

- **NEOSHELL-R3**: neoshell Ring 3 `priority/high` `area/shell` `type/feature` `v0.37 — Ring 3 Shell`
  Full-featured Ring 3 shell with built-ins, TAB completion, history, PATH dispatch, env vars.
  state: closed

- **DIR-REORG**: Directory structure reorganization `priority/medium` `area/kernel` `type/feature` `v0.37 — Ring 3 Shell`
  NT-style paths: \System, \Programs, \Packages, \Users. Flat driver dir.
  state: closed

- **CORE-BINS**: Core user binaries `priority/medium` `area/shell` `type/feature` `v0.37 — Ring 3 Shell`
  DIR.NXE, HELP.NXE, VER.NXE, DATETIME.NXE, CD.NXE, ECHO.NXE, MEM.NXE, VOL.NXE, TREE.NXE.
  state: closed

- **R3-SYSCALLS**: Ring 3 FS syscalls `priority/high` `area/kernel` `type/feature` `v0.37 — Ring 3 Shell`
  sys_spawn with fd redirection, sys_readdir, sys_mkdir, sys_unlink, sys_rmdir, sys_rename. HANDLE_DIR type.
  state: closed

### v0.35 — NeoInit & APC

- **NEOINIT**: NeoInit PID 1 init process `priority/high` `area/kernel` `type/feature` `v0.35 — NeoInit & APC`
  PID 1 userland supervisor, spawns neoshell, respawn on exit, save/restore mechanism.
  state: closed

- **APC-ENGINE**: APC engine `priority/high` `area/kernel` `type/feature` `v0.35 — NeoInit & APC`
  Per-thread APC queues, kernel/user APCs, alertable wait, IRP completion via APC.
  state: closed

- **SYS-POWEROFF**: sys_poweroff `priority/medium` `area/kernel` `type/feature` `v0.35 — NeoInit & APC`
  Cache flush, EVENT_SHUTDOWN, HAL poweroff chain (QEMU + ACPI + PS/2).
  state: closed

### v0.33 — HAL v0.4

- **HAL-RAW-SAFE**: HAL v0.4 raw/safe split `priority/high` `area/kernel` `type/feature` `v0.33 — HAL v0.4`
  55 inline asm calls confined to hal/raw/. Type-safe wrappers in hal/safe/. Zero asm outside hal/.
  state: closed

### v0.23 — SMP & Event Bus

- **SMP-SUPPORT**: SMP support `priority/high` `area/kernel` `type/feature` `v0.23 — SMP & Event Bus`
  SMP trampoline, per-CPU KPRCB, IPI (reschedule, TLB shootdown, call-function).
  state: closed

- **EVENT-BUS-V2**: Event Bus v2 `priority/high` `area/kernel` `type/feature` `v0.23 — SMP & Event Bus`
  Dual priority queues, event filters, backpressure, dynamic payload, syscall-boundary dispatch.
  state: closed

- **WORK-QUEUE**: Deferred work queues `priority/medium` `area/kernel` `type/feature` `v0.23 — SMP & Event Bus`
  High/low priority lock-free SPSC rings, processed in syscall return and idle loop.
  state: closed

- **PRIORITY-SCHED**: Priority scheduler with aging `priority/high` `area/kernel` `type/feature` `v0.23 — SMP & Event Bus`
  4 priority levels, dynamic time slicing, aging, work stealing, PRI command.
  state: closed

### v0.20 — Page Cache & ACPI

- **PAGE-CACHE**: Global page cache `priority/high` `area/fs` `type/feature` `v0.20 — Page Cache & ACPI`
  512-entry × 4 KB page cache, LRU eviction, dirty write-back, timer-driven flush.
  state: closed

- **ACPI-POWEROFF**: ACPI Poweroff NEM driver `priority/medium` `area/drivers` `type/feature` `v0.20 — Page Cache & ACPI`
  ACPI S5 via PIIX4/ICH9, EVENT_SHUTDOWN, fallback cascade (QEMU + PS/2).
  state: closed

- **PCI-NEM**: PCI NEM driver `priority/high` `area/drivers` `type/feature` `v0.20 — Page Cache & ACPI`
  Standalone NEM v3 PCI driver with Event Bus config service.
  state: closed

- **ATA-NEM**: ATA NEM standalone driver `priority/high` `area/drivers` `type/feature` `v0.20 — Page Cache & ACPI`
  NEM v3 ATA driver, DMA + PIO, NemBlockDevice registration.
  state: closed

### v0.16 — NEM v3 Driver Framework

- **NEM-V3**: NEM v3 driver framework `priority/high` `area/drivers` `type/feature` `v0.16 — NEM v3 Driver Framework`
  NEM v3 format, isolation X4 (16×1 MB slots), ABI version negotiation, dependency resolver.
  state: closed

- **BOOT-LOADER-V2**: Boot loader v2 `priority/high` `area/drivers` `type/feature` `v0.16 — NEM v3 Driver Framework`
  Topological sort by category, ABI validation, dependency graph, symbolic dep resolution.
  state: closed

- **LIBNEODOS**: libneodos user library `priority/high` `area/kernel` `type/feature` `v0.16 — NEM v3 Driver Framework`
  Standard library for Ring 3: syscall wrappers, IO, FS, mem, print macros.
  state: closed

- **HANDLE-TABLE**: Unified handle table `priority/high` `area/kernel` `type/feature` `v0.16 — NEM v3 Driver Framework`
  Per-process handle table, typed handles (STDIN/STDOUT/PIPE/FILE/DEVICE/EVENT), per-handle offset.
  state: closed

- **KOBJ-SYS**: Kernel Object Manager (KOBJ) `priority/high` `area/kernel` `type/feature` `v0.16 — NEM v3 Driver Framework`
  9 KObjTypes, reference counting, 64-slot registry, shell KOBJ command.
  state: closed

### v0.14 — Memory & Scheduling

- **SLAB-ALLOC**: Slab allocator `priority/high` `area/memory` `type/feature` `v0.14 — Memory & Scheduling`
  9 size classes (8-2048 bytes), O(1) alloc/free, linked_list_allocator fallback.
  state: closed

- **PRIORITY-SCHED-V1**: Priority scheduler v1 `priority/high` `area/scheduler` `type/feature` `v0.14 — Memory & Scheduling`
  4 priority levels, round-robin, time slicing, aging, Ring 3 preemption, PRI command.
  state: closed

### v0.08 — Multitasking

- **RING3-PROCS**: Ring 3 process model `priority/high` `area/kernel` `type/feature` `v0.08 — Multitasking`
  User mode processes, per-process user slots, syscall interface (int 0x80), non-blocking RUN, KILL.
  state: closed

### v0.05 — First Kernel

- **UEFI-BOOT**: UEFI bootloader + kernel boot `priority/critical` `area/kernel` `type/feature` `v0.05 — First Kernel`
  UEFI bootloader, GPT parsing, kernel ELF loading, GDT/IDT/PIC, serial, VGA console.
  state: closed

- **ATA-PIO**: ATA PIO driver `priority/critical` `area/drivers` `type/feature` `v0.05 — First Kernel`
  ATA PIO read/write, GPT scan, block device abstraction.
  state: closed

- **FAT32**: FAT32 filesystem `priority/high` `area/fs` `type/feature` `v0.05 — First Kernel`
  FAT32 read/write support for ESP partition.
  state: closed

- **NEOFS-V1**: NeoFS v1 filesystem `priority/high` `area/fs` `type/feature` `v0.05 — First Kernel`
  Custom filesystem with superblock, inodes, directories, basic file operations.
  state: closed

- **SHELL-R0**: Ring 0 shell `priority/high` `area/shell` `type/feature` `v0.05 — First Kernel`
  Kernel-mode shell with 18 commands (DIR, TYPE, CD, MD, RD, COPY, DEL, REN, DATE, TIME, CLS, ECHO, MEM, VOL, VER, HELP, PROMPT, SET).
  state: closed

- **PS2-KEYBOARD**: PS/2 keyboard driver `priority/high` `area/drivers` `type/feature` `v0.05 — First Kernel`
  PS/2 keyboard IRQ, scancode→ASCII translation, US and Spanish layouts.
  state: closed

- **RTC**: RTC driver `priority/medium` `area/drivers` `type/feature` `v0.05 — First Kernel`
  RTC date/time read, DATE/TIME commands.
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
