# Changelog

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
