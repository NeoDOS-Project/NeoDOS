# NeoDOS — Roadmap de 100 Items

> Versión actual: v0.24.5 (301 tests, Multi-DLL system).
> Objetivo: v0.25 — kernel modular, estable, extensible.
> Última revisión: Junio 2026.

---

## COMPLETED (74 items)

### Boot & Core Kernel
1. **x86_64 boot** — entry `_start` en 0x200000, long mode vía UEFI bootloader.
2. **GDT/IDT/PIC** — segmentos Ring 0/3, IDT 256 entradas, PIC remapeado IRQ 32–47.
3. **Identity paging 4 GiB** — páginas enormes 2 MB, identidad hasta 4 GB.
4. **Heap allocator** — 16 MB @ 0x1000000, `linked_list_allocator`, Box/Vec/String.
5. **A3. Kernel slab allocator** — 9 size classes (8B–2KB), O(1) alloc/free via per-slot free lists on 4 KB slab pages. Uses `hal::alloc_page()` for page allocation. Falls through to linked-list allocator for >2 KB or >16-byte alignment. 9 self-tests.
6. **A2. Scheduler prioritario** — 4 niveles de prioridad (HIGH/ABOVE_NORMAL/NORMAL/IDLE), time-slicing dinámico (400/200/100/50 ticks), preemption desde Ring 3, aging cada 100 ticks para evitar starvation. 7 tests. Total: 255 tests.
7. **A5. Global page cache (base)** — `buffer/page_cache.rs`: central 4 KB page cache (512 entries × 4 KB = 2 MB) for filesystem file data I/O and mmap file-backed pages. LRU eviction with dirty write-back. Indexed by `(inode, block_num)` with stored `data_lba` for safe flush. Integrated with NeoFS read/write paths (`read_file_to_buf`, `write_file`, `read_file`) to read/write 8 sectors at once through the cache. mmap `load_file_mmap_page` checks PageCache first before falling back to VFS read. Timer-driven flush via `NEED_PAGE_CACHE_FLUSH` alongside existing `NEED_CACHE_FLUSH`. 8 unit tests. Total: 245 tests.
8. **PS/2 keyboard driver** — IRQ1, ring-buffer lock-free 1024 bytes.
9. **Serial console** — COM1, `serial_print!`/`serial_println!`.
10. **Framebuffer console** — GOP 1280×800, font VGA 8×16, `println!`.
11. **X1. Kernel Object Manager (KOBJ)** — `src/kobj/mod.rs`: unified kernel object system with reference counting and common metadata. 64-slot registry, KObjType enum, auto-register on create. Integration with processes, drivers, pipes. Shell command `KOBJ`. 8 unit tests.
12. **X5. Deferred work queues** — `src/work_queue.rs`: bottom-half system for deferred execution outside IRQ context. Two-level architecture (high/low priority). Lock-free SPSC ring buffer (64 slots per level). `WORK_QUEUE.push_high()`/`push_low()`/`process_high()`/`process_low()`. 6 tests.
13. **X6. Async I/O (IRP system)** — `src/irp/mod.rs`: unified I/O Request Packet model. Global 64-slot pool, `IrpQueue` per-device (32 entries), completion callbacks via work queue, scheduler integration (`irp_block_current`/`irp_wake_waiter`), IRP chaining. BlockDevice trait extended with `submit_irp`/`poll_irp`. 5 BlockDevice implementors. 11 tests. Total: 284 tests.
14. **V1. Global page cache (advanced)** — `src/buffer/page_cache.rs`: hash map O(1) index for `(inode, block_num)` lookups. LRU doubly-linked list for O(1) access updates. Adaptive readahead (sequential detection). Async write-back (flush in batch at threshold). Dynamic sizing via slab pool. 13 tests.

### Storage
15. **ATA PIO driver** — read/write por puertos 0x1F0/0x3F6.
16. **AHCI driver** — DMA polling, PRDT scatter-gather, ATA + ATAPI.
17. **ATA bus-master DMA** — PCI BAR4, buffers alineados, hasta 8 sectores.
18. **NeoFS** — filesystem propio: inodos 256 B, bloques 4 KB, timestamps, permisos, directorios, 75 tests.
19. **FAT32 read** — lectura de sector absoluto desde ESP.
20. **GPT partition parsing** — detecta partición NeoDOS por UUID.
21. **Unified GPT disk image** — `disk_image.img` (ESP FAT32 + NeoDOS FS).
22. **VFS layer** — `FileSystem` trait, `resolve_path()`, FAT32 + NeoDOS + ISO9660.
23. **ISO9660 read** — driver completo con PVD, extent cache, Joliet.
24. **BlockDevice abstraction** — `BlockDevice` trait, `StorageManager` unifica ATA/AHCI.

