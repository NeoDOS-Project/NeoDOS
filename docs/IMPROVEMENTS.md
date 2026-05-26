# NeoDOS — Roadmap de 100 Items

> Versión actual: v0.16.4 (245+ tests, NEM v2 ABI validation, Boot Driver Loader, Rust reference drivers).
> Objetivo: v0.20 — kernel modular, estable, extensible.
> Última revisión: Mayo 2026.

---

## COMPLETED (54 items)

### Boot & Core Kernel
1. **x86_64 boot** — entry `_start` en 0x200000, long mode vía UEFI bootloader.
2. **GDT/IDT/PIC** — segmentos Ring 0/3, IDT 256 entradas, PIC remapeado IRQ 32–47.
3. **Identity paging 4 GiB** — páginas enormes 2 MB, identidad hasta 4 GB.
4. **Heap allocator** — 16 MB @ 0x1000000, `linked_list_allocator`, Box/Vec/String.
5. **A3. Kernel slab allocator** — 9 size classes (8B–2KB), O(1) alloc/free via per-slot free lists on 4 KB slab pages. Uses `hal::alloc_page()` for page allocation. Falls through to linked-list allocator for >2 KB or >16-byte alignment. 9 self-tests.
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
21. **ACPI driver** — RSDP scan, RSDT/XSDT, PM1a_CNT_BLK, usado por SHUTDOWN.
22. **HAL ABI v0.3** — 26 primitives `extern "C"` (CPU, port I/O, page mem, IRQ, timers).
23. **Device Model + HAL Binding** — 32-slot registry, handles opacos, 5 boot devices.
24. **Event Bus v1** — SPSC 64 slots, 11 event types, callbacks max 32, 9 tests.
25. **Driver Runtime** — DriverInstance con ID/nombre/estado/contadores, built-in callbacks.
26. **NDREG / LOADNEM / NEMLIST** — driver registry CLI, LOADNEM carga .nem drivers.
41. **Driver Certification Pipeline v1** — estado Loaded→Initialized→Registered→Bound→Active, state machine con transiciones estrictas, función `certify_and_activate()`, error tracking (`last_error` + `certification_step`), ndreg DEBUG para diagnóstico LOADED≠ACTIVE, 21 tests de state machine + pipeline.
42. **A4. Memory-mapped files** — `MmapRegion` + VMA list per-process, sys_mmap lazy (RAX=19), sys_munmap (RAX=20), región 0x20000000–0x22000000, anónimo + file-backed vía page fault handler, `is_user_ptr_valid` extendido, 6 tests mmap.
43. **S2. IPC / Pipes** — `src/pipe.rs`: PipeManager con 16 buffers de 4 KB, refcounting automático. Per-process `fd_table[16]` con FdEntry (stdin/stdout/pipe reader/pipe writer). Syscalls: `sys_pipe` (RAX=5), `sys_dup2` (RAX=6). `sys_read`/`sys_write`/`sys_close` modificados para pipe fds. Blocking reads via `ProcessState::Blocked` + `wake_pipe_readers()` scheduler integration. 13 tests pipe: alloc/free, write/read, múltiples writes, EOF, buffer capacity, EPIPE, max pipes, bloqueo/desbloqueo, fd table.
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
1. X2. Unified handle table

Tabla de handles global por proceso para abstraer recursos (files, pipes, devices, events).
Permite un modelo único de acceso a recursos del sistema.

3. X1. Kernel Object Manager (KOBJ)

Sistema unificado de objetos kernel con refcount y metadata común.
Convierte todo el kernel en objetos gestionables.

4. A5. Global page cache (base)

Caché central de páginas para filesystem, mmap e I/O.
Reduce acceso a disco y unifica modelo de memoria.

⚙️ FASE 2 — CONCURRENCY & EXECUTION MODEL
5. A2. Scheduler prioritario

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
20. A10. PCIe enumeration

Escaneo completo de buses PCIe.
Base para hardware moderno real.

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

43. B7. NeoTOP

Monitor de procesos y recursos.
Visibilidad del sistema.

44. B11. NeoShell scripting

Lenguaje de scripting del sistema.
Automatización avanzada.

45. B12. Compositor 2D

Sistema gráfico básico de ventanas.
Base GUI.

🔐 FASE 12 — SECURITY HARDENING
46. U1. Module signature validation

Validación criptográfica de drivers.
Evita código no confiable.

47. U3. Driver permission enforcement

Control granular de permisos de drivers.
Reduce superficie de ataque.

48. U4. Secure boot chain

Cadena de arranque verificada.
Protección del sistema completo.

⚡ FASE 13 — PERFORMANCE & MEMORY EVOLUTION
49. V2. Zero-copy pipes

IPC sin copias de memoria.
Reduce overhead.

50. V3. Copy-on-write fork

Fork eficiente por páginas compartidas.
Optimiza procesos.

51. X10. Per-CPU allocators

Allocators por CPU.
Escalabilidad SMP.

🧱 FASE 14 — SMP ENABLEMENT
52. X8. SMP-safe kernel refactor

Soporte multi-core completo.
Reescritura mínima del core.

🧪 FASE 15 — EXPERIMENTAL FUTURE
53. E4. Full GUI system

Interfaz gráfica completa con ventanas.

54. E5. Advanced secure boot

Secure boot extendido con políticas avanzadas.

55. E6. Package manager

Sistema de paquetes y repositorios.

56. T4. Time-travel debugging

Reproducción determinista de ejecuciones.

57. T5. Live kernel patching

Parches en caliente del kernel.

58. T2. Distributed NeoDOS nodes

NeoDOS en red distribuida.

🧭 RESUMEN FINAL
1. Memory core (slab + handles + KOBJ + page cache)
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