# Changelog

## v0.48.7 — 2026-07-04

### Added
- **B4.10 NeoInit Registry-driven config** — `userbin/neoinit/`: lee DefaultShell, EnableVT, WaitForNetwork, AutoStartServices desde `\Registry\Machine\System\CurrentControlSet\Services\NeoInit`.
- **AUDIT-1. Registry Ob info classes** — `handler_ob_query_info`/`handler_ob_set_info` ahora soportan RegistryKey, RegistryValue, RegistryCreateKey, RegistryDeleteKey, RegistrySetValue, RegistryDeleteValue.
- **AUDIT-2. libneodos Ob enum sync** — `ObInfoClass` completo (22 variantes, derives), `ObSetInfoClass` como enum (27 variantes), `sys_ob_set_info` tipado.
- **AUDIT-10. Security descriptor via ObSetInfo** — Handler para `ObSetInfoClass::Security` parsea ACL desde buffer y llama `ob_set_security`.
- **AUDIT-4. DPC overflow tracking** — `DPC_DROPPED_COUNT` global, 2 nuevos tests de cobertura.

### Changed
- **AUDIT-3. MAX_MOUNTS rename** — `fs/vfs.rs`: `MAX_MOUNTS` → `MAX_SUBDIR_MOUNTS` para eliminar ambigüedad.
- **AUDIT-9. Doc fixes** — Corregidas direcciones de kernel link en 4 docs (`0x200000` → `0x4000000`).
- **AGENTS.md** — ToC actualizado, versión y test count actualizados.
- **Tests:** 645 → 646. Fixes: `ahci_ncq_out_of_order` (iterator bug) y `net_arp_cache_static_survives_eviction` (evict_oldest skip static entries).
- Version bumps: libneodos 0.2.1, neoinit 0.3.1, cpuinfo/pri/kill/keyb/coreren/cmdtest 0.1.1.

## v0.48.6 — 2026-07-02

### Added
- **A5.2 VirtIO Block driver (BOOT_DRIVER)** — `src/drivers/virtio_blk.rs` (322 lines) + `src/virtio/` VIO-ARCH layer (`vring.rs` + `transport.rs`). PCI detection 0x1AF4:0x1001 (legacy I/O) and 0x1AF4:0x1042 (modern MMIO, detected but requires MMIO mapping). Legacy virtqueue I/O BAR interface, 256-entry split vring, read/write/flush via virtio-blk protocol, BlockDevice trait. Storage priority: NVMe > VirtIO > AHCI > ATA. 9 tests (4 unit + 5 integration stubs). `-drive if=virtio` / `--virtio` flag for QEMU. I/O verified via QEMU TCG (pending KVM bugfix for PAGE_TABLE_CORRUPTION).

## v0.48.5 — 2026-07-01

### Added
- **VFS-4.1 Device IDs estables** — `BlockDeviceManager.register()` ahora escanea slots libres (índices estables). `find_by_name()` permite búsqueda por nombre. `device_name()` en el trait `BlockDevice`.
- **VFS-4.2 Hot-unload safety** — `IoStack.stale` flag. `read_sectors()`/`write_sectors()`/`with_device()` fallan si `stale`. `mark_stale()` para invalidación externa. `is_valid()` verifica device alive.
- **VFS-4.3 Refcount block devices** — `refcounts[u32; MAX]` en `BlockDeviceManager`. `acquire()`/`release()`/`refcount()`. `remove()` solo si refcount=0. `force_remove()` bypass.
- **OBF-07** — Ya implementado: `ob_err_to_syscall()` en `syscall/mod.rs` con test `ob_err_to_syscall_mapping`.

### Changed
- `IoStack` struct: añadidos `stale: bool`, `acquire_ref()`/`release_ref()`/`is_valid()`. `IoStack::with_partition()` ya no adquiere lock automáticamente (evita deadlock en boot).
- `unregister_nem_block_device()` ahora verifica refcount antes de remover.
- `BlockDeviceManager`: register escanea slots libres (índices estables).

## v0.48.4 — 2026-07-01

### Added
- **NET-1 F1: Ethernet/UDP/ARP TX helpers** — `build_ethernet_frame()` en `ethernet.rs`, `build_udp_datagram()` en `udp.rs`, `arp_resolve()` en `arp.rs` (cache lookup + broadcast ARP request).
- **NET-1 F2: UDP dispatch + ICMP Port Unreachable** — `udp_dispatch()` en `socket.rs` para rutear UDP recibido a sockets. `build_port_unreachable()` en `icmp.rs` (Type 3/Code 3).
- **NET-1 F3: TCP receive dispatch** — `tcp_dispatch()` en `socket.rs`, `parse_tcp_segment()` en `tcp.rs`, handler para TCP en `net_handle_incoming_packet()`.
- **NET-1 F4: TCP real three-way handshake** — `build_tcp_segment()`, `send_tcp_segment()`, `tcp_send_syn_ack()`, `tcp_handle_ack()` en `tcp.rs`. SYN→SYN-ACK→ACK con I/O real sobre NIC.
- **DHCP client** — `dhcp.rs`: cliente DHCP (RFC 2131) con Discover/Offer/Request/Ack. Arranque automático al boot. Renovación de lease.
- **No hardcoded IPs** — kernel arranca desde `0.0.0.0`, DHCP asigna IP automáticamente.

## v0.48.3 — 2026-07-01

### Added
- **VFS-2.4 PageCache drive context** — `make_key()` ahora incluye `drive_id: u8` en bits 56-63 de la clave. Todos los métodos públicos (`read_page`, `get_page_mut`, `peek`, `mark_dirty`, `flush_inode`, `invalidate_inode`, `prefetch`) aceptan `drive_id`. Dos FS (C: y D:) con mismo inode_num ya no colisionan en PageCache.

### Changed
- **VFS-2.1 NeoFS encapsulation** — 5 métodos cambiados de `pub` a `pub(crate)`: `rebuild_bitmap_with_io`, `rebuild_bitmap`, `inode_data_block_count`, `get_inode_block_ptr`, `find_entry_in_directory`. Solo accesibles a través del trait `FileSystem`.
- `NeoDosFs.drive_id` añadido para identificar cada instancia de FS en PageCache.

## v0.48.2 — 2026-07-01

### Added
- **FS-1.1 Dynamic inode allocator** — `InodeCache.inodes` reemplazado de `[Option<Inode>; 256]` a `Vec<Option<Inode>>`. `load_inode()` extiende el Vec según demanda. `find_free_inode()` itera hasta `num_inodes` del superblock. Sin límite fijo de 256 inodos.
- **FS-1.2 Dynamic block bitmap** — `BlockBitmap.bits` reemplazado de `[u8; 320]` a `Vec<u8>` con tamaño = `ceil(num_blocks/8)`. Soportan FS de hasta 40+ MB.
- **FS-1.3 Hardcoded sector offsets eliminados** — Añadido `data_start_sector()` con fórmula `1 + ceil(num_inodes*256/512)`. Reemplazados todos los `200` literales (12 en kernel, 1 en fsck.rs, 19 en create_neodos_image.py, 1 en neodos_fs.py del MCP).
- **NS-1.1 Namespace ownership tracking** — `DirectoryObject.protected` flag y `set_protected()`. `ob_insert_object_checked()` y `ob_create_directory_checked()` verifican protección parental.
- **NS-1.2 Protected root directories** — `\Device`, `\Global`, `\Driver`, `\FileSystem`, `\Ob`, `\Registry`, `\Process`, `\DosDevices` marcados como protegidos en `init_object_namespace()`.
- **CAP_NS_WRITE** — Nueva capability flag (bit 12) para operaciones de namespace write. Añadida a `cap_name()` y `all_cap_names()`.
- **11 new tests** — 7 namespace (protected flag, subdir inheritance, set/clear, internal bypass) + 4 mod.rs (insert_checked rejects protected parent, allows normal, create_directory_checked).

### Changed
- AGENTS.md version bump: v0.48.1 → v0.48.2.

## v0.48.1 — 2026-07-01

### Added
- **VFS-1.3 Stale namespace entry cleanup** — `ob_remove_by_id()` en `src/object/namespace.rs`: busca y elimina namespace entries por ObId. `ob_destroy_object()` y `ob_close_object()` llaman `ob_remove_by_id()` al destruir un ObObject, garantizando que no queden entries huérfanas en el namespace. 3 tests: `vfs_namespace_cleanup_on_destroy`, `vfs_namespace_cleanup_on_close`, `vfs_namespace_no_orphan_on_close_with_refs`.
- **VFS-1.4 HandleTable → ObObject consistency** — `is_valid()` e `is_open_and_valid()` en `HandleEntry` (`src/handle.rs`): verifican que el ObId referenciado sigue vivo en el Object Manager. `close()` solo llama `ob_close_object` si `is_valid()`. Corregido `has_ob_object()` que falsamente trataba STDIN/STDOUT (sentinels) como ObObjects reales. 5 tests: `vfs_ownership_is_valid`, `vfs_ownership_is_valid_after_obj_destroyed`, `vfs_ownership_double_close_safe`, `vfs_ownership_stdio_always_valid`, `vfs_ownership_closed_not_valid`.

### Changed
- Kernel test count: 580 → 588.
- AGENTS.md version bump: v0.48.0 → v0.48.1.
- docs/IMPROVEMENTS.md: VFS-1.3 y VFS-1.4 marcados como completados.

## v0.48.0 — 2026-07-01

### Added
- **Userland Networking Architecture** — `docs/NETWORK_USERLAND_ARCHITECTURE.md`: diseño completo de net.nxl (API userland de red), herramientas NXE (ipconfig, ping, dhcp), migración de NeoInit a Registry, modelo de configuración Registry+NeoFS, sistema de paquetes NeoPkg, y roadmap de implementación (19 tareas pre-1.0, 9 post-1.0). Incluye especificación detallada de gaps del kernel TCP/IP, plan de implementación por fases (transmit path → receive path → UDP → TCP real), y respaldo arquitectónico para la integración Ob + networking userland.

## v0.47.1 — 2026-06-30

### Added
- **USR-001 SAM database** — `src/security/sam.rs`: `SamDatabase` + `SamEntry` con username, SID, flags (admin/disabled/locked), full_name, comment. Formato binario (cabecera 16B + entries de longitud variable). `parse_sam()`/`serialize_sam()` para persistencia. 11 tests (roundtrip, max 64 entries, case-insensitive lookup, flags, errores).
- **USR-002 Token NT extendido** — `src/security/token.rs`: `Token` ahora incluye `groups: Vec<Sid>`, `privileges: u64` (12 flags SE_*_PRIVILEGE), `session_id: u32`. Métodos `add_group()`, `is_in_group()`, `has_privilege()`, `enable_privilege()`, `disable_privilege()`, `inherit_from()`. Admin token recibe `SE_ADMIN_PRIVILEGES`, user token solo `SE_CHANGE_NOTIFY`. `Token::inherit_from()` clona SID/groups/privileges/session_id del padre en procesos hijo.
- **test_false! macro** — Nueva macro de testing en `src/testing.rs`.
- **SAM docs** — `AGENTS.md` actualizado con tabla SAM y Token extendido.

### Changed
- `Token` ya no implementa `Copy` (solo `Clone`) por `Vec<Sid>`.
- 6 call sites actualizados: `*DEFAULT_ADMIN_TOKEN` → `.clone()`, `ep.token` → `.clone()`.
- Security tests: 12 → 23.

## v0.47.0 — 2026-06-28

### Added
- **B3.1 D9 Network I/O** — `\Device\Tcp` y `\Device\Udp` como objetos de dispositivo en el namespace NT5. `ObType::Socket=18` con creación vía `ob_create(Socket)` y operaciones vía `ob_set_info` (SocketConnect/Bind/Listen/Send/Close) y `ob_query_info` (SocketInfo/SocketAddr/TcpStatus/NicInfo).
- **B3.2 E3 TCP/IP stack** — Stack completo en kernel: Ethernet (14B header, FCS), ARP (64-entry cache, 300s timeout, static entries, request/reply), IPv4 (20B header, checksum, TTL), ICMP (echo request/reply con checksum), UDP (pseudo-header checksum), TCP (11-state machine, conn lifecycle, send/recv buffers).
- **e1000 NIC driver (kernel stub)** — Intel e1000 (82540EM/82543GC/82545EM/82574L) con detección PCI, MMIO BAR0, ring buffers RX(32)/TX(16), DMA polling, init en Phase 3.88.
- **e1000.nem (NEM v3 standalone driver)** — `drivers/e1000/` —  SYSTEM driver con PCI probe, MMIO init, registro via `hst_register_network_device`, callbacks send/poll para integración con kernel NIC registry. Compilación en `build.sh` y empaquetado en `create_neodos_image.py`.
- **hst_register_network_device/hst_unregister_network_device** — Nuevas HST exports para que drivers NEM registren NICs en el kernel via callbacks C ABI. Incluye bridge `NemNetworkDevice` que implementa `NetworkInterface`.
- **KWait network reasons** — `WaitReason::SocketRead/SocketConnect/SocketAccept` con magics `0x0009_*` para bloqueo en operaciones de red.
- **17 kernel tests** — MAC/IPv4 addr, ARP cache, TCP lifecycle, ICMP echo reply, socket manager, UDP header, NIC registry.
- **EVENT_NETWORK_PACKET=17** — Nuevo tipo de evento para notificar paquetes de red recibidos.

### Changed
- **ObType::Socket=18** añadido al Object Manager.
- **ObInfoClass** — SocketInfo(17), SocketAddr(18), TcpStatus(19), NicInfo(20).
- **ObSetInfoClass** — SocketConnect(18), SocketBind(19), SocketListen(20), SocketSend(21), SocketClose(22).
- **Kernel version bump** — v0.46.2 → v0.47.0.
- **Hotreload ResourceType** — añadido NetworkDevice=1.
- **AGENTS.md** — v0.47 con documentación completa del subsistema de red.

## v0.46.2 — 2026-06-27

### Added
- **A5.3 AHCI NCQ** — Native Command Queuing en AHCI: FPDMA QUEUED READ (0x60) / WRITE (0x61), tag-based dispatch con 32 slots, batch issue concurrente, fallback a legacy DMA EXT sin NCQ.

### Removed
- **`driver_loader.rs` (128 líneas)** — Eliminado del kernel. LOADNEM/UNLOADNEM ya estaban migrados a `loadnem.nxe` (Ring 3, `ob_create(Driver)`/`sys_driver_unload`). NEMLIST migrado a `ndreg.nxe LIST` (Ring 3, `ob_query_info(Drivers)`). La lógica de carga manual ya residía en `nem/loader.rs` + `boot_loader/` + `hotreload.rs`. Todas las funciones eran dead code sin callers.
  - `irp::IrpTagMap`: per-device [Option<IrpId>; 32] con alloc_tag/free/lookup para dispatch out-of-order.
  - `boot_ahci.rs`: detección NCQ vía IDENTIFY DEVICE word 76 bit 8, `ncq_batch_xfer()` batch de 32 comandos, `ncq_submit_irp_batch()` batch IRP, tag-based `poll_irp()`.
  - `drivers/ahci/src/lib.rs` (NEM): per-slot buffers (32×2), NCQ path en read/write, `ahci_ncq_batch_read` export, detección NCQ por puerto.
  - 5 tests: 32 concurrent dispatch, tag-based completion, fallback, out-of-order, stress 100 cycles.

## v0.46.1 — 2026-06-27

### Added
- **NeoMem v0.1** — Nueva herramienta de diagnóstico de memoria (`neomem.nxe`) que reemplaza `mem.nxe`. Muestra memoria física, kernel heap, memoria de usuario y estadísticas de paginación.
- **MemoryStats extendido** — `MemoryStats` struct ampliado de 6 a 15 campos: kernel_heap_total/used/free, user_memory_total/used/free, total/free/used_pages. Backward compatible via size-based dispatch en handler.
- **Slab allocator stats** — `SlabAllocator::usage()` expone páginas, capacidad, objetos asignados y bytes usados.
- **Buddy allocator total_frames()** — Nueva API pública para obtener el total de frames gestionados.
- **used_heap_slots()** — Contador de slots de heap de usuario activos en `paging.rs`.
- **System Tools roadmap** — Nueva sección en `docs/IMPROVEMENTS.md` con hoja de ruta completa del ecosistema de herramientas administrativas (NeoMem, NeoTop, NeoTask, NeoCfg, NeoLog, NeoStat, NeoFS, NeocCtl, NeoDebug).

### Fixed
- **GPF handler** — Añadida detección de `cs=0x8 rip=user` (contexto corrompido en salida de proceso). Si el RIP está en la ventana de usuario (0x400000-0x2400000), se trata como excepción de usuario y se mata el proceso en lugar de panic del kernel. El fix previene el kernel panic cuando el scheduler restaura un CS incorrecto tras `sys_exit`.
- **OB-046 process lifecycle** — `cleanup_terminated_process(pid)` era llamado en `handler_ob_wait` inmediatamente tras `kwait_block()`, pero `kwait_block` retorna antes del context switch — el hijo (recién creado, Ready en run queue) era destruido antes de ejecutarse. Fix: handler_exit difiere reciclaje del EPROCESS vía work_queue; handler_ob_wait hace check-and-block atómico bajo scheduler lock para evitar race condition.
- **handler_close pipe ref leak** — `handler_close` no decrementaba `PIPE_MANAGER.dec_read_ref`/`dec_write_ref` para pipe handles, causando leak de buffers al hacer close normal. Fix: añadidos decrementos de ref antes de marcar handle como closed.
- **Thread ObType::Process → ObType::Thread** — `add_ring3_process` y `add_thread_to_process` registraban kthread como `ObType::Process` en vez de `ObType::Thread`.
- **Stress test** — `scripts/stress_300.py` añadido: 300 comandos variados sobre NeoShell sin crash.

