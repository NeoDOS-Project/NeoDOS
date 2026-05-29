# NeoDOS — Roadmap de 100 Items

> Versión actual: v0.22.0 (248 tests, ATA NEM standalone driver).
> Objetivo: v0.23 — kernel modular, estable, extensible.
> Última revisión: Mayo 2026.

---

## COMPLETED (62 items)

### Boot & Core Kernel
1. **x86_64 boot** — entry `_start` en 0x200000, long mode vía UEFI bootloader.
2. **GDT/IDT/PIC** — segmentos Ring 0/3, IDT 256 entradas, PIC remapeado IRQ 32–47.
3. **Identity paging 4 GiB** — páginas enormes 2 MB, identidad hasta 4 GB.
4. **Heap allocator** — 16 MB @ 0x1000000, `linked_list_allocator`, Box/Vec/String.
5. **A3. Kernel slab allocator** — 9 size classes (8B–2KB), O(1) alloc/free via per-slot free lists on 4 KB slab pages. Uses `hal::alloc_page()` for page allocation. Falls through to linked-list allocator for >2 KB or >16-byte alignment. 9 self-tests.
 6. **A5. Global page cache (base)** — `buffer/page_cache.rs`: central 4 KB page cache (512 entries × 4 KB = 2 MB) for filesystem file data I/O and mmap file-backed pages. LRU eviction with dirty write-back. Indexed by `(inode, block_num)` with stored `data_lba` for safe flush. Integrated with NeoFS read/write paths (`read_file_to_buf`, `write_file`, `read_file`) to read/write 8 sectors at once through the cache. mmap `load_file_mmap_page` checks PageCache first before falling back to VFS read. Timer-driven flush via `NEED_PAGE_CACHE_FLUSH` alongside existing `NEED_CACHE_FLUSH`. 8 unit tests. Total: 245 tests.
6. **PS/2 keyboard driver** — IRQ1, ring-buffer lock-free 1024 bytes.
6. **Serial console** — COM1, `serial_print!`/`serial_println!`.
7. **Framebuffer console** — GOP 1280×800, font VGA 8×16, `println!`.

### Storage
8. **ATA PIO driver** — read/write por puertos 0x1F0/0x3F6.
9. **AHCI driver** — DMA polling, PRDT scatter-gather, ATA + ATAPI.
10. **ATA bus-master DMA** — PCI BAR4, buffers alineados, hasta 8 sectores.
11. **NeoFS** — filesystem propio: inodos 256 B, bloques 4 KB, timestamps, permisos, directorios, 75 tests.
12. **FAT32 read** — lectura de sector absoluto desde ESP.
13. **GPT partition parsing** — detecta partición NeoDOS por UUID.
14. **Unified GPT disk image** — `disk_image.img` (ESP FAT32 + NeoDOS FS).
15. **VFS layer** — `FileSystem` trait, `resolve_path()`, FAT32 + NeoDOS + ISO9660.
16. **ISO9660 read** — driver completo con PVD, extent cache, Joliet.
17. **BlockDevice abstraction** — `BlockDevice` trait, `StorageManager` unifica ATA/AHCI.