### Drivers & Dispositivos
25. **Module ABI v0 (.NDM)** — header 64 bytes, kernel service table, LOAD command.
26. **NEM module** — NeoDOS Driver Format v1, 6 tipos, 14 tests parse.
27. **RTC driver** — CMOS RTC, get_datetime(), usado por DATE/TIME.
28. **ACPI driver** — NEM v3 standalone ACPI poweroff driver (`drivers/acpi/`). Scans PCI for PIIX4/ICH9 LPC bridge, detects PM1a port, writes SLP_TYP_S5|SLP_EN. Legacy RSDP/RSDT/FADT parser (`neodos-kernel/src/drivers/acpi.rs`) deleted. Fallback QEMU Bochs port + PS/2 reset. `EVENT_SHUTDOWN` event bus constant. `POWEROFF`/`SHUTDOWN`/`EXIT` command pushes event → ACPI driver → HAL poweroff fallback.
29. **HAL ABI v0.3** — 26 primitives `extern "C"` (CPU, port I/O, page mem, IRQ, timers).
30. **Device Model + HAL Binding** — 32-slot registry, handles opacos, 5 boot devices.
31. **Event Bus v2** — Dual priority queues (high 16 + normal 64), subscription filters, dynamic payload, backpressure, dispatch on syscall return. 17 tests.
32. **Driver Runtime** — DriverInstance con ID/nombre/estado/contadores, built-in callbacks.
33. **NDREG / LOADNEM / NEMLIST** — driver registry CLI, LOADNEM carga .nem drivers.
34. **Driver Certification Pipeline v1** — estado Loaded→Initialized→Registered→Bound→Active, state machine con transiciones estrictas, función `certify_and_activate()`, error tracking (`last_error` + `certification_step`), ndreg DEBUG para diagnóstico LOADED≠ACTIVE, 21 tests de state machine + pipeline.
35. **A4. Memory-mapped files** — `MmapRegion` + VMA list per-process, sys_mmap lazy (RAX=19), sys_munmap (RAX=20), región 0x20000000–0x22000000, anónimo + file-backed vía page fault handler, `is_user_ptr_valid` extendido, 6 tests mmap.
36. **S2. IPC / Pipes** — `src/pipe.rs`: PipeManager con 16 buffers de 4 KB, refcounting automático. Per-process `handle_table[16]` con HandleEntry (stdin/stdout/stderr/pipe reader/pipe writer). Syscalls: `sys_pipe` (RAX=5), `sys_dup2` (RAX=6). `sys_read`/`sys_write`/`sys_close` modificados para pipe fds. Blocking reads via `ProcessState::Blocked` + `wake_pipe_readers()` scheduler integration. 13 tests pipe: alloc/free, write/read, múltiples writes, EOF, buffer capacity, EPIPE, max pipes, bloqueo/desbloqueo, handle table.
37. **S7. Process exit: full cleanup** — `Scheduler::recycle_terminated(pid)` + `cleanup_terminated_process()` reciclan slot scheduler y liberan `Box<AlignedKStack>` (kernel stack) al salir. `kill_pid()` reescrito: libera heap, mmap, pipes, user slot, kernel stack y recicla slot inmediatamente. En waitpid desde Ring 3, el slot del proceso esperado se recicla automáticamente tras detectar Terminated.
38. **S5. FSCK utility** — `src/fs/fsck.rs`: superblock validation (magic, block_size, num_blocks, num_inodes, label), inode table integrity check (mode bits, inode_num mismatch, block pointer bounds, cross-linked block detection), directory tree walk with cycle protection (MAX_DIR_DEPTH=32), orphan inode detection, dangling directory entry detection, entry-type vs inode-mode mismatch detection. Repair mode (`FSCK /F`). Shell command `FSCK`. 6 unit tests.
39. **BDL1. NEM v2 ABI fields** — `src/nem/mod.rs`: extended NEM format to v2 (48-byte header) with ABI validation fields (abi_min, abi_target, abi_max), driver category (Boot/System/Demand), 16-byte driver name. Backward-compatible with v1. 9 new tests.
40. **BDL2. Boot Driver Loader System** — `src/drivers/boot_loader/mod.rs`: automatic boot-time scanning and loading of .nem drivers from `C:\SYSTEM\DRIVERS\BOOT\` and `C:\SYSTEM\DRIVERS\SYSTEM\`. Implements `driver_scan()`, `driver_load()`, `driver_init()`, `driver_activate()`, `driver_unload()` API with full certification pipeline integration. Connected to boot sequence in `main.rs` as PHASE 3.85. 8 kernel tests.
41. **BDL3. Driver Instance extended** — `src/drivers/driver_runtime.rs`: added `DriverCategory` field and ABI fields (abi_min, abi_target, abi_max) to `DriverInstance`. New `register_ext()` method. `DriverCategory` enum (Boot=0, System=1, Demand=2) embedded in NEM v2 header.
42. **BDL4. ABI Validation Policy** — `src/drivers/nem/policy.rs`: new `validate_abi()` function checks driver/kernel ABI compatibility window. Rejects drivers if `abi_min > ABI_MAX_VALID`, `abi_max < ABI_MIN_VALID`, or `abi_target` outside range. Boot/System drivers require v2 format.
43. **BDL5. Rust reference .nem drivers** — `src/drivers/reference/`: three complete reference Rust driver implementations for PS/2 keyboard, framebuffer, and storage. Each demonstrates `extern "C"` entrypoint contract, event integration, lifecycle management, null-safety, and parameter validation. 32 kernel tests.
44. **BDL6. NDREG updated** — `src/shell/commands/ndreg.rs`: LIST and SHOW subcommands now display driver category (BOOT/SYSTEM/DEMAND) and ABI range (v1/v2 format). RUNTIME snapshot shows category per driver.
45. **BDL7. NEM v3 standalone serial driver** — `drivers/serial/build_nem.py` compila `serial.nem` (SYSTEM category). driver_init() configura UART 16550A (38400, 8N1, FIFO, RDA IRQ). IRQ4 desenmascarado en PIC (mask 0xE8). IDT[36] serial_handler con while-loop draining + push EVENT_SERIAL_DATA. **Bugfix**: V3_EVENT_FN reemplazado por tabla de dispatch por event_type para soportar múltiples drivers v3 simultáneamente.
46. **BDL8. NEM ps2kbd layout switching** — KEYB US|SP command envía EVENT_KEYB_LAYOUT (type=9) via Event Bus. ps2kbd.nem driver_on_event() maneja EVENT_KEYB_LAYOUT y cambia layout atómico. Sin cambio en kernel export table.
47. **W1. ABI negotiation layer** — `src/drivers/abi/mod.rs`: formalized ABI version negotiation between kernel and NEM drivers. `AbiVersion` struct (min/target/max), `NegotiationResult` enum (Compatible/CompatibleWithWarnings/Incompatible), `negotiate()` function with window overlap check and warning levels. Integrated into v3loader `validate_v3_abi()`. 10 unit tests.
48. **W4. Driver dependency resolver** — `src/drivers/dependency/mod.rs`: automatic dependency resolution for NEM drivers. `DependencyGraph` with topological sort via DFS and cycle detection. Convention: `__dep_DRIVERNAME` symbols in NEM symbol table. Boot loader v2 uses dependency-resolved order within each category. 13 unit tests.
49. **Device Model + TSR removal** — Removed legacy `src/devices/mod.rs` (Device Model v0.3) and `src/tsr/mod.rs` (TSR system). Shell commands DEVICES and TSR removed. Reduces kernel code by ~530 lines.
50. **X2. Unified handle table** — `src/handle.rs`: unified handle table per-process replacing `FdEntry`/`FdTable`. Handle types: CLOSED, STDIN, STDOUT, STDERR, PIPE_READ, PIPE_WRITE, FILE, DEVICE, EVENT. File handles store drive+inode+offset cursor. `sys_open` returns fd. `sys_readfile`/`sys_writefile`/`sys_close` use fd. All process lifecycle code cleans up via handle table. 229 kernel tests + 4 user-mode binaries.
51. **PS/2 double-character fix** — Boot loader `_` fallthrough arm registered `v3_event_bridge` for `EVENT_KEYBOARD_INPUT` with unknown drivers' `driver_on_event`, creating a duplicate event bus handler. Fixed by changing `_` to `true` (bind without handler).
52. **ACPI NEM poweroff driver** — NEM v3 standalone driver for ACPI S5 poweroff via PCI-based PM1a detection. Replaces legacy RSDP/RSDT/FADT table parser. `EVENT_SHUTDOWN` event bus constant (type 12). `POWEROFF`/`SHUTDOWN` command dispatches event with `hal::poweroff()` fallback. Added `-no-reboot` to `qemu-debug.sh`.
53. **PCI NEM driver** — `drivers/pci/` standalone NEM v3 driver (SYSTEM category). Logs all PCI devices with vendor/device/class/subclass/prog-if/rev. Handles config read/write via Event Bus (events 0x1000–0x1003). Kernel `src/drivers/pci.rs` reduced to 4 low-level config primitives. 4947 bytes.
54. **A10. PCIe bus enumeration** — Extended PCI NEM driver to discover all PCI buses recursively via PCI-to-PCI bridge detection. Scans bus 0, detects bridges (class 0x06, subclass 0x04), reads secondary bus numbers from config offset 0x18. 3 kernel tests. Total: 248 kernel tests.
55. **A6. ATA NEM standalone driver** — `drivers/ata/` NEM v3 standalone driver (SYSTEM category). Scans PCI for IDE controller with bus-master DMA capability, initializes primary + secondary channels. Each active channel registers a `NemBlockDevice`. Kernel-side `ata.rs` reduced to `BootAta` PIO-only boot stub. QEMU machine changed from `q35` to `pc` (PIIX3).
56. **A11. AHCI NEM standalone driver** — `drivers/ahci/` NEM v3 standalone driver (SYSTEM category). Scans PCI for AHCI controllers (class 0x01 subclass 0x06), initializes HBA, detects ATA/ATAPI devices per port. Uses DMA polling with PRDT (up to 8 entries), single-slot command engine. Kernel built-in AHCI driver removed.
57. **A12. BootAhci kernel stub** — `boot_ahci.rs` built-in kernel driver (Phase 3 storage init) for AHCI early-boot access. Minimal DMA polling driver (single port, single command slot, 8-sector PRDT). Registers as block device idx=0 before NEM AHCI driver loads. Priority: NVMe > BootAhci > BootAta (PIO).
58. **X3. Capability system** — `src/drivers/caps.rs`: 64-bit capability bitmap per driver (11 flags: IRQ, DMA, MMIO, PORTIO, ALLOC_PAGE, BLOCK_DEVICE, EVENT_BUS, INPUT, LOG, TIMING, MEMORY). Capability inheritance by category (BOOT=all, SYSTEM=restricted, DEMAND=sandboxed). Runtime capability check in every `hst_*` export — denies execution if capability missing. Capability escalation via Event Bus (SYSTEM→CAP_ALLOC_PAGE|BLOCK_DEVICE|MEMORY; DEMAND blocked). `NDREG SHOW` displays capabilities. 11 unit tests.

### Userland & Memoria
58. **Demand paging (4 KB)** — frame allocator, split_2mb, heap page fault handler.
59. **sys_brk / sys_mmap** — ajuste program break, asignación zero-filled.
60. **ELF64 loader** — src/elf.rs: carga segmentos PT_LOAD a vaddr, 7 tests.
61. **User-mode processes** — IRETQ a Ring 3, EXIT_RSP/EXIT_RIP, scheduler add_ring3_process.
62. **Kernel private stacks** — TSS.RSP0 por proceso, actualizado en cada context switch.
63. **Syscall table (INT 0x80)** — 14 syscalls: exit, write, yield, getpid, read, waitpid, open, readfile, writefile, close, chdir, getcwd, brk, mmap.
64. **Scheduler blocking** — ProcessState::Blocked, wake_waiters(), idle HLT.
65. **S6. libneodos** — `libneodos/`: standard library para procesos Ring 3 en Rust. Syscall wrappers con `int 0x80` inline asm (sys_exit, sys_write, sys_read, sys_open, sys_readfile, sys_writefile, sys_brk, sys_mmap, sys_munmap, etc.). IO module con Stdout/Stdin/Stderr + `core::fmt::Write` impl. FS module (File::open/read/write). Memory module (sbrk, mmap, munmap). Safe macros (print!, println!, eprint!, eprintln!). Panic handler que llama sys_exit(1). Sample user binary `userbin/hello_lib/`.

### Shell & Testing
66. **301 kernel self-tests** — 36 suites, comando `test`, 4 user-mode binaries.
67. **4 user-mode test binaries** — HELLO.BIN, SYSTEST.BIN, FILETEST.BIN, ALLTEST.BIN.
68. **Command history** — buffer circular 32, ↑/↓ navegación.
69. **TAB autocomplete** — comandos built-in + archivos del directorio actual.
70. **Keyboard layouts** — KBDUS.klc / KBDSP.klc compilados en build-time.
71. **Shell commands básicos** — HELP, DATE, TIME, VER, DEL, REN, RD, SHUTDOWN, EXIT, LOAD.
72. **S1. Estabilizar syscall ABI** — `SyscallNum` enum + `from_u64()`, `SyscallError` enum (16 codes), `err_to_u64()` negative encoding, `syserr!` macro, `validate_abi()` boot-time assertion, clean `match` dispatch, `[SYS]` log pruning.

### Shared Libraries & Loading
73. **B6b. Shared library system (libneodos DLL)** — Compila libneodos como binario standalone (DLL) con tabla de exportación `AbiTable` en sección `.export_table` en dirección fija `0x1e000000`. 8 slots de 256 KB en la región `0x1e000000..0x1e200000`. Se carga automáticamente en boot (PHASE 3.86).
74. **Multi-DLL system** — `sys_loadlib` (RAX=21) para cargar DLLs desde NeoFS en runtime. `LOADLIB` shell command. `libmath-dll/` crate (17 funciones exportadas: abs, min, max, pow, sqrt, sin, cos, log, exp, etc.) en slot 1 (`0x1e040000`). `libneodos::loadlib(path)` wrapper para user-mode. Build system integrado.

---

NeoDOS — ORDERED IMPROVEMENTS (WITH DESCRIPTION)

Versión: v0.20 → v1.0
Objetivo: eliminar reescrituras, estabilizar kernel core, escalar a sistema completo

🧩 FASE 4 — DRIVER ARCHITECTURE SAFETY LAYER
1. **X4. Driver isolation layer**

Capa de aislamiento parcial para ejecutar drivers NEM con límites de memoria y acceso, reduciendo el riesgo de que un driver defectuoso corrompa el kernel. Actualmente, los drivers NEM se ejecutan en Ring 0 (kernel space) y tienen acceso completo a toda la memoria del kernel, incluyendo tablas de páginas, estructuras del scheduler, y datos de otros drivers. Un buffer overflow en un driver puede corromper cualquier estructura del kernel.

El isolation layer exploraría: (1) **Segmentación por página** — los drivers se cargan en una región de memoria aislada (ej. `0x30000000..0x31000000`, 16 MB) cuya page table tiene permisos restrictivos: código RX, datos RW (sin ejecución), sin acceso a páginas del kernel fuera de la región y la export table. (2) **Export table como unico puente** — el driver solo puede llamar a funciones del kernel a través de la export table (tabla de punteros a función); no puede hacer llamadas directas a memoria arbitraria del kernel. (3) **Argument validation** — las funciones de la export table validan que los punteros pasados por el driver apunten a memoria del driver o a buffers válidos del usuario (no a estructuras internas del kernel). (4) **Sandbox opcional** — para drivers DEMAND, se podría usar el page fault handler para detectar accesos inválidos y marcar el driver como FAULTED automáticamente.

Nota: el isolation total requeriría cambios en cómo se invocan los entrypoints del driver (`driver_init`, `driver_on_event`, `driver_fini`), probablemente envolviendo cada llamada en un callback que cambie CR3 a una page table aislada. Esto tiene overhead, por lo que se implementaría como opt-in para drivers DEMAND y como hard-requirement para drivers de terceros.

Archivos: `src/drivers/isolation.rs`, modificaciones en `src/drivers/loader.rs` y `src/drivers/driver_runtime.rs`.

3. **W2. Hot reload drivers**

Carga, descarga y recarga de drivers en runtime sin necesidad de reiniciar el sistema. Actualmente, los drivers NEM solo se cargan en el boot (PHASE 3.85) o mediante el comando LOADNEM/NDREG LOAD. Una vez cargados, no hay forma de descargarlos limpiamente ni de recargar una versión actualizada.

El sistema hot reload requeriría: (1) **Driver state machine extendida** — añadir transiciones `Active → Unloading` (descarga en curso) y `Unloaded → Loaded` (recarga). (2) **Ownership tracking** — al descargar un driver, el kernel debe asegurarse de que ningún otro driver o subsistema tenga referencias a recursos del driver (handles de eventos, block devices registrados, páginas de código). Esto requiere que cada driver declare sus recursos explícitamente, o que el kernel los rastree mediante KOBJ. (3) **Graceful drain** — antes de descargar, el kernel envía un evento `EVENT_DRIVER_UNLOAD` y espera a que el driver libere sus recursos (timeout configurable). Si el driver no responde, se fuerza la descarga y se marcan sus recursos como huérfanos. (4) **Binary version check** — al recargar, se verifica que la nueva versión del driver sea ABI-compatible con el kernel actual.

Casos de uso: actualizar un driver sin reboot (ej. fix de bug en ATA driver), cargar un driver de depuración solo cuando sea necesario, descargar un driver que falló y recargarlo.

Archivos: `src/drivers/hotreload.rs`, modificaciones en `driver_runtime.rs` y `shell/commands/ndreg.rs`.

🧪 FASE 5 — OBSERVABILITY & DEBUGGING (CRÍTICO)
4. **Y1. Kernel tracing infrastructure**

Infraestructura de tracing de eventos del kernel en tiempo real para diagnóstico y profiling. Actualmente, la única forma de ver qué ocurre dentro del kernel es mediante `serial_println!` (logging) y el Event Bus (eventos de alto nivel). No hay un sistema estructurado para medir latencias, conteo de llamadas, o seguir el flujo de ejecución a través de subsistemas.

El tracing infrastructure incluiría: (1) **Trace points** — macros `trace!(category, event, args)` que insertan puntos de instrumentación en ubicaciones clave del kernel (syscall entry/exit, scheduler context switch, IRQ handler entry/exit, VFS operations, block device I/O). Los trace points son compilados condicionalmente (feature `tracing`), sin overhead en release normal. (2) **Trace buffer** — buffer circular lock-free de 64 KB (8192 entradas de 8 bytes cada una: timestamp + category + event + args) en memoria reservada, escritura O(1) desde cualquier contexto (incluyendo IRQ). (3) **Trace categories** — SYS (syscalls), SCHED (scheduler), IRQ (interrupts), FS (filesystem), BLK (block I/O), DRV (drivers), MEM (memory). (4) **User-facing interface** — comando `TRACE` del shell para iniciar/parar tracing, mostrar buffer, filtrar por categoría, y exportar a serial en formato CSV. (5) **Circular overwrite** — cuando el buffer se llena, sobrescribe las entradas más antiguas (modo flight recorder).

Archivos: `src/trace/mod.rs`, modificaciones en `src/syscall.rs`, `src/scheduler.rs`, `src/interrupts/idt.rs`, etc. para añadir trace points.

5. **Y4. Crash dump framework**

Sistema de captura de estado completo del sistema cuando ocurre un fallo irrecuperable (page fault en Ring 0, triple fault, panic, assert fallido). Actualmente, cuando el kernel panic, `hal::poweroff()` se llama directamente (a través de `_start` → `poweroff`), sin capturing de estado. La única información disponible es el mensaje de panic enviado por serial.

El crash dump framework introduciría: (1) **Panic handler mejorado** — antes de apagar, el kernel captura y escribe a serial (o a disco si es posible): registros CPU (RAX, RBX, RCX, RDX, RSI, RDI, RSP, RBP, RIP, RFLAGS, CR0, CR2, CR3, CR4), stack trace de las últimas 32 entradas (desenrollado del stack), estado del scheduler (procesos activos, PID actual), y los últimos 128 trace points del trace buffer (si está activo). (2) **Crash dump partition** — partición reservada en el disco (o área especial en NeoDOS FS) donde se escribe el crash dump en formato binario simple, para análisis post-mortem tras reinicio. (3) **Dump analysis tool** — script Python que parsea el crash dump y produce un reporte legible (registros, stack trace, procesos). (4) **Triple fault handler** — configurar el vector 0 (triple fault) para capturar estado antes de que QEMU reinicie.

Archivos: `src/crash/mod.rs`, modificaciones en `src/main.rs` (panicking), `scripts/crashdump_analyzer.py`.

6. **Y2. NeoTrace system**

Herramienta de visualización y análisis de trazas del kernel, construida sobre la infraestructura de tracing (Y1). Mientras que Y1 proporciona la recolección cruda de datos, NeoTrace es el front-end que permite al desarrollador navegar, filtrar y comprender las trazas.

NeoTrace incluiría: (1) **Comando `NEOTRACE`** en el shell — interfaz interactiva para consultar el trace buffer con filtros por categoría, PID, rango de tiempo, y tipo de evento. Ejemplo: `NEOTRACE FILTER SYS,SCHED` para ver solo syscalls y scheduler; `NEOTRACE LAST 50` para ver las últimas 50 entradas. (2) **Trace export** — `NEOTRACE EXPORT BIN` descarga el buffer completo por serial en formato binario; `NEOTRACE EXPORT CSV` en formato CSV legible (timestamp_ticks,category,event_type,arg1,arg2). (3) **Trace timeline** — vista de línea temporal que muestra la secuencia de eventos; útil para visualizar latencias entre una syscall entry y su exit, o entre una IRQ y su dispatch. (4) **Trace markers** — puntos de referencia manuales que el desarrollador puede insertar con `trace_mark!("mi marca")` para etiquetar momentos clave en el código.

La herramienta no requiere conexión de red ni puerto externo; funciona enteramente dentro del shell del kernel, con salida por serial para logging externo.

Archivo: `src/shell/commands/neotrace.rs`.

7. **Y5. Kernel debugger**

Debugger interactivo para inspección de memoria, procesos y drivers en runtime, sin necesidad de GDB ni conexión JTAG. A diferencia del tracing (pasivo), el debugger permite inspección activa: leer estructuras del kernel, modificar variables, y controlar la ejecución de procesos.

El kernel debugger incluiría: (1) **Comando `DEBUG`** en el shell — interfaz interactiva con subcomandos: `DEBUG MEM <addr> <len>` (dump de memoria en hex+ASCII), `DEBUG PROC <pid>` (estado del proceso: registros, prioridad, time slice, handles), `DEBUG DRV <name>` (inspección detallada del driver: estado, capacidades, errores, recursos KOBJ), `DEBUG SCHED` (cola de procesos listos por prioridad), `DEBUG STACK` (stack trace del proceso actual), `DEBUG VAR <addr> <value>` (escribir valor en dirección). (2) **Breakpoints software** — el comando `DEBUG BP <addr>` reemplaza un byte en la dirección con `0xCC` (INT3). Cuando se ejecuta, el handler de INT3 guarda el estado y devuelve el control al shell para inspección. (3) **Watchpoints** — con apoyo del page fault handler, se puede vigilar accesos de escritura/lectura a direcciones específicas marcando la página como not-present y verificando en el page fault. (4) **Stack unwinding** — función `backtrace()` que camina el stack frame a frame usando RBP (frame pointer) y muestra símbolos si están disponibles (tabla de símbolos del kernel).

A diferencia del NDREG DEBUG (que solo verifica el pipeline de certificación), el kernel debugger es una herramienta de propósito general para depurar cualquier aspecto del kernel.

Archivos: `src/debugger/mod.rs`, modificaciones en `src/interrupts/idt.rs` (INT3 handler), `src/shell/commands/debug.rs`.

8. **Y6. Watchdog subsystem**

Sistema de detección de bloqueos y hangs del kernel con recuperación automática. Actualmente, si el kernel entra en un bucle infinito (por ejemplo, un driver que no retorna de `driver_on_event`, o un bucle en el scheduler), el sistema se cuelga sin posibilidad de recuperación. El watchdog proporciona un mecanismo de timeout hardware para detectar y mitigar estas situaciones.

El watchdog subsystem incluiría: (1) **HPET watchdog timer** — configurar un timer HPET (o el PIT channel 2) como watchdog con timeout de 5 segundos. El kernel debe "petear" (resetear) el watchdog periódicamente desde el timer tick handler (IRQ0). Si el tick handler no se ejecuta durante 5 segundos (porque el sistema está colgado), el watchdog genera un reset o una NMI. (2) **Watchdog petting** — `watchdog_pet()` llamado desde `timer_handler_inner` en cada tick. Si el handler no se ejecuta (sistema colgado con interrupciones deshabilitadas o bucle infinito en Ring 0), el watchdog hardware fuerza un reset. (3) **NMI watchdog opcional** — si el hardware lo soporta, configurar una NMI para watchdog en lugar de reset; la NMI puede capturar estado (registros, stack) antes de reiniciar. (4) **User-mode watchdog** — para procesos Ring 3, el scheduler puede marcar un proceso como "hung" si no ha cedido CPU (via sys_yield o time slice expiry) durante más de N ticks (configurable, ej. 10000 ticks ≈ 10 segundos). El proceso es terminado y su slot reciclado.

Archivos: `src/watchdog/mod.rs`, modificaciones en `src/interrupts/timer.rs` (watchdog pet), `src/hal/watchdog.rs` (primitivas HAL para HPET/PIT).

🖥️ FASE 6 — MODERN HARDWARE LAYER
9. **A11. MSI/MSI-X**

Sistema de interrupciones basado en mensajes (Message Signaled Interrupts) como reemplazo del PIC legacy (8259A) actual. Actualmente, el kernel usa el PIC clásico: IRQ0–IRQ15 mapeados a vectores 32–47, con EOI por puerto I/O (0x20/0xA0). Esto limita el número de dispositivos (16 IRQs), fuerza reparto estático de prioridades, y no escala a sistemas SMP.

MSI/MSI-X permitiría: (1) **Configuración vía PCI** — cada dispositivo PCI con capacidad MSI tiene un Message Address Register (MAR) y Message Data Register (MDR) en su configuration space. El kernel escribe la dirección y datos que el dispositivo usará para generar una interrupción como una escritura MMIO. (2) **Vectores dinámicos** — cada dispositivo MSI puede tener su propio vector IDT, eliminando la necesidad de compartir IRQs y detectar quién generó la interrupción. (3) **MSI-X** — variante que permite hasta 2048 interrupciones por dispositivo, cada una con su propia dirección y datos, ideal para NVMe con múltiples colas. (4) **Kernel infrastructure** — tabla de vectores MSI asignados dinámicamente, función `msi_alloc_vector(device)` / `msi_free_vector(vector)`, integración con el PCI NEM driver para detectar y configurar capacidades MSI/MSI-X de cada dispositivo.

El PIC legacy se mantendría como fallback para dispositivos legacy (PS/2, serial). Los dispositivos modernos (NVMe, VirtIO, USB xHCI) usarían MSI-X.

Archivos: `src/interrupts/msi.rs`, modificaciones en `src/drivers/pci.rs` (pci config primitives), `src/interrupts/idt.rs` (allocation de vectores libres).

10. **C3. HPET / APIC timers**

Timers de alta precisión basados en HPET (High Precision Event Timer) y Local APIC timer, como reemplazo del PIT (8254) actual de 18.2 Hz. El kernel actual usa el PIT channel 0 a 18.2 Hz (~55 ms de resolución) para el timer tick. Esto limita la granularidad del scheduler (time slices de 50-400 ticks = 2.75-22 segundos en el nivel más bajo) y no permite mediciones de tiempo precisas.

HPET: (1) **Detección en boot** — buscar la tabla ACPI HPET (firmware reservada en memoria). Leer dirección base MMIO y configurar el counter a 1 MHz (tick cada 1 µs). (2) **One-shot vs periodic** — HPET puede configurarse en modo periódico (como el PIT actual) o one-shot (interrupción única después de N µs). (3) **Mayor resolución** — timer tick configurable a 1 KHz (1 ms) o incluso 10 KHz (100 µs) para scheduling más preciso.

Local APIC timer: (1) **Detectar APIC** — leer MSR IA32_APIC_BASE (0x1B) para verificar que el Local APIC está presente y habilitado. (2) **Configurar timer** — programar el LVT Timer register del APIC con divisor y contador inicial, usando el bus del procesador como referencia de tiempo. (3) **SMP foundation** — cada CPU tiene su propio Local APIC timer, necesario para scheduler por-CPU en sistemas SMP.

Implementación escalonada: primero HPET como timer principal (reemplazando PIT), luego APIC timer como opción para sistemas SMP.

Archivos: `src/timers/hpet.rs`, `src/timers/apic.rs`, `src/hal/timers.rs`, modificaciones en `src/interrupts/timer.rs` y `arch/x64/pic.rs`.

⚡ FASE 7 — MODERN I/O DRIVERS
11. **A8. VirtIO driver**

Driver paravirtualizado para dispositivos VirtIO en QEMU/KVM, proporcionando almacenamiento y red de alto rendimiento sin emulación hardware. VirtIO es el estándar de paravirtualización en QEMU: ofrece latencias mucho menores que la emulación de hardware real (PIIX3 IDE, AHCI, e1000).

El driver VirtIO implementaría: (1) **Detección PCI** — escanear PCI para dispositivos con vendor 0x1AF4 (Red Hat). Identificar dispositivos VirtIO por device ID (1-40): VirtIO Block (0x1001) para almacenamiento, VirtIO Console (0x1003) para terminal, VirtIO Network (0x1000) para red futura. (2) **VirtIO transport** — configurar la cola de descriptores (virtqueue) mediante MMIO de la PCI BAR 0. Los descriptors forman un ring buffer donde el driver escribe peticiones y el dispositivo escribe completions. (3) **VirtIO Block** — operaciones de lectura/escritura por sector (512 B) o multi-sector, usando el protocolo VirtIO Block Request (struct `virtio_blk_req` con tipo, sector, buffer, status byte). Soporte para flush (cache sync) y get_id (serial del dispositivo). (4) **Integración con BlockDevice trait** — registrar cada dispositivo VirtIO Block como un `NemBlockDevice` (como hace el ATA NEM driver), permitiendo que NeoFS, el page cache, y las syscalls de archivos lo usen directamente.

Beneficio principal: en QEMU/KVM, VirtIO Block es ~10× más rápido que la emulación IDE (PIIX3) porque evita la emulación de registros PIO/DMA y usa shared memory directamente.

Archivos nuevos en `drivers/virtio/` (NEM v3 standalone driver: `build_nem.py` + `main.c` o `main.rs`). Archivos kernel: `neodos-kernel/src/drivers/virtio.rs` (primitivas básicas de detección y virtqueue), más el driver standalone.

12. **A9. NVMe driver**

Driver para almacenamiento NVMe (Non-Volatile Memory Express) sobre PCI Express, proporcionando acceso a discos SSD modernos con latencias de microsegundos y múltiples colas paralelas. Actualmente, el kernel solo soporta ATA (PIO/DMA en PIIX3 IDE) y AHCI (SATA emulado). NVMe es el estándar moderno para SSDs.

El NVMe driver requeriría: (1) **Detección PCI** — buscar dispositivos con class 0x01 (mass storage), subclass 0x08 (NVMe). O bien usar el PCI NEM driver para detectar el dispositivo. (2) **Admin Queue setup** — crear la Admin Completion Queue (ACQ) y Admin Submission Queue (ASQ) en memoria, escribir las puertas MMIO (doorbells) para notificar al controlador. Enviar comandos admin: Identify (obtener información del controlador y namespaces), Get Features, Set Features. (3) **I/O Queues** — crear uno o más pares de I/O Submission Queue (SQ) y I/O Completion Queue (CQ) en memoria. Cada queue puede tener hasta 64 KB de profundidad (65536 entradas). Múltiples queues permiten I/O paralelo. (4) **Comandos I/O** — comando NVM Read/Write con direcciones LBA (sectores de 512 B a 4 KB), usando PRP (Physical Region Page) entries para scatter-gather DMA. (5) **Integración con IRP system** — NVMe es inherentemente asíncrono: se escribe un comando en la SQ, se notifica via MMIO doorbell, y el dispositivo escribe la completion en la CQ y genera una interrupción MSI-X. Esto encaja perfectamente con el sistema de IRPs (X6). (6) **Namespace management** — detectar namespaces activos, tamaños de sector, y capacidades.

Dependencia fuerte del IRP system (X6) y MSI/MSI-X (A11). Sin async I/O, el driver NVMe tendría que hacer polling (como el AHCI actual), perdiendo la ventaja principal del protocolo.

Archivos: `drivers/nvme/build_nem.py` + `main.rs` (NEM v3 standalone driver), `neodos-kernel/src/drivers/nvme.rs` (primitivas), o integración directa como driver NEM SYSTEM.

13. **C6. AHCI NCQ**

Soporte para Native Command Queuing (NCQ) en el driver AHCI, permitiendo que hasta 32 comandos estén pendientes simultáneamente en un disco SATA. Actualmente, el driver AHCI usa DMA polling con un solo comando activo por puerto (descriptor de comando único en el Command List). Esto infrautiliza la capacidad de los discos SATA modernos, especialmente en operaciones con patrón de acceso aleatorio (multiple procesos leyendo archivos diferentes).

NCQ requeriría: (1) **Command List expandido** — el Command List AHCI soporta hasta 32 slots de comando por puerto, pero el driver actual solo usa el slot 0. Habría que gestionar los 32 slots como un pool, asignando slots libres a medida que llegan peticiones y liberándolos al completarse. (2) **Received FIS parsing** — el D2H Register FIS y el Set Device Bits FIS indican qué slot ha completado. El driver debe parsear el Received FIS area para detectar completions y despertar al solicitante. (3) **NCQ command format** — los comandos NCQ usan el protocolo DMA Setup FIS con el bit NCQ activado y el tag en el sector count. El comando ATA READ/WRITE FPDMA QUEUED (0x60/0x61) reemplaza a READ/WRITE DMA EXT (0x25/0x35). (4) **Out-of-order completion** — en NCQ, los comandos pueden completar en orden diferente al de envío. El driver debe asociar cada completion con su IRP original mediante el tag del slot.

NCQ extiende el driver AHCI existente (no requiere reescritura completa). Se integra con el IRP system (X6) para encolar comandos concurrentes.

Archivo: `neodos-kernel/src/drivers/ahci.rs` (modificaciones para NCQ).

🔌 FASE 8 — INPUT & STORAGE DEVICES
14. **C1. USB HID**

Soporte para dispositivos de interfaz humana (Human Interface Device) por USB: teclados, ratones y otros periféricos de entrada. Actualmente, el único método de entrada es PS/2 (IRQ1, teclado). No hay soporte para ratón (PS/2 mouse no está implementado) ni para teclados USB.

USB HID requeriría: (1) **USB host controller driver** — implementar o habilitar un driver UHCI (Universal Host Controller Interface) para USB 1.x en PIIX3 (compatible con QEMU machine type `pc`). UHCI usa puertos I/O (ej. 0x60–0x6F para PIIX3) y gestiona la comunicación USB por tramas de 1 ms. Alternativamente: OHCI (USB 1.x, común en hardware real) o xHCI (USB 3.x, más complejo pero moderno). (2) **USB device enumeration** — detectar dispositivos conectados, asignar dirección USB, leer device descriptor (vendor ID, product ID, class/subclass/protocol), y configurar pipes de control e interrupción. (3) **HID report parser** — para dispositivos HID, leer el HID Report Descriptor y parsear los campos (teclas, movimiento de ratón, botones). Convertir a eventos de entrada estándar (scancodes PS/2-like para teclado, delta X/Y para ratón). (4) **Integración con input system** — los eventos HID se inyectan en el ring-buffer de input actual (o se publican en el Event Bus como EVENT_KEYBOARD_INPUT extendido con flag USB). (5) **Hotplug** — detectar conexión/desconexión de dispositivos USB en runtime mediante el UHCI port status change.

Dependencia: UHCI driver funcional (C7), scheduler blocking, work queues para enumeration.

Archivos: `neodos-kernel/src/drivers/usb/` (nuevo módulo, varios archivos: `uhci.rs`, `hub.rs`, `hid.rs`), o un NEM v3 standalone driver `drivers/usb_hid/`.

15. **C2. USB mass storage**

Soporte para dispositivos de almacenamiento masivo USB (pendrives, discos externos USB) usando el protocolo Bulk-Only Transport (BOT) sobre USB. Esto permitiría arrancar NeoDOS desde un pendrive y usar discos USB como almacenamiento secundario.

USB Mass Storage requeriría: (1) **USB BOT protocol** — implementar el protocolo USB Mass Storage Class, Bulk-Only Transport: Command Block Wrapper (CBW) enviado por bulk OUT endpoint, respuesta Command Status Wrapper (CSW) leída por bulk IN endpoint. Los comandos SCSI se encapsulan en el CBW. (2) **SCSI command set** — comandos esenciales: READ_10 (lectura de sectores), WRITE_10 (escritura), READ_CAPACITY_10 (tamaño del disco), TEST_UNIT_READY, INQUIRY. (3) **BlockDevice integration** — cada dispositivo USB mass storage se registra como un `NemBlockDevice` mediante `hst_register_block_device()`, exactamente igual que los discos ATA. El VFS y NeoFS lo ven como otro drive (ej. C: para ATA, D: para USB). (4) **Hotplug** — detectar inserción/extracción de dispositivos USB y registrar/desregistrar BlockDevice dinámicamente.

Dependencia: USB host controller funcional (UHCI/OHCI/xHCI de C1), IRP system (X6) para I/O asíncrono.

Archivos: `neodos-kernel/src/drivers/usb/msd.rs`, o NEM v3 standalone `drivers/usb_msd/`.

16. **C7. USB UHCI fix**

Corrección y estabilización del driver UHCI legacy, que actualmente no es funcional en la máquina PIIX3 de QEMU. El código UHCI existente en el kernel (mencionado en AGENTS.md como "driver UHCI para USB no funcional en PIIX3") necesita ser diagnosticado y reparado.

Las correcciones incluirían: (1) **Diagnóstico** — determinar por qué el UHCI falla en PIIX3: ¿problema de dirección MMIO? ¿configuración PCI incorrecta? ¿errores de programación de tramas USB? ¿el emulador de QEMU PIIX3 no implementa UHCI correctamente? (2) **PCI identification** — verificar que el dispositivo UHCI en PIIX3 (PCI vendor 0x8086, device 0x7020, class 0x0C, subclass 0x03, prog-if 0x00) se detecta correctamente y sus BARs de I/O port están mapeadas. (3) **Frame list programming** — corregir la programación de la Frame List (base address register, frame count, link pointers). UHCI divide el tiempo en tramas de 1 ms, cada una representada por una entrada en la Frame List que apunta a una Queue Head (QH) o Transfer Descriptor (TD). (4) **Interrupt handling** — configurar correctamente el IRQ del UHCI en el PIC y proporcionar un handler que procese completions de transferencia USB. (5) **Testing** — probar la enumeración de un dispositivo USB simple (ej. teclado USB HID) en QEMU con la opción `-usb -device usb-kbd`.

Si el UHCI de PIIX3 no funciona en QEMU, considerar cambiar a OHCI (USB 1.x, común en hardware real) o xHCI (USB 3.x). Alternativa: habilitar UHCI en QEMU cambiando a una máquina que tenga UHCI funcional (ej. `-machine pc,usb=on` con el controller UHCI de Intel 82371SB PIIX3).

Archivo: `neodos-kernel/src/drivers/usb/uhci.rs` (reescritura/reparación del driver existente).

🧠 FASE 9 — SERVICE LAYER & SYSTEM CORE
17. **Z1. NeoInit service manager**

Gestor de servicios del sistema — el proceso de inicio (`PID 1`) que orquesta el arranque de todos los servicios de userland, gestiona sus dependencias y ciclo de vida, y supervisa su salud. Actualmente, tras el boot del kernel, el shell es el único proceso userland. No hay un sistema de servicios: cada binario se ejecuta manualmente con el comando `RUN`.

NeoInit sería: (1) **Primer proceso userland** — el kernel arranca `C:\SYSTEM\NEODOS.SYS` (o similar) como PID 1 después de PHASE 4, antes del shell. NeoInit lee un archivo de configuración (`C:\SYSTEM\INIT.CFG`) que define los servicios del sistema. (2) **Declaración de servicios** — el archivo INIT.CFG especifica por servicio: nombre, path al ejecutable, prioridad, dependencias de otros servicios, capacidades requeridas, y política de restart (always/on-fail/never). (3) **Lifecycle management** — NeoInit lanza servicios en orden de dependencias (igual que el boot loader con drivers). Supervisa que estén vivos: si un servicio termina inesperadamente, lo relanza según la política configurada. Servicios pueden estar en estados: Stopped, Starting, Running, Stopping, Crashed. (4) **Servicios del sistema** — ejemplos: `NEODOS.SYS` (shell principal), `NEOTRACE` (tracing daemon), `NETSRV` (network stack), `USBHID` (USB input daemon). (5) **Shutdown coordination** — al recibir `EVENT_SHUTDOWN`, NeoInit para servicios en orden inverso, envía `EVENT_SERVICE_STOPPING`, espera grace period, y finalmente llama a la syscall `sys_shutdown`.

NeoInit requiere: libneodos funcional con syscalls completas, IPC por pipes para comunicación entre servicios, y soporte de señales (o Event Bus) para notificaciones de shutdown.

Archivos: `userbin/neoinit/` (nuevo proyecto userland), `C:\SYSTEM\INIT.CFG` en el NeoDOS FS image.

18. **Z6. System configuration registry**

Registro central de configuración persistente del sistema, accesible por drivers y procesos de userland. Similar al registro de Windows o a los archivos de configuración de `/etc` en Unix, pero unificado en un solo sistema con acceso por clave-valor.

El System Configuration Registry incluiría: (1) **Registry storage** — archivo `C:\SYSTEM\CONFIG.REG` en el NeoDOS FS, con formato binario simple: sección cabecera con magic "NERG" (Neodos Entity Registry), seguida de entradas clave-valor con tipo (STRING, DWORD, BINARY, BOOL), nombre, y valor. (2) **Registry keys** — estructura jerárquica con separador `/`: `SYSTEM/DRIVERS/ATA/TIMEOUT`, `SYSTEM/SCHEDULER/TIME_SLICE_HIGH`, `USER/SHELL/HISTORY_SIZE`, `NETWORK/IP/MODE` (DHCP o static). (3) **Kernel API** — funciones `reg_read(key, &value) -> Result`, `reg_write(key, &value) -> Result`, `reg_delete(key)`, `reg_enumerate(section) -> Vec<String>`. Integración con la syscall table para acceso desde userland. (4) **Boot defaults** — valores por defecto hardcoded en el kernel para cuando CONFIG.REG no existe o falta una clave. (5) **Shell command** — `REG QUERY <key>`, `REG SET <key> <value>`, `REG DELETE <key>`, `REG LIST [section]`.

Casos de uso: configuración de red (IP estática vs DHCP), tamaño de page cache, prioridad por defecto de nuevos procesos, habilitar/deshabilitar drivers en boot, layout de teclado por defecto.

Archivos: `src/registry/mod.rs`, `src/shell/commands/reg.rs`, `scripts/init_config.py` para generar CONFIG.REG inicial en la imagen de disco.

19. **Z2. Unified resource namespace**

Sistema de nombres unificado que expone todos los recursos del kernel (procesos, drivers, dispositivos, objetos KOBJ, memoria) como un espacio de nombres accesible mediante una API unificada. La idea es que todo en NeoDOS tenga un nombre canónico y sea referenciable de forma homogénea.

El Unified Resource Namespace (URN) incluiría: (1) **URN scheme** — formato `neodos://<category>/<path>`: ej. `neodos://process/42` (PID 42), `neodos://driver/ata` (driver ATA), `neodos://device/com1` (serial port), `neodos://kobj/pipe/3` (pipe con KOBJ ID 3). (2) **URN resolver** — función `resolve_urn(urn_str) -> ResourceHandle` que parsea el URN, busca en el registro correspondiente (scheduler, driver_runtime, KOBJ registry, etc.), y devuelve un handle opaco. (3) **URN operations** — cada recurso resuelto soporta operaciones según su tipo: info (metadatos), stats (contadores), control (operaciones específicas). Por ejemplo, `neodos://driver/ata` soporta `info()` (ABI, estado, errores) y `stats()` (sectores leídos/escritos, timeouts). (4) **Shell command** — `URN INFO neodos://process/self`, `URN STATS neodos://driver/ata`.

URN no reemplaza los mecanismos existentes (PID, KOBJ ID, nombres de driver), sino que los unifica bajo una interfaz común. Útil para herramientas de diagnóstico y scripting.

Archivos: `src/namespace/mod.rs`.

20. **Z3. Virtual FS objects**

Sistema `/proc`-like que expone estructuras internas del kernel como archivos virtuales dentro del NeoDOS File System. Mientras que Z2 (URN) es una API programática, Z3 es un filesystem virtual accesible a través de las syscalls `sys_open`/`sys_read`/`sys_readfile` desde cualquier proceso userland, incluyendo el shell.

Virtual FS Objects (VFO) incluiría: (1) **Pseudo-partición** — un nuevo drive virtual (ej. `K:`) montado automáticamente por el kernel, que no corresponde a un disco físico sino a un `VirtualFS` que implementa el trait `FileSystem`. (2) **Estructura de directorios** — `K:\PROC\<PID>\` (status, cmdline, priority, handles), `K:\DRV\<name>\` (state, abi, caps, errors), `K:\MEM\` (total, free, slab usage, page cache stats), `K:\SYS\` (uptime ticks, version, build info), `K:\IRQ\` (vectors, counts). (3) **Lectura dinámica** — cuando un proceso hace `open("K:\\PROC\\42\\STATUS")`, el VFS genera el contenido en el momento: "PID: 42\nSTATE: Running\nPRIORITY: NORMAL\nTIME_SLICE: 100\nHANDLES: 4/16". (4) **Escritura de control** — ciertos archivos virtuales soportan escritura para control: `echo 0 > K:\PROC\42\PRIORITY` cambiaría la prioridad del proceso 42 a HIGH. (5) **Integración con URN (Z2)** — los paths de VFO mapean a URNs: `K:\PROC\42` ↔ `neodos://process/42`.

VFO permitiría que scripts de shell y herramientas userland inspeccionen y controlen el kernel sin necesidad de comandos especiales del shell ni syscalls exóticas. Por ejemplo, `TYPE K:\SYS\VERSION` mostraría la versión del kernel.

Archivos: `src/vfs/virtual_fs.rs` (nuevo VirtualFS), modificaciones en `src/vfs/mod.rs` (montar K: drive automáticamente en boot).

🌐 FASE 10 — NETWORKING STACK
21. **D9. Socket API**

API de sockets al estilo POSIX para procesos userland. Es la capa de abstracción que permite a los programas de usuario crear conexiones de red sin conocer los detalles del hardware de red ni del stack TCP/IP subyacente.

La Socket API incluiría: (1) **Syscalls de socket** — nuevas syscalls (RAX 30-39): `sys_socket(domain, type, protocol) -> fd` (crea socket, devuelve fd), `sys_bind(fd, addr, addrlen)`, `sys_connect(fd, addr, addrlen)`, `sys_listen(fd, backlog)`, `sys_accept(fd, &addr, &addrlen) -> new_fd`, `sys_send(fd, buf, len, flags) -> bytes_sent`, `sys_recv(fd, buf, len, flags) -> bytes_recv`, `sys_setsockopt(fd, level, opt, val)`. (2) **Socket handles** — nuevo tipo `Socket` en el handle system (handle.rs), con estado (CREATED, BOUND, LISTENING, CONNECTED, CLOSED), colas de receive/send, y dirección local/remota. (3) **Address families** — inicialmente solo `AF_INET` (IPv4) con `struct sockaddr_in` (sin_addr, sin_port en network byte order). (4) **libneodos wrappers** — funciones `socket()`, `bind()`, `connect()`, `listen()`, `accept()`, `send()`, `recv()` en libneodos que llaman a las syscalls.

La Socket API es la frontera entre userland y el stack de red. El stack interno (TCP/IP) implementa la lógica; la Socket API solo expone los endpoints a userland.

Archivos: `src/net/socket.rs`, `src/syscall.rs` (nuevas syscalls), `libneodos/src/net.rs`.

22. **E3. Network stack (TCP/IP)**

Stack completo de red con soporte para IPv4, ARP, ICMP, UDP y TCP, implementado como un driver NEM v3 (SYSTEM) que gestiona una tarjeta de red (inicialmente VirtIO Network, después RTL8139 o e1000 emulados por QEMU).

El stack incluiría: (1) **Ethernet driver** — VirtIO Network (VirtIO device ID 0x1000) o RTL8139 PCI (vendor 0x10EC, device 0x8139). Inicializa el dispositivo, configura las colas de recepción/transmisión, y registra un handler de interrupción para paquetes entrantes. (2) **ARP (Address Resolution Protocol)** — resolver direcciones IP a MAC. Cache ARP con timeout. Responder a solicitudes ARP para nuestra IP. (3) **IPv4** — recibir y enrutar paquetes IPv4. Soporte básico: checksum validation, reassembly (fragmentation opcional). (4) **ICMP** — responder a Echo Request (ping). Útil para verificar conectividad. (5) **UDP** — sockets UDP sin conexión. Soporte para checksum. (6) **TCP** — el más complejo: three-way handshake (SYN, SYN-ACK, ACK), sequence numbers, ventana deslizante, retransmisión con timeout, FIN/RST, estado de conexión (LISTEN, SYN_SENT, SYN_RECEIVED, ESTABLISHED, FIN_WAIT_1, FIN_WAIT_2, CLOSE_WAIT, CLOSING, LAST_ACK, TIME_WAIT, CLOSED). Máquina de estados TCP completa. (7) **Loopback** — interfaz `lo` (127.0.0.1) para comunicación entre procesos locales.

El stack de red debe integrarse con: IRP system (X6) para I/O asíncrono (recibir un paquete mientras se espera una respuesta), work queues (X5) para procesar paquetes fuera de IRQ context, scheduler (timer TCP retransmission), y Event Bus (notificar conexiones establecidas).

Dependencias críticas: IRP system (X6), work queues (X5), MSI/MSI-X (A11) para interrupciones de red, scheduler blocking para `recv()` bloqueante.

Archivos: `drivers/net/` (driver NEM v3), `neodos-kernel/src/net/` (protocol layers: `ether.rs`, `arp.rs`, `ipv4.rs`, `icmp.rs`, `udp.rs`, `tcp.rs`), `src/net/mod.rs`.

23. **D8. DHCP client**

Cliente DHCP (Dynamic Host Configuration Protocol) para obtener configuración de red automáticamente al arrancar: dirección IP, máscara de red, gateway predeterminado, y servidores DNS. Sin DHCP, la configuración de red debe ser manual (IP estática configurada en CONFIG.REG, Z6).

DHCP implementa: (1) **Protocolo** — enviar DHCPDISCOVER (broadcast UDP a 255.255.255.255:67), recibir DHCPOFFER, enviar DHCPREQUEST, recibir DHCPACK. Manejar renewal (antes del lease timeout, enviar DHCPREQUEST unicast). (2) **Transición de estados** — INIT (enviar DISCOVER), SELECTING (esperar OFFER), REQUESTING (enviar REQUEST), BOUND (configuración activa), RENEWING (renovar lease), REBINDING (broadcast renewal si unicast falla). (3) **Integración** — configurar la interfaz de red con la IP/máscara obtenida, añadir ruta por defecto al gateway, y almacenar DNS servers. (4) **Interfaz user** — comando `DHCP RENEW` para forzar renovación, `DHCP STATUS` para ver lease actual.

Implementado como proceso userland (parte de NEODOS.SYS o servicio independiente) usando la Socket API (D9). Depende de UDP (E3) y sockets.

Archivos: `userbin/dhcp/` (proyecto userland), libneodos socket wrappers.

24. **D7. NTP client**

Cliente NTP (Network Time Protocol) para sincronizar el reloj del sistema con servidores de tiempo en Internet. Actualmente, el tiempo del sistema se obtiene del RTC (CMOS) durante el boot y no se sincroniza después, acumulando deriva.

NTP client: (1) **Protocolo** — enviar petición NTP (modo 3, cliente) a un servidor NTP (pool.ntp.org o configurable). Recibir respuesta (modo 4, servidor) con timestamp de 64 bits (segundos + fracción desde 1900). (2) **Offset calculation** — calcular el offset entre el reloj local y el servidor usando los cuatro timestamps NTP (T1, T2, T3, T4). Ajuste gradual (no salto brusco) del contador de ticks del sistema. (3) **Periodic sync** — sincronizar cada 15-60 minutos. Almacenar el último offset para evitar grandes correcciones. (4) **Configuración** — servidor NTP configurable via CONFIG.REG (Z6): `NETWORK/NTP/SERVER = "pool.ntp.org"`. (5) **Fallback RTC** — si no hay red, mantener el tiempo del RTC con deriva estimada.

Implementado como proceso userland usando la Socket API (D9) y UDP (E3). Depende de DHCP (D8) o configuración manual de red.

Archivos: `userbin/ntp/` (proyecto userland).

🧑‍💻 FASE 11 — USERLAND USABLE SYSTEM
25. **S8. PATH resolution**

Sistema de búsqueda de ejecutables en múltiples directorios, equivalente a la variable `PATH` en Unix o MS-DOS. Actualmente, el comando `RUN` solo busca el binario en el directorio actual o con ruta absoluta. No hay un concepto de "path de búsqueda".

PATH resolution incluiría: (1) **Variable de entorno PATH** — al arrancar el shell, se define un PATH por defecto: `\BIN;.;\SYSTEM`. El usuario puede modificarlo con `SET PATH=\BIN;\SYSTEM;\UTILS`. (2) **Búsqueda en múltiples directorios** — al ejecutar un comando sin ruta absoluta, el shell itera por los directorios del PATH buscando un archivo con extensión `.BIN` o `.ELF` (y sin extensión). (3) **Prioridad de extensiones** — `.BIN` tiene prioridad sobre `.ELF`. Si hay un comando built-in y un binario con el mismo nombre, gana el built-in (como en MS-DOS con `DIR` vs `DIR.BIN`). (4) **TAB autocomplete con PATH** — al completar la primera palabra, el shell busca no solo en el directorio actual sino también en todos los directorios del PATH, mostrando los resultados completos.

La implementación afecta a `handler.rs` (búsqueda de comandos), `shell.rs` (`try_complete` para autocomplete), y `commands/run.rs` (resolución de path antes de cargar binario).

Archivos: `src/shell/path.rs` (nuevo, lógica de PATH), modificaciones en `src/shell/handler.rs` y `src/shell/commands/run.rs`.

26. **S9. Shell pipes**

Conectores entre procesos (pipes en línea de comandos) que permiten encadenar la salida de un proceso como entrada del siguiente, usando el operador `|`. Ejemplo: `DIR | SORT.BIN` listaría archivos y los pasaría al binario SORT para ordenarlos.

Shell pipes requeriría: (1) **Parser de pipes** — el shell detecta el carácter `|` en la línea de comandos y divide la línea en comandos separados: `DIR` y `SORT.BIN`. Para cada pipe, crea un pipe del sistema (`sys_pipe`) antes de ejecutar el comando. (2) **Redirección de fd** — para el comando antes del pipe: su stdout (fd 1) se redirige al writer del pipe mediante `sys_dup2(pipe_writer, 1)`, y se cierra el pipe_writer original. Para el comando después del pipe: su stdin (fd 0) se redirige al reader del pipe mediante `sys_dup2(pipe_reader, 0)`. (3) **Ejecución secuencial** — los procesos se lanzan en orden, pero el shell espera a que ambos terminen (o espera al último si el primero termina antes). (4) **Múltiples pipes** — soporte para cadenas: `CMD1 | CMD2 | CMD3` crea dos pipes y tres procesos, conectando stdout→pipe1→stdin→CMD2→stdout→pipe2→stdin→CMD3. (5) **Pipe errors** — si un pipe falla (EPIPE porque el reader cerró antes), el escritor debe recibir `-EPIPE` en `sys_write` y manejarlo.

Requiere: IPC/pipes existente (sys_pipe + sys_dup2), waitpid para esperar procesos encadenados.

Archivos modificados: `src/shell/parser.rs` (análisis de pipes), `src/shell/commands/run.rs` (ejecución con pipes).

27. **S3. Shell redirection**

Redirección de entrada/salida estándar a archivos usando los operadores `>`, `>>` y `<`. Ejemplo: `DIR > LISTADO.TXT` (escribe salida de DIR a archivo), `SORT.BIN < ENTRADA.TXT > SALIDA.TXT` (lee de archivo, escribe a archivo).

Redirección incluiría: (1) **Parser** — detectar `>`, `>>` (append), `<` en la línea de comandos. Separar el comando de los archivos de redirección. (2) **Redirección de salida (`>`)** — antes de ejecutar el comando, el shell abre (o crea) el archivo de salida con `sys_open(filename, O_WRONLY|O_CREAT)`. Luego usa `sys_dup2(file_fd, 1)` para redirigir stdout al archivo. Cierra el fd original. (3) **Append (`>>`)** — igual que `>` pero abre con `O_WRONLY|O_APPEND`, posicionándose al final del archivo. (4) **Redirección de entrada (`<`)** — abre el archivo de entrada con `sys_open` y usa `sys_dup2(file_fd, 0)` para redirigir stdin. (5) **Combinación con pipes** — `DIR | SORT.BIN > SALIDA.TXT` combina pipe y redirección: stdout de DIR va al pipe, stdin de SORT.BIN lee del pipe, stdout de SORT.BIN va al archivo.

Requiere: sys_open con flags, sys_dup2 existente, parser de shell.

Archivos modificados: `src/shell/parser.rs`, `src/shell/commands/run.rs`.

28. **B2. ANSI terminal**

Soporte de secuencias de escape ANSI para control de terminal: colores, posicionamiento de cursor, borrado de pantalla, y estilos de texto. Actualmente, el shell usa salida raw: solo texto plano CRLF, sin colores ni control de cursor.

ANSI terminal incluiría: (1) **Parser ANSI** — el módulo de salida de consola (framebuffer + serial) parsea secuencias ESC `\x1B[`: `\x1B[31m` (texto rojo), `\x1B[1;32m` (negrita + verde), `\x1B[2J` (clear screen), `\x1B[H` (cursor home), `\x1B[<row>;<col>H` (cursor position), `\x1B[K` (clear line), `\x1B[?25l` / `\x1B[?25h` (ocultar/mostrar cursor). (2) **Tabla de colores** — 16 colores estándar ANSI (0-15: black, red, green, yellow, blue, magenta, cyan, white + bright variants) mapeados a la paleta del framebuffer (si es RGB) o a colores VGA fijos. (3) **Integración con output** — las funciones `_print()` y `_eprint()` no parsean ANSI directamente; en su lugar, se añade un `AnsiWriter` que envuelve el writer base y parsea secuencias antes de llamar al writer real. (4) **Shell prompt** — usar códigos ANSI para mostrar el prompt en verde (`\x1B[32mC:\> \x1B[0m`), comandos en cian al autocompletar, errores en rojo. (5) **Mensajes de error** — errores del kernel y shell en rojo, warnings en amarillo, info en blanco.

Archivos: `src/console/ansi.rs`, modificaciones en `src/io.rs` (console output), `src/shell.rs` (prompt y mensajes coloreados).

29. **B1. Virtual terminals**

Múltiples sesiones de consola virtual (VT) accesibles mediante combinaciones de teclas (ej. Alt+F1, Alt+F2, etc.), permitiendo tener varios contextos de shell simultáneos. Actualmente, solo hay una sesión de shell: la salida del framebuffer y la entrada del teclado están dedicadas al único shell.

Virtual terminals requeriría: (1) **VT Manager** — estructura que mantiene N terminales virtuales (ej. 4), cada una con su propio buffer de texto (80×25 caracteres), cursor position, estado ANSI, y proceso shell asociado. (2) **Shell switching** — al pulsar Alt+F1..F4, el VT Manager guarda el estado del terminal actual (buffer de texto) y restaura el del nuevo terminal. Cambia el framebuffer para mostrar el buffer del VT activo. (3) **Keyboard routing** — la entrada de teclado se dirige al shell del VT activo (cada VT tiene su propio ring-buffer de input). Los procesos en VTs inactivos siguen ejecutándose en background. (4) **Process-per-VT** — cada VT ejecuta su propia instancia del shell. Al cambiar de VT, el shell activo pasa a background y el del nuevo VT pasa a foreground. (5) **Serial output** — en serial se muestra el VT activo (o todos, según configuración). La salida serial añade una cabecera `[VT1]` para identificar el VT de origen.

Archivos: `src/console/vt.rs`, modificaciones en `src/shell.rs`, `src/input.rs`, `src/framebuffer.rs`.

30. **B6. NeoEdit**

Editor de texto en modo texto para el shell NeoDOS, permitiendo crear y modificar archivos de texto directamente desde la terminal. Es la primera herramienta de usuario real (más allá de comandos de shell).

NeoEdit sería: (1) **Modo de uso** — comando `EDIT <archivo>`. Si el archivo no existe, se crea nuevo. Si existe, se carga en el buffer de edición. (2) **Interfaz** — editor modal (similar a MS-DOS Edit o Nano): área de texto ocupa la mayor parte de la pantalla, barra de estado en la parte inferior (nombre de archivo, cursor row/col, modo insert/overwrite), barra de acceso rápido en la parte superior (F1=Help, F2=Save, F3=Open, F10=Exit). (3) **Funciones** — navegación con flechas, PageUp/PageDown, Home/End, Insert (toggle insert/overwrite), Delete, Backspace. Selección con Shift+flechas. Portapapeles con Ctrl+C (copy), Ctrl+X (cut), Ctrl+V (paste). (4) **Archivo** — Save (Ctrl+S o F2): escribe el buffer al archivo mediante sys_writefile. Open (F3): abre otro archivo. Exit (F10 o ESC): si hay cambios no guardados, pregunta "Save changes? (Y/N/Cancel)". (5) **Búsqueda** — Ctrl+F: buscar texto, Enter para siguiente, Shift+Enter para anterior.

Implementado como binario userland (proyecto en `userbin/neoedit/`) usando libneodos, syscalls de archivo, y control ANSI de terminal. Depende de ANSI terminal (B2) para control de cursor y pantalla.

Archivos: `userbin/neoedit/main.rs`, `userbin/neoedit/Cargo.toml`, `userbin/neoedit/user.ld`.

31. **B6b. Shared library system (libneodos DLL)**

Sistema de biblioteca compartida para procesos Ring 3. Compilar libneodos como binario standalone en una dirección fija reservada en el espacio de usuario (ej: `0x30000000`), con tabla de exportación de funciones `extern "C"` (syscall wrappers, IO, FS, mem, panic). El kernel mapea el DLL en cada proceso Ring 3 al crearlo (`spawn_usermode`). Los binarios de usuario se enlazan contra la DLL en lugar de incluir el código estáticamente, reduciendo tamaño en disco y compartiendo páginas de código en RAM entre procesos. Requiere: dirección fija reservada fuera de user slots y heap, export table con ABI estable, loader en el kernel, actualización de linker scripts (`user.ld`) y build system, y compatibilidad `extern "C"` en todos los puntos de entrada de libneodos.

32. **B7. NeoTOP**

Monitor interactivo de procesos y recursos del sistema, similar al comando `top` de Unix o `TASKMAN` de MS-DOS. Ejecutable como comando `TOP` desde el shell.

NeoTOP incluiría: (1) **Pantalla en tiempo real** — refresco periódico (ej. cada 2 segundos) mostrando: PID, nombre (si está disponible), estado (Running/Ready/Blocked/Terminated), prioridad, time slice restante, uso de CPU (porcentaje de ticks desde última actualización), memoria usada (heap + mmap), handles abiertos. (2) **Ordenación** — por defecto por PID; opciones: `TOP -CPU` (por uso de CPU), `TOP -MEM` (por memoria), `TOP -PRI` (por prioridad). (3) **Acciones** — desde NeoTOP se pueden enviar señales a procesos: `K` (kill), `P` (cambiar prioridad). Requiere soporte de teclado para comandos de una tecla. (4) **Barra de sistema** — cabecera con: uptime, procesos totales/activos, memoria total/libre/usada, carga de CPU promedio. (5) **Implementación** — programa userland que lee `K:\PROC\*` (Virtual FS Objects, Z3) para obtener información de procesos, y usa syscalls para acciones.

Depende de VFO (Z3) para obtener datos de procesos sin syscalls especiales. Alternativa: syscall `sys_process_info(pid) -> ProcessInfo` si VFO no está disponible.

Archivos: `userbin/neotop/main.rs`, `userbin/neotop/Cargo.toml`.

33. **B11. NeoShell scripting**

Lenguaje de scripting para el shell NeoDOS, permitiendo automatizar tareas mediante scripts (archivos `.BAT` o `.SH`). Actualmente, el shell solo ejecuta comandos individuales introducidos por el usuario.

NeoShell scripting incluiría: (1) **Script execution** — comando `CALL SCRIPT.BAT` o ejecución directa de `.BAT`. El shell lee el archivo línea por línea y ejecuta cada línea como un comando del shell. (2) **Variables de script** — `SET VAR=valor`, `%VAR%` para expansión. Variables locales al script (no persistentes como las de entorno). (3) **Control de flujo** — `IF EXIST archivo (comando)`, `IF %VAR%==valor (comando)`, `GOTO label`, `:label`, `FOR %%F IN (*.TXT) DO (comando)`. (4) **Comandos de script** — `ECHO` (mostrar texto), `PAUSE` (esperar tecla), `REM` (comentario), `SHIFT` (shift argumentos), `CALL` (llamar otro script). (5) **Parámetros** — `%1`..`%9` para argumentos del script, `%*` para todos los argumentos. (6) **Exit code** — `ERRORLEVEL` para detectar si el último comando falló.

Implementación: el parser de shell existente se extiende para detectar y manejar construcciones de script. La ejecución de scripts se hace en el shell actual (no como proceso separado), para mantener el estado de variables.

Archivos: `src/shell/script.rs`, modificaciones en `src/shell/handler.rs` (detectar `.BAT`), `src/shell/parser.rs` (extensiones de parsing).

34. **B12. Compositor 2D**

Sistema gráfico básico para dibujar ventanas y elementos 2D en el framebuffer, la base para una futura interfaz gráfica de usuario (GUI). Actualmente, el framebuffer solo muestra texto de la consola del shell. Un compositor 2D permitiría dibujar ventanas, botones, y gráficos simples.

El Compositor 2D incluiría: (1) **Compositor kernel** — servicio Ring 0 que gestiona el framebuffer como una escena de capas (layers): cada layer es un buffer de píxeles (ARGB 32-bit) con posición (x, y) y tamaño. El compositor mezcla las capas en orden Z en el framebuffer real. (2) **API de dibujo** — funciones `compositor_create_window(x, y, w, h, title) -> window_id`, `compositor_destroy_window(id)`, `compositor_set_pixel(id, x, y, color)`, `compositor_fill_rect(id, x, y, w, h, color)`, `compositor_blit(id, src_x, src_y, dst_x, dst_y, w, h, buf)`. (3) **Font rendering** — renderizar texto en ventanas usando la font VGA 8×16 (existente) o una font bitmap adicional. Función `compositor_draw_text(id, x, y, text, color)`. (4) **Integración con teclado** — el Event Bus envía eventos de teclado al compositor, que los reenvía a la ventana activa (focus). (5) **Shell integration** — el shell actual se convierte en una ventana más del compositor, o sigue usando el framebuffer directamente mientras el compositor está inactivo.

Implementación escalonada: primero el compositor como capa sobre el framebuffer existente (manteniendo la consola de texto como layer 0), después exponiendo la API de dibujo como syscalls (RAX 40-49). Procesos userland pueden dibujar ventanas llamando a las syscalls del compositor.

Archivos: `src/compositor/mod.rs`, `src/compositor/font.rs`, `src/compositor/layer.rs`, syscalls en `src/syscall.rs`.

🔐 FASE 12 — SECURITY HARDENING
35. **U1. Module signature validation**

Validación criptográfica de drivers NEM mediante firmas digitales antes de cargarlos, impidiendo la ejecución de código no confiable o modificado. Actualmente, cualquier driver .nem se carga sin verificar su origen o integridad. Un atacante con acceso al disco podría reemplazar un driver legítimo por uno malicioso.

Module signature validation incluiría: (1) **Formato de firma** — extensión del header NEM v4 para incluir una firma digital: algoritmo (RSA-2048 o Ed25519), hash del contenido del driver (SHA-256), y la firma del hash. El campo `signature` se añade al final del archivo .nem, después del código. (2) **Clave pública del kernel** — una clave pública embebida en el kernel en tiempo de compilación. Solo los drivers firmados con la clave privada correspondiente se consideran de confianza. (3) **Verificación en carga** — durante `driver_load()`, antes de ejecutar el driver, el kernel: (a) calcula SHA-256 del contenido del driver (excluyendo la firma), (b) verifica la firma RSA/Ed25519 contra la clave pública embebida. (4) **Política de firma** — configurable via CONFIG.REG (Z6): `SYSTEM/SECURITY/SIGNATURE_POLICY` = `REQUIRED` (rechazar drivers sin firmar), `WARN` (cargar pero loguear advertencia), `DISABLED` (sin verificación, comportamiento actual). Los drivers BOOT requieren firma siempre. (5) **Key management** — herramienta `scripts/sign_driver.py` para firmar un .nem con la clave privada. Almacenamiento seguro de la clave privada fuera del repositorio.

Dependencia: hashing SHA-256 en kernel (nuevo módulo `src/crypto/sha256.rs`), verificación RSA/Ed25519 (o usar una biblioteca existente).

Archivos: `src/crypto/sha256.rs`, `src/drivers/signature.rs`, `scripts/sign_driver.py`, modificación de `src/drivers/loader.rs` y header NEM v4.

36. **U3. Driver permission enforcement**

Control granular de permisos para drivers NEM en runtime, asegurando que un driver solo accede a los recursos que explícitamente se le han concedido. Mientras que el Capability System (X3) controla qué tipo de recursos puede usar un driver, Permission Enforcement controla a qué instancias específicas de esos recursos puede acceder.

Permission enforcement incluiría: (1) **Access Control Lists (ACLs)** — cada recurso protegido (block device, IRQ vector, puerto I/O, región MMIO, pipe) tiene una ACL que lista qué drivers tienen permiso para acceder. Formato: lista de `(driver_id, permissions_mask)`. (2) **Permission types** — por ejemplo, para un block device: `PERM_READ`, `PERM_WRITE`, `PERM_FLUSH`, `PERM_GET_INFO`. Un driver de diagnóstico puede tener solo `PERM_READ` en el disco del sistema. (3) **Runtime enforcement** — cada función de la export table (`hst_register_block_device`, `hst_send_event`, etc.) verifica que el driver llamante tenga el permiso necesario en el recurso objetivo. Si no, retorna `-EACCES` (Permission Denied). (4) **Permission inheritance** — los drivers BOOT reciben permisos completos sobre todos los recursos. Los drivers SYSTEM reciben permisos según su declaración (en el header NEM v4, campo `required_permissions`). Los drivers DEMAND deben solicitar permisos explícitamente. (5) **Shell command** — `PERM <driver> <resource> <+/-perm>` para modificar permisos en runtime (solo con privilegios de administrador).

Archivos: `src/drivers/perms.rs`, modificaciones en export table (`src/hal/exports.rs` o equivalente), shell command `perm`.

37. **U4. Secure boot chain**

Cadena de arranque verificada desde el firmware UEFI hasta el kernel y los drivers del sistema, asegurando que cada componente en la cadena de arranque es auténtico y no ha sido modificado. Es la extensión del Secure Boot de UEFI hacia el sistema NeoDOS completo.

Secure boot chain incluiría: (1) **UEFI Secure Boot** — el bootloader.efi está firmado con una clave que la UEFI firmware reconoce (mediante el protocolo Secure Boot de UEFI). El firmware verifica la firma antes de ejecutar el bootloader. (2) **Bootloader verifica kernel** — antes de cargar kernel.elf, el bootloader verifica su firma (similar a U1 pero en el bootloader). Solo carga kernel.elf firmado. (3) **Kernel verifica drivers BOOT** — al cargar drivers de `C:\SYSTEM\DRIVERS\BOOT\` (PHASE 3.85), el kernel verifica sus firmas. Si un driver BOOT no está firmado o la firma es inválida, el boot se detiene con un mensaje de error. (4) **Kernel verifica drivers SYSTEM** — los drivers SYSTEM se verifican igual que los BOOT, pero si fallan, el boot continúa (el driver se marca FAULTED) y se muestra una advertencia. (5) **Measured boot (opcional)** — extendido con TPM (Trusted Platform Module): el bootloader mide (hashea) cada componente en los PCRs del TPM antes de cargarlo, permitiendo atestación remota de la integridad del sistema.

La cadena de confianza es: `UEFI FW → Bootloader (firmado) → kernel.elf (verificado por bootloader) → drivers BOOT (verificados por kernel) → drivers SYSTEM (verificados por kernel)`. Si cualquier eslabón falla, el sistema no arranca (o arranca en modo seguro con funcionalidad reducida).

Depende de: U1 (signature validation), infraestructura criptográfica en bootloader.

Archivos: modificación de `neodos-bootloader/` para verificación de firmas, scripts de firma, gestión de claves.

⚡ FASE 13 — PERFORMANCE & MEMORY EVOLUTION
38. **V2. Zero-copy pipes**

Optimización del sistema de pipes (IPC) para eliminar copias de datos innecesarias entre procesos. Actualmente, cada escritura a un pipe copia los datos del buffer del usuario al pipe buffer (4 KB estático), y cada lectura copia los datos del pipe buffer al buffer del usuario. Esto significa dos copias de memoria por transferencia.

Zero-copy pipes eliminarían las copias mediante: (1) **Page-based pipes** — en lugar de un buffer fijo de 4 KB, el pipe manager asigna páginas de 4 KB directamente del frame allocator. Al escribir, el kernel mapea la página del escritor como lectura en el espacio del lector (o viceversa), evitando la copia. (2) **Shared memory ring buffer** — el pipe buffer es una región de memoria compartida entre escritor y lector, con un ring buffer de páginas. El escritor llena páginas y pasa la propiedad (refcount) al lector. (3) **sys_pipe con pages** — `sys_pipe` devuelve fd de lectura/escritura como antes, pero internamente el pipe se compone de slots de página. La escritura hace `swap` de páginas: la página del escritor se añade a la cola del lector, y se asigna una página nueva (limpia) al escritor. (4) **Large transfers** — para transferencias > 4 KB, se pueden encadenar múltiples páginas en una lista, permitiendo transfers de hasta 64 KB sin copias intermedias. (5) **mmap-backed pipes** — alternativa: el pipe buffer se implementa como una región mmap compartida entre procesos (MAP_SHARED), accesible directamente por ambos sin syscalls de copia.

Zero-copy beneficia especialmente a pipelines de shell (S9) y procesos que transfieren grandes volúmenes de datos.

Dependencia: mmap avanzado (MAP_SHARED), frame allocator, manejo de refcount de páginas.

Archivos: `src/pipe.rs` (reescritura del pipe buffer), `src/syscall.rs` (sys_pipe extendido).

39. **V3. Copy-on-write fork**

Implementación de `fork()` con copy-on-write (COW) para crear procesos hijos de forma eficiente, compartiendo páginas de memoria entre padre e hijo hasta que uno de los dos escribe. Actualmente, la única forma de crear un proceso es `RUN` que carga un binario desde disco en una dirección fija. No hay `fork()`.

COW fork incluiría: (1) **sys_fork(RAX=40)** — nueva syscall sin argumentos. Crea un proceso hijo que es una copia exacta del padre: mismo código, mismo stack, mismo heap, mismos handles (los pipes incrementan refcount). El hijo retorna 0, el padre retorna el PID del hijo. (2) **COW page tables** — al hacer fork, el kernel no copia páginas. En su lugar, copia la page table del padre al hijo, pero marca todas las páginas como read-only y reserva un flag COW en el PTE (bit 9, disponible para software). (3) **Page fault COW** — cuando el padre o el hijo escriben a una página COW, el page fault handler detecta que la página es COW (no es un accesso inválido), asigna una nueva página física, copia el contenido, actualiza la PTE del escritor con permisos RW, y decrementa el refcount de la página original. (4) **Zombie prevention** — al hacer fork, el kernel registra la relación padre-hijo en el scheduler. Cuando el hijo termina, su slot no se recicla hasta que el padre hace `sys_waitpid(pid)` (similar a Unix). Si el padre termina antes que el hijo, los hijos se reasignan al proceso init (PID 1). (5) **Integración con COW y exec** — después de fork, el hijo típicamente hace `sys_execve(path, args)` (syscall futura) para cargar un nuevo binario, que descarta todas las páginas COW y carga el nuevo programa.

COW fork es la base para procesos ligeros, shells modernos, y servidores que necesitan aislar peticiones.

Dependencia: page table manipulation en `arch/x64/paging.rs`, page fault handler extendido, scheduler (relación padre-hijo).

Archivos: `src/process.rs` (sys_fork), `arch/x64/paging.rs` (COW PTE flag, COW page fault handler), `src/syscall.rs`.

40. **X10. Per-CPU allocators**

Allocadores de memoria independientes por cada núcleo CPU para eliminar contención en el slab allocator global. Actualmente, el slab allocator usa un único `spin::Mutex` que protege las 9 caches de size classes. En un sistema SMP, esto significa que todos los núcleos compiten por el mismo lock, limitando el escalabilidad.

Per-CPU allocators incluirían: (1) **CPU-local slab caches** — cada CPU tiene su propio conjunto de 9 slab caches (size classes 8–2048 bytes), sin locks compartidos. La asignación de un objeto pequeño (≤2KB) se hace desde el cache local, sin adquirir ningún lock. (2) **Borrowing** — si el cache local de una CPU se vacía, "toma prestados" slabs del cache global (con lock breve). De manera similar, si un CPU acumula demasiados slabs libres, los devuelve al pool global. (3) **Large allocations (>2KB)** — siguen yendo al `linked_list_allocator` global, que ya tiene su propio lock. Para escalar, se podría tener un heap por CPU o un heap con arena locking. (4) **CPU-local page pools** — cada CPU mantiene un pequeño pool de páginas físicas pre-asignadas (ej. 64 páginas = 256 KB) para `alloc_page()`, reduciendo la contención en el frame bitmap global. (5) **Lock-free statistics** — contadores de uso (allocated, freed, slab misses) por CPU, agregados globalmente solo cuando se consultan (ej. comando `MEM`).

Dependencia: identificación de CPU (APIC ID), soporte SMP (X8), memoria per-CPU (segmento de datos por CPU).

Archivos: `src/slab.rs` (reescritura para per-CPU), `src/memory.rs` (per-CPU page pool), `arch/x64/smp.rs` (identificación de CPU).

🧱 FASE 14 — SMP ENABLEMENT
41. **X8. SMP-safe kernel refactor**

Reescritura mínima del kernel para soportar múltiples núcleos de CPU (Symmetric Multi-Processing). Actualmente, el kernel entero asume un solo núcleo: el scheduler es una cola global única, el slab allocator tiene un solo lock, el frame bitmap no es seguro para acceso concurrente, y no hay coordinación entre CPUs.

El SMP refactor incluiría: (1) **APIC discovery** — detectar Local APIC y I/O APIC mediante ACPI MADT (Multiple APIC Description Table). Obtener el número de CPUs, sus APIC IDs, y la dirección MMIO del I/O APIC. (2) **AP startup** — usando SIPI (Startup IPI), despertar los APs (Application Processors) desde el BSP (Bootstrap Processor). Cada AP ejecuta su propio código de inicialización: configura GDT, IDT, paginación, y su TSS. (3) **Per-CPU data** — cada CPU tiene su propia estructura de datos (CPU-local storage), accesible mediante segmento GS (swap GS). Contiene: CPU ID, idle stack, idle process pointer, y puntero al proceso actual. (4) **Scheduler SMP** — el scheduler se vuelve per-CPU: cada CPU tiene su propia cola de procesos ready. Un proceso se asigna a una CPU al crearse (o mediante migración si una CPU está idle). Se implementa load balancing: si una CPU está idle y otra tiene procesos esperando, se migra uno. (5) **Locking audit** — revisar todos los locks del kernel (`spin::Mutex`, `spin::RwLock`) para asegurar que protegen correctamente el estado compartido. Identificar datos globales que deben ser per-CPU o protegidos con locks más finos. (6) **Inter-CPU interrupts (IPI)** — enviar interrupciones entre CPUs para: TLB shootdown (cuando un CPU modifica una page table), reschedule remoto (cuando un proceso se despierta en otra CPU), y funciones de llamada remota (call_function).

El objetivo es que todas las funcionalidades existentes (syscalls, VFS, pipes, drivers) funcionen correctamente en múltiples CPUs sin cambios en su lógica interna.

Dependencias: APIC timers (C3), per-CPU allocators (X10) para rendimiento, MSI/MSI-X (A11) para interrupciones de dispositivos.

Archivos: `arch/x64/smp.rs` (AP startup, IPI), `src/scheduler.rs` (per-CPU run queues, load balancing), `src/memory.rs` (per-CPU page pool), `src/lock.rs` (audit de locks), modificaciones en `src/slab.rs`, `src/drivers/` etc.

🧪 FASE 15 — EXPERIMENTAL FUTURE
42. **E4. Full GUI system**

Sistema de interfaz gráfica de usuario completo con ventanas, menús, botones, y soporte para aplicaciones gráficas, construido sobre el Compositor 2D (B12). Es la evolución de la interfaz de texto a un entorno gráfico moderno.

Incluiría: (1) **Window Manager** — servicio userland que gestiona ventanas: creación, destrucción, mover (drag con ratón), redimensionar, minimizar, maximizar, foco (z-order). Cada ventana tiene un title bar con botones de cerrar/minimizar/maximizar. (2) **Widget toolkit** — biblioteca gráfica en userland (`libneogui`) con widgets: Button, Label, TextBox, ListBox, MenuBar, ScrollBar, CheckBox, RadioButton. Event-driven (click, keypress, paint). (3) **Mouse support** — capturar eventos de ratón (necesita PS/2 mouse driver o USB HID) y pasarlos a la ventana bajo el cursor. Cursor renderizado por el compositor como una capa más. (4) **Desktop** — fondo de pantalla, barra de tareas (taskbar) con botones de ventanas abiertas, menú de inicio (o equivalente). (5) **Font rendering avanzado** — en lugar de la font VGA 8×16, usar una font bitmap de mayor calidad (ej. Terminus 8×16 o 10×20) o font TrueType rasterizada.

Dependencias: ratón (PS/2 o USB HID, C1), Compositor 2D (B12), scheduler para eventos de input.

43. **E5. Advanced secure boot**

Extensión del Secure Boot chain (U4) con políticas avanzadas de arranque, atestación remota, y soporte para múltiples claves y jerarquías de confianza.

Incluiría: (1) **Multiple key hierarchy** — claves de fabricante (OEM), claves del sistema (NeoDOS), claves del usuario (drivers personalizados). El kernel verifica la cadena de firmas según el origen del driver. (2) **TPM integration** — usar el TPM (Trusted Platform Module) para medir (hash) cada componente del boot en los PCRs (Platform Configuration Registers). Al arrancar, el kernel extiende los PCRs con la medición de cada driver antes de cargarlo. (3) **Remote attestation** — un servicio userland puede leer los PCRs del TPM y generar un certificado de atestación para verificar remotamente que el sistema está ejecutando software no modificado. (4) **Secure boot policies** — políticas configurables via CONFIG.REG: `REQUIRED` (todos los drivers deben estar firmados), `BOOT_ONLY` (solo drivers BOOT, policy actual), `PERMISSIVE` (advertencias pero permite continuar). (5) **Boot audit log** — registro sellado (firmado) de todos los componentes cargados durante el boot, almacenado en una partición protegida, legible solo por herramientas autorizadas.

Dependencias: U1 (signature validation), U4 (secure boot chain), TPM driver.

44. **E6. Package manager**

Sistema de gestión de paquetes para instalar, actualizar y eliminar software en NeoDOS, con repositorios remotos y resolución de dependencias. Es el equivalente a `apt` (Debian), `pacman` (Arch), o `pkg` (FreeBSD).

Incluiría: (1) **Package format** — archivos `.NDP` (NeoDOS Package): cabecera con metadatos (nombre, versión, descripción, autor, dependencias), archivos incluidos (binarios, configuraciones, scripts), y firma digital. (2) **Package database** — base de datos local en `C:\SYSTEM\PKG\` con registro de paquetes instalados, versiones, checksums, y lista de archivos instalados. (3) **Dependency resolution** — al instalar un paquete, resolver sus dependencias recursivamente. El resolutor maneja versiones mínimas, conflictos, y paquetes opcionales. (4) **Repository support** — repositorios remotos accesibles vía red (HTTP) cuando el networking stack (E3) esté disponible. Repositorio oficial: `pkg.neodos.io`. Repositorios locales: desde disco o USB. (5) **Commands** — `PKG INSTALL <package>`, `PKG REMOVE <package>`, `PKG UPDATE`, `PKG LIST`, `PKG INFO <package>`, `PKG REPO ADD <url>`. (6) **Integrity verification** — cada paquete se verifica criptográficamente antes de instalarse (firma + checksum). Los archivos se extraen con permisos seguros.

Dependencias: red (E3), HTTP client, firmas digitales (U1), resolver de dependencias (inspirado en W4 pero para userland).

45. **T4. Time-travel debugging**

Sistema de depuración determinista que graba la ejecución del kernel y permite reproducirla hacia adelante y hacia atrás, facilitando el diagnóstico de bugs intermitentes y condiciones de carrera.

Incluiría: (1) **Execution recorder** — componente del kernel que graba en un buffer circular: el flujo de interrupciones (IRQ, IPI), la secuencia de syscalls, los resultados de operaciones no-deterministas (RDTSC, lecturas de puerto I/O), y los context switches del scheduler. (2) **Deterministic replay** — el reproductor puede reiniciar la ejecución desde un punto grabado, reproduciendo exactamente la misma secuencia de eventos. El estado inicial (registros, memoria) se restaura desde un snapshot. (3) **Reverse execution** — el reproductor puede ejecutar hacia atrás: restaura un snapshot anterior y reproduce hacia adelante hasta el punto deseado. (4) **Condition checking** — el desarrollador establece condiciones: "reproducir hasta que la variable X en la dirección 0x1234 sea 5", o "hasta que ocurra un page fault en Ring 0". (5) **Integration con GDB** — el registro de ejecución se exporta por serial y se convierte a un formato que GDB puede consumir (o un front-end custom en Python).

Dependencia: infraestructura de tracing (Y1), capacidad de snapshot de memoria.

46. **T5. Live kernel patching**

Sistema para aplicar parches al kernel en caliente (sin reiniciar), permitiendo corregir bugs de seguridad o estabilidad en el kernel en ejecución sin interrumpir los servicios.

Incluiría: (1) **Patch format** — archivos `.NHP` (NeoDOS Hot Patch): contiene la función original (firma), la función de reemplazo (código x86-64), y la firma del parche. (2) **Function replacement** — el kernel reemplaza el prólogo de la función original con un salto (`jmp`) a la función de reemplazo (ftrace-style). Alternativa: modificaciones en la llamada (e. g., modificar la GOT/export table). (3) **Atomic replacement** — para funciones críticas (scheduler, syscall dispatch), el reemplazo se hace con un RCU-style (Read-Copy-Update): se publica un nuevo puntero a la función después de asegurar que ningún núcleo está ejecutando la versión antigua. (4) **Verification** — el kernel verifica que el parche es compatible con la versión actual del kernel (checksum de la función original) y que está firmado. (5) **Rollback** — posibilidad de revertir un parche (restaurar la función original) si causa problemas.

Dependencias: firmas digitales (U1), conocimiento de símbolos del kernel (tabla de símbolos), soporte SMP para reemplazo atómico.

47. **T2. Distributed NeoDOS nodes**

Sistema de nodos NeoDOS distribuidos en red, permitiendo que múltiples instancias de NeoDOS se coordinen como un cluster, compartan recursos, y ejecuten tareas distribuidas.

Incluiría: (1) **Node discovery** — detectar otros nodos NeoDOS en la red local mediante multicast/broadcast o un servicio de directorio centralizado. (2) **Distributed resource namespace** — extensión del URN (Z2) para referenciar recursos en nodos remotos: `neodos://node2/process/5` (proceso 5 en node2). (3) **Remote IPC** — pipes y sockets que funcionan a través de la red mediante un protocolo de comunicación entre nodos (NeoDOS Distributed Protocol, NDP). (4) **Distributed filesystem** — el NeoDOS FS se extiende para operar en modo distribuido: un nodo puede montar el FS de otro nodo y leer/escribir archivos remotamente. (5) **Computation distribution** — un proceso puede migrar de un nodo a otro (process migration), o un nodo puede ejecutar tareas en otros nodos (remote procedure call / remote task execution). (6) **Fault tolerance** — si un nodo cae, sus servicios son reasignados a otros nodos (failover).

Dependencias: networking stack completo (E3), URN (Z2), distributed consensus (Raft/Paxos) para coordinación.

🧭 RESUMEN FINAL
1. Memory core (slab + handles + KOBJ + page cache) ✅
2. Concurrency (scheduler + events + work queues) ✅
3. Async I/O system (IRP) ✅
4. Driver safety layer (ABI + deps + capability done; isolation, hot-reload pending)
5. Observability & debugging — pending
6. Modern hardware (PCIe + MSI + APIC) — pending
7. Storage + USB drivers (ATA/AHCI done; VirtIO, NVMe, NCQ, USB pending)
8. Service layer (NeoInit + namespace) — pending
9. Networking stack — pending
10. Userland usable system (libneodos + DLL system done; PATH, pipes, redirect, ANSI, edit, top, scripting, compositor pending)
11. Security hardening — pending
12. Performance tuning — pending
13. SMP enablement — pending
14. Experimental features — pending