### Changed
- **userbin/mem/** → **userbin/neomem/** — El directorio del binario se renombró. `mem.nxe` reemplazado por `neomem.nxe`.

## v0.46.0 — 2026-06-27

### Added
- **Timer Object (OBF-10)** — `ObType::Timer=15` completado. `ob_create(Timer, period_ms)` via RAX 61, `ob_set_info(TimerStart/TimerCancel)` via RAX 63, `ob_wait(Timer)` via RAX 65. Timer manager con soporte oneshot y periódico, integrado con el tick del timer (HPET/PIT a 1 KHz). 6 tests.
- **Semaphore Object (OBF-11)** — `ObType::Semaphore=14` completado. `ob_create(Semaphore, initial_count, max_count)` via RAX 61, `ob_set_info(SemaphoreRelease)` via RAX 63, `ob_wait(Semaphore)` via RAX 65. Semaphore manager con wait decrement y release con wake de waiters. 8 tests.
- **Section Object (OBF-12)** — `ObType::Section=17` completado. `ob_create(Section, size, prot)` via RAX 61, `ob_set_info(SectionMapView/SectionUnmapView)` via RAX 63. Mapea vistas en la región mmap (0x21000000+), con split de 2 MB pages y cleanup automático en destroy. 5 tests.
- **KWait Semaphore variant** — `WaitReason::Semaphore { sem_id }` con `MAGIC_SEMAPHORE_BASE=0x0008_0000`. Timer variant cambiada de `timeout_ms` a `timer_id` para soporte multi-instancia.
- **ObSetInfoClass extensions** — `TimerStart=10`, `TimerCancel=11`, `SemaphoreRelease=12`, `SectionMapView=13`, `SectionUnmapView=14`.
- **32 nuevos tests** — timer (6), semaphore (8), section (5), kwait timer+semaphore (4), ob init (1). Total: 560 tests.
- **Section mapped pages** — usa `mmap_alloc_page()` en región 0x21000000+ con split de 2 MB pages automático.

### Changed
- **pipe.rs movido** — de `src/pipe.rs` a `src/object/pipe.rs` como submodule de `object/`. Todas las referencias actualizadas de `crate::pipe::` a `crate::object::pipe::`.
- **Tres nuevos módulos** — `src/object/timer.rs`, `src/object/semaphore.rs`, `src/object/section.rs` con managers + ObOperations + test suites.
- **Kernel version bump** — v0.44.0 → v0.46.0.
- **MAX_TESTS** — 530 → 560.
- **ObSetInfoClass** — añadidos 5 nuevos valores enum (TimerStart, TimerCancel, SemaphoreRelease, SectionMapView, SectionUnmapView).

### Architecture
- **Fase 2 Objectification completada** — Timer, Semaphore, Section como ObObjects con lifecycle gestionado por el Object Manager (ObOperations::on_destroy para cleanup automático).
- **Timer integrado con timer tick** — `object::timer::tick()` llamado desde `timer_handler_inner` en cada interrupción de timer.
- **Sección mapea en mmap region** — las vistas de sección se alojan en la región mmap compartida, con soporte de split de páginas de 2 MB y TLB shootdown.

## v0.44.7 — 2026-06-27

### Added
- **ObInfoClass enums completados** — `CpuInfo=7`, `ReadContent=15`, `VolumeLabel=16` añadidos a `ObInfoClass` (kernel + libneodos).
- **ObSetInfoClass enums completados** — `ProcessTerminate=4`, `VfsRename=6`, `WriteContent=7`, `SetCwd=8`, `SetVolumeLabel=9` añadidos.
- **KWait integration in waitpid** — `handler_waitpid` now uses `kwait_block(ChildExit)` instead of busy-loop `sti; hlt`, enabling proper blocking and wake via `ChildExit` magic.
- **SSDT ABI v7 cleanup** — Removed dead syscall slots (RAX 14, 15, 50) from SSDT and `SyscallNum` enum. `validate_abi()` simplified — no more reserved slot checks. 32 active handlers down from 40.
- **Exit-to-kernel wakes KWait waiters** — `handler_exit` now broadcasts `WaitReason::ChildExit` to any thread blocking on the exiting PID, enabling proper process join.

### Changed
- **Kernel handlers use enum variants** — `handler_ob_query_info` and `handler_ob_set_info` match arms replaced from raw `0..16` literals to `_ if info_class == ObInfoClass::Name as u32 =>` pattern guards.
- **libneodos ABI_VERSION 6→7** — Dead wrappers removed: `sys_writefile`, `sys_chdir`, `sys_chdir_parent`, `sys_readdir`, `sys_mkdir`, `sys_unlink`, `sys_rmdir`, `sys_rename`, `sys_waitpid`. AbiTable struct trimmed from 62→46 fields.
- **libneodos-nxl modularized** — `libneodos-nxl/src/main.rs` split into separate modules (`syscall.rs`, `io.rs`, `fs.rs`, `process.rs`, `mem.rs`, `error.rs`). Dead nxl_sys_* function stubs removed.
- **NXL AbiTable v7** — Mirrors libneodos: removed dead syscall entries (`sys_writefile`, `sys_pipe`, `sys_dup2`, `sys_waitpid`, `sys_chdir`, `sys_chdir_parent`, `sys_readdir`, `sys_mkdir`, `sys_unlink`, `sys_rmdir`, `sys_rename`). Fields trimmed from 62→46.
- **AGENTS.md** — Syscall table updated with "Estado" column. "Estado Objectification" table added. User binaries descriptions updated to Ob API.
- **docs/ARCHITECTURE.md** — KOBJ, LOADNEM, NDREG descriptions updated to Ob API.
- **docs/IMPROVEMENTS.md** — Items 117-121 updated. AI-5 marked completed. "Objectification Roadmap" section added (~190 lines).
- **docs/SYSCALLS.md** — ABI v7: removed dead syscalls, documented all remaining with correct ABI.

### Removed
- **Dead syscall handlers** — `handler_ndreg` (RAX 50), `Ioctl` (RAX 14), `RegisterDevice` (RAX 15) removed from SSDT.
- **Dead structs** — `KObjEntryRaw`, `DriveInfoRaw`, `DriverInfoRaw` moved to local scope near their sole users in `handler_ob_query_info`.
- **libneodos wrappers (9)** — `sys_writefile`, `sys_chdir`, `sys_chdir_parent`, `sys_readdir`, `sys_mkdir`, `sys_unlink`, `sys_rmdir`, `sys_rename`, `sys_waitpid`.
- **Thread Object via Ob API** — `ob_create(Thread)` crea KTHREAD, `ob_wait(Thread)` via KWait, `ob_set_info(ThreadPriority)` para thread específico. RAX 22/23 eliminados del SSDT. `libneodos::ob_thread_create/join/set_priority` wrappers.
- **cpuinfo.nxe** — ahora muestra PROCESS INFORMATION (PID, PPID, prioridad, threads, estado) via `ob_open("\Ob\Process\{pid}")`.
- **neotop.nxe** — monitor de sistema Ring 3: lista procesos con PID/PPID/prioridad/threads/estado + barra de memoria vía console.nxl. Modo `/W` watch con refresco cada ~1s. Inode 67.
- **libmath-nxl modularizado** — `src/main.rs` dividido en `main.rs` (entry+export table) + `math.rs` (funciones puras + `MathAbiTable`). Misma ABI, mismo binario.

## v0.44.5 — 2026-06-26

### Added
- **libconsole-nxl** (`console.nxl`, inode 64) — reusable Ring 3 console library: readline, history (32-entry circular buffer), TAB completion handler registry, progress bars. Export table v2.
- **libneodos::console** — lazy-loaded wrapper (`libneodos/src/console.rs`) for console.nxl. Provides `read_byte()`, `history_add/prev/next/reset/get_count/get_entry`, `register_completion`, `progress_*`.
- **progress.nxe** — standalone progress bar demo binary (inode 65).
- **ANSI escape support** — CUU (ESC[A), CUD (ESC[B), CUF (ESC[C), CUB (ESC[D), CHA (ESC[G) in kernel console (`neodos-kernel/src/console.rs`).
- **NXL reuse check** — `nxl_load()` returns existing base if NXL already loaded, preventing double-load on repeated `sys_loadlib`.
- **Serial flush()** — `SerialPort::flush()` waits for transmitter empty (LSR bit 6). Called after every `write_str`.
- **Syscalls 67–76 documented** — `sys_ob_logon` through `sys_ob_consent_response` with architecture rule in AGENTS.md.

### Changed
- **neoshell refactored** — history (↑/↓) and TAB completion migrated from inline code to `console.nxl`. Shell still handles echo/display directly (proven reliable). Internal `history` arrays removed.
- **Serial FIFO disabled** — FCR set to 0x06 (disable FIFO, clear TX+RX) instead of 0xC7 (14-byte threshold) to prevent serial output buffering loss in QEMU.
- **Scheduler aging** — `serial_println!` removed from timer ISR to eliminate `[SCHED]` serial log interleaving.

### Fixed
- **Serial output loss in QEMU** — small writes (<150 bytes) no longer lost due to FIFO buffering. `flush()` ensures transmitter is empty before releasing lock.

## v0.44.3 — 2026-06-26

### Added
- **A4.4 Input Subsystem Redesign** — `src/input/` directory with `InputManager`, 4 VT queues (`VtInputQueue`), per-VT input routing. `switch_vt()` via Alt+F1-F4 keyboard handler. Per-process `vt_num` in EPROCESS, inherited from parent.
- **B4.5 Virtual Terminals** — Console state save/restore per VT (`ConsoleState`), framebuffer shadow redraw on switch (`VtShadowBuffer`). `\Global\Info\VtInfo` Ob object for reading/setting VT number.
- **Syscall 11 (readfile) restored** — Re-registered in SSDT for NXL `sys_readfile` compatibility.
- **8 VT tests** — input_vt_switch_framebuffer, input_vt_independent_queues, input_vt_rapid_switching, input_4vt_concurrent_stress, input_event_bus_dispatch_vt, vt_switch_alt_f1_f2, vt_independent_input, vt_framebuffer_swap.
- **docs/AUDIT_REPORT.md** — Comprehensive architectural audit covering 11 areas: architecture, kernel core, syscalls, Ob, URN, userland, drivers, VFS, documentation, testing. Identifies 3 critical SMP-unsafe bugs, 7 architectural issues, and 4 documentation gaps.

### Changed
- **`src/input.rs`** → `src/input/` directory (mod.rs, vt.rs, manager.rs)
- **`handler_read`** reads from per-VT queue based on process's `vt_num`
- **Banner shows `[VTn]`** — NeoShell version display includes current VT number

## v0.44.2 — 2026-06-26

### Fixed
- **exit_to_kernel race** — `request_exit_to_kernel()` ahora solo se activa cuando `pid == current_wait_pid()`. Evita que procesos hijo (ej. help.nxe, ndreg.nxe) al hacer `sys_exit` activen `exit_to_kernel` y hagan creer a NeoInit que el shell terminó. El shell ya no respawnea tras cada comando.
- **handler_ob_create(PROCESS) path** — Se añadió `strip_prefix("\\Global\\FileSystem\\")` antes de llamar `vfs.resolve_path()`. Los comandos .NXE ya no daban "Bad command or file name" cuando el shell los invocaba con paths Ob completos.
- **ob_set_info(VfsRename) namespace** — Al renombrar, ahora limpia la entrada namespace anterior y crea la nueva. Usa `buf_size` en vez de `copy_user_string` (sin null terminator). Previene entradas huérfanas en `\Global\FileSystem\`.
- **handler_ob_destroy stale namespace** — Remueve la entrada namespace después de destruir un objeto VFS. `dir_exists`/`file_exists` ya no encuentran entradas obsoletas.
- **create_directory no auto-crea padres** — Si un directorio padre no existe, retorna `OB_NOT_FOUND` en vez de crearlo automáticamente.
- **vfs::rename leaf extraction** — Extrae solo el nombre de archivo (leaf) del `new_name`, en vez de pasar el path completo al FS subyacente.
- **rename_file OOB bounds** — Los límites `min(255)` cambiados a `min(DIR_ENTRY_SIZE - 7)`. Previene `range end index 518 out of range for slice of length 512` al renombrar archivos.
- **ObType::Driver queryable** — `handler_ob_query_info(class=12)` ahora acepta `ObType::Driver` individual (no solo el bulk `\Global\Info\Drivers`).
- **Test corrections** — `syscall_table_validation_boot` actualizado con ASSIGNED/RESERVED correctos. `ob_set_info_name_too_long` espera 64 (OB_NAME_LEN=128).

## v0.44.2 — 2026-06-23

### Added
- **OB-015 (legacy paths via Ob)** — `sys_open` ahora convierte `C:\...` paths a `\Global\FileSystem\C:\...` y resuelve mediante `ob_open_path`, aplicando SeAccessCheck.
- **OB-018 (URN todos schemes via Ob)** — `urn_open` implementado para todos los 4 schemes (file, device, registry, kobj) mediante `ob_open_path`. Namespace `\Registry` creado en `init_object_namespace`.
- **OB-020 (ObWait multi-type)** — `handler_ob_wait` extendido para soportar `PipeRead`, `Event`, `Timer` además de `ChildExit`. Quick non-blocking peek para pipes.
- **OB-025 (URN frontend completo)** — File scheme migrado de VFS directo a `ob_open_path("\Global\FileSystem\...")`. Registry/kobj schemes implementados via Ob namespace.
- **OB-030 (SeAccessCheck en todas las rutas)** — `check_legacy_path_access()` helper añadido. Security checks en `sys_spawn` (ACCESS_EXECUTE), `sys_mkdir` (ACCESS_WRITE), `sys_unlink`/`sys_rmdir` (ACCESS_DELETE), `sys_rename` (ACCESS_WRITE|DELETE).
- **OB-031 (KWait full integration)** — Pipe blocking (`block_current_for_pipe`, `wake_pipe_readers`) y ThreadJoin (`block_current_for_thread`, `wake_thread_joiner`) migrados de ad-hoc magic numbers a KWait. `handler_thread_join` refactorizado.
- **OB-046 (processes as ObObjects)** — `Eprocess.ob_id` añadido. Procesos registrados como `ObType::Process` en `\Process\<pid>`. Cleanup en `recycle_terminated` y `kill_pid`.

### Changed
- **URN file scheme** — Ya no usa VFS directamente; todo resuelve via `ob_open_path` con security checks.
- **Scheduler** — `add_ring3_process` registra EPROCESS en Ob namespace. `recycle_terminated`/`kill_pid` limpian ObObject.
- **Pipe blocking** — Usa `kwait_block/kwait_wake` con `WaitReason::PipeRead` en vez de ad-hoc `0xFFFF_0000` magic.
- **Thread join** — Usa `kwait_block/kwait_wake` con `WaitReason::ThreadJoin` en vez de ad-hoc `0x8000_0000` magic.
- **Syscall table test** — Actualizado: RAX 48, 51, 52 marcados como RESERVED (migrados a Ob).
- **Test count** — `pipe_block_current_wake` → `pipe_block_current_wake_kwait` (usa KWait magic).

## v0.44.1 — 2026-06-23

### Added
- **OB-020 (sys_ob_wait RAX=65)** — `handler_ob_wait` con integración KWait. Soporta wait simple sobre objetos Process (ChildExit). `kwait_wake` llamado desde scheduler `wake_waiters`.
- **libneodos sys_ob_wait** + NXL export.

### Changed
- **OB-024 (HandleEntry cleanup)** — Eliminados campos `kind`, `id`, `extra` de `HandleEntry`. Stdio fds usan ObId sentinel. Pipe read/write discriminados via offset (0=read, 1=write). Drive index almacenado en flags del ObObject. Todos los consumidores migrados a métodos helper (`is_pipe_read()`, `obj_type()`, `native_id()`, `drive()`).
- **OB-001/OB-010/OB-011/OB-012/OB-013/OB-014 (Object Manager syscalls)** — `sys_ob_open` (RAX=60), `sys_ob_create` (61), `sys_ob_query_info` (62), `sys_ob_set_info` (63), `sys_ob_enum` (64) fully implemented and callable from user mode.
- **libneodos Ob API** — `ObBasicInfo`, `ObEnumEntry`, `ObProcessInfo` structs + `sys_ob_open/create/query/set/enum` wrappers + `ob_access` constants.
- **libneodos-nxl Ob exports** — 5 new AbiTable entries + version 5.
- **`ob_open_path` auto-create directories** — `src/object/mod.rs`: when a namespace path is a valid directory without an object entry, an `ObObject` is created on-the-fly and inserted, enabling `ObOpen` on namespace directories.
- **`ob_is_directory()`** — `src/kobj/namespace.rs`: public method to check if a namespace path exists as a directory node.
- **ProcessTerminate info class** — `handler_ob_set_info` class 4 terminates a process via `ObSetInfo(fd, ProcessTerminate)`.
- **`ps.nxe` migrado a Ob** — usa `ObOpen("\Ob\Process")` + `ObEnum` + `ObQueryInfo(Process)` para mostrar datos reales (PID, PPID, prioridad, thread_count, estado).
- **`kobj.nxe` migrado a Ob** — usa `ObOpen("\Ob")` + `ObEnum` para mostrar el namespace Ob jerárquico.
- **`pri.nxe` migrado a Ob** — usa `ObOpen("\Ob\Process\eproc/<pid>")` + `ObSetInfo(ProcessPriority)`.
- **`kill.nxe` migrado a Ob** — usa `ObOpen(...)` + `ObSetInfo(ProcessTerminate)`.

### Changed
- `libneodos/src/syscall.rs`: all Ob wrappers use safe asm macros with temp register copy to prevent register overlap in PIE mode.

## v0.44.0 — 2026-06-23

### Added
- **ASLR v1 (Address Space Layout Randomization)** — PIE user binaries (ET_DYN) loaded at random slot base addresses within the user window (0x400000..0x2400000, 32 slots × 128 KB).
- **RDRAND entropy source** — `src/hal/raw/cpu.rs`: `raw_rdrand()` + `raw_has_rdrand()` inline asm. Safe wrapper `hal::rdrand()` with 10-retry loop and TSC fallback.
- **PIE ELF loading** — `src/elf.rs`: `load_offset: u64` parameter on `load_elf()`, `Elf64Dyn`/`Elf64Rela` structs, `find_rela_dyn()`/`apply_rela_relocations()` helpers, `R_X86_64_RELATIVE` relocation support (3 entries in neoshell binary).
- **ASLR slot allocator** — `src/arch/x64/paging.rs`: `alloc_user_slot()` picks random free slot via RDRAND/TSC, `free_user_slot()` for error cleanup.
- **PIE user binaries** — All 30+ user binaries compiled as position-independent (`. = 0` in linker script, `relocation-model=pie`, `-pie` flag). `user.ld` base address changed from 0x400000 to 0.
- **Per-slot process loading** — NeoInit and `handler_spawn` allocate random slots; no save/restore needed (each process lives in its own slot).
- **5 new ELF tests** — 4 PIE-specific (load with offset, accept zero vaddr with offset, offset out of user window, overlapping segments with offset) + 1 additional coverage.

### Changed
- **ELF loader** — `load_elf()` now takes `load_offset: u64` parameter (backward compatible, existing callers pass 0).
- **User window slot allocation** — `alloc_user_slot()` uses RDRAND (entropy) with TSC fallback instead of first-fit sequential.
- **Shell `spawn` flow** — `handler_spawn` allocates slot before loading, passes `slot.code_base` as load_offset, applies RELA relocations at load time.
- **Cmdtest loading** — `main.rs` loads cmdtest via slot instead of raw offset 0, fixing PIE loading for user-mode tests.

### ABI Notes
- ASLR v1 uses shared identity-mapped page table (single CR3) — no per-process page tables in v1.
- User window: 0x400000..0x2400000 (32 MB), 32 slots of 128 KB each (64 KB code + 64 KB stack).
- Heap slots extend from 0x10000000 (16 × 2 MB), unchanged.

---

## v0.43.0 — 2026-06-23

### Added
- **SeAccessCheck NT-compatible (ACE order NT-correct)** — `src/security/access.rs`: `check_dacl()` two-pass: Deny ACEs evaluated first, Allow ACEs second. `src/security/acl.rs`: `insert_ace_canonical()` maintains canonical deny-first order. 3 new tests.
- **sys_poll (RAX=59)** — `handler_poll()` with PollFd struct (fd, events, revents). POLLIN/POLLOUT/POLLHUP/POLLERR. Supports stdin, stdout/stderr, pipes, files, dirs. User-level syscall.
- **Pipe/IRP protocol freeze** — FROZEN ABI v0.43 markers in `pipe.rs` and `irp/mod.rs` with documented protocol invariants.
- **Pipe poll helpers** — `pipe_peek_read_ready()`, `pipe_peek_write_closed()`, `pipe_peek_read_closed()` public functions for non-blocking pipe state inspection.
- **509 kernel tests** (+8 from v0.42: 3 security, 5 frozen ABI).

### Changed
- **SeAccessCheck**: ACE iteration now NT-correct — all Deny ACEs processed first regardless of position in ACL.

### ABI Freeze (v0.43)
- Pipe protocol: alloc/read/write/refcount/blocking magic (0xFFFF_0000)
- IRP protocol: pool (64 slots), completion/dispatch/blocking magic (0xAAAA_0000)
- Driver error codes (12 existing codes frozen)
- Pipe refcount protocol (dup2/close behavior)

---

## v0.42.0 — 2026-06-22

### Added
- **B9.9 FSCK syscall (RAX=55)** — `sys_fsck`: Run filesystem integrity check from Ring 3 via `userbin/fsck/` → `fsck.nxe`. Wrapper en `libneodos::syscall::sys_fsck()`. Retorna `FsckStats` struct.
- **B9.11 NDREG syscall (RAX=56)** — `sys_driver_enum`: Enumerate registered NEM drivers from Ring 3 via `userbin/ndreg/` → `ndreg.nxe`. Soporta LIST, SHOW, QUERY, RUNTIME.
- **B9.12 LOADNEM syscalls** — `sys_driver_load` (RAX=57) + `sys_driver_unload` (RAX=58, admin): Load/unload NEM drivers from Ring 3 via `userbin/loadnem/` → `loadnem.nxe`.
- **Ring 0 cleanup**: Eliminados comandos CALL, NDREG, LOADNEM, NEMLIST, FSCK del kernel shell. Solo RUN y CRASH permanecen en Ring 0.
- **Unified Wait Engine (KWait)** — `src/kwait/mod.rs`: Nueva abstracción de espera bloqueante que unifica todos los mecanismos ad-hoc. `WaitReason` enum con 7 variantes (PipeRead, IrpComplete, ThreadJoin, ChildExit, Event, Timer, Alertable). `kwait_block(reason)` / `kwait_wake(reason)` API. Magic encoding único por tipo. 10 tests.
- **ABI Freeze v0.42** — `src/abi_freeze.rs`: Sistema de verificación de interfaces congeladas. Verifica valores de 15 event types (0–15), 12 capability flags (bits 0–11), y KWait magic tags. Llamado en boot Phase 3.9 (panic si hay violación). 4 tests.
- **ABI freeze markers** — `src/eventbus/mod.rs`: Event types 0–15 marcados FROZEN v0.42. `src/drivers/caps.rs`: Capability flags bits 0–11 marcados FROZEN v0.42. `src/interrupts/ioapic.rs`: API pública marcada FROZEN v0.42.
- **HandleEntry full Object Manager integration** — `src/handle.rs`: Todos los constructores de HandleEntry (`pipe_read`, `pipe_write`, `file`, `device`, `event`, `dir`) ahora crean objetos en el Object Manager via `ob_create_object()`. Nuevo método `HandleEntry::close()` que llama `ob_close_object()`. Helper methods `is_open()`, `is_pipe()`, `is_file()`, `is_dir()`, etc.
- **pci.nem ECAM MMIO** — `drivers/pci/src/lib.rs`: Migrado de legacy PIO (0xCF8/0xCFC) a ECAM MMIO. 3 nuevas exportaciones `hst_ecam_is_active/read_dword/write_dword` en `v3loader.rs` con check `CAP_MMIO`. `driver_init()` detecta ECAM al arranque, fallback PIO transparente. QEMU actualizado a `-machine q35` para ECAM real. Tests de máquina actualizados para Q35.

### Changed
- `Cargo.toml`: version `0.40.0` → `0.42.0`
- `src/handle.rs`: HandleEntry.set_object_id() añadido para migración progresiva. Todos los constructores establecen `object_id` automáticamente.
- `src/eventbus/mod.rs`: Comentarios ABI FROZEN, valores 0–15 protegidos.
- `src/drivers/caps.rs`: Comentarios ABI FROZEN, bits 0–11 protegidos.
- `src/interrupts/ioapic.rs`: Comentarios ABI FROZEN en cabecera del módulo.

### ABI Freeze (v0.42)
| Interfaz | Estado | Notas |
|----------|--------|-------|
| Event types 0–15 | FROZEN | No reasignar. Añadir nuevos en 16+. |
| Event struct (56 bytes) | FROZEN | No cambiar layout repr(C). |
| Capability flags (bits 0–11) | FROZEN | No reasignar bits. Añadir en bit 12+. |
| IOAPIC public API | FROZEN | init, is_active, mask/unmask_irq, route_pci_vector, eoi_irq. |
| KWait WaitReason variants | FROZEN | No reordenar/eliminar. Añadir al final. |

## v0.41.0 — 2026-06-22

### Added
- **OB-001. Módulo base Object Manager** — `src/object/mod.rs`, `src/object/types.rs`: `ObObject`, `ObObjectTable`, `ObOperations` trait, `ObType` (16 tipos), `ObId`, `ObError`. API: `ob_create_object`, `ob_destroy_object`, `ob_lookup`, `ob_open_object`, `ob_close_object`, `ob_reference`, `ob_dereference`, `ob_enum_snapshot`. 10 tests.
- **OB-002. HandleEntry object_id** — `src/handle.rs`: nuevo campo `object_id: u64` en `HandleEntry`. Inicializado a 0 en todos los constructores. Migración progresiva hacia Object Manager.
- **OB-003. KOBJ → ObObjectTable** — `src/kobj/mod.rs`: refactorizado para delegar en `ObObjectTable` internamente. `kobj_register()` crea `ObObject`, `kobj_unregister()` lo destruye. API pública sin cambios. 8 tests legacy intactos.
- **OB-004. sys_close como primer wrapper Ob** — `src/syscall/mod.rs`: `handler_close` refactorizado para llamar a `ob_close_object(handle.object_id)` eliminando el `match entry.kind`. `ob_close_object` auto-destroy al llegar a refcount 0. Tests: 4 (ob_close_object_auto_destroy, ob_close_object_keeps_alive_with_refs, handler_close_file, handler_close_pipe).
- **OB-005. init_object_manager en boot phase** — `src/object/mod.rs`: `init_object_manager()` ahora crea el directorio raíz `\` y 9 entradas de tipo base en el Object Manager. Llamado desde Phase 2.759. Tests: 2 (ob_init_root_directory, ob_init_type_entries).

### Changed
- `src/object/mod.rs`: re-exporta `ObError`, `ObId`, `ObType`, `OB_NAME_LEN`, `ObObjectSnapshot`.
- `src/kobj/mod.rs`: `KObjType` convertido a `ObType` internamente; `KObjEntry` es wrapper snapshot de `ObObject`; `KObjId = ObId`.
- **Slab&lt;T&gt; contenedor** — `src/slab_container.rs`: nuevo, generic slab container con `insert`, `get_by_idx`, `remove_by_idx`, `set`, `iter`. 5 tests.
- **Scheduler Vec dinámico** — `src/scheduler/mod.rs`: `eprocesses` y `kthreads` cambiados de `[Option<...>; N]` a `Vec<Option<...>>`. Sin límites fijos (antes 16/32). `alloc_eprocess_slot`/`alloc_kthread_slot` crecen el Vec si lleno.
- **Pipe buffers dinámicos + MAX_PIPES** — `src/pipe.rs`: `PipeInner.buf` es `Box<[u8; 4096]>` (heap). `PipeManager.pipes` es `Vec<Option<Mutex<PipeInner>>>`. Añadido `MAX_PIPES = 16` para evitar heap exhaustion. Fix reentrancy deadlock en `alloc()` y `maybe_free_pipe()`.
- **Shell pipeline (pipe operator `|`)** — `userbin/neoshell/src/main.rs`: soporte para pipelines `cmd1 | cmd2 | cmd3` con pipes nativos, redirección de stdin/stdout, hasta 16 comandos encadenados.

## v0.40.3 — 2026-06-22

### Fixed
- **AHCI reclaim** — `boot_ahci.rs`: guarda `clb`/`fb` en `BOOT_AHCI_INFO` y los restaura en `reclaim_ahci_port()`. El driver NEM AHCI (Phase 3.85) sobrescribía PORT_CLB/PORT_FB, rompiendo el DMA de BootAhci para la carga del NXL en Phase 3.87.

### Changed
- **Ring 0 → Ring 3**: Eliminados de la shell Ring 0 los comandos `KEYB`, `PS`, `PRI`, `DRIVES`, `KILL`, `HELP`, `LABEL`. Todos tienen equivalentes Ring 3 (`keyb.nxe`, `ps.nxe`, `pri.nxe`, `drives.nxe`, `kill.nxe`, `corehelp.nxe`, `label.nxe`).
- **Syscall `SetVolumeLabel` (RAX=54)**: Nueva syscall para cambiar la etiqueta del volumen desde Ring 3. Wrapper en `libneodos`.

### Added
- **label.nxe**: Nuevo binario Ring 3 para el comando `LABEL` (muestra y cambia etiqueta del volumen). Incluido en la imagen del FS.
- **Test `ring0_call_still_dispatched`, `ring0_run_still_dispatched`, `ring0_ndreg_still_dispatched`**: Reemplazan los tests de HELP eliminados.

## v0.40.0 — 2026-06-22

### Added
- **Buddy bitmap dinámico (>4GB)** — `src/memory/buddy.rs`: bitmap dinámico (heap allocated) en vez de `[u64; 16384]`. Calcula tamaño de `phys_max`. Fallback a 4GB tracking si no hay páginas contiguas. `LEGACY_BITMAP_WORDS=16384`.
- **User window 32MB** — `USER_LIMIT` expandido de `0x0080_0000` a `0x0240_0000` (4MB→32MB). Slot count: 32→256. Kernel heap reubicado a `0x0240_0000` (36MB). Kernel load address movida a `0x4000000` (64MB) para evitar solapamiento con user window.
- **Static buffers→heap** — `BootAhci` búferes DMA (`cmd_list`, `recv_fis`, `cmd_table`, `dma_buf`) ahora heap-allocados via `alloc_zeroed`. `main.rs` CMD_BUF/BIN_BUF convertidos a `alloc::vec!`. Implementación `Drop` para liberación.

### Changed
- `src/memory/buddy.rs`: `BITMAP_WORDS` eliminado, `bitmap` es `*mut u64`, `init_bitmap()` separado de `init_from_regions()`
- `src/memory/mod.rs`: calcula y reserva páginas para bitmap dinámico desde la memory map UEFI
- `src/arch/x64/paging.rs`, `src/scheduler/address_space.rs`: USER_LIMIT=0x2400000
- `src/memory/layout.rs`, `src/allocator.rs`: kernel_heap en 0x2400000
- `kernel.ld`: kernel en 0x4000000
- `src/drivers/isolation.rs`: rangos de validación ajustados
- `src/syscall/mod.rs`: `is_user_ptr_valid()` y `handler_thread_create()` usan USER_LIMIT
- `src/elf.rs`: tests actualizados con nuevas direcciones
- `src/drivers/boot_ahci.rs`: búferes heap-allocados con alineación 1024/256/128
- `src/panic_classification.rs`, `src/testing.rs`: direcciones kernel actualizadas

### Tests
- 479 kernel tests (de 469) + 14 command tests

## v0.40.2 — 2026-06-22

### Added
- **X7. NeoDOS Object Manager (Ob)** — Documento de arquitectura y roadmap de implementación:
  - `docs/OBJECT_MANAGER_ARCHITECTURE.md`: Diseño completo del Object Manager que unifica Handles, KOBJ, URN y Security bajo una sola abstracción. Define ObObject, ObHandle, ObDirectory, ObOperations, integración con seguridad, y 6 nuevas syscalls (RAX 60–65).
  - `docs/IMPROVEMENTS.md`: Nueva sección X7 con 40 tests planificados, análisis de dependencias, impacto en archivos, métricas objetivo y riesgos.
  - Plan de implementación detallado dividido en 23 issues organizados en 4 versiones (v0.41→v1.0), con ~1920 líneas nuevas estimadas y 69 tests.

### Changed
- **AGENTS.md**: Updated priorities to include Ob milestones (v0.41–v1.0). Added reference to `OBJECT_MANAGER_ARCHITECTURE.md`.
- **IMPROVEMENTS.md**: Updated progress to 132/160 items. Added X7 section and Ob milestones to v0.41/v0.42/v0.50/v1.0 roadmap phases.

### Added
- **B9.4 PS** (`userbin/ps/`): Ring 3 `ps.nxe` — process listing via `sys_kobj_enum` (RAX=48). Shows PID, TID, name.
- **B9.5 KILL** (`userbin/kill/`): Ring 3 `kill.nxe` — terminate process by PID via `sys_kill_process` (RAX=52, admin).
- **B9.6 PRI** (`userbin/pri/`): Ring 3 `pri.nxe` — set process priority via `sys_set_priority` (RAX=51, admin).
- **B9.10 KEYB** (`userbin/keyb/`): Ring 3 `keyb.nxe` — change keyboard layout via `sys_set_keyboard_layout` (RAX=49).
- **B9.13 CALL**: Built-in batch execution in neoshell. Reads `.BAT` files via `sys_open`/`sys_readfile`, executes lines sequentially.
- **Syscall 49** (`handler_set_keyboard_layout`): Push `EVENT_KEYB_LAYOUT` event to Event Bus from Ring 3.
- **Syscall 51** (`handler_set_priority`, admin): Set process scheduling priority (0–3) from Ring 3.
- **Syscall 52** (`handler_kill_process`, admin): Terminate a process by PID from Ring 3.

### Changed
- **nEX**: `execute()` refactored into `execute_line()` for reuse by CALL batch execution.
- **AGENTS.md**: Updated syscall table with RAX 49, 51, 52, 53.
- **IMPROVEMENTS.md**: Marked B9.4, B9.5, B9.6, B9.10, B9.13 as completed (136/145 items).

## v0.40.1 — 2026-06-21

### Added
- **Cursor blink** (`neodos-kernel/src/console.rs`, `arch/x64/idt.rs`): Autoblink driven by the 1 KHz timer IRQ. Toggles `_` cursor every 18 ticks (~55 Hz) while enabled.
- **Syscall 53** (`sys_cursor_blink`): Enable/disable cursor blinking from Ring 3.
- **neoshell**: Calls `sys_cursor_blink(true)` on readline entry, `false` on exit.

### Fixed
- **Prompt** `C:>` → `C:\>`: `sys_getcwd` returns `n` bytes (no null), but `buf[..n-1]` stripped the trailing `\`. Fixed to `buf[..n]`.
- **Inode conflict**: NXE files at 37-40 collided with Packages/Users dirs. Moved to 56-59.

## v0.39.11 — 2026-06-21

### Removed
- **B9.2 SET command from Ring 0**: Deleted `neodos-kernel/src/shell/commands/set.rs`. Ring 0 no longer responds to SET. Ring 3 `neoshell.nxe` handles SET as built-in.
- **B9.3 EXIT/SHUTDOWN from Ring 0**: Deleted `neodos-kernel/src/shell/commands/shutdown.rs`. Ring 0 no longer responds to EXIT. Ring 3 `neoshell.nxe` handles EXIT and POWEROFF as built-ins invoking `sys_poweroff` (RAX=42).

### Added
- **Tests**: `ring0_set_removed` and `ring0_exit_removed` verify Ring 0 no longer dispatches SET/EXIT.

### Changed
- **AGENTS.md**: Updated test count to 469. Updated KERNEL.md to reflect Ring 3-only EXIT/POWEROFF.
- **IMPROVEMENTS.md**: Marked B9.2 and B9.3 as completed.

## v0.39.10 — 2026-06-21

### Added
- **B9.1 HELP command (Ring 0→Ring 3)** (`neodos-kernel/src/shell/commands/help.rs`, `userbin/corehelp/`):
  - Ring 0 HELP → stub que redirige a neoshell.
  - Ring 3 HELP NT-style: cada `.NXE` embebe descripción en `.rodata` entre `::HELP::`/`::END::` y responde a `/?`.
  - `HELP` escanea `C:\Programs\*.NXE` buscando `::HELP::`, extrae descripciones y lista comandos.
  - `HELP <cmd>` spawnea `<cmd>.NXE /?` via sys_spawn con pipe y captura la salida.
  - 17 `.NXE` actualizados con `/?` flag handling y `::HELP::` markers.
  - 4 kernel tests: `help_ring0_stub_output`, `help_ring0_stub_output_detail`, `help_ring0_stub_no_old_behavior`, `help_ring0_slash_question`.
- **B9.8 DRIVES syscall + user binary** (`neodos-kernel/src/syscall/mod.rs`, `userbin/drives/`):
  - `sys_get_drives` (RAX=33) handler: enumera unidades montadas con tipo de FS, etiqueta y tamaño.
  - `DriveInfo` struct ABI-stable en kernel y libneodos.
  - `FileSystem` trait extendido con `fs_type()` / `total_sectors()` (FAT32, NeoDOS, ISO9660, KDrive).
  - `drives.nxe`: Ring 3 DRIVES command que lista letra, FS type, label y tamaño.
- **libneodos wrappers**: `sys_pipe()` (RAX=5) y `sys_dup2()` (RAX=6) añadidos a `libneodos/src/syscall.rs`.

### Changed
- **AGENTS.md**: Updated to 467 tests in 46 suites. Added HELP (4), DRIVES binary. Updated corehelp description.
- **IMPROVEMENTS.md**: Marked B9.1 HELP and B9.8 DRIVES as completed.
- **testing.rs**: Registered 4 help tests.

## v0.39.9 — 2026-06-21

### Added
- **NT5.5 Unified Resource Namespace (URN)** (`neodos-kernel/src/urn/mod.rs`): Abstracción sobre NT5 Ob que unifica acceso a recursos heterogéneos bajo esquema `neodos://<scheme>/<path>`. Soporta schemas `device` (ObNamespace), `file` (VFS), `registry` y `kobj` (stubs). API: `urn_open()`, `urn_read()`, `urn_write()`, `urn_seek()`. 11 tests.
- **NT5.6 Virtual K:\ drive** (`neodos-kernel/src/vfs/kdrive.rs`): Drive virtual `K:\` que expone objetos NT5 internos como archivos de solo lectura via VFS. Directorios: `K:\Processes\` (info de PIDs), `K:\Drivers\` (info de NEM drivers), `K:\Memory\` (estadísticas), `K:\Interrupts\` (contadores). 12 tests.

### Changed
- **AGENTS.md**: Updated test count to 463 in 45 suites. Added URN and KDrive sections.
- **IMPROVEMENTS.md**: Marked NT5.5 y NT5.6 as completed.
- **testing.rs**: Registered URN (11 tests) + KDrive (12 tests).

## v0.39.8 — 2026-06-21

### Fixed
- **sys_exit GPF on `exit` command** (`neodos-kernel/src/syscall/mod.rs`, `neodos-kernel/src/arch/x64/idt.rs`):
  - `handler_exit` only called `request_exit_to_kernel()` when `pid == current_wait_pid()` (someone waiting via `sys_waitpid`). When no process was waiting, the asm trampoline returned to user mode via `iretq`, and the NXL's `nxl_sys_exit` executed the privileged `HLT` instruction, causing a GPF (error=0x0) at RIP in the DLL region.
  - **Fix 1**: Always call `request_exit_to_kernel()` when the last thread exits, regardless of `sys_waitpid` state. This ensures the asm handler always takes the `exit_to_kernel` path on `sys_exit`.
  - **Fix 2**: Added `is_thread_terminated()` check in the asm handler for non-last thread exits. When a non-last thread is terminated, `syscall_try_resched` is called to switch to the next runnable thread instead of returning to user mode.

### Changed
- **AGENTS.md**: Version bumped to v0.39.8. Clarified that neoshell Ring 3 TAB completion only covers built-in commands (CWD, SET, EXIT, POWEROFF), not PATH scanning for `.NXE` files.

### Removed
- **builtin_drivers.rs** (`neodos-kernel/src/drivers/builtin_drivers.rs`): Removed legacy stub built-in driver callbacks (null, echo, timer_listener). These were development stubs from the early NEM driver model. All actual drivers are now loaded as `.nem` files from NeoDOS FS.

## v0.39.7 — 2026-06-21

### Changed
- **IMPROVEMENTS.md**: Complete rewrite of NT alignment section. Restructured formatting and improved readability.

## v0.39.6 — 2026-06-21

### Changed
- **NeoDOS LSP refinements** (`neodos-lsp/src/cache.rs`, `database.rs`, `indexer.rs`):
  - `NeodosItemKind` enum replaces raw string type tags for better type safety.
  - Removed `ImportInfo` struct (unused).
  - Fixed whitespace and minor cleanup.
  - `main.rs`: Reduced dependency on `unwrap()`, improved fault tolerance.

## v0.39.5 — 2026-06-21

### Added
- **NeoDOS LSP** (`neodos-lsp/`): Language Server Protocol implementation for NeoDOS. See v0.39.4 entry for full description (merged concurrently).

## v0.39.4 — 2026-06-21

### Added
- **A2.1 — PCIe ECAM config space** (`src/hal/pci.rs`, `src/drivers/pci.rs`, `src/timers/hpet.rs`):
  - **MMIO ECAM**: Enhanced Configuration Access Mechanism (ECAM) based on ACPI MCFG table. Addressing: `ECAM_BASE + (bus<<20) + (dev<<15) + (func<<12) + offset`.
  - **MCFG table parsing**: Extended ACPI scanner in `hpet.rs` to locate MCFG table via RSDP → RSDT/XSDT. `get_ecam_info()` returns ECAM base address, segment, bus range.
  - **ECAM mapping**: `drivers::pci::init_ecam()` maps ECAM region as UC- (uncacheable) in page tables at Phase 2.3. Splits 2 MB huge pages into 4 KB PTEs for precise MMIO caching control.
  - **Dual path**: `drivers::pci::pci_config_read/write_*()` auto-select ECAM MMIO or legacy PIO (0xCF8/0xCFC) via `ecam_is_active()`. No MCFG → silent fallback with log warning.
  - **BAR utilities**: `read_bar()`, `read_bar64()`, `map_bar_mmio()` for PCI BAR MMIO mapping with size detection.
  - **Tests**: `ecam_base_default`, `ecam_address_calc`, `ecam_mcfg_table_parse`, `ecam_fallback_to_pio_if_no_mcfg`, `ecam_read_match_legacy_pio` (5 integration + 2 unit).
- **A2.2 — I/O APIC + MSI-X** (`src/interrupts/ioapic.rs`, `src/interrupts/msi.rs`, `src/hal/x64/irq.rs`):
  - **I/O APIC init**: Detects I/O APIC from ACPI MADT table. Reads IOAPICID/IOAPICVER for version and pin count. Masks all redirection entries initially.
  - **ISA IRQ routing**: Routes timer (IRQ0→vec32), keyboard (IRQ1→vec33), serial (IRQ4→vec36), PS/2 mouse (IRQ12→vec44) via IOAPIC pins. Respects MADT ISA interrupt source overrides (polarity, trigger mode). Unused IRQs stay masked.
  - **PIC disable**: On IOAPIC init success, masks all PIC IRQs via ports 0x21/0xA1. `ack_irq()` uses APIC EOI (via Local APIC MMIO) for all vectors when IOAPIC is active, skipping PIC PIO EOI.
  - **MSI-X per-entry table**: `configure_msix_entry()` reads MSI-X capability (BAR index + table offset from BIR), maps BAR MMIO as UC-, writes per-entry message address/data/vector_control. `configure_msix_entries()` configures N entries with vector allocation and handler registration.
  - **Integration**: IOAPIC init at Phase 2.91 (after heap, before SMP). Toggle in main.rs log message.
  - **Tests**: `ioapic_has_valid_pin_count`, `ioapic_resolve_gsi_no_override`, `ioapic_resolve_gsi_with_override`, `ioapic_mask_unmask_safe`, `ioapic_pic_disabled_when_ioapic_active` (5 tests).

- **NeoDOS LSP** (`neodos-lsp/`):
  - **LSP server**: Full Language Server Protocol implementation for NeoDOS development. Written in Rust, runs as stdio LSP server. Supports completion (symbols + syscalls + shell commands + capabilities), go-to-definition, find references, hover (type signatures + NeoDOS annotations), diagnostics (unbalanced delimiters, missing semicolons), rename, and document symbols.
  - **Background indexing**: Discovers and indexes all `.rs` files in the workspace via rayon-based parallel parsing. Polling-based file change detection.
  - **NeoDOS-aware parsing**: Detects syscall handlers by naming convention (`sys_*`) and attributes (`#[syscall(num)]`). Recognizes boot phase functions, capability constants (`CAP_*`), shell command entries, driver state enums, and `impl FileSystem` patterns.
  - **Database**: `dashmap`-backed in-memory database with symbol IDs, file index, name prefix index, reference edges, and NeoDOS-specific registries (syscalls, shell commands, drivers).
  - **LSP MCP tools**: 8 new MCP tools (`lsp_list_symbols`, `lsp_search_symbol`, `lsp_get_syscalls`, `lsp_get_shell_commands`, `lsp_get_capabilities`, `lsp_get_diagnostics`, `lsp_get_driver_states`, `lsp_get_kernel_modules`) for AI-level code analysis without needing the LSP server running.
  - **opencode.json integration**: Registers `neodos-lsp` for `.rs` files with workspace root and log level configuration.
  - **Tests**: 34 unit tests (cache, database, handlers, indexer, workspace). All tests pass.

## v0.39.3 — 2026-06-21

### Added
- **NT6 Security Reference Monitor** (`src/security/`):
  - **NT6.1 — SID + Access Token**: `Sid` struct (S-R-I-S* format, `sid_builtin_admin`/`sid_builtin_user`), `Token` struct with `is_admin` flag. Token field added to `Eprocess`. Token inheritance in `add_ring3_process()` from parent PID. Boot processes get admin token by default.
  - **NT6.2 — ACL/ACE on objects**: `Ace` (allow/deny, access_mask, SID), `Acl` (revision + ACE vector), `SecurityDescriptor` (owner, group, DACL). Programmatic creation of ACLs with fine-grained access masks (READ/WRITE/EXECUTE/DELETE/ALL).
  - **NT6.3 — Access check on open**: `se_access_check()` compares caller token SID against object DACL. Admin bypass. No SD/ACL → allow. No match → deny by default. Infrastructure ready for sys_open integration when objects carry SDs.
  - **NT6.4 — Admin vs user token**: `is_current_admin()` now uses `ep.token.is_admin_token()` replacing PID-based check. Syscall 50 (ndreg) enforced via token. 12 unit tests across all 4 sub-phases.
  - **Files**: `src/security/mod.rs`, `sid.rs`, `token.rs`, `acl.rs`, `access.rs`.
  - **Integration**: Phase 2.77 at boot. Token inherited via scheduler's `add_ring3_process`. `is_current_admin()` token-based in `syscall/mod.rs`.

### Fixed
- **libneodos inline asm register clobber** (`libneodos/src/syscall.rs`): `sys_open_with_flags`, `sys_get_volume_label`, `sys_kobj_enum` used direct `int 0x80` inline asm that wrote to `rbx`/`rcx`/`rdx` without saving them. The Rust compiler, unaware of the clobber, reused those registers for local variables, corrupting fd values (e.g. fd=5 became fd=216). Fixed by adding explicit `push`/`pop` around `int 0x80`.
- **NeoDOS FS write sets inode.size prematurely** (`neodos_fs.rs`): `write_file` set `inode.size = BLOCK_SIZE` (4096) during block allocation, before data was written. A 33-byte write left size=4096, causing reads to return garbage after EOF. Removed premature size assignment.
- **Page cache evicts unnecessarily** (`buffer/page_cache.rs`): `evict_lru()` always evicted the LRU tail even when free slots existed, discarding in-use pages and causing writes to be lost on subsequent reads. Fixed by checking for free slots first.
- **Handle leaks in cmdtest** (`userbin/cmdtest/src/main.rs`): `file_exists`/`dir_exists` opened fds via `sys_open` without closing them. Fixed to close fds after checking existence.

### Changed
- **Debug traces removed** from kernel syscall handlers (`syscall/mod.rs`), page cache (`page_cache.rs`), and NeoDOS FS (`neodos_fs.rs`).

## v0.39.2 — 2026-06-21

### Added
- **B4.4 B2. ANSI terminal emulator** — `console.rs` + `font.rs`: ANSI escape sequence parser in kernel console driver. Supports `\x1b[Nm` (SGR: 16 fg/bg colors, bold, reset), `\x1b[H`/`\x1b[row;colH`/`\x1b[f` (cursor position), `\x1b[2J` (clear screen w/ current bg), `\x1b[K` (erase to EOL). 16-color ANSI palette. `font::draw_char()` takes fg+bg colors. `print_str()` now decodes UTF-8 via `chars()`. Box-drawing glyphs (U+2500/2502/2514/251C) added to 8×16 font at slots 0x82-0x85. 3 tests: `ansi_color_foreground`, `ansi_cursor_position`, `ansi_clear_screen`.
- **LOADLIB command removed from Ring 0 shell** — `cmd_loadlib` and `loadlib.rs` removed; LOADLIB functionality was already migrated to user-mode LOAD.NXE.

## v0.39.1 — 2026-06-21

### Added
- **TREE.NXE** — `userbin/tree/`: Ring 3 TREE command showing directory tree with `├──`/`└──` connectors. Recursive up to 6 levels, directories first, case-insensitive alphabetical sort. Optional path argument (default: CWD).

### Changed
- **Shell commands removed from Ring 0**: TYPE, LOAD, TEST commands removed from kernel shell (`handler.rs`), migrated to Ring 3 as `type.nxe`, `run.nxe`, and auto-run from NeoInit respectively.
- **userbin/coretype/**: New Ring 3 TYPE command replacing the built-in kernel TYPE.

## v0.39.0 — 2026-06-20

### Added
- **NT5.1 — Object directory tree**: Dynamic Vec-based KObj registry (replaces 64-slot fixed array). Root `\` and standard directories (`\Device`, `\DosDevices`, `\Global`, `\Driver`, `\FileSystem`, `\Ob`) created at boot. Added `KObjType::Symlink`, `KObjType::MountPoint`, `KObjType::Directory` variants.
- **NT5.2 — Symbolic links**: `SymlinkEntry` with name/target, `insert_symlink()`, `lookup_symlink()`, `remove_symlink()`. Resolution follows up to 10 hops; loop detection.
- **NT5.3 — Path resolution API**: `ob_lookup_by_path()` with path normalization (`.`, `..`, trailing `\` strip). Case-insensitive name comparison via lowercased keys.
- **NT5.4 — VFS mount points**: `src/vfs/mount.rs` with `MountManager`, `MountPoint`, `FilesystemType` (NeoDosFs, Fat32, Iso9660). Mount creates KObj + `\DosDevices\{letter}:` symlink. Real mounts at boot register C: and A: in the namespace.
- **sys_kobj_enum (RAX=48)** — `handler_kobj_enum`: enumerates kernel objects into user buffer. Returns array of `KObjEntryRaw`. Accessible from Ring 3.
- **KOBJ.NXE** — `userbin/kobj/`: Ring 3 KOBJ command migrated from Ring 0. Lists all kernel objects (ID, type, name, refcount, native ID) via `sys_kobj_enum`.
- **libneodos wrapper** — `sys_kobj_enum(buf)`, `KObjEntryRaw` struct in `libneodos/src/syscall.rs`.

### Fixed
- **Test KObj leaks**: All 38 leaking tests fixed (21 driver_runtime, 2 boot_loader, 4 hotreload, 5 mount, 6 namespace). Added `rt.remove(id)` cleanup for local `DriverRuntime` tests, `DRIVER_RUNTIME.lock().remove(id)` for global tests, and unmount at end of mount tests.
- **Deadlock in init_object_namespace**: Split into two loops — first creates namespace directories, then registers KObjs (outside the namespace lock).

### Changed
- **KObjRegistry**: Dynamic `Vec<Option<KObjEntry>>` instead of fixed 64-slot array. No hard limit.
- **kobj_register**: Auto-inserts into namespace via `ob_insert_object_auto()`.
- **kobj_unregister**: Auto-removes from namespace via `ob_remove_object_auto()`.
- **MountManager::mount()**: Now takes `drive_letter: char` instead of `volume_name: &str`. Derives `{letter}:` for name, `\Device\{letter}:` for device path, `\DosDevices\{letter}:` for DosDevices symlink.
- **Tests**: 416 total (403 original + 8 new namespace + 5 new mount). 41 test suites.

## v0.38.2 — 2026-06-20

### Added
- **sys_get_meminfo (RAX=45)** — `handler_get_meminfo`: fills `MemInfo` struct from memory stats.
- **sys_get_volume_label (RAX=46)** — `handler_get_volume_label`: obtiene la etiqueta del volumen desde VFS.
- **sys_chdir_parent (RAX=47)** — `handler_chdir_parent`: cambia el CWD del proceso padre (usado por CD.NXE).
- **CD.NXE** — `userbin/cd/`: migrado de Ring 0 a Ring 3. Resuelve rutas absolutas/relativas con `..`/`.` normalization, valida el directorio, comunica el resultado al shell padre vía buffer compartido.
- **ECHO.NXE** — `userbin/echo/`: migrado de Ring 0 a Ring 3. Imprime texto recibido como argumento.
- **MEM.NXE** — `userbin/mem/`: migrado de Ring 0 a Ring 3. Muestra uso de memoria vía sys_get_meminfo.
- **VOL.NXE** — `userbin/vol/`: migrado de Ring 0 a Ring 3. Muestra etiqueta del volumen vía sys_get_volume_label.
- **libneodos wrappers** — `sys_get_meminfo(info)`, `sys_get_volume_label(drive, buf)`, `sys_chdir_parent(path)` en `libneodos/src/syscall.rs`. `MemInfo` struct en API pública.
- **AbiTable v4** — nuevos campos `sys_chdir_parent`, `sys_get_meminfo`. ABI_VERSION bump de 2 a 4.

### Changed
- **neoshell** — CD y ECHO quitados de built-ins; ahora se ejecutan como .NXE externos via PATH. El fallthrough dispatch escribe args en buffer compartido 0x41F000 antes de spawn. CD.NXE usa el buffer para devolver el path resuelto al shell.
- **coredir.nxe** — refactorizado: parsea argumentos (/W, /P, path) desde el buffer compartido, muestra permisos RWXSD, resuelve path relativo contra CWD.
- Updated `scripts/build.sh` y `scripts/create_neodos_image.py` para compilar e incluir `cd.nxe`, `echo.nxe`, `mem.nxe`, `vol.nxe` en NeoDOS FS image.
- **CD/ECHO/MEM/VOL commands** — eliminados de Ring 0 (handler.rs, commands/*.rs, commands/mod.rs).

## v0.38.1 — 2026-06-19

### Fixed
- **LBA translation in file data reads** — `read_file_to_buf`, `read_file`, and `write_file` in `neodos_fs.rs` computed partition-relative LBAs but bypassed `abs_lba()` (which adds partition base via IoStack). Directory operations correctly used `abs_lba()`, so file data read from the wrong absolute disk location, returning garbage. This caused NEOINIT.NXE and libneodos.nxl to fail with `InvalidMagic`.
- **Driver isolation pointer validation** — `validate_driver_ptr` in `isolation.rs` only allowed kernel heap (`0x01000000..0x02000000`) but the boot stack lives at `0x1FFFF000` (above heap), causing `[ISO] DENIED: hst_log with invalid pointer` when NEM drivers passed stack-allocated buffers to `hst_log`.

## v0.38.0 — 2026-06-16

### Added
- **sys_get_version (RAX=43)** — `handler_get_version`: copies kernel version string to user buffer.
- **sys_get_datetime (RAX=44)** — `handler_get_datetime`: fills `SysDateTime` struct from RTC bridge.
- **VER.NXE** — `userbin/ver/`: standalone user-mode binary that displays kernel version via sys_get_version.
- **DATETIME.NXE** — `userbin/datetime/`: standalone user-mode binary that displays RTC date and time via sys_get_datetime, with `/D` (date only) and `/T` (time only) flags.
- **libneodos wrappers** — `sys_get_version(buf)` and `sys_get_datetime(dt)` in `libneodos/src/syscall.rs`. `DateTime` struct in public API.

### Changed
- Updated `scripts/build.sh` and `scripts/create_neodos_image.py` to build and include `datetime.nxe` and `ver.nxe` in NeoDOS FS image.
- Removed stale test binaries (`hello.nxe`, `systest.nxe`, `filetest.nxe`, `alltest.nxe`, `cputest.nxe`, `test.nxe`) from build and image creation.
- `spawn_hello_binary_path_resolve` test uses `dir.nxe` instead of removed `hello.nxe`.

## v0.37.0 — 2026-06-15

### Changed
- **Directory structure reorganized** — New NeoDOS FS layout:
  - `\System\Kernel\boot.cfg` (was `\SYSTEM\BOOT.CFG`)
  - `\System\Config\system.cfg` (was `\SYSTEM\CONFIG.SYS`)
  - `\System\Config\input.cfg` (new)
  - `\System\Drivers\` (flat, was `\SYSTEM\DRIVERS\BOOT\` + `\SYSTEM\DRIVERS\SYSTEM\`)
  - `\System\Libraries\` (was `\SYSTEM\LIB\`)
  - `\System\Layouts\` (new: es-ES.nkb, en-US.nkb)
  - `\Programs\` (was `\BIN\` + `\SYSTEM\BIN\` + root .NXE files)
  - `\Packages\`, `\Users\`, `\Temp\`, `\Data\`, `\Logs\` (new empty dirs)
  - All paths updated in kernel (nxl.rs, shell.rs, handler.rs, boot_loader, ndreg, etc.), user-mode binaries (neoshell, neoinit, cpuinfo, test, corehelp, filetest, alltest), and docs.
- **PATH** default: `\Programs` (was `\BIN;\SYSTEM`)
- Kernel loads config from `C:\System\Config\system.cfg` (was `C:\CONFIG.SYS` + `C:\SYSTEM\CONFIG.SYS`)
- Drivers: flattened into single `C:\System\Drivers` — category determined by NEM header, not directory.
- NEM driver renaming: `ps2kbd.nem` → `keyboard.nem`, `ata.nem` → `disk.nem`
- Libraries: `libneodos.nxl` stored as `fs.nxl`, `io.nxl`, and `process.nxl` (same binary)

### Added
- **B8.6 HELP.NXE (corehelp)** — Standalone user-mode help utility (`userbin/corehelp/`):
  - Scans `C:\BIN\*.NXE` via `sys_open` (directory handle) + `sys_readdir`
  - Lists available core tools with count
  - Embedded `::HELP::` text for neoshell's HELP display
- **B8.1 DIR.NXE (coredir)** — Standalone user-mode directory listing utility (`userbin/coredir/`):
  - Lists directory contents via `sys_open` (directory handle) + `sys_readdir`
  - Multi-column output (3 columns with `<DIR>` markers for directories)
  - `/W` (wide) mode: 5-column compact listing
  - `/P` (pause) mode: pauses after each screenful
- **A4.7. neoshell (Ring 3 shell)** — Full-featured interactive shell running at Ring 3:
  - Built-in commands: HELP, CLS, ECHO, VER, CD, CWD, DIR, SET, POWEROFF, EXIT
  - DIR uses sys_open + sys_readdir to list directories with entry counts
  - External command dispatch: scans PATH dirs (\\Programs) for `CMD.NXE`, spawns via sys_spawn + sys_waitpid
  - TAB completion: matches built-in commands (single match replaces word cleanly, multiple lists all)
  - Command history: circular 32-entry buffer with ↑/↓ navigation
  - Drive change: `C:`, `D:`, etc. via sys_chdir
  - Environment variables: `SET` built-in with `SET VAR=VAL` and `SET VAR` display
  - Prompt shows current working directory: `C:\> `

### Changed
- **neoshell binary** — Grew from ~15 KB to ~27 KB with all new features
- **NeoInit spawn** — Fixed stdin_fd/stdout_fd/stderr_fd to use 0xFF (inherit) instead of 0 (explicit fd 0), allowing neoshell output to appear on console
- **AGENTS.md** — Updated to v0.37.0

## v0.36.0 — 2026-06-15

### Added
- **A4.6. Syscalls para shell Ring 3** — 6 new syscalls for Ring 3 shell FS/process operations:
  - `sys_spawn (RAX=7)`: Now supports fd redirection (RBX=path, RCX=stdin_fd, RDX=stdout_fd, R8=stderr_fd). `0xFF` = inherit default. Child handle table customized for redirected fds.
  - `sys_readdir (RAX=8)`: Read directory entries from HANDLE_DIR handles. Returns `DirEntryRaw` struct (inode, mode, size, name[260]).
  - `sys_mkdir (RAX=25)`: Create directory via VFS.
  - `sys_unlink (RAX=26)`: Delete file via VFS.
  - `sys_rmdir (RAX=27)`: Remove empty directory via VFS.
  - `sys_rename (RAX=28)`: Rename file/directory via VFS. Extracts leaf name from new path.
- **HANDLE_DIR (type 9)** — New handle type for directory handles. `sys_open` now accepts directories and returns HANDLE_DIR.
- **libneodos v0.2.0 / libneodos-nxl v0.2.0** — New NXL ABI table entries: `sys_spawn`, `sys_readdir`, `sys_mkdir`, `sys_unlink`, `sys_rmdir`, `sys_rename`. Safe Rust wrappers in `syscall.rs`. `DirEntry` struct for readdir.
- **6 new syscall tests** — `spawn_hello_binary_path_resolve`, `spawn_with_fd_redirection_helpers`, `readdir_list_root`, `mkdir_rmdir_roundtrip`, `unlink_file`, `rename_file`.

### Changed
- **handler_spawn** — Rewritten to accept stdin_fd/stdout_fd/stderr_fd parameters (RBX=path, RCX=stdin_fd, RDX=stdout_fd, R8=stderr_fd). 0xFF = inherit default. Redirected fds increment pipe refcounts.
- **handler_open** — Now accepts directories: returns HANDLE_DIR (type 9) instead of E_ISDIR.
- **ABI table version** — Updated to version 2 with 6 new function pointer slots.

## v0.35.0 — 2026-06-13

### Added
- **NeoInit (PID 1) init process** — `userbin/neoinit/` is a Ring 3 init process that spawns NEOSHELL.NXE via sys_spawn (RAX=7). When the shell exits, NeoInit respawns it. Uses a save/restore mechanism in the kernel to preserve NeoInit's code+stack at 0x400000 while the child binary runs.
- **`sys_spawn` (RAX=7)** — handler_spawn saves NeoInit's slot (0x400000..0x420000) to a kernel heap buffer, loads the child ELF at 0x400000, enters it via execute_usermode, restores TSS.RSP0 on exit, restores NeoInit's code+stack, and returns the child PID. Handles TSS.RSP0 switching, scheduler current_tid save/restore, WAIT_PID setting.
- **`cmd_poweroff` in neoshell** — New POWEROFF command in the Ring 3 shell that calls sys_poweroff (RAX=42) to shut down the machine.
- **`sys_poweroff` (RAX=42)** — handler_poweroff flushes caches, sends EVENT_SHUTDOWN, and calls hal::poweroff() (QEMU debug port + ACPI S5 + PS/2 reset).
- **`set_wait_pid(pid)`** — New public function in `usermode.rs` for setting WAIT_PID externally (needed by handler_spawn).
- **BOOT.CFG `NEOINIT` flag** — `NEOINIT=0` in `C:\SYSTEM\BOOT.CFG` skips NeoInit and boots directly into kernel shell for testing. `NEOINIT=1` (default) loads NeoInit as PID 1.

### Changed
- **main.rs PHASE 4** — Replaced kernel-mode respawn loop with NeoInit init process loading. Falls back to kernel shell if NEOINIT.NXE not found or when NEOINIT=0 in BOOT.CFG.
- **Removed INV-10 panic** — PID 1 is no longer blocked from exiting (former INV-10 invariant removed). NeoInit manages itself via sys_spawn loop.
- **`create_neodos_image.py`** — Updated BOOT.CFG with NEOINIT setting; removed SHELL.NXE alias (inode 18); added NEOINIT.NXE (inode 17).
- **`build.sh`** — Builds and copies neoinit binary to kernel.elf ESP directory.

### Fixed

### A4.5 APC engine — Asynchronous Procedure Calls

#### Added
- **`src/apc/mod.rs`** — Per-thread APC queues (kernel + user, 64 entries each). `queue_kernel_apc()`, `queue_user_apc()`, `dispatch_kernel_apcs()`, `dispatch_one_user_apc()`, `has_pending_user_apcs()`, `irp_complete_with_apc()`, `irp_queue_apc_dpc_completion()`.
- **`irp_complete_with_apc(irp_id, tid)`** — New IRP completion path: DIRQL → DPC (DISPATCH) → user APC (PASSIVE). Device ISR enqueues DPC via `irp_queue_apc_dpc_completion()`, DPC calls `irp_complete_with_apc()` which queues user APC to target thread.
- **`sys_wait_alertable` (RAX=40)** — Alertable wait: if APC pending, dispatches it and returns `APC_ALERTED` (1). Otherwise blocks thread in alertable state.
- **`sys_sleep_ex` (RAX=41)** — Yield CPU with alertable APCs: checks for pending APCs before/after yield.
- **`apc_dispatch_on_syscall_return()`** — Called from syscall handler assembly before IRETQ to Ring 3. Dispatches pending kernel APCs (cleanup, post-I/O) and one user APC on every syscall return.
- **Kthread** — Added `kernel_apc_queue`, `user_apc_queue`, `apc_pending` fields to `Kthread` struct.

#### Changed
- `src/arch/x64/idt.rs` — Added `call apc_dispatch_on_syscall_return` in `syscall_handler_asm` before IRETQ.
- `src/syscall/mod.rs` — Added `WaitAlertable = 40`, `SleepEx = 41` to `SyscallNum` enum, SSDT entries, and permission table.
- `src/irp/mod.rs` — `IrpPool::inner` and `IrpPoolInner::get_mut` made `pub(crate)` for APC integration.

#### Tests
- 5 APC tests: `apc_kernel_dispatch_during_cleanup`, `apc_user_alertable_wait_receives`, `apc_queue_overflow_handling`, `irp_completion_dispatches_apc`, `apc_stress_100_concurrent_irps` (386 total kernel tests).

## v0.33.0 — 2026-06-11

### A2.3 HAL v0.4 — raw/safe split

#### Added
- **`src/hal/raw/`** — Bare asm primitives: `raw_read_msr`, `raw_write_msr`, `raw_read_tsc`, `raw_cpuid`, `raw_sti`, `raw_cli`, `raw_halt`, `raw_read_cr2/3/4`, `raw_write_cr3`, `raw_invlpg`, `raw_invpcid`, `raw_read_rflags`, `raw_lgdt`, `raw_lidt`, `raw_ltr`, `raw_pause`, `raw_set_segment_regs`, `raw_gs_read/write_u64/u32/u16/u8`, `raw_inb/outb/inw/outw/inl/outl`, `raw_rep_stosd`, `raw_debug_port_write`, plus GPR readers for crash dump.
- **`src/hal/safe/`** — Type-safe wrappers: `Msr` trait with `read_msr<T: Msr>()` / `write_msr<T: Msr>()`, MSR constants (`GS_BASE`, `KERNEL_GS_BASE`, `FS_BASE`, `APIC_BASE_MSR`, `EFER`, etc.) with `IsSafe` flag. `read_cr2()` safe wrapper. `GdtDescriptor`/`IdtDescriptor` types.
- **Audit constraint:** `grep -rn 'asm!(' src/ --exclude-dir=hal/` returns 0. All 55 inline asm calls confined to `hal/`.

#### Changed
- `src/hal/x64/` — All extern "C" ABI functions now delegate to `hal::raw` primitives.
- `src/arch/x64/msr.rs` — `rdmsr`/`wrmsr` use `hal::raw::raw_read_msr`/`raw_write_msr`.
- `src/cpu.rs` — `cpuid()` delegates to `hal::raw::raw_cpuid`.
- `src/boot_benchmark.rs` — `rdtsc()` delegates to `hal::raw::raw_read_tsc`.
- `src/arch/x64/gdt.rs` — Segment register loading via `raw_set_segment_regs`/`raw_set_gs`/`raw_set_fs`.
- `src/arch/x64/smp.rs` — Pause/lidt/hlt via `hal::raw`.
- `src/arch/x64/ipi.rs` — Pause via `hal::raw`.
- `src/arch/x64/cpu_local.rs` — GS-segment reads/writes via `hal::raw`.
- `src/timers/apic.rs` — MSR read/write via `hal::raw`.
- `src/timers/hpet.rs` — Pause via `hal::raw`.
- `src/graphics.rs` — `rep stosd` via `hal::raw`.
- `src/drivers/nvme.rs` — Debug port write via `hal::raw`.
- `src/drivers/usb_hid/` — `nop` delay replaced with `spin_loop()`.
- `src/processes.rs` — `nop` delay replaced with `spin_loop()`.
- `src/crash/mod.rs` — GPR/CR reads via `hal::raw`.
- `src/main.rs` — RSP read via `hal::raw`.

#### Tests
- 5 HAL v0.4 tests: `hal_v04_abi_msr_safe`, `hal_msr_read_write_consistency`, `hal_no_asm_outside_hal_dir`, `hal_cr2_page_fault_addr`, `hal_invpcid_tlb_invalidation`.

## v0.32.0 — 2026-06-11

### A3.1 Crash Dump Framework

#### Added
- **`src/crash/mod.rs`** — Crash dump subsystem with ring buffer, serial dump at panic, 16 KB `CrashDumpHeader` (magic, version, cause, stack trace, GPRs, CR registers, scheduler state, PML4 snapshot, trace events). Functions: `fill_header()`, `dump_to_serial()`. Recursion guard via `CRASH_DUMP_OCCURRED` atomic.
- **`src/shell/commands/crash.rs`** — Shell commands: `CRASH` (show crash status), `CRASH DUMP` (dump to serial). Crash dump area @ `0x0F000000` (16 MB), initialized at boot.
- **`scripts/crash_analyzer.py`** — Python script to parse crash dump output from serial log.

#### Tests
- 5 crash dump tests: `crash_dump_header_size`, `crash_dump_new_zeroed`, `crash_dump_header_layout`, `crash_dump_fill_and_serialize`, `crash_dump_no_recursion`.

### sys_getcpuinfo (RAX=24)

#### Added
- **`src/cpu.rs`** — `CpuInfoFull` struct with vendor, brand, family/model/stepping, features (30 flags), SMP topology, timers. `get_cpu_info_full()` returns all CPUID data.
- **`src/syscall/mod.rs`** — `handler_get_cpuinfo()` (RAX=24): reads RBX=buf_ptr, RCX=buf_size, copies `CpuInfoFull` to user buffer.
- **`libneodos/src/syscall.rs`** — `sys_getcpuinfo(buf)` wrapper for user-mode.
- **`libneodos/src/export.rs`** — Export table updated.

### cpuinfo — User-mode CPU Info

#### Added
- **`userbin/cpuinfo/`** — `cpuinfo.nxe` user-mode binary that loads `cpuinfo.nxl` via `sys_loadlib` and displays vendor, brand, family/model/stepping, features, topology, timers.
- **`libcpu-nxl/`** — `cpuinfo.nxl` NXL library with `CpuInfoAbiTable` (46 function pointers) in `.export_table`. Null-terminated feature names.
- **`scripts/build.sh`** — Build support for `cpuinfo.nxl` + `cpuinfo.nxe`.
- **`scripts/create_neodos_image.py`** — Include `cpuinfo.nxl` and `cpuinfo.nxe` in NeoDOS FS image.

### NXL Loader Improvements

#### Changed
- **`src/nxl.rs`** — `find_slot_for_base(compiled_base)` replaces `find_free_slot()`: parses ELF header PT_LOAD vaddr and selects slot matching compiled address. `mark_segment_user_accessible()` sets `WRITABLE` on segments with `PF_W` (2) in `p_flags`.
- **`src/elf.rs`** — `SegmentInfo` gains `flags: u32` field. `load_elf()` passes segment flags.
- **`src/scheduler/address_space.rs`** — `SegmentInfo.flags` field.
- **`src/syscall/mod.rs`** — `is_user_ptr_valid()` extended to include NXL region (`0x1E000000..0x1E200000`), allowing NXL functions to pass buffers to syscalls.

### TLB Shootdown Deadlock Fix

#### Fixed
- **`src/arch/x64/paging.rs`** — `heap_free_range()` and `mmap_free_range()` now track `freed_first`/`freed_last` only when `phys != addr` (actual free), preventing unnecessary `shootdown_range()` calls that tried to acquire the scheduler lock while `handler_exit` already holds it.

### Test command extended

#### Changed
- **`src/shell/commands/test.rs`** — `test` command now runs all 7 user-mode binaries: HELLO, SYSTEST, FILETEST, ALLTEST, CPUTEST, TEST, CPUINFO.

### Cleanup

#### Changed
- Deleted obsolete scripts: `auto_test_ahci.py`, `test_cputest.py`, `test_disks.py`.

#### Tests
- 5 new crash dump tests.
- Total: 376 kernel tests (371 + 5 new).
- 7 user-mode binaries (6 + 1 new: CPUINFO.NXE).

## v0.31.0 — 2026-06-10

### A2.4. IRQL Framework

#### Added
- **`hal/x64/irql.rs`** — Per-CPU IRQL (Interrupt Request Level) mechanism replacing blanket `cli`/`sti`. Levels: PASSIVE(0), APC(1), DISPATCH(2), DIRQL(3–11), HIGH(15). `raise_irql()`/`lower_irql()` with automatic CLI/STI at DISPATCH+. `IrqMutex<T>` wrapper for spinlocks with automatic IRQL raise/lower. `at_dispatch()` closure helper. Constants: `PASSIVE_LEVEL`, `APC_LEVEL`, `DISPATCH_LEVEL`, `DIRQL_BASE`, `HIGH_LEVEL`.
- **`arch/x64/cpu_local.rs`** — Replaced `in_dispatch_level` bool with `current_irql: u8` at GS offset 0x016. Added `this_cpu_irql()`, `this_cpu_set_irql()`, `this_cpu_in_dispatch_level()` accessors. `OFFSET_CURRENT_IRQL` constant with compile-time assertion.
- **`arch/x64/idt.rs`** — INV-14: Page fault handler now checks `current_irql()` at entry. If >= DISPATCH_LEVEL, panics with `BUGCHECK KI_EXCEPTION_ACCESS_VIOLATION`.

#### Changed
- **`work_queue.rs`** — `process_high_safe()`/`process_low_safe()` now use `raise_irql(DISPATCH)` + `lower_irql` instead of `without_interrupts`.
- **`scheduler/mod.rs`** — All global helpers (`current_pid`, `current_tid`, `get_current_cwd`, `set_current_cwd`, `current_process_heap_range`, `set_current_heap_break`, `current_process_mmap_regions`, `add_current_mmap_region`, `remove_current_mmap_region`, `current_teb_base`, `block_current_for_thread`, `wake_thread_joiner`, `cleanup_terminated_process`) migrated from `without_interrupts` to `raise_irql(DISPATCH)` + `lower_irql`.
- **`pipe.rs`** — `wake_pipe_readers()`, `block_current_for_pipe()` migrated from `without_interrupts` to `raise_irql(DISPATCH)` + `lower_irql`.

#### Tests
- 5 new IRQL tests: `irql_raise_lower_passive_dispatch`, `irql_page_fault_at_dispatch_panics`, `irql_spinlock_implicit_raise`, `irql_nesting_stack`, `irql_preemption_threshold`.
- Total: 366 kernel tests (361 + 5 new).

### A2.5. DPC Engine

#### Added
- **`src/dpc/mod.rs`** — Deferred Procedure Call engine with per-CPU queues (128 entries, SPSC ring buffer). Functions: `insert_queue_dpc()` (enqueue from DIRQL), `dpc_dispatch_pending()` (drain at DISPATCH), `dpc_has_pending()`. Nesting limit `MAX_DPC_DEPTH=10` prevents infinite recursion.
- **`arch/x64/cpu_local.rs`** — Removed inline `DpcQueue` from KPRCB (would exceed 4096-byte limit); stored in separate `DPC_QUEUES[16]` static array. Removed `OFFSET_DPC_QUEUE` constant.
- **`arch/x64/idt.rs`** — `timer_handler_inner()` calls `dpc_dispatch_pending()` for DIRQL→DISPATCH transition.
- **`syscall/mod.rs`** — `clear_need_resched()` calls `dpc_dispatch_pending()` for syscall-return dispatch.

#### Changed
- **`work_queue.rs`** — DPC engine complements existing work queue; no code changes needed.

#### Tests
- 5 new DPC tests: `dpc_enqueue_dispatch_level`, `dpc_irq_to_dispatch_transition`, `dpc_nesting_depth_limit`, `dpc_callback_execution_order`, `dpc_stress_100_irqs`.
- Total: 371 kernel tests (366 + 5 new).

## v0.30.1 — 2026-06-09

### A1.3. Per-CPU Slab Allocator

#### Added
- **`src/slab.rs`** — Complete rewrite with per-CPU fast path: 32-object hot caches in KPRCB via GS-segment for O(1) alloc/free without locks. `refill_from_global()` / `drain_to_global()` with global `spin::Mutex` for cross-CPU replenishment. Batch size = 32 objects. Fallback to `LockedHeap` for >2KB or >16-byte alignment.
- **`arch/x64/cpu_local.rs`** — Per-CPU slab accessor functions: `this_cpu_slab_alloc_local()`, `this_cpu_slab_free_local()`, `this_cpu_slab_head()`, `this_cpu_set_slab_head()`. GS-segment helpers: `gs_read_u16()`, `gs_write_u16()`. Layout constants for `PerCpuSlabCache` fields (head, free_list, free_count, slot_size, total_allocated, total_freed).

### A1.4. IPI Infrastructure + TLB Shootdown

#### Added
- **`arch/x64/ipi.rs`** — Unified IPI module: `send_ipi()`, `send_ipi_all()`, `send_ipi_all_excl_self()`, `send_ipi_mask()` via Local APIC ICR. Three IPI vectors: `IPI_RESCHEDULE` (0xF0), `IPI_TLB_SHOOTDOWN` (0xF1), `IPI_CALL_FUNCTION` (0xF2). Synchronous TLB shootdown with `TlbShootdownPayload` (start, end, target_mask, ack_count, done). Cross-CPU function call with `CallFunctionPayload` and `CallFunctionCb` dispatch. IDT handlers for vectors 0xF1 and 0xF2.
- **`arch/x64/paging.rs`** — TLB shootdown coordination: `build_tlb_target_mask()` scans scheduler for active threads on remote CPUs. `shootdown_single_page()` and `shootdown_range()` called from `heap_free_page()`, `heap_free_range()`, `mmap_free_page()`, `mmap_free_range()`, `set_page_user_accessible()`.

#### Changed
- **`hal/x64/irq.rs`** — `ack_irq()` rewritten with proper vector routing: IPI vectors (0xF0–0xF2) always use APIC EOI; timer vector 32 uses APIC EOI when APIC timer active; legacy device IRQs (32–47) always use PIC EOI. Fixed bug where APIC EOI was sent for keyboard IRQ (33), causing input to hang.
- **`scheduler.rs`** — `enqueue_to_cpu_run_queue()` sends `IPI_RESCHEDULE` to remote CPU when thread is enqueued on another CPU's run queue.
- **`main.rs`** — Added PHASE 2.9: IPI subsystem initialization after SMP init.

#### Tests
- 5 new per-CPU slab tests: `per_cpu_slab_alloc_free_concurrent`, `per_cpu_refill_drain_batching`, `slab_scaling_8cpu`, `slab_under_irql_dispatch`, `slab_stress_100k`.
- 5 new IPI tests: `ipi_constants`, `ipi_tlb_shootdown_struct`, `ipi_call_function_struct`, `ipi_tlb_shootdown_local_only`, `ipi_call_function_no_targets`.
- Total: 353 kernel tests (343 + 10 new).

## v0.30.0 — 2026-06-08

### A1.1/A1.2. Per-CPU Data Structures + SMP + Run Queues

#### Added
- **`arch/x64/cpu_local.rs`** — `Kprcb` struct (4 KB page per CPU, `#[repr(C, align(4096))]`): cpu_id, apic_id, current_thread, current_pid, idle, need_resched, in_dispatch_level, `CpuRunQueue` (64-entry ring buffer), `PerCpuSlabCache[9]` (32-object hot lists), interrupt/context_switch/timer_tick counters, exit trampoline (exit_rsp/exit_rip/exit_rbx/exit_r12-r15/exit_rbp via GS), exit_now.
- **`arch/x64/cpu_local.rs`** — GS-segment accessors: `gs_read_u64/u32/u8()`, `gs_write_u64/u8()` (inline asm `gs:[offset]`). High-level: `this_cpu_id()`, `this_cpu_current_thread()`, `this_cpu_need_resched()`, `this_cpu_set_need_resched()`, run queue accessors.
- **`arch/x64/cpu_local.rs`** — 20 compile-time `offset_of!` assertions enforcing KPRCB layout consistency.
- **`arch/x64/msr.rs`** — Centralized MSR access: `rdmsr()`/`wrmsr()`, typed accessors for `IA32_GS_BASE`, `IA32_KERNEL_GS_BASE`, `read_gs_base()`, `write_gs_base()`, `is_bsp()`, `rdtsc()`, `rdtscp()`.
- **`arch/x64/smp.rs`** — SMP boot via INIT-SIPI-SIPI: AP trampoline (16→32→64-bit), `init_smp()`, `ap_entry()`, per-CPU GS base setup, AP readiness detection.
- **Per-CPU run queues**: `CpuRunQueue` in KPRCB (64-entry ring buffer). `enqueue_to_cpu_run_queue()`, `try_dequeue_local()`, `try_work_steal()`. Scheduler tries local queue → work stealing → global fallback.
- **IPI infrastructure**: `send_ipi()`, `send_ipi_all()`, `send_ipi_all_excl_self()` via Local APIC ICR. IPI_RESCHEDULE vector 0xF0 with IDT handler. `ipi_reschedule_handler` sets per-CPU `need_resched` via GS.
- **Per-CPU `need_resched`**: Hot path in `syscall_handler_asm` reads GS:0x015 before falling back to global `NEED_RESCHED` AtomicBool. Timer handler uses per-CPU flag.
- **Per-CPU exit trampoline**: `execute_usermode_asm` and `exit_to_kernel` now read/write exit context (RSP, RIP, RBX, R12-R15, RBP) via GS segment offsets in KPRCB.
- **8 new kernel tests**: `cpu_local_kprcb_size`, `cpu_local_slab_cache_count`, `cpu_local_run_queue_ops`, `cpu_local_kprcb_init`, `cpu_local_offset_sanity`, `smp_constants`, `smp_trampoline_size`, `smp_bsp_is_cpu0`.

#### Fixed
- **Deadlock in `handler_exit`** — double-locking `SCHEDULER` mutex when calling `wake_thread_joiner()`. Inlined the wake call to use the already-held lock.
- **`request_exit_to_kernel()` bug** — read value at GS offset as pointer instead of using `gs_write_u8`. Fixed to use direct GS write.
- **Stale KPRCB offset constants** — 13 offsets after `slab_caches` were 2 bytes too low due to CpuRunQueue alignment (262→264 bytes). Fixed all constants and added compile-time assertions.

## v0.29.0 — 2026-06-07

### A1.5. EPROCESS/KTHREAD — Process/Thread Split
- **Añadido**: `src/scheduler.rs` — `Eprocess` struct (shared resources: handle table, CWD, heap, mmap, thread_count)
- **Añadido**: `src/scheduler.rs` — `Kthread` struct (per-thread CPU context, state, priority, kernel stack, TEB)
- **Añadido**: `ThreadState` enum (`Ready`, `Running`, `Blocked{waiting_for}`, `Terminated`) reemplazando `ProcessState`
- **Añadido**: `sys_thread_create` (RAX=22) — crea nuevo thread en EPROCESS existente
- **Añadido**: `sys_thread_join` (RAX=23) — espera a que un thread termine
- **Añadido**: `Kthread::new_ring3()` / `Eprocess::new_ring3()` / `Scheduler::add_ring3_process()` constructores
- **Añadido**: `add_thread_to_process()` para crear threads adicionales
- **Modificado**: `Scheduler` — `processes[16]` → `eprocesses[16]` + `kthreads[32]`
- **Modificado**: `schedule()` retorna `*mut Kthread` en lugar de `*mut Process`
- **Modificado**: Pipe blocking — `ThreadState::Blocked` + `Scheduler::kthreads` en lugar de `ProcessState`
- **Modificado**: IRP blocking — `current_kthread_mut()` en lugar de `current_process_mut()`
- **Modificado**: `cleanup_terminated_process()` recycles EPROCESS solo cuando último thread termina
- **Modificado**: `find_eprocess`, `find_kthread`, `alloc_*_slot` — ahora son `pub` (acceso externo)
- **Eliminado**: `Process` struct, `ProcessState` enum, `current_process_mut()` — API removed
- **Eliminado**: `scheduler.processes` field — reemplazado por `eprocesses`/`kthreads`
- **Tests**: 4 nuevos tests de Kthread/Eprocess + 9 tests de scheduler adaptados
- **Total**: 330 kernel tests (antes 329)

### A4.2. Syscall dispatch table (SSDT)
- **Añadido**: `src/syscall/table.rs` — `Registers` struct, `SyscallFn` type alias, `MAX_SYSCALL` constant
- **Añadido**: `src/syscall/permission.rs` — `SyscallPermission` struct (caps, ring_min, admin), `CAP_ADMIN` constant
- **Añadido**: `src/syscall/mod.rs` — SSDT `[Option<SyscallFn>; 256]` via `lazy_static!` con 23 handlers + admin stub
- **Añadido**: `src/syscall/mod.rs` — SYSCALL_PERMISSIONS `[SyscallPermission; 256]` tabla paralela de permisos
- **Añadido**: `sys_ndreg` (RAX=50) — admin-only stub para operaciones NDREG desde user-space
- **Añadido**: `check_syscall_permission()` — validación de permisos antes de cada dispatch
- **Modificado**: `syscall_dispatch()` — table-based lookup reemplaza match monolítico
- **Modificado**: `validate_abi()` — itera SSDT para verificar integridad de entradas y permisos
- **Modificado**: `src/syscall.rs` → `src/syscall/mod.rs` — reestructuración a módulo con submódulos
- **Tests**: 5 nuevos tests: `syscall_table_sparse_dispatch`, `syscall_permission_admin_check`, `syscall_enosys_unknown`, `syscall_table_validation_boot`, `syscall_add_new_easy`
- **Total**: 335 kernel tests (antes 330)

## v0.28.0 — 2026-06-06

### MCP Server — Kernel Introspection & VFS Analysis
- **Añadido**: `scripts/mcp_server/` — MCP server completo con 18 tools, 3 resources, 3 prompts.
- **Añadido**: `parsers/neodos_fs.py` — Parser offline de NeoDOS FS (superblock, inodes, directory tree).
- **Añadido**: `parsers/nem_parser.py` — Parser offline de NEM v3 drivers (80B header, relocs, symbols, ABI check).
- **Añadido**: `parsers/elf_parser.py` — Parser ELF64 para DLLs y user binaries (segments, exports, symbols).
- **Añadido**: `tools/kernel_tools.py` — Introspection de kernel (source index, symbol search, build verification).
- **Añadido**: `tools/vfs_tools.py` — Análisis VFS (list, read, stat, tree, superblock, inodes).
- **Añadido**: `tools/module_tools.py` — Análisis de módulos runtime (NEM drivers, DLLs, sys_loadlib sim).
- **Añadido**: `tools/libneodos_tools.py` — Análisis de API libneodos (AbiTable, coverage, ABI check).
- **Añadido**: `tools/system_tools.py` — Consistency checker, invariants, system resource.
- **Añadido**: `scripts/mcp-server.sh` — Launch script con soporte stdio y one-shot --tool.

### A0.1–A0.4. Memory Architecture Rewrite
- **Añadido**: `src/memory/buddy.rs` — Buddy system frame allocator con 11 órdenes (4 KB → 4 MB). `alloc_frames(order)`/`free_frames(addr, order)` — O(log n). Free lists intrusivas en memoria libre. Bitmap como validación secundaria.
- **Añadido**: `src/memory/layout.rs` — MemoryLayout manager dinámico con 32 slots de región. `reserve_region(size, align, flags)` y `reserve_at(base, size, name, flags)` con verificación de solapamiento. `init_default()` replica el layout legacy.
- **Añadido**: `src/memory/mod.rs` — Módulo de memoria unificado. `MemoryMap { total_phys, highest_page }` detectado dinámicamente del memory map UEFI. `validate_layout_consistency()` verifica constantes vs layout en boot.
- **Modificado**: `src/memory.rs` → `src/memory/mod.rs` — Reestructuración a módulo con sub-módulos.
- **Modificado**: `src/handle.rs` — `HandleTable` reescrita con `Vec<HandleEntry>` interno. Sin límite fijo. `Index`/`IndexMut` para compatibilidad con código existente. `MAX_HANDLES` eliminado.
- **Modificado**: `src/scheduler.rs` — Adaptado a nueva `HandleTable` (Vec-based).
- **Modificado**: `src/syscall.rs` — Eliminados bounds checks de `MAX_HANDLES`. Toda la tabla crece dinámicamente.
- **Añadido**: 6 nuevos tests de stress: buddy allocator (4) + handle table (2).
- **Modificado**: `src/testing.rs` — `MAX_TESTS` aumentado de 320 a 400. Stress suite de 8 → 14 tests.
- **Total**: 329 kernel tests + 5 user-mode binaries.

## v0.27.0 — 2026-06-06

### C3. HPET / APIC Timers — Añadido
- **Añadido**: `src/timers/hpet.rs` — HPET driver: detección vía ACPI RSDP/RSDT table scanning (legacy BIOS, EBDA, option ROM, boot-provided address), MMIO register definitions, `init_hpet()` configura timer 0 en modo periódico a 1 KHz con legacy replacement routing a IRQ0.
- **Añadido**: `src/timers/apic.rs` — Local APIC timer driver: detección via `IA32_APIC_BASE` MSR, calibración contra HPET (1 ms one-shot, divider 16), LVT timer en modo periódico, APIC EOI, `init_apic_timer()` deshabilita HPET legacy y enmascara PIC IRQ0 al activarse.
- **Añadido**: `src/timers/mod.rs` — Timer subsystem: `TimerSource` enum, `init()` que prueba HPET → APIC → PIT fallback.
- **Añadido**: `neodos-bootloader/src/main.rs` — RSDP lookup en UEFI configuration tables antes de ExitBootServices; pasa `acpi_rsdp_addr` en BootInfo.
- **Modificado**: `src/hal/x64/time.rs` — `init_system_timer()`, `get_tick_rate()`, `sleep_hint()` con HPET para delays de µs.
- **Modificado**: `src/hal/x64/irq.rs` — `ack_irq()` envía APIC EOI para vector 32 cuando APIC timer activo.
- **Modificado**: `src/scheduler.rs` — `AGING_INTERVAL_TICKS`=500, `MAX_STARVATION_TICKS`=5000 para scheduler a 1 KHz.
- **Modificado**: `src/testing.rs` — `sched_aging_boosts_starved` usa constantes importadas.
- **Total**: 320 kernel tests + 5 user-mode binaries.

## v0.26.0 — 2026-06-05

### W2. Hot reload drivers — Añadido
- **Añadido**: `src/drivers/hotreload.rs` — Nuevo sistema de recarga en caliente de drivers NEM sin reinicio. Sigue el diseño W2.
- **Añadido**: Estado `Unloading = 7` en `DriverState` con transiciones `Active → Unloading → Unloaded → Loaded` (reload path).
- **Añadido**: `EVENT_DRIVER_UNLOAD = 13` y `EVENT_DRIVER_UNLOAD_ACK = 14` en Event Bus.
- **Añadido**: Resource tracking — `ResourceRegistry` global que rastrea bloqueos de dispositivos por driver_id. Hooks en `hst_register_block_device()` y `hst_unregister_block_device()`.
- **Añadido**: Graceful drain — `unload_driver()` llama a `driver_fini()`, envía `EVENT_DRIVER_UNLOAD`, espera ACK con timeout de 100 ticks. Force mode (`/F`) salta espera.
- **Añadido**: `reload_driver()` — lectura de nuevo binario, ABI compatibility check via `negociate_default()`, unload del anterior, load + init + activate del nuevo.
- **Añadido**: `NDREG UNLOAD <name> [/F]` y `NDREG RELOAD <path>` comandos de shell.
- **Añadido**: `init_hot_reload()` en PHASE 3.87 de `main.rs` — registra handler de `EVENT_DRIVER_UNLOAD_ACK`.
- **Añadido**: `register_load_result()` en boot loader y loader para persistir `entry_fini` info.
- **Añadido**: `BlockDeviceManager::remove()` en `block.rs` y `unregister_nem_block_device()` funcional.
- **Añadido**: Errores `ERR_UNLOAD_FAILED = 10` y `ERR_UNLOAD_TIMEOUT = 11` en driver_runtime.
- **Total**: 323 kernel tests + 5 user-mode binaries.

## v0.25.2 — 2026-06-05

### TEST.EXE — libmath.nxl self-test user binary
- **Añadido**: `math_add`, `math_sub`, `math_mul` en `libmath-nxl/src/main.rs` — tres nuevas funciones aritméticas con sus correspondientes entradas en `MathAbiTable`.
- **Añadido**: `userbin/test/` — nuevo proyecto userland (`TEST.EXE`) que carga `libmath.nxl` dinámicamente vía `sys_loadlib` y ejecuta 5 fases: LOAD TEST (carga + resolución de símbolos), BASIC ARITHMETIC TESTS (add, sub, mul, div), EDGE CASES (ceros, negativos, overflow), STRESS TEST (1M iteraciones add(i, i+1)), DETERMINISM (1000 iteraciones idénticas), e INTEGRITY CHECKS (ABI stability cross-call). Imprime reporte PASS/FAIL.
- **Actualizado**: `scripts/build.sh` — añadido `test` a la lista de bins a compilar.
- **Actualizado**: `scripts/create_neodos_image.py` — incluye `TEST.NXE` como inode 12, entry en root directory.
- **Total**: 312 kernel tests + 5 user-mode binaries.

## v0.25.1 — 2026-06-05

### Default file permissions by context — Añadido
- **Añadido**: `NeoDosFs::default_perms_for_filename()` asigna permisos RWXSD según la extensión del archivo al crearse: `.NXE/.COM/.EXE` → `R|X`, `.NEM` → `R`, `.NXL` → `R|X`, `.BAT/.CMD` → `R|X`, `.SYS` → `R`, `.CFG/.INI` → `R|W`, `.TXT/.MD/.LOG` → `R|W`, otros → `R|W`.
- **Modificado**: `create_file_at()` usa `default_perms_for_filename()` en vez de `MODE_FILE` sin permisos.
- **Modificado**: `create_directory_at()` establece `MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D` (permisos completos para directorios).
- **Actualizado**: `scripts/create_neodos_image.py` — la imagen inicial del FS usa los mismos criterios de permisos por extensión (`.nxe` → `R|X`, `.nem` → `R`, `.nxl` → `R|X`, `.sys` → `R`, `.bat` → `R|X`, `.cfg` → `R|W`, `.txt` → `R|W`, directorios → `RWXD`).

## v0.25.0 — 2026-06-05

### X4. Driver Isolation Layer — Añadido
- **Añadido**: `src/drivers/isolation.rs` — Nuevo módulo de aislamiento de drivers con región de 16 MB (0x30000000–0x31000000, 16 slots × 1 MB). Inicialización divide páginas enormes 2 MB en page tables de 4 KB, elimina identity mapping durante init.
- **Añadido**: `allocate_driver_slot()`/`free_driver_slot()` con `DriverMemoryRegion` tracking (driver_id, base, size, isolation_mode). `alloc_isolated_page()`/`free_isolated_page()` para mapeo bajo demanda de páginas 4K en la región aislada.
- **Añadido**: `validate_driver_ptr()`/`validate_driver_str_ptr()` — validación de punteros en export table: acepta región del driver, kernel heap (0x01000000–0x02000000), kernel .rodata/.text (0x00100000–0x01000000), user heap (0x10000000–0x12000000), mmap (0x20000000–0x22000000). Rechaza direcciones fuera de estos rangos.
- **Añadido**: `handle_isolated_page_fault()` — integración con page fault handler para sandbox opcional (DEMAND drivers → FAULTED).
- **Añadido**: `CAP_ISOLATION` (bit 11) en `src/drivers/caps.rs`.
- **Añadido**: Isolation mode por driver (Basic para BOOT/SYSTEM, Sandbox para DEMAND). Set desde `register_driver_ext()` en driver_runtime.
- **Modificado**: `v3loader.rs` — `alloc_driver_memory()` usa región aislada con fallback a heap. `bind_isolated_driver()` asocia driver con slot. `hst_log` y `hst_register_block_device` validan punteros.
- **Modificado**: `boot_loader/mod.rs` — llama a `bind_isolated_driver()` tras registro.
- **Modificado**: `ndreg.rs` — `NDREG SHOW` y `NDREG RUNTIME` muestran isolation mode y región.
- **Modificado**: `testing.rs` — MAX_TESTS incrementado de 300 a 320 para acomodar nuevos tests.
- **Añadido**: 12 tests de isolation (constantes, bounds, alloc/free, driver_id lookup, layout, pointer validation, overflow, max slots, str ptr, mode for category, mode str).
- **Total**: 312 kernel tests, 4 user-mode binaries.

## v0.24.6 — 2026-06-05

### Fixed
- **AHCI port reclaim after NEM AHCI driver init** — NEM AHCI driver's `port_init()` overwrites HBA PORT_CLB/PORT_FB registers with its own buffer addresses, breaking BootAhci DMA for any subsequent uncached filesystem read (DLL loading at PHASE 3.87, DIR, TYPE, etc.). Added `BootAhci::reclaim_ahci_port()` (PHASE 3.86) that stops the port, restores BootAhci's static buffer pointers, clears error status, and restarts the port — called between `boot_load_all()` and DLL loading.
- **stress_syscall_invalid_numbers test** — Updated to test syscall number 22 (instead of 21) since `LoadLib` (RAX=21) is now a valid syscall. All 300 kernel tests pass.

### Total
- 300 kernel tests, 4 user-mode binaries.

## v0.24.5 — 2026-06-05

### Multi-DLL System
- **Añadido**: `sys_loadlib` (RAX=21) — Nueva syscall que carga un DLL desde NeoFS en un slot libre de la región de DLLs (0x1e000000..0x1e200000). Devuelve la dirección base del DLL cargado.
- **Añadido**: `LOADLIB <path>` — Nuevo comando del shell que carga un DLL desde el filesystem usando `dll_load()`.
- **Añadido**: `libmath-nxl/` — Nueva crate que compila una librería de matemáticas como DLL standalone en `0x1e040000` (slot 1). Exporta 17 funciones: `abs`, `abs_f64`, `min`, `max`, `clamp`, `pow`, `modulo`, `div`, `sqrt_int`, `sqrt_f64`, `sin`, `cos`, `tan`, `log2`, `log`, `exp`.
- **Añadido**: `libneodos/src/lib.rs` — Función `loadlib(path)` que invoca `sys_loadlib` y devuelve la dirección base del DLL.
- **Añadido**: `libneodos-nxl/src/main.rs` — `nxl_sys_loadlib` wrapper y campo `sys_loadlib` en `AbiTable`.
- **Modificado**: `scripts/build.sh` — Añadido build step para libmath-nxl.
- **Modificado**: `scripts/create_neodos_image.py` — Añadido `libmath.nxl` al directorio `LIB` en la imagen NeoDOS FS (inode 30).
- **Total**: 301 kernel tests.

## v0.24.4 — 2026-06-04

### X3. Capability System — Añadido
- **Añadido**: `src/drivers/caps.rs` — Nuevo módulo de capabilities con 11 flags (CAP_IRQ=1, CAP_DMA=2, CAP_MMIO=4, CAP_PORTIO=8, CAP_ALLOC_PAGE=16, CAP_BLOCK_DEVICE=32, CAP_EVENT_BUS=64, CAP_INPUT=128, CAP_LOG=256, CAP_TIMING=512, CAP_MEMORY=1024).
- **Añadido**: `CapabilitySet` wrapper con `has()`, `add()`, `remove()`, `format()`, `count()`.
- **Añadido**: `capability_for_category()` — herencia por categoría: BOOT→todas, SYSTEM→PORTIO|IRQ|MMIO|DMA|EVENT_BUS|INPUT|LOG|TIMING, DEMAND→EVENT_BUS|LOG|TIMING.
- **Añadido**: Capability checking en runtime en cada `hst_*` export function (v3loader.rs y hst.rs). Las funciones rechazan la ejecución si el driver no tiene la capability requerida.
- **Añadido**: `current_driver_id()` en `driver.rs` — tracking del driver activo para capability checks.
- **Añadido**: `caps: u64` field en `DriverInstance` + `set_capabilities()`/`get_capabilities()` en driver_runtime.
- **Añadido**: `ERR_CAPABILITY_DENIED=9` — nuevo código de error para denegaciones de capability.
- **Añadido**: `EVENT_CAP_ESCALATION` (type `0x2000`) — escalation policy: SYSTEM puede pedir CAP_ALLOC_PAGE|BLOCK_DEVICE|MEMORY; DEMAND no puede escalar.
- **Añadido**: `NDREG SHOW` ahora muestra capabilities del driver en hex y formato legible.
- **Añadido**: 11 tests de capability system (flags, CapabilitySet, category defaults, check/enforce, escalation policy).
- **Modificado**: `boot_loader/mod.rs` — establece current driver context antes de llamar entry points.
- **Modificado**: `register_v3_event_bus_handler()` — ahora recibe `driver_id` para establecer contexto en dispatch de eventos.
- **Modificado**: `V3HandlerEntry` — incluye `driver_id` para capability checks en event bridge.
- **Total**: 301 kernel tests (+11).

### Chore: DEVICESEND eliminado
- **Eliminado**: `src/shell/commands/devicesend.rs` — comando legacy obsoleto que solo señalaba un flag atómico sin protocolo real. El Event Bus v2 cubre toda la comunicación con dispositivos.

## v0.24.3 — 2026-06-04

### B6b. Shared library system (libneodos DLL) — COMPLETED
- **Añadido**: `libneodos-nxl/` — Nueva crate que compila libneodos como binario standalone (DLL) con tabla de exportación `AbiTable` en sección `.export_table` en dirección fija `0x1e000000`.
- **Añadido**: `neodos-kernel/src/dll.rs` — Subsistema de carga de DLLs: `init_dll_region()` divide páginas enormes 2MB para región de DLL, `dll_load()` carga ELF, `load_dll()` carga `libneodos.nxl` al arrancar (PHASE 3.86). 8 slots de 256 KB cada uno.
- **Añadido**: `neodos-kernel/src/arch/x64/paging.rs` — `set_pd_user_accessible()` para marcar entradas PD como USER_ACCESSIBLE en regiones no-heap/mmap.
- **Modificado**: `libneodos/` — Refactor completo: todas las llamadas a syscall ahora pasan por la export table del DLL (`export::get_table().*`) en lugar de inline asm directo.
- **Añadido**: `libneodos/src/export.rs` — Estructura `AbiTable` mirror del DLL para acceso a funciones exportadas.
- **Añadido**: `sys_chdir` y `sys_getcwd` — wrappers en DLL y thin lib, conectados al kernel vía AbiTable (syscall 16 y 17).
- **Modificado**: `scripts/build.sh` — Añadido build step para libneodos-nxl.
- **Modificado**: `scripts/create_neodos_image.py` — Añadido directorio `LIB` con `libneodos.nxl` en la imagen NeoDOS FS.
- **Modificado**: `.gitignore` — Ignorar `*.nxl`.

## v0.24.2 — 2026-06-04

### V1. Global Page Cache (advanced) — Reescritura completa
- **Reescrito**: `src/buffer/page_cache.rs` — Reemplazado el page cache de 512 entradas con array plano por un sistema avanzado con hash map O(1) + LRU doubly-linked list O(1).
- **Añadido**: Tabla hash open-addressing personalizada (128 slots, FNV-1a, linear probing, tombstones) — búsqueda O(1) por `(inode, block_num)` sin dependencias externas.
- **Añadido**: LRU doubly-linked list — move-to-head O(1) en acceso, evict-from-tail O(1). Reemplaza el scan lineal de 512 entradas.
- **Añadido**: `flush_batch()` — flush asíncrono por lotes (máx 8 páginas por batch), evita flush síncrono completo.
- **Añadido**: `needs_async_flush()` — dirty threshold al 10% de capacidad para trigger automático.
- **Añadido**: `prefetch()` — pre-lectura explícita de bloques contiguos.
- **Añadido**: `stats()` / `hit_rate()` — estadísticas de hit rate, dirty count, pending writes.
- **Añadido**: Readahead adaptativo — detección de acceso secuencial con ventana exponencial (4→32 bloques).
- **Modificado**: `globals.rs` — `flush_cache_if_needed()` usa `flush_batch()` en vez de `flush()` para write-back asíncrono.
- **Modificado**: `main.rs` — Mensaje de init actualizado a "128 × 4 KB = 512 KB, hash + LRU".
- **Modificado**: `testing.rs` — 13 tests de page cache (create, peek, dirty, invalidate, capacity, stats, hit_rate).
- **Mejora**: Reducción de uso de memoria: 512 KB (128 × 4 KB) vs 2 MB (512 × 4 KB) anteriores.
- **Mejora**: Búsqueda O(1) vs O(n) anterior — rendimiento constante independiente del tamaño del cache.

## v0.24.1 — 2026-06-02

### Boot Benchmark & AHCI Performance Fix
- **Añadido**: `boot_benchmark.rs` — Nuevo sistema de profiling de boot con precisión sub-milisegundo (TSC calibrado contra PIT). Registra `KernelEntry`, `StorageInit`, `StorageReady`, `FirstRead`, `FsMounted`, `ShellReady`.
- **Añadido**: Watchdog de boot integrado en el benchmark (timeout global de 60s, per-stage de 15s) para detectar y loggear cuelgues durante la fase crítica de inicialización sin pánicos crípticos.
- **Modificado**: `boot_ahci.rs` — Añadida instrumentación (comandos, tiempo de espera medio/máximo, iteraciones de polling, timeouts, errores DMA).
- **Corregido**: **AHCI Performance Fix** — Se cambió `hlt_once()` por `spin_loop()` en el bucle de polling de DMA (`dma_xfer`) y port reset. `hlt_once` bloqueaba artificialmente la CPU hasta el siguiente tick del sistema (50ms) por cada comando rápido de AHCI, ralentizando el boot drásticamente. El boot en AHCI pasó de ~15 segundos a **~76 ms**.
- **Corregido**: El timeout de polling en AHCI ahora comprueba el tiempo real `elapsed_ms` (cada 10.000 vueltas del spin_loop) en lugar de un contador de iteraciones estático, evitando falsos timeouts tras cargar el SO.
- **Modificado**: `qemu-debug.sh` y `auto_test.py` ahora aceptan los argumentos `--ahci` (por defecto) y `--ata`.
- **Modificado**: `boot_loader/mod.rs` — El boot loader de drivers NEM ahora descarta intentar cargar la inicialización completa de `ahci.nem` si el benchmark detectó que el boot se completó en modo ATA, evitando warnings confusos en el log.
## v0.24.0 — 2026-06-02

### A11. AHCI NEM standalone driver — Añadido
- **Añadido**: `drivers/ahci/` — Nuevo driver NEM v3 standalone AHCI (SYSTEM category). Inicializa HBA, detecta puertos ATA/ATAPI, registra block devices. DMA polling con PRDT.
- **Eliminado**: `neodos-kernel/src/drivers/ahci.rs` — AHCI driver built-in eliminado (reemplazado por NEM standalone).
- **Modificado**: `boot_loader/mod.rs` — Añadido filtro por `DriverCategory` en `collect_driver_data()`: solo carga drivers con category coincidente (BOOT→Boot, SYSTEM→System).
- **Añadido**: `boot_ahci.rs` — BootAhci stub built-in (DMA polling, single port) para early-boot en fase 3. Prioridad: NVMe > BootAhci > BootAta PIO.

### X6. Async I/O (IRP system) — Añadido
- **Añadido**: `src/irp/mod.rs` — Sistema de I/O Request Packets con `IrpOp` (Read/Write/Flush/IoCtl), `IrpStatus` (Pending/Completed/Error), pool global de 64 slots protegido por `Mutex`, IDs únicos por `AtomicU32`.
- **Añadido**: `irp_alloc()`/`irp_free()`/`irp_get_params()`/`irp_complete()` — API completa de ciclo de vida de IRPs. `irp_get_params()` evita doble-lock devolviendo snapshot de parámetros.
- **Añadido**: `irp_complete()` con soporte de: (a) wake-up de proceso vía scheduler integration con `IRP_WAIT_MAGIC`, (b) completion callback diferido a `WORK_QUEUE` high-priority mediante `Box<IrpCbDispatch>`, (c) chaining via `chain_next` field.
- **Añadido**: `IrpQueue` — cola FIFO circular de 32 IrpId para que dispositivos asíncronos encolen operaciones pendientes.
- **Añadido**: BlockDevice trait extendido con `submit_irp()` e `poll_irp()`. `read_blocks`/`write_blocks` se mantienen como métodos abstractos. Todos los drivers (RamDisk, BootAta, AhciDriver, NvmeDriver, NemBlockDevice) implementan `submit_irp`.
- **Añadido**: `irp_block_current()`/`irp_wake_waiter()` — integración con scheduler: procesos se bloquean en un IRP específico con `waiting_for: IRP_WAIT_MAGIC | irp_id` y son despertados por `irp_complete()`.
- **Añadido**: `irp_sync_read()`/`irp_sync_write()` — helpers síncronos que usan IRPs internamente (útiles para código nuevo que quiera el path IRP).
- **Añadido**: 11 tests (alloc/free, status update, error codes, unique IDs, reuse, queue FIFO, queue wraparound, callback dispatch, Flush op, IoCtl op, params extraction). Total: 284 tests.

### X7. Event Bus v2 — Añadido
- **Añadido**: Event Bus v2 unificado con colas separadas por prioridad: cola de alta prioridad (16 slots, lock-free SPSC) para eventos críticos (timers, IRQ completions) y cola de prioridad normal (64 slots) para eventos de sistema.
- **Añadido**: Suscripción con filtro (`EventFilter`) — los handlers se registran con filtro por event_type, source_mask bitfield y device_id. `register_handler_v2()` con filtro estricto; `register_handler()` crea filtro por tipo automáticamente (backward compatible).
- **Añadido**: Backpressure — ambas colas retornan `Err(())` cuando están llenas (productor no sobrescribe). Nueva constante `ERR_EVENT_BUS_FULL` (−16) para drivers NEM.
- **Añadido**: Eventos con payload dinámico (`push_event_with_dyn_payload()`) — copia del payload en heap, puntero almacenado en data0/data1, auto-liberado tras dispatch.
- **Añadido**: Dispatch en `clear_need_resched()` — eventos procesados en cada retorno de syscall (syscall boundary), garantizando dispatch incluso con sistema en carga.
- **Modificado**: `src/eventbus/mod.rs` — eliminada la separación v1/v2. Arquitectura unificada: cola alta (16 slots) + cola normal (64 slots) + tabla de handlers con filtros (64 entradas). Backward compatible: todas las APIs v1 existentes (`push_event`, `register_handler`, `unregister_handler`, `dispatch_pending`, `dispatch_one`) mantienen su firma.
- **Modificado**: `src/eventbus/v2.rs` — eliminado (contenido migrado a mod.rs).
- **Modificado**: `src/syscall.rs::clear_need_resched()` — añadido `EVENT_BUS.dispatch_pending()` para procesar eventos en cada syscall return.
- **Modificado**: Event struct sin cambios (ABI-stable para drivers NEM v3).
- **Añadido**: 8 nuevos tests: priority_order, filter_by_type, strict_filter, unregister_by_name, high_queue_overflow, dyn_payload_lifecycle, filter_wildcard, filter_source_mask.
- **Total**: 273 kernel tests + 4 user-mode binaries.

## v0.23.2 — 2026-06-02

### X5. Deferred work queues — Añadido
- **Añadido**: Sistema de bottom-half (work queues) para ejecución diferida de trabajo fuera del contexto de IRQ.
  Dos niveles de prioridad: (1) **High-priority** procesada en `clear_need_resched()` (syscall return path), y
  (2) **Low-priority** procesada en el idle loop del scheduler.
- **Añadido**: `src/work_queue.rs` — implementación lock-free SPSC ring buffer (64 slots por nivel)
  con `WorkQueueManager` global y API `push_high()`/`push_low()`/`process_high()`/`process_low()`.
- **Modificado**: `scheduler.rs` idle loop — procesa high-priority y low-priority work queues
  antes de `EVENT_BUS.dispatch_pending()`.
- **Modificado**: `syscall.rs::clear_need_resched()` — procesa high-priority work queue en cada
  retorno de syscall (interruptores ya deshabilitados en handler int 0x80).
- **Añadido**: 6 tests de work queue: push/pop, FIFO order, empty, overflow, high/low isolation,
  pending flag.
- **Total**: 265 kernel tests + 4 user-mode binaries.

## v0.23.1 — 2026-06-02

### Bugfix: User-mode callee-saved register corruption
- **Corregido**: `exit_to_kernel` ahora restaura registros callee-saved (rbx, r12-r15, rbp) que el proceso usuario pisaba, corrompiendo las variables locales del shell (PID, filename). Fix: guardar/restaurar en `execute_usermode_asm`/`exit_to_kernel` (`usermode.rs`).
- **Corregido**: Race condition en `sys_exit`: `request_exit_to_kernel()` se llamaba fuera de `without_interrupts`, permitiendo que un timer IRQ se disparara antes de que `EXIT_NOW=1`, causando GPF en la cadena de retorno. Fix: mover la llamada dentro del closure (`syscall.rs`).
- **Total**: 259 kernel tests + 4 user-mode binaries.

## v0.23.0 — 2026-05-29

### A2. Priority Scheduler — Añadido
- **Añadido**: Sistema de 4 niveles de prioridad (`PRIORITY_HIGH`, `PRIORITY_ABOVE_NORMAL`, `PRIORITY_NORMAL`, `PRIORITY_IDLE`) con time-slicing dinámico (400/200/100/50 ticks).
- **Añadido**: `schedule()` ahora selecciona procesos por nivel de prioridad (HIGH→IDLE), round-robin dentro del mismo nivel.
- **Añadido**: `on_timer_tick()` decrementa `time_slice_remaining` cada tick; al expirar, marca el proceso Ready y dispara `NEED_RESCHED`.
- **Añadido**: Preemption desde Ring 3 en `timer_handler_inner`: detecta CS=0x1B, guarda RSP, llama `schedule()`, cambia TSS.RSP0, retorna nuevo RSP.
- **Añadido**: Aging cada 100 ticks: procesos Ready sin ejecutar por >= 1000 ticks reciben boost de prioridad (evita starvation).
- **Añadido**: `sys_yield` (RAX=2) implementado correctamente: Running→Ready + reseteo de time slice + `NEED_RESCHED`.
- **Añadido**: 7 tests de scheduler: prioridad, round-robin, time-slice, aging.
- **Modificado**: `Process` struct: nuevos campos `priority`, `time_slice_remaining`, `ticks_since_scheduled`.
- **Modificado**: `Process::new_ring3()` asigna `PRIORITY_NORMAL` por defecto.
- **Añadido**: `PRI` shell command — cambia la prioridad de un proceso en tiempo de ejecución.
- **Añadido**: `sched_set_process_priority()` en `Scheduler` (validación de rango, reseteo de time slice).
- **Añadido**: Columna `PRI` en salida de `PS` (H/AN/N/I para niveles de prioridad).
- **Añadido**: `CPUTEST.NXE` — binary user-mode para tests de prioridad (CPU-bound, cuenta hasta 200M).
- **Añadido**: Test `sched_set_process_priority` en suite de scheduler.
- **Total**: 256 kernel tests + 4 user-mode binaries.

## v0.22.0 — 2026-05-29

### ATA NEM Standalone Driver — Añadido
- **Añadido**: `drivers/ata/` — NEM v3 standalone driver for ATA storage (SYSTEM category). Scans PCI for IDE controller with bus-master DMA capability, initializes primary + secondary channels, supports DMA read/write (via PRDT) and PIO multi-sector fallback. Each active channel registers a `NemBlockDevice` via `hst_register_block_device()`.
- **Añadido**: `drivers/block.rs` — `NemBlockDevice` struct wrapping NEM driver callbacks as a `BlockDevice` trait. `register_nem_block_device()` / `unregister_nem_block_device()` public API.
- **Añadido**: `v3loader.rs` — kernel export `hst_register_block_device()` and `hst_unregister_block_device()` for NEM drivers to register block devices with the kernel's `BlockDeviceManager`.
- **Modificado**: `ata.rs` (kernel) — reducido a `BootAta` PIO-only boot stub (primary channel only, no DMA). Used during early boot for GPT parsing, superblock read, and block cache warmup before NEM drivers load.
- **Modificado**: `storage_manager.rs` — simplificado: NVMe → AHCI → ATA boot stub priority. Removed legacy `find_ide_controller()` and `enable_bus_master()` inline PCI scan (now handled by the standalone NEM ATA driver).
- **Modificado**: `block.rs` — removed `AtaWithAhciFallback` wrapper. `BootAta` directly implements `BlockDevice`.
- **Modificado**: `scripts/build.sh` — añadida compilación de `ata.nem` via `build_nem.py`.
- **Modificado**: `scripts/create_neodos_image.py` — añadido `ata.nem` a la imagen del sistema de archivos NeoDOS.
- **Modificado**: `scripts/qemu-debug.sh` — cambiado `-machine q35` a `-machine pc` (PIIX3) para compatibilidad con controlador IDE.
- **Eliminado**: ATA bus-master DMA inline code (DMA buffers, PRDT, PCI scan) — movido al standalone NEM driver.
- **Categoría**: SYSTEM (cargado desde `C:\SYSTEM\DRIVERS\SYSTEM\`).
- **Total**: 248 kernel tests + 4 user-mode binaries.

## v0.21.0 — 2026-05-28

### PCI NEM Driver — Añadido
- **Añadido**: `drivers/pci/` — NEM v3 standalone driver para configuración PCI. Escanea el bus 0 al iniciar y lista todos los dispositivos encontrados (vendor, device, clase, subclass, prog-if, revisión).
- **Añadido**: Servicio Event Bus para otros drivers NEM: `EVENT_PCI_READ_CONFIG` (0x1000) y `EVENT_PCI_WRITE_CONFIG` (0x1001) con respuestas `EVENT_PCI_READ_RESULT` (0x1002) y `EVENT_PCI_WRITE_DONE` (0x1003).
- **Modificado**: `drivers/pci.rs` (kernel) — reducido a solo 4 primitivas de acceso al espacio de configuración PCI (`pci_config_read/write_dword/word`).
- **Modificado**: `storage_manager.rs` — `find_ide_controller()` y `enable_bus_master()` movidos inline desde el módulo PCI.
- **Modificado**: `nvme.rs` — `find_nvme_controller()` y `nvme_enable()` movidos inline.
- **Eliminado**: `pci::find_acpi_pm1_cnt_port()` — código muerto (ACPI NEM driver ya tiene su propia detección PCI).
- **Categoría**: SYSTEM (cargado desde `C:\SYSTEM\DRIVERS\SYSTEM\`), Lifecycle type (2).
- **Total**: 245 kernel tests + 4 user-mode binaries.

## v0.20.0 — 2026-05-28

### A5. Global Page Cache — Añadido
- **Añadido**: `src/buffer/page_cache.rs` — Central 4 KB page cache (512 entries × 4 KB = 2 MB) for filesystem file data I/O.
- **LRU eviction**: `find_lru()` scans for oldest `last_access` entry; prefers invalid slots.
- **Dirty write-back**: `flush()` writes all dirty pages via `dev.write_blocks()`. `flush_inode()` flushes one inode.
- **Read/write integration**: NeoFS `read_file_to_buf()`, `read_file()`, and `write_file()` now take `&mut PageCache` and go through the cache (8 sectors at a time via `read_page()`/`get_page_mut()`).
- **`with_page_cache()`**: Public accessor in `globals.rs` — `PAGE_CACHE` global behind `spin::Mutex`.
- **Timer-driven flush**: `NEED_PAGE_CACHE_FLUSH` atomic set every 180 ticks in timer IRQ, flushed in `flush_cache_if_needed()` alongside existing `NEED_CACHE_FLUSH`.
- **mmap integration**: `load_file_mmap_page()` checks PageCache first before falling back to VFS read.
- **Optimización**: Hizo `PageCache::new()` un `const fn` para evitar un temporal de 2 MB en la pila de `rust_start`, que causaba un page fault al arrancar.
- **Tests**: 8 unit tests (create_empty, peek_miss, mark_dirty, invalidate_noop, invalidate_multiple, entry_count_bounds, dirty_count, peek_returns_none).
- **Total**: 245 kernel tests + 4 user-mode binaries.

## v0.19.0 — 2026-05-28

### ACPI Poweroff Driver — Añadido
- **Añadido**: `drivers/acpi/` — NEM v3 standalone driver for ACPI S5 poweroff. Scans PCI for PIIX4 (0x7113) / ICH9 (0x2918/0x2916) LPC bridges, detects PM1a port via GPIO/ABASE registers, and writes `SLP_TYP_S5 | SLP_EN` to trigger soft-off.
- **Añadido**: Fallback poweroff ports — QEMU Bochs (0x604, 0x2000) and PS/2 keyboard reset (0x64, 0xFE) in cascade after ACPI S5.
- **Añadido**: `EVENT_SHUTDOWN = 12` to event bus constants. `POWEROFF`/`SHUTDOWN`/`EXIT` shell command pushes event → ACPI driver dispatches → HAL poweroff fallback.
- **Añadido**: `-no-reboot` flag to `scripts/qemu-debug.sh` so QEMU exits on guest shutdown.
- **Añadido**: ACPI match arm in boot loader (`register_v3_event_bus_handler` for `EVENT_SHUTDOWN`).
- **Modificado**: `shell/commands/shutdown.rs` — calls `hal::poweroff()` after event dispatch as final fallback (replaced bare HLT loop).
- **Eliminado**: `neodos-kernel/src/drivers/acpi.rs` — legacy RSDP/RSDT/FADT parser (replaced by NEM driver PCI-based detection).
- **Tests**: 237 kernel tests + 4 user-mode binaries (previous count before v0.20.0).

### PS/2 Double-Character Fix — Corregido
- **Corregido**: Boot loader fallthrough `_` arm registered `v3_event_bridge` for `EVENT_KEYBOARD_INPUT` with unknown drivers' `driver_on_event`. This created a duplicate event bus handler that called `process_scancode` twice per keystroke → every character appeared doubled (e.g. `tteesstt`).
- **Fix**: Changed `_` arm to `true` (bind without registering any handler). Known drivers (PS2KBD, SERIAL, RTC, ACPI) have explicit match arms.

## v0.18.0 — 2026-05-27

### X1. Kernel Object Manager (KOBJ) — Añadido
- **Añadido**: `src/kobj/mod.rs` — KOBJ core module. Unified kernel object system with reference counting, type identification, and metadata tracking.
- **KObjType**: Enum with 9 types (Unknown, Process, Driver, Device, Pipe, EventBus, BlockDevice, Filesystem, MemoryRegion).
- **KObjEntry**: Per-object metadata (KObjId, refcount, type, 24-byte name, flags, creation tick, native_id).
- **KObjRegistry**: 64-slot thread-safe registry protected by `spin::Mutex`. Register, unregister, lookup, ref_inc, ref_dec, iteration.
- **Public API**: `kobj_register()`, `kobj_unregister()`, `kobj_ref()`, `kobj_unref()`, `kobj_lookup()`, `kobj_count()`, `kobj_iter_snapshot()`.
- **Integración**: Processes registered on creation (`scheduler.rs`), unregistered on kill/exit. Drivers registered on load (`driver_runtime.rs`), unregistered on remove. Pipes registered on alloc (`pipe.rs`), unregistered on free.
- **Shell**: `KOBJ` command lists all registered kernel objects (ID, type, name, refcount, native ID).
- **Tests**: 8 tests (register/unregister, refcount, type enum, entry name, registry full, lookup, double unregister, count).
- **Total**: 237 kernel tests + 4 user-mode binaries.

## v0.17.2 — 2026-05-27

### X2. Unified Handle Table — Añadido
- **Añadido**: `src/handle.rs` — Unified handle table module. Per-process resource abstraction replacing `FdEntry`/`FdTable`.
- **Handle types**: CLOSED, STDIN, STDOUT, STDERR, PIPE_READ, PIPE_WRITE, FILE, DEVICE, EVENT.
- **File handles**: store drive+inode+per-open offset cursor for independent read/write positioning.
- **sys_open**: now returns a small integer fd (handle index) instead of packed `(drive<<32)|inode`.
- **sys_readfile / sys_writefile**: take fd instead of packed handle; respect per-handle offset.
- **sys_close**: handles all resource types (pipes, files, devices, events).
- **sys_mmap** (file-backed): takes fd instead of packed handle.
- **Modificado**: `scheduler.rs` — `Process.fd_table` → `Process.handle_table`.
- **Modificado**: `pipe.rs` — removed `FdEntry`, `FdTable`, FD_* constants (moved to handle.rs).
- **Modificado**: `libneodos` — `File` struct uses `u8` fd, `sys_open` returns `u8`.
- **Modificado**: user binaries `filetest`, `systest`, `alltest` — use fd-based API.
- **Total**: 233+ kernel tests + 4 user-mode binaries.

## v0.17.1 — 2026-05-26

### Device Model + TSR Removal — Eliminado
- **Eliminado**: `src/devices/mod.rs` — Device Model + HAL Binding Layer v0.3 (replaced by direct NEM v3 driver model + Event Bus + HAL ABI v0.3)
- **Eliminado**: `src/tsr/mod.rs` — TSR (Terminate-and-Stay-Resident) module system (legacy, superseded by NEM v3 driver framework)
- **Eliminado**: `src/shell/commands/devices.rs` — DEVICES shell command
- **Eliminado**: `src/shell/commands/tsr.rs` — TSR shell command
- **Modificado**: `globals.rs` — removed `DEVICE_REGISTRY` global
- **Modificado**: `main.rs` — removed `devices::register_boot_devices()` call
- **Modificado**: `handler.rs` — removed TSR and DEVICES command entries
- **Modificado**: `idt.rs` — removed `tsr::dispatch_interrupt(0x1C)` from timer handler
- **Total**: 229 kernel tests + 4 user-mode binaries (unchanged)

## v0.17.0 — 2026-05-26

### W1. ABI Negotiation Layer — Añadido
- **Añadido**: `src/drivers/abi/mod.rs` — ABI version negotiation formalizada entre kernel y drivers NEM. `AbiVersion` struct, `NegotiationResult` enum (Compatible/CompatibleWithWarnings/Incompatible), `negotiate()` con overlap window check y niveles de warning.
- **Integrado**: v3loader `validate_v3_abi()` ahora delega en `drivers::abi::negotiate_default()`.
- **Tests**: 10 tests unitarios (válido, demasiado nuevo, demasiado antiguo, campos cero, out-of-order, warnings).

### W4. Driver Dependency Resolver — Añadido
- **Añadido**: `src/drivers/dependency/mod.rs` — Resolución automática de dependencias entre drivers NEM. `DependencyGraph` con topological sort DFS y detección de ciclos.
- **Convención**: Drivers declaran dependencias mediante símbolos `__dep_DRIVERNAME` en la symbol table NEM. `resolve_nem_symbol_dependencies()` extrae deps automáticamente.
- **Integrado**: Boot loader v2 escanea drivers, construye grafo de dependencias y carga en orden topológico por categoría.
- **Tests**: 13 tests unitarios (empty, simple, chain, diamond, ciclo, missing dep, case insensitivity, multi-driver).

### Boot Loader v2
- **Actualizado**: `src/drivers/boot_loader/mod.rs` — `boot_load_all()` v2 usa `DependencyGraph` para ordenar carga dentro de cada categoría (BOOT/SYSTEM). ABI validation delegada al módulo ABI negotiation.
- **Tests**: +2 tests (collect_driver_data_empty, build_dep_graph_empty).

### Total
- **Nuevos tests**: 25 (10 ABI + 13 dependency + 2 boot loader)
- **Total**: 229 kernel tests + 4 user-mode binaries
- **Bump**: v0.17.0

## v0.16.8 — 2026-05-26

### Kernel Slab Allocator (A3) — Añadido
- **Añadido**: `src/slab.rs` — slab allocator con 9 size classes (8, 16, 32, 64, 128, 256, 512, 1024, 2048 bytes). O(1) alloc/free mediante free list de u16 indices dentro de páginas de 4 KB. Cada SlabPage tiene header de 32 bytes con magic "SLAB" + metadatos de lista libre.
- **Añadido**: `allocator.rs` reescrito para usar `SlabAllocator` como `#[global_allocator]`, con `linked_list_allocator::LockedHeap` como fallback para objetos >2 KB o alineación >16 bytes.
- **Añadido**: `memory::reserve_range()` — función pública para marcar rangos de frames como usados, evitando colisiones entre slab pages y el heap del fallback.
- **Añadido**: 9 tests slab: `slab_box_u8`, `slab_box_u64`, `slab_box_many_small`, `slab_box_many_64`, `slab_box_large_fallback`, `slab_string_heap`, `slab_vec_u32`, `slab_mix_sizes`, `slab_free_reuse`.
- **Total**: 204 kernel tests + 4 user-mode binaries

## v0.16.7 — 2026-05-25

### libneodos (S6) — Añadido
- **Añadido**: `libneodos/` — standard library para procesos Ring 3 en Rust
- **Añadido**: `libneodos/src/syscall.rs` — wrappers seguros para todas las syscalls (exit, write, read, open, readfile, writefile, close, brk, mmap, munmap, yield, getpid) con inline asm `int 0x80`
- **Añadido**: `libneodos/src/io.rs` — módulo IO con Stdout/Stdin/Stderr, implementación `core::fmt::Write` para formatted output, funciones `_print`/`_eprint` con buffer stack de 1024 bytes
- **Añadido**: `libneodos/src/fs.rs` — módulo FS con `File::open()`, `File::read()`, `File::write()` sobre handles devueltos por sys_open
- **Añadido**: `libneodos/src/mem.rs` — módulo memoria con `brk()`, `sbrk()`, `mmap()`, `munmap()`, constantes `PROT_READ`, `PROT_WRITE`, `MAP_ANONYMOUS`
- **Añadido**: `libneodos/src/macros.rs` — macros `print!`, `println!`, `eprint!`, `eprintln!` con soporte CRLF
- **Añadido**: `libneodos/src/lib.rs` — panic handler que llama `sys_exit(1)`
- **Añadido**: `libneodos/user.ld` — linker script de referencia para compilar ELF64 a 0x400000
- **Añadido**: `userbin/hello_lib/` — sample user binary en Rust que demuestra el uso de libneodos (print, getpid, yield, file read, sys_exit)
- **Total**: 196 kernel tests + 4 user-mode binaries + libneodos compilado

## v0.16.6 — 2026-05-25

### NEM v3 Serial Driver (COM1 IRQ4) — Añadido
- **Añadido**: `drivers/serial/` — NEM v3 serial driver para COM1 con soporte IRQ4 (RX data vía Event Bus `EVENT_SERIAL_DATA`). driver_init() reconfigura UART 16550A (38400 baud, 8N1, FIFO 14 bytes, RDA interrupt habilitado). driver_on_event() recibe bytes seriales y hace loopback por THR.
- **Añadido**: `scripts/build.sh` — compila serial driver a `SYSTEM/serial.nem` en el paso `--neodos-image`
- **Añadido**: `scripts/create_neodos_image.py` — inodo 22 para serial.nem, data blocks en bloque 23+, entrada en directorio SYSTEM
- **Modificado**: `arch/x64/pic.rs` — master PIC mask cambiado de 0xF8 a 0xE8 (IRQ4 desenmascarado)
- **Modificado**: `arch/x64/idt.rs` — añadido `serial_handler` en IDT[36] (IRQ4) con while-loop que drena FIFO y envía `EVENT_SERIAL_DATA` al Event Bus. `ack_irq(36)` envía EOI al master PIC.
- **Modificado**: `devices/mod.rs` — com1 registrado con `CAP_IRQ` y `irq=Some(36)`
- **Modificado**: `drivers/boot_loader/mod.rs` — serial driver registrado en Event Bus para `EVENT_SERIAL_DATA` durante boot
- **Corregido**: `drivers/nem/v3loader.rs` — **BUG CRÍTICO**: `V3_EVENT_FN` era un único AtomicUsize global sobrescrito al cargar el segundo driver v3 (serial), causando que todos los eventos de teclado se enrutaran al driver serial y se perdieran silenciosamente. Reemplazado por una tabla de dispatch (`V3_HANDLERS` con `MAX_V3_HANDLERS=8` entradas) que busca el handler correcto por `event_type`. El bug existía desde la implementación de v3 bridge (v0.16.0) pero era invisible con un solo driver.
- **Total**: 195 tests kernel + 4 user-mode binaries

## v0.16.4 — 2026-05-23

### FSCK utility (S5) — Añadido
- **Añadido**: `src/fs/fsck.rs` — módulo de verificación de integridad NeoDOS
- **Añadido**: Superblock validation (magic, block_size, num_blocks, num_inodes, label length)
- **Añadido**: Inode table integrity checks (mode bits, inode_num mismatch, block pointer bounds)
- **Añadido**: Cross-linked block detection via block ownership map
- **Añadido**: Directory tree walk with cycle protection (MAX_DIR_DEPTH=32)
- **Añadido**: Orphan inode detection (inodes not reachable from root)
- **Añadido**: Dangling directory entry detection and entry-type vs mode mismatch
- **Añadido**: Repair mode (`FSCK /F`) — restores superblock, clears invalid modes, removes cross-links, frees orphans, deletes dangling entries, flushes cache
- **Añadido**: `cmd_fsck` — shell command `FSCK` with `[drive:]` and `/F` support
- **Añadido**: 6 unit tests for validation helpers (mode, block ptr, block count, is_used, range)
- **Total**: 196 tests kernel + 4 user-mode binaries

## v0.16.3 — 2026-05-23

### Process exit full cleanup (S7) — Modificado
- **Añadido**: `Process::take_kernel_stack()` — método público para tomar y liberar `Box<AlignedKStack>`
- **Añadido**: `Scheduler::recycle_terminated(pid)` — remueve proceso Terminated de la tabla, liberando kernel stack, cwd_path y demás owned resources
- **Añadido**: `scheduler::cleanup_terminated_process(pid)` — wrapper público con `without_interrupts`
- **Modificado**: `kill_pid()` — ahora libera heap, mmap, pipes, user slot y kernel stack, y recicla el slot inmediatamente
- **Modificado**: `cmd_run()` — llama a `cleanup_terminated_process()` tras `wait_for_process()` para reciclar slot y kernel stack al salir
- **Modificado**: `sys_waitpid` — recicla slot del proceso esperado tras detectar Terminated
- **Total**: 190 tests kernel + 4 user-mode binaries

## v0.16.2 — 2026-05-23

### IPC / Pipes (S2) — Añadido
- **Añadido**: `src/pipe.rs` — PipeManager con 16 buffers de 4 KB + refcounting automático
- **Añadido**: Per-process `fd_table[16]` en Process, con FdEntry (stdin/stdout/pipe reader/pipe writer)
- **Añadido**: `sys_pipe` (RAX=5) — crea pipe, devuelve [read_fd, write_fd]
- **Añadido**: `sys_dup2` (RAX=6) — duplica fd para redirección stdin/stdout
- **Modificado**: `sys_read` (RAX=4) — soporta pipe reader fds, bloquea con -EAGAIN vía scheduler
- **Modificado**: `sys_write` (RAX=1) — soporta pipe writer fds y fd como primer argumento
- **Modificado**: `sys_close` (RAX=13) — cierra pipe fds (decrementa refcount, libera pipe si refs=0)
- **Modificado**: `syscall_try_resched` — ya no sobreescribe estado Blocked
- **Añadido**: 13 pipe tests: alloc/free, write/read, EOF, EPIPE, blocking, fd table
- **Total**: 190 tests kernel + 4 user-mode binaries

## v0.16.1 — 2026-05-23

### Memory-mapped files (A4) — Añadido
- **Añadido**: `MmapRegion` struct + VMA list per-process en `scheduler.rs`
- **Añadido**: `sys_mmap` (RAX=19) — lazy mapping: solo registra VMA, páginas al page fault
- **Añadido**: `sys_munmap` (RAX=20) — libera páginas físicas y elimina VMA
- **Añadido**: Región mmap dedicada 0x20000000..0x22000000 (32 MB) con demand paging
- **Añadido**: Soportes: anónimo (zero-filled lazy) y file-backed (lazy loading desde NeoFS)
- **Añadido**: `handle_mmap_page_fault()` en page fault handler para resolución on-demand
- **Añadido**: `Vfs::stat()` wrapper público, `Vfs` ahora exporta `stat(drive, inode)`
- **Añadido**: `is_user_ptr_valid()` extendido para cubrir regiones mmap
- **Añadido**: 6 tests mmap: estructura, flags, direcciones, VMA add/remove
- **Añadido**: sys_exit ahora libera todas las regiones mmap del proceso
- **Modificado**: syscall trampoline pasa R8/R9 como arg4/arg5 (nuevos parámetros mmap)
- **Modificado**: `syscall_dispatch` firma: 6 argumentos (rax, rbx, rcx, rdx, r8, r9)
- **Total**: 177 tests kernel + 4 user-mode binaries

## v0.16.0 — 2026-05-23

### Driver Certification Pipeline v1
- **Añadido**: State machine de 7 estados: Loaded → Initialized → Registered → Bound → Active + Faulted + Unloaded
- **Añadido**: `try_transition()` con validación estricta — solo transiciones secuenciales permitidas
- **Añadido**: `certify_and_activate()` — solo activa driver si completó todas las 5 etapas
- **Añadido**: `last_error: u32` + `certification_step: u8` en `DriverInstance` (9 códigos de error)
- **Añadido**: `inactive_reason()` — diagnóstico humano de por qué un driver no es ACTIVE
- **Añadido**: `pipeline_progress()` — array de 5 bools mostrando progreso del pipeline
- **Añadido**: `PipelineStep` enum — tracking de qué etapa falló (LOAD/INIT/REGISTER/BIND/CERTIFY)
- **Añadido**: `state_counts()`, `loaded_count()`, `faulted_count()` — desglose por estado
- **Modificado**: `active_count()` ahora solo cuenta ACTIVE (no "not Unloaded")
- **Modificado**: `drivers/nem/loader.rs` — pipeline completo con transiciones en cada etapa
- **Modificado**: `drivers/driver_loader.rs` — legacy loader deja driver en LOADED (no init)
- **Añadido**: `NDREG DEBUG <name>` — checklist de 5 pasos diagnósticos LOADED≠ACTIVE
- **Añadido**: Pipeline visual `█████` en NDREG LIST/RUNTIME (progreso L-I-R-B-A)
- **Añadido**: 21 tests de state machine: transiciones válidas/inválidas, certify, error tracking, counts, pipeline_progress
- **Total**: 171 tests kernel + 4 user-mode binaries

## v0.15.0 — 2026-05-21

### ELF64 Loader — Añadido
- **Añadido**: `src/elf.rs` — ELF64 loader (header validation, PT_LOAD segment loading, .bss zero-fill)
- **Añadido**: Auto-detección ELF vs flat binary en `cmd_run` (por magic `\x7fELF`)
- **Añadido**: 7 tests ELF64 (header validation, invalid magic/class/machine, truncated header, segment loading, bad phentsize)
- **Añadido**: `userbin/generate_hello_elf.py` — genera `hello.elf` (ELF64 equivalente a `hello.nxe`)
- **Añadido**: `hello.elf` incluido en imagen NeoDOS FS
- **Total**: 150 tests kernel + 4 user-mode binaries

### Syscall ABI Stabilization (S1)
- **Añadido**: `SyscallNum` enum con `from_u64()` — mapeo declarativo de números a syscalls
- **Añadido**: `SyscallError` enum (16 códigos: Inval, NoEnt, NoMem, Acces, BadF, Fault, NoSys, Again, Pipe, Exist, NotDir, IsDir, Io, NoDev, Busy)
- **Añadido**: `err_to_u64()` — codifica errores como u64 negativo (NoEnt→`0xFFFF_FFFF_FFFF_FFFE`)
- **Añadido**: `syserr!` macro — retorno limpio de errores desde handlers
- **Añadido**: `validate_abi()` — assert boot-time de todos los números y codificaciones
- **Modificado**: `syscall_dispatch` reescrito como `match num { SyscallNum::Xxx => ...}` en lugar de `match rax`
- **Modificado**: `sys_read` usa `input::pop_byte()` en vez del buffer interno del teclado
- **Eliminado**: `[SYS]` debug logs redundantes de paths exitosos
- **Eliminado**: doble-print (`[user]` prefix) en sys_write
- **Total**: 150 tests kernel + 4 user-mode binaries

## v0.14.0 — 2026-05-21

### Event Bus v1 + 9 tests + 143 total
- **Añadido**: `src/eventbus/mod.rs` — Event Bus v1 subsystem
- **Añadido**: `Event` structure (`#[repr(C)]`, event_id, type, source, timestamp, device_id, data0/data1, flags) — 56 bytes, monotonic ID
- **Añadido**: Lock-free SPSC ring buffer (64 slots) for IRQ-safe event injection
- **Añadido**: 11 event types (TIMER_TICK, KEYBOARD_INPUT, SERIAL_DATA, DISK_IO_COMPLETE, PROCESS_EXIT, DRIVER_LOADED, DRIVER_CRASH, POLICY_VIOLATION, FS_MOUNTED, USER)
- **Añadido**: 4 event sources (HAL, DRIVER, KERNEL, USERLAND)
- **Añadido**: `register_handler()` / `unregister_handler()` — driver callback registration (max 32)
- **Añadido**: `dispatch_one()` / `dispatch_pending()` — scheduler-controlled dispatch
- **Añadido**: `push_event()` in IRQ handlers (TimerTick→PIT IRQ, KeyboardInput→PS/2 IRQ)
- **Añadido**: `EVENT_BUS.dispatch_pending()` in idle loop (scheduler integration)
- **Añadido**: 9 Event Bus tests: create, push/pop, ordering, overflow, monotonic ID, handler register/dispatch, type filter, unregister, empty queue
- **Total**: 143 tests kernel + 4 user-mode binaries

### Command history + HELP system + NeoFS tests + Bugfixes

- **Añadido**: Historial de comandos — ↑/↓ navegan historial circular (32 entradas). Flechas emitidas como 0x01/0x02 desde el driver PS/2
- **Añadido**: `usage` field en `CommandEntry` con texto detallado por comando
- **Añadido**: `HELP <comando>` muestra ayuda detallada (ej: `HELP DIR`)
- **Añadido**: `DIR /?`, `TYPE -h`, `CD --help` etc. — `/?, -h, --help` funciona en todos los comandos automáticamente
- **Añadido**: 21 nuevos tests NeoFS (75 total): permission rendering (7), all 32 5-bit combinations, upper-bit isolation, timestamp boundaries/independence, DirectoryEntry max name/all attrs/inode_num edge cases, Inode all-fields-max/mixed, corruption byte-flip (Inode + DirectoryEntry), deterministic LCG serialization stress (500 iter each), mode field full u16 cycle
- **Añadido**: `PERM_R/W/X/S/D` constantes públicas en `neodos_fs.rs` (test-local copies eliminadas)
- **Añadido**: Comando `DIR` muestra permisos `RWXSD` vía `fmt_perms()` en `dir.rs`
- **Añadido**: `normalize_path()` en `cd.rs` — resuelve `..`, `.`, separadores duplicados
- **Corregido**: `neofs_dirent_all_attribute_bits` — `copy_from_slice` con 3-byte source en slice de 4 bytes
- **Corregido**: `neofs_perm_render_with_file_mode` — esperaba `--XSD` pero no incluía `PERM_D`
- **Corregido**: `neofs_corrupt_inode_flip_byte` — flip en byte 2 corrompía `inode_num`, cambiado a padding
- **Corregido**: `0..65536u16` → `0..=65535u16` (u16 overflow)
- **Total**: 120 tests kernel + 4 user-mode binaries

## v0.15.2 — 2026-05-20

### DIR permissions display + NeoFS test constants públicas

- **Añadido**: Constantes `PERM_R`/`PERM_W`/`PERM_X`/`PERM_S`/`PERM_D` en `neodos_fs.rs` (bits 0-4 del campo `mode`, coexisten con MODE_DIR/MODE_FILE)
- **Añadido**: El comando `DIR` muestra permisos en formato `RWXSD` (guión por permiso ausente)
- **Migrado**: Tests de permisos NeoFS usan las constantes públicas de `neodos_fs.rs` en vez de locales
- **Corregido**: Test `neofs_dirent_invalid_entry_type` — `copy_from_slice` fallaba por mismatch de longitud (3-byte "BAD" en slice de 4 bytes)
- **Total**: 99 tests kernel + 4 user-mode binaries

## v0.15.1 — 2026-05-20

### NeoFS Metadata Validation Test Suite

- **Añadido**: 36 tests de metadatos NeoFS en testing.rs (10 categorías)
- **Cubierto**: mode (FILE/DIR), timestamps (atime/mtime/ctime), serialización round-trip
- **Cubierto**: DirectoryEntry attributes (DOS attrs: R, H, S, V, D, A)
- **Cubierto**: inode_block_count pure function (edge cases: empty, cross-block, max, root dir)
- **Cubierto**: corruption/edge cases (zero-length name, max values, extra bits en mode)
- **Cubierto**: stress (toggle mode, uid cycle, timestamp churn)
- **Total**: 81 tests kernel + 4 user-mode binaries

## v0.15.0 — 2026-05-20

### Storage Manager — init estructurado + limpieza de globals legacy

- **Añadido**: `drivers/storage_manager.rs` — orquestador de inicialización de almacenamiento
- **Refactorizado**: `main.rs` init de ATA/AHCI/PCI reemplazado por `storage_manager::init_storage()`
- **Migrado**: `iso9660.rs` de `ATA_DRIVER.lock()` → `BLOCK_DEVICES.lock().get(0)`
- **Migrado**: `fat32.rs` de `ATA_DRIVER.lock()` → `BLOCK_DEVICES.lock().get(0)`
- **Eliminado**: `globals::ATA_DRIVER`, `globals::ATA_DRIVER_SECONDARY`, `globals::AHCI_DRIVER` (legacy)
- **Eliminada**: dependencia directa de FAT32/ISO9660 en globals legacy

## v0.14.0 — 2026-05-19

### HAL ABI v0.3 — KCR Compliance Fix

- **Añadido** (HAL): `inw`/`outw`/`inl`/`outl` — I/O de 16 y 32 bits para ATA, PCI, UHCI
- **Añadido** (HAL): `read_cr2`/`read_cr3`/`write_cr3` — registros de control de x86_64
- **Añadido** (HAL): `flush_tlb(virt)` — invlpg público
- **Añadido** (HAL): `interrupts_enabled()` — lectura de RFLAGS.IF vía pushfq
- **Añadido** (HAL): `hlt_once()` — HLT individual (retorna tras la próxima IRQ)
- **Añadido** (HAL): `increment_ticks()` — incremento atómico del contador de ticks
- **Añadido** (HAL): `without_interrupts(||{})` — helper con save/restore de IF
- **Movido**: `walk_ptes_4k` de `arch/x64/paging.rs` a `hal/x64/mem.rs` — elimina dependencia circular HAL→arch
- **Eliminada**: dependencia de HAL en `crate::arch::x64::paging::walk_ptes_4k` — HAL es self-contained
- **Eliminado**: código duplicado `flush_tlb_entry` en `paging.rs` — usa `hal::flush_tlb`
- **Migrado**: 8 drivers (ATA, PCI, keyboard, RTC, UHCI, USB HID, serial, PIC) de `x86_64::Port`/`asm!()` a `hal::inb/outb/inw/outw/inl/outl`
- **Migrado**: 12 usos de `without_interrupts()` del crate `x86_64` a `hal::without_interrupts()`
- **Migrado**: 5 `asm!("hlt")` a `hal::hlt_once()` en shell, scheduler, syscall, shutdown
- **Migrado**: 5 accesos directos a `TIMER_TICKS` a `hal::get_ticks()`, escritura a `hal::increment_ticks()`
- **Migrado**: frame allocator en `paging.rs` usa `hal::alloc_page/free_page`
- **Migrado**: page table ops en `paging.rs` usa `hal::map_page/unmap_page`
- **Migrado**: CR accesos en `idt.rs`/`paging.rs` a `hal::read_cr2/read_cr3/write_cr3`
- **Actualizado**: `docs/HAL_ABI.md` a v0.3 (26 funciones extern "C")
- **Actualizado**: `docs/KCR_COMPLIANCE.md` — FAIL→PASS, verificación completa
- **Validado**: 45 tests kernel + 4 user-mode PASS, nm con 26 símbolos T globales

## v0.13.0 — 2026-05-19

### HAL v0 + NDM Removal

- **Añadido**: `src/hal/` — Hardware Abstraction Layer v0 con ABI v0.2. 14 primitivas: enable/disable_interrupts, halt, poweroff, inb/outb, alloc_page/free_page, map_page/unmap_page, register_irq, ack_irq, get_ticks, sleep_hint, memory_barrier. Implementación x86_64 en `hal/x64/`.
- **Eliminado**: `src/module_abi.rs` (NDM). Se elimina todo el sistema de módulos `.ndm`: header parser, KernelServiceTableV1, init_kernel_service_table(), driver.ndm, generate_driver.py, ndm_builder.py, docs/MODULE_ABI.md.
- **Migrado**: kernel code ahora usa `hal::enable_interrupts()`, `hal::halt()`, `hal::ack_irq()`, `hal::poweroff()` en vez de `arch::x64::*`.
- **Simplificado**: `arch/mod.rs` pierde el trait `Platform` (reemplazado por HAL). `arch/x64/` queda solo para init (GDT, IDT, PIC, serial, paging) y policy (user slots, heap pages).
- **Refactorizado**: PIC EOI reemplazado por `hal::ack_irq()` (port I/O directo en vez de `PICS.lock()`).

## v0.12.0 — 2026-05-19

### BlockDevice Abstraction

- **Añadido**: `BlockDeviceManager` en `drivers/block.rs` — registro dinámico de hasta 8 dispositivos de bloque. Métodos `register()`, `get()`, `swap()`, `count()`.
- **Añadido**: `BLOCK_DEVICES` global en `globals.rs` — reemplaza el acceso directo a ATA/AHCI para nuevas rutas de código.
- **Simplificado**: `main.rs` — la inicialización del storage stack registra el mejor dispositivo (AHCI si existe, ATA si no) en el `BlockDeviceManager` y lo usa para GPT scan, NeoDOS FS mount y FAT32 init. Se elimina la compleja coreografía de `AtaWithAhciFallback`.
- **Actualizado**: `flush_cache_if_needed()` usa `BLOCK_DEVICES.get(0)` en vez de `ATA_DRIVER`.

## v0.11.0 — 2026-05-19

### Eliminación de Panic Paths

- **Eliminados**: todos los `.unwrap()` del kernel (13 calls) reemplazados por: `.expect("msg")` en boot paths, pattern matching (`if let`/`match`) en runtime, y acceso directo a bytes (`as_bytes()[0]`) en lugar de `chars().next().unwrap()` para extraer drive letters.

### Archivos modificados

- `src/main.rs`: ATA DMA init usa `if let`, mount falla con `panic!("...")` descriptivo
- `src/shell/shell.rs`: `parts.next()` → `match`, `chars().next()` → `as_bytes()[0]`
- `src/shell/commands/cd.rs`: `chars().next()` → `as_bytes()[0]`
- `src/fs/vfs.rs`: `chars().next()` → `as_bytes()[0]`
- `src/drivers/ahci.rs`: `result[0].as_mut().unwrap()` → `match` con `continue`
- `src/scheduler.rs`: `.unwrap()` → `.expect("msg")`

## v0.10.5 — 2026-05-19

### Fixes

- **Corregido**: Version mismatch bootloader/kernel — bootloader actualizado de v0.10.3 a v0.10.5 (`Cargo.toml` + `BOOT_VERSION`).
- **Corregido**: Kernel panic "Failed to read superblock" en Q35 (AHCI) — el kernel usaba ATA PIO para leer el disco ignorando el driver AHCI. Se reemplazó el `BlockDevice` directo por `AtaWithAhciFallback`, que prueba AHCI primero (Q35) y cae a ATA (PIIX3). También se aplicó el mismo fallback a la inicialización FAT32.
- **Corregido**: FAT32 también usaba ATA PIO en vez de AHCI cuando estaba disponible.

## v0.10.5 — 2026-05-18

### Architecture refactoring (subsystem decoupling)

- **Creado**: `KERNEL_SUBSYSTEMS.md` — documento arquitectónico con 16 subsistemas definidos, responsabilidades, APIs, dependencias prohibidas, ciclo de vida y sincronización.
- **Añadido**: Trait `Platform` en `arch/mod.rs` — abstracción de plataforma (`halt`, `poweroff`, `enable_interrupts`, `disable_interrupts`, `cpu_info`). Implementado por `X64Platform`. El código genérico del kernel usa `Platform::halt()` en vez de `arch::x64::halt()`.
- **Eliminado**: `AtaDriver::ahci_fallback` — el driver ATA ya no conoce AHCI. El fallback se maneja mediante composición en `drivers/block.rs` con `AtaWithAhciFallback` que prueba AHCI primero, luego ATA.
- **Eliminado**: Acceso a RAM disk desde `AtaDriver` — la RAM disk ahora es un `BlockDevice` separado (`RamDisk` en `drivers/block.rs`).
- **Movido**: `RAM_DISK_BASE/SIZE` de `globals.rs` a `drivers/block.rs`.
- **Simplificado**: `globals.rs` eliminadas funciones `ram_disk_buf()` y `with_ata()`.

### Module ABI (Phase 7)

- **Añadido**: `src/module_abi.rs` — procesado del header NDM v1 (`NdModuleHeader`, `ParsedModule`), tabla de servicios del kernel (`KernelServiceTableV1`) en `0x4FFFF00` para módulos Ring 0 con funciones de I/O, consola, frame allocator y block device.
- **Añadido**: `docs/MODULE_ABI.md` — especificación completa del formato `.ndm`, estructura del header, tabla de servicios, compatibilidad de versiones, ciclo de vida del módulo y dispatch de TSR.
- **Actualizado**: `LOAD` command (`shell/commands/load.rs`) — valida el header NDM v1 antes de cargar; soporta módulos con secciones code+data separadas y entry point explícito; fallback a binario raw para `.nxe` legacy.
- **Actualizado**: `generate_driver.py` — produce `driver.ndm` con header NDM v1 (64 bytes) + code + data.
- **Inicializado**: `module_abi::init_kernel_service_table()` en `main.rs` (Phase 2.75, tras heap allocator).

### Estabilidad del scheduler

- **Corregido**: `schedule()` ya no selecciona idle (PID 0) cuando hay procesos no-idle listos. El round-robin ahora escanea todos los PIDs > 0 antes de caer en idle.
- **Corregido**: `timer_handler_inner` ya no guarda `current.rsp`. El timer puede dispararse durante ejecución en Ring 0 (syscalls) generando un frame IRETQ de 3 items. Solo `syscall_try_resched` guarda RSP porque INT 0x80 siempre viene de Ring 3 con frame de 5 items.
- **Consecuencia**: `ALLTEST.NXE` pasa completo por primera vez (yield, getpid, open, readfile, close, chdir, getcwd, brk → ALL_TESTS_PASSED).

### Herramientas

- **Añadido**: `scripts/check_deps.py` — validador de dependencias entre subsistemas. Detecta imports prohibidos (ej: scheduler → drivers, VFS → arch).

### Validation & Regression Infrastructure

- **Añadido**: `src/trace.rs` — Ring-buffer de eventos lock-free (1024 entradas) para reconstrucción post-mortem. Eventos: context switch, syscall enter/exit, IRQ timer tick, scheduler decisions, panic. Dump automático en panic.
- **Añadido**: `src/panic_classification.rs` — Sistema de clasificación de panics con 14 categorías (STACK_CORRUPTION, INVALID_IRETQ, IRQ_REENTRANCY, ABI_MISMATCH, etc.). Clasificación por vector de excepción + RIP + error code. Dump forense con trace buffer + estado del scheduler.
- **Añadido**: `src/invariants.rs` — Capa de validación de invariantes en runtime: contador de nesting IRQ, guarda de context switch desde timer IRQ, verificación de alineación de stack, macros `kern_assert!` (solo con feature `validation`).
- **Añadido**: `docs/KERNEL_VALIDATION.md` — Filosofía de validación, 25 invariantes documentadas (scheduler, IRQ, syscall, memoria, block device), política de regresión zero-tolerance, formato de dump forense.
- **Añadido**: `scripts/regression_runner.py` — Test runner determinista de 100+ iteraciones con detección de fallos intermitentes, clasificación de panics, informe estructurado (pass/fail, crash frequency, panic signatures).
- **Añadido**: `userbin/ndm_builder.py` — Biblioteca Python compartida para generar headers NDM v1.
- **Ampliado**: `src/testing.rs` — 8 nuevos tests de stress (scheduler: rapid yield, state transitions; syscall: rapid getpid, invalid number fuzzing, pointer validation; memory: alloc/free storm, vec churn, string churn). Total: 45 tests.
- **Ampliado**: `src/arch/x64/idt.rs` — Todos los exception handlers clasifican panics antes de llamar a `panic!()`. Timer handler integra trace events + invariant checks (IRQ nesting, contexto válido).
- **Ampliado**: `src/syscall.rs` — `syscall_dispatch` valida ABI (rechaza números de syscall > 19 con u64::MAX). `syscall_try_resched` con invariantes (no llamar desde timer IRQ, verificar Running state). Trace points en dispatch y context switch.
- **Ampliado**: `src/scheduler.rs` — Trace points en `schedule()`, `add_ring3_process()`, `kill_pid()`. Invariant: no llamar `schedule()` desde timer IRQ context.
- **Ampliado**: `src/main.rs` — Panic handler mejorado: muestra clase de panic, dump forense (trace buffer + scheduler state) a serial.
- **Añadido**: `Cargo.toml` features `validation` y `stress` — perfiles de build con aserciones extra (cfg-gated).
- **Actualizado**: `src/module_abi.rs` — Assertions de layout en compile-time (`NdModuleHeader` = 64 bytes, `KernelServiceTableV1` = 168 bytes).

## v0.10.4 — 2026-05-16

### Procesos en Ring 3

- **Corregido**: `timer_handler_inner` ya no sobrescribe el estado `Terminated` de un proceso que salió. Previene que el timer reactive procesos muertos o cambie el contexto prematuramente cuando el shell corre en Ring 0 fuera del scheduler.
- **Corregido**: `syscall_try_resched` solo marca `Ready` si el proceso estaba `Running` (no `Terminated`).
- **Corregido**: `EXIT_NOW` cambiado a `AtomicU8` con `SeqCst` store. El compilador podía eliminar el `= 1` con LTO `opt-level=3`, haciendo que `sys_exit` hiciera `IRETQ` al espacio de usuario en vez de saltar a `exit_to_kernel`, ejecutando datos como código (page fault en RIP=0x4002ad).
- **Añadido**: `ALLTEST.NXE` — test exhaustivo de syscalls (open, readfile, close, chdir, getcwd, brk, yield, getpid, exit). Incluido en la imagen NeoDOS FS.

### Estabilidad en arranque

- **Corregido**: `allocator::init()` ahora se ejecuta **antes** de `enable_interrupts()`. El timer IRQ0 podía dispararse en la ventana entre STI y la inicialización del heap, causando un panic por allocación fallida (`LockedHeap::empty()`). Síntoma: `ALLOCATION ERROR size: 1, align: 1` en `src/allocator.rs:25`, intermitente según timing de TCG.

### Excepciones del CPU

- **Corregido**: `DOUBLE_FAULT_IST_INDEX` cambiado de 0 (reservado, no usable como IST) a 1, con índice correcto en el array `interrupt_stack_table` (`IST - 1`) y stack dedicado de 20 KB. Sin esto, un doble fault durante el manejo de otra excepción causaba triple fault y reboot.

### Versiones

- Bump kernel a v0.10.4 (Cargo.toml + KERNEL_VERSION_CODE).