### Drivers & Dispositivos
18. **Module ABI v0 (.NDM)** — header 64 bytes, kernel service table, LOAD command.
19. **NEM module** — NeoDOS Driver Format v1, 6 tipos, 14 tests parse.
20. **RTC driver** — CMOS RTC, get_datetime(), usado por DATE/TIME.
21. **ACPI driver** — NEM v3 standalone ACPI poweroff driver (`drivers/acpi/`). Scans PCI for PIIX4/ICH9 LPC bridge, detects PM1a port, writes SLP_TYP_S5|SLP_EN. Legacy RSDP/RSDT/FADT parser (`neodos-kernel/src/drivers/acpi.rs`) deleted. Fallback QEMU Bochs port + PS/2 reset. `EVENT_SHUTDOWN` event bus constant. `POWEROFF`/`SHUTDOWN`/`EXIT` command pushes event → ACPI driver → HAL poweroff fallback.
22. **HAL ABI v0.3** — 26 primitives `extern "C"` (CPU, port I/O, page mem, IRQ, timers).
23. **Device Model + HAL Binding** — 32-slot registry, handles opacos, 5 boot devices.
24. **Event Bus v1** — SPSC 64 slots, 11 event types, callbacks max 32, 9 tests.
25. **Driver Runtime** — DriverInstance con ID/nombre/estado/contadores, built-in callbacks.
26. **NDREG / LOADNEM / NEMLIST** — driver registry CLI, LOADNEM carga .nem drivers.
41. **Driver Certification Pipeline v1** — estado Loaded→Initialized→Registered→Bound→Active, state machine con transiciones estrictas, función `certify_and_activate()`, error tracking (`last_error` + `certification_step`), ndreg DEBUG para diagnóstico LOADED≠ACTIVE, 21 tests de state machine + pipeline.
42. **A4. Memory-mapped files** — `MmapRegion` + VMA list per-process, sys_mmap lazy (RAX=19), sys_munmap (RAX=20), región 0x20000000–0x22000000, anónimo + file-backed vía page fault handler, `is_user_ptr_valid` extendido, 6 tests mmap.
43. **S2. IPC / Pipes** — `src/pipe.rs`: PipeManager con 16 buffers de 4 KB, refcounting automático. Per-process `handle_table[16]` con HandleEntry (stdin/stdout/stderr/pipe reader/pipe writer). Syscalls: `sys_pipe` (RAX=5), `sys_dup2` (RAX=6). `sys_read`/`sys_write`/`sys_close` modificados para pipe fds. Blocking reads via `ProcessState::Blocked` + `wake_pipe_readers()` scheduler integration. 13 tests pipe: alloc/free, write/read, múltiples writes, EOF, buffer capacity, EPIPE, max pipes, bloqueo/desbloqueo, handle table. (Antes: `fd_table[16]` con `FdEntry` — migrado a handle table unificada en X2.)
44. **S7. Process exit: full cleanup** — `Scheduler::recycle_terminated(pid)` + `cleanup_terminated_process()` reciclan slot scheduler y liberan `Box<AlignedKStack>` (kernel stack) al salir. `kill_pid()` reescrito: libera heap, mmap, pipes, user slot, kernel stack y recicla slot inmediatamente. En waitpid desde Ring 3, el slot del proceso esperado se recicla automáticamente tras detectar Terminated. 3 ficheros modificados: `scheduler.rs`, `run.rs`, `syscall.rs`.
45. **S5. FSCK utility** — `src/fs/fsck.rs`: superblock validation (magic, block_size, num_blocks, num_inodes, label), inode table integrity check (mode bits, inode_num mismatch, block pointer bounds, cross-linked block detection), directory tree walk with cycle protection (MAX_DIR_DEPTH=32), orphan inode detection, dangling directory entry detection, entry-type vs inode-mode mismatch detection. Repair mode (`FSCK /F`): restores superblock fields, clears invalid modes, removes cross-linked block references, frees orphan inodes, deletes dangling entries, fixes entry type mismatches, flushes cache to disk. Shell command `FSCK` registered in handler table. 6 unit tests for validation helpers.
46. **BDL1. NEM v2 ABI fields** — `src/nem/mod.rs`: extended NEM format to v2 (48-byte header) with ABI validation fields (abi_min, abi_target, abi_max), driver category (Boot/System/Demand), 16-byte driver name. Backward-compatible with v1. ABI validation ensures driver/kernel compatibility window. 9 new tests for v2 parsing, categories, ABI constants.
47. **BDL2. Boot Driver Loader System** — `src/drivers/boot_loader/mod.rs`: automatic boot-time scanning and loading of .nem drivers from `C:\SYSTEM\DRIVERS\BOOT\` and `C:\SYSTEM\DRIVERS\SYSTEM\`. Implements `driver_scan()`, `driver_load()`, `driver_init()`, `driver_activate()`, `driver_unload()` API with full certification pipeline integration. Connected to boot sequence in `main.rs` as PHASE 3.85. 8 kernel tests for scan/load/unload/init/activate, category ordering.
48. **BDL3. Driver Instance extended** — `src/drivers/driver_runtime.rs`: added `DriverCategory` field and ABI fields (abi_min, abi_target, abi_max) to `DriverInstance`. New `register_ext()` method for extended registration. `DriverCategory` enum (Boot=0, System=1, Demand=2) embedded in NEM v2 header.
49. **BDL4. ABI Validation Policy** — `src/drivers/nem/policy.rs`: new `validate_abi()` function checks driver/kernel ABI compatibility window. Rejects drivers if `abi_min > ABI_MAX_VALID`, `abi_max < ABI_MIN_VALID`, or `abi_target` outside range. Boot/System drivers require v2 format.
50. **BDL5. Rust reference .nem drivers** — `src/drivers/reference/`: three complete reference Rust driver implementations for PS/2 keyboard, framebuffer, and storage. Each demonstrates `extern "C"` entrypoint contract (`driver_init`, `driver_on_event`, `driver_fini`), event integration, lifecycle management, null-safety, and parameter validation. 32 kernel tests across all three reference drivers.
51. **BDL6. NDREG updated** — `src/shell/commands/ndreg.rs`: LIST and SHOW subcommands now display driver category (BOOT/SYSTEM/DEMAND) and ABI range (v1/v2 format). RUNTIME snapshot shows category per driver.
52. **BDL7. NEM v3 standalone serial driver** — `drivers/serial/build_nem.py` compila `serial.nem` (SYSTEM category). driver_init() configura UART 16550A (38400, 8N1, FIFO, RDA IRQ). IRQ4 desenmascarado en PIC (mask 0xE8). IDT[36] serial_handler con while-loop draining + push EVENT_SERIAL_DATA. com1 device con CAP_IRQ + irq=36. Boot loader registra evento serial. **Bugfix**: V3_EVENT_FN reemplazado por tabla de dispatch por event_type para soportar múltiples drivers v3 simultáneamente (el bug causaba pérdida de eventos de teclado al cargar más de un driver v3).
53. **BDL8. NEM ps2kbd layout switching** — KEYB US|SP command envía EVENT_KEYB_LAYOUT (type=9) via Event Bus. ps2kbd.nem driver_on_event() maneja EVENT_KEYB_LAYOUT y cambia layout atómico. Sin cambio en kernel export table.
54. **W1. ABI negotiation layer** — `src/drivers/abi/mod.rs`: formalized ABI version negotiation between kernel and NEM drivers. `AbiVersion` struct (min/target/max), `NegotiationResult` enum (Compatible/CompatibleWithWarnings/Incompatible), `negotiate()` function with window overlap check and warning levels. Integrated into v3loader `validate_v3_abi()`. 10 unit tests.
55. **W4. Driver dependency resolver** — `src/drivers/dependency/mod.rs`: automatic dependency resolution for NEM drivers. `DependencyGraph` with topological sort via DFS and cycle detection. Convention: `__dep_DRIVERNAME` symbols in NEM symbol table declare dependencies. Boot loader v2 uses dependency-resolved order within each category. 13 unit tests.
56. **Device Model + TSR removal** — Removed legacy `src/devices/mod.rs` (Device Model v0.3) and `src/tsr/mod.rs` (TSR system). The device model was superseded by direct NEM v3 driver model + Event Bus v1 + HAL ABI v0.3. TSR system was legacy from the NDM era. Shell commands DEVICES and TSR removed. Reduces kernel code by ~530 lines.
57. **X2. Unified handle table** — `src/handle.rs`: unified handle table per-process replacing `FdEntry`/`FdTable`. Handle types: CLOSED, STDIN, STDOUT, STDERR, PIPE_READ, PIPE_WRITE, FILE, DEVICE, EVENT. File handles store drive+inode+offset cursor, enabling per-open file offset tracking. `sys_open` returns fd instead of packed `(drive<<32)|inode`. `sys_readfile`/`sys_writefile`/`sys_close` use fd. `sys_mmap` file-backed uses fd. All process lifecycle code (exit, kill) cleans up via handle table. `libneodos` and all user binaries updated. 229 kernel tests + 4 user-mode binaries.
58. **PS/2 double-character fix** — Boot loader `_` fallthrough arm registered `v3_event_bridge` for `EVENT_KEYBOARD_INPUT` with unknown drivers' `driver_on_event`, creating a duplicate event bus handler. Every keyboard event dispatched `process_scancode` twice → all characters doubled. Fixed by changing `_` to `true` (bind without handler). Known drivers have explicit match arms.
59. **ACPI NEM poweroff driver** — NEM v3 standalone driver for ACPI S5 poweroff via PCI-based PM1a detection. Replaces legacy RSDP/RSDT/FADT table parser. `EVENT_SHUTDOWN` event bus constant (type 12). `POWEROFF`/`SHUTDOWN` command dispatches event to ACPI driver, with `hal::poweroff()` fallback. Added `-no-reboot` to `qemu-debug.sh`.
60. **PCI NEM driver** — `drivers/pci/` standalone NEM v3 driver (SYSTEM category, Lifecycle type 2). Logs all PCI devices with vendor/device/class/subclass/prog-if/rev. Handles config read/write via Event Bus (events 0x1000–0x1003). Kernel `src/drivers/pci.rs` reduced to 4 low-level config primitives. `find_ide_controller()`/`enable_bus_master()` moved inline to `storage_manager.rs`. `find_nvme_controller()`/`nvme_enable()` moved inline to `nvme.rs`. Dead `find_acpi_pm1_cnt_port()` removed. 4947 bytes.
61. **A10. PCIe bus enumeration** — Extended PCI NEM driver to discover all PCI buses recursively via PCI-to-PCI bridge detection. Scans bus 0, detects bridges (class 0x06, subclass 0x04), reads secondary bus numbers from config offset 0x18, and enqueues them for scanning up to 256 buses. Added 3 kernel tests validating bus 0 devices, bus 1 emptiness, and bridge detection. QEMU PIIX3: 6 devices on bus 0 (no bridges). Total: 248 kernel tests + 4 user-mode binaries.
62. **A6. ATA NEM standalone driver** — `drivers/ata/` NEM v3 standalone driver (SYSTEM category) for ATA storage. Scans PCI for IDE controller with bus-master DMA capability; enables bus-mastering and initializes primary + secondary channels. Supports DMA read/write (via PRDT, up to 8 sectors) and PIO multi-sector fallback. Each active channel registers a `NemBlockDevice` via `hst_register_block_device()`. Kernel-side `ata.rs` reduced to `BootAta` PIO-only boot stub (GPT parsing, superblock read, cache warmup before NEM load). `AtaWithAhciFallback` removed. QEMU machine type changed from `q35` to `pc` (PIIX3) for IDE controller compatibility. Total: 248 kernel tests + 4 user-mode binaries.

### Userland & Memoria
27. **Demand paging (4 KB)** — frame allocator, split_2mb, heap page fault handler.
28. **sys_brk / sys_mmap** — ajuste program break, asignación zero-filled.
29. **ELF64 loader** — src/elf.rs: carga segmentos PT_LOAD a vaddr, 7 tests.
30. **User-mode processes** — IRETQ a Ring 3, EXIT_RSP/EXIT_RIP, scheduler add_ring3_process.
31. **Kernel private stacks** — TSS.RSP0 por proceso, actualizado en cada context switch.
32. **Syscall table (INT 0x80)** — 14 syscalls: exit, write, yield, getpid, read, waitpid, open, readfile, writefile, close, chdir, getcwd, brk, mmap.
33. **Scheduler blocking** — ProcessState::Blocked, wake_waiters(), idle HLT.
54. **S6. libneodos** — `libneodos/`: standard library para procesos Ring 3 en Rust. Syscall wrappers con `int 0x80` inline asm (sys_exit, sys_write, sys_read, sys_open, sys_readfile, sys_writefile, sys_brk, sys_mmap, sys_munmap, etc.). IO module con Stdout/Stdin/Stderr + `core::fmt::Write` impl. FS module (File::open/read/write). Memory module (sbrk, mmap, munmap). Safe macros (print!, println!, eprint!, eprintln!). Panic handler que llama sys_exit(1). Sample user binary `userbin/hello_lib/` with linker script `user.ld`.

### Shell & Testing
34. **150 kernel self-tests** — 15 suites, comando `test`, 4 user-mode binaries.
35. **4 user-mode test binaries** — HELLO.BIN, SYSTEST.BIN, FILETEST.BIN, ALLTEST.BIN.
36. **Command history** — buffer circular 32, ↑/↓ navegación.
37. **TAB autocomplete** — comandos built-in + archivos del directorio actual.
38. **Keyboard layouts** — KBDUS.klc / KBDSP.klc compilados en build-time.
39. **Shell commands básicos** — HELP, DATE, TIME, VER, DEL, REN, RD, SHUTDOWN, EXIT, LOAD.
40. **S1. Estabilizar syscall ABI** — `SyscallNum` enum + `from_u64()`, `SyscallError` enum (16 codes), `err_to_u64()` negative encoding, `syserr!` macro, `validate_abi()` boot-time assertion, clean `match` dispatch, `[SYS]` log pruning.

---

NeoDOS — ORDERED IMPROVEMENTS (WITH DESCRIPTION)

Versión: v0.20 → v1.0
Objetivo: eliminar reescrituras, estabilizar kernel core, escalar a sistema completo

🧱 FASE 1 — KERNEL FOUNDATION (MEMORY + OBJECT MODEL)
1. **X2. Unified handle table — COMPLETED** (ver v0.17.2 en CHANGELOG)

Tabla de handles global por proceso para abstraer recursos (files, pipes, devices, events).
Permite un modelo único de acceso a recursos del sistema.

2. **X1. Kernel Object Manager (KOBJ) — COMPLETED** (ver v0.18.0 en CHANGELOG)

Sistema unificado de objetos kernel con refcount y metadata común.
Convierte todo el kernel en objetos gestionables.

3. **A5. Global page cache (base) — COMPLETED** (ver CHANGELOG)

Caché central de páginas para filesystem, mmap e I/O.
Reduce acceso a disco y unifica modelo de memoria.

4. A2. Scheduler prioritario

Planificador con prioridades y time-slicing dinámico.
Permite multitarea real con control de CPU.

6. X5. Deferred work queues

Sistema de bottom-half para mover trabajo fuera de IRQ context.
Evita bloqueos en interrupciones y mejora estabilidad.

7. X7. Event Bus v2

Sistema de eventos asíncrono con colas y dispatch controlado.
Base de comunicación entre kernel, drivers y userland.

🔁 FASE 3 — ASYNC I/O CORE
8. X6. Async I/O (IRP system)

Modelo unificado de peticiones I/O asincrónicas.
Base para discos, red, USB y filesystem moderno.

9. V1. Global page cache (advanced)

Evolución del cache con LRU, write-back y readahead.
Optimiza rendimiento de almacenamiento y mmap.

🧩 FASE 4 — DRIVER ARCHITECTURE SAFETY LAYER
10. X3. Capability system

Sistema de capacidades explícitas (IRQ, DMA, MMIO, etc).
Controla qué puede hacer cada driver.

11. X4. Driver isolation layer

Aislamiento parcial de drivers con límites de memoria y acceso.
Reduce riesgo de crash kernel por drivers defectuosos.

12. W1. ABI negotiation layer

Compatibilidad entre kernel y drivers mediante ABI versionado.
Permite evolución sin romper drivers antiguos.

13. W4. Driver dependency resolver

Resuelve dependencias entre drivers automáticamente.
Evita orden incorrecto de carga.

14. W2. Hot reload drivers

Carga y descarga de drivers en runtime sin reboot.
Requiere ownership tracking y aislamiento estable.

🧪 FASE 5 — OBSERVABILITY & DEBUGGING (CRÍTICO)
15. Y1. Kernel tracing infrastructure

Sistema de tracing de eventos del kernel en tiempo real.
Base para debugging avanzado.

16. Y4. Crash dump framework

Captura de estado completo del sistema tras fallo.
Permite análisis post-mortem.

17. Y2. NeoTrace system

Herramienta de visualización y análisis de trazas del kernel.
Debugging estructurado del sistema.

18. Y5. Kernel debugger

Debugger interactivo para inspección de memoria, procesos y drivers.
Permite depuración runtime.

19. Y6. Watchdog subsystem

Sistema de detección de bloqueos y hangs del kernel.
Recuperación automática de fallos.

🖥️ FASE 6 — MODERN HARDWARE LAYER
21. A11. MSI/MSI-X

Sistema moderno de interrupciones basado en mensajes.
Reemplaza PIC legacy.

22. C3. HPET / APIC timers

Timers de alta precisión y soporte SMP inicial.
Base del scheduling moderno.

⚡ FASE 7 — MODERN I/O DRIVERS
23. A8. VirtIO driver

Driver paravirtualizado para testing rápido y estable.
Simplifica desarrollo en QEMU.

24. A9. NVMe driver

Soporte moderno de almacenamiento de alta velocidad.
Requiere async I/O.

25. C6. AHCI NCQ

Soporte avanzado SATA con colas múltiples.
Optimiza discos tradicionales.

🔌 FASE 8 — INPUT & STORAGE DEVICES
26. C1. USB HID

Soporte para teclado y dispositivos HID USB reales.
Reemplaza PS/2 legacy.

27. C2. USB mass storage

Soporte para pendrives y discos USB.
Integra con async I/O.

28. C7. USB UHCI fix

Corrección y estabilización del driver USB legacy.
Completa compatibilidad hardware antiguo.

🧠 FASE 9 — SERVICE LAYER & SYSTEM CORE
29. Z1. NeoInit service manager

Sistema de servicios con dependencias y lifecycle.
Base del userland moderno.

30. Z6. System configuration registry

Persistencia de configuración del sistema.
Similar a registry moderno.

31. Z2. Unified resource namespace

Todo el sistema accesible como filesystem virtual.
Unifica procesos, drivers y dispositivos.

32. Z3. Virtual FS objects

/proc-like system para introspección del kernel.
Expone estado interno como archivos.

🌐 FASE 10 — NETWORKING STACK
33. D9. Socket API

API de sockets unificada para userland.
Base de networking moderno.

34. E3. Network stack (TCP/IP)

Stack completo de red.
Requiere async I/O y scheduler estable.

35. D8. DHCP client

Configuración automática de red.
Depende del stack TCP/IP.

36. D7. NTP client

Sincronización de tiempo del sistema.
Depende de networking estable.

🧑‍💻 FASE 11 — USERLAND USABLE SYSTEM
37. S8. PATH resolution

Ejecución de comandos desde múltiples directorios.
Base de shell usable.

38. S9. Shell pipes

Conexión de procesos por streams.
Permite composición de comandos.

39. S3. Shell redirection

Redirección de stdout/stderr a archivos.
Base de scripting.

40. B2. ANSI terminal

Soporte de colores, cursor y control terminal.
Mejora UX shell.

41. B1. Virtual terminals

Múltiples sesiones de consola.
Separación de contextos de usuario.

42. B6. NeoEdit

Editor de texto del sistema.
Primera herramienta real de usuario.

43. B6b. Shared library system (libneodos DLL)

Sistema de biblioteca compartida para procesos Ring 3. Compilar libneodos como binario
standalone en una dirección fija reservada en el espacio de usuario (ej: `0x30000000`),
con tabla de exportación de funciones `extern "C"` (syscall wrappers, IO, FS, mem, panic).
El kernel mapea el DLL en cada proceso Ring 3 al crearlo (`spawn_usermode`). Los binarios
de usuario se enlazan contra la DLL en lugar de incluir el código estáticamente, reduciendo
tamaño en disco y compartiendo páginas de código en RAM entre procesos. Requiere: dirección
fija reservada fuera de user slots y heap, export table con ABI estable, loader en el kernel,
actualización de linker scripts (`user.ld`) y build system, y compatibilidad `extern "C"` en
todos los puntos de entrada de libneodos.

44. B7. NeoTOP

Monitor de procesos y recursos.
Visibilidad del sistema.

45. B11. NeoShell scripting

Lenguaje de scripting del sistema.
Automatización avanzada.

46. B12. Compositor 2D

Sistema gráfico básico de ventanas.
Base GUI.

🔐 FASE 12 — SECURITY HARDENING
47. U1. Module signature validation

Validación criptográfica de drivers.
Evita código no confiable.

48. U3. Driver permission enforcement

Control granular de permisos de drivers.
Reduce superficie de ataque.

49. U4. Secure boot chain

Cadena de arranque verificada.
Protección del sistema completo.

⚡ FASE 13 — PERFORMANCE & MEMORY EVOLUTION
50. V2. Zero-copy pipes

IPC sin copias de memoria.
Reduce overhead.

51. V3. Copy-on-write fork

Fork eficiente por páginas compartidas.
Optimiza procesos.

52. X10. Per-CPU allocators

Allocators por CPU.
Escalabilidad SMP.

🧱 FASE 14 — SMP ENABLEMENT
53. X8. SMP-safe kernel refactor

Soporte multi-core completo.
Reescritura mínima del core.

🧪 FASE 15 — EXPERIMENTAL FUTURE
54. E4. Full GUI system

Interfaz gráfica completa con ventanas.

55. E5. Advanced secure boot

Secure boot extendido con políticas avanzadas.

56. E6. Package manager

Sistema de paquetes y repositorios.

57. T4. Time-travel debugging

Reproducción determinista de ejecuciones.

58. T5. Live kernel patching

Parches en caliente del kernel.

59. T2. Distributed NeoDOS nodes

NeoDOS en red distribuida.

🧭 RESUMEN FINAL
1. Memory core (slab + handles + KOBJ + page cache ✅)
2. Concurrency (scheduler + events + work queues)
3. Async I/O system
4. Driver safety layer
5. Observability & debugging
6. Modern hardware (PCIe + MSI + APIC)
7. Storage + USB drivers
8. Service layer (NeoInit + namespace)
9. Networking stack
10. Userland usable system
11. Security hardening
12. Performance tuning
13. SMP enablement
14. Experimental features