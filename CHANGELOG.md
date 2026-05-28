# Changelog

## v0.19.0 — 2026-05-28

### ACPI Poweroff Driver — Añadido
- **Añadido**: `drivers/acpi/` — NEM v3 standalone driver for ACPI S5 poweroff. Scans PCI for PIIX4 (0x7113) / ICH9 (0x2918/0x2916) LPC bridges, detects PM1a port via GPIO/ABASE registers, and writes `SLP_TYP_S5 | SLP_EN` to trigger soft-off.
- **Añadido**: Fallback poweroff ports — QEMU Bochs (0x604, 0x2000) and PS/2 keyboard reset (0x64, 0xFE) in cascade after ACPI S5.
- **Añadido**: `EVENT_SHUTDOWN = 12` to event bus constants. `POWEROFF`/`SHUTDOWN`/`EXIT` shell command pushes event → ACPI driver dispatches → HAL poweroff fallback.
- **Añadido**: `-no-reboot` flag to `scripts/qemu-debug.sh` so QEMU exits on guest shutdown.
- **Añadido**: ACPI match arm in boot loader (`register_v3_event_bus_handler` for `EVENT_SHUTDOWN`).
- **Modificado**: `shell/commands/shutdown.rs` — calls `hal::poweroff()` after event dispatch as final fallback (replaced bare HLT loop).
- **Eliminado**: `neodos-kernel/src/drivers/acpi.rs` — legacy RSDP/RSDT/FADT parser (replaced by NEM driver PCI-based detection).
- **Tests**: 237 kernel tests + 4 user-mode binaries.

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
- **Añadido**: `userbin/generate_hello_elf.py` — genera `hello.elf` (ELF64 equivalente a `hello.bin`)
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
- **Actualizado**: `LOAD` command (`shell/commands/load.rs`) — valida el header NDM v1 antes de cargar; soporta módulos con secciones code+data separadas y entry point explícito; fallback a binario raw para `.bin` legacy.
- **Actualizado**: `generate_driver.py` — produce `driver.ndm` con header NDM v1 (64 bytes) + code + data.
- **Inicializado**: `module_abi::init_kernel_service_table()` en `main.rs` (Phase 2.75, tras heap allocator).

### Estabilidad del scheduler

- **Corregido**: `schedule()` ya no selecciona idle (PID 0) cuando hay procesos no-idle listos. El round-robin ahora escanea todos los PIDs > 0 antes de caer en idle.
- **Corregido**: `timer_handler_inner` ya no guarda `current.rsp`. El timer puede dispararse durante ejecución en Ring 0 (syscalls) generando un frame IRETQ de 3 items. Solo `syscall_try_resched` guarda RSP porque INT 0x80 siempre viene de Ring 3 con frame de 5 items.
- **Consecuencia**: `ALLTEST.BIN` pasa completo por primera vez (yield, getpid, open, readfile, close, chdir, getcwd, brk → ALL_TESTS_PASSED).

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
- **Añadido**: `ALLTEST.BIN` — test exhaustivo de syscalls (open, readfile, close, chdir, getcwd, brk, yield, getpid, exit). Incluido en la imagen NeoDOS FS.

### Estabilidad en arranque

- **Corregido**: `allocator::init()` ahora se ejecuta **antes** de `enable_interrupts()`. El timer IRQ0 podía dispararse en la ventana entre STI y la inicialización del heap, causando un panic por allocación fallida (`LockedHeap::empty()`). Síntoma: `ALLOCATION ERROR size: 1, align: 1` en `src/allocator.rs:25`, intermitente según timing de TCG.

### Excepciones del CPU

- **Corregido**: `DOUBLE_FAULT_IST_INDEX` cambiado de 0 (reservado, no usable como IST) a 1, con índice correcto en el array `interrupt_stack_table` (`IST - 1`) y stack dedicado de 20 KB. Sin esto, un doble fault durante el manejo de otra excepción causaba triple fault y reboot.

### Versiones

- Bump kernel a v0.10.4 (Cargo.toml + KERNEL_VERSION_CODE).
