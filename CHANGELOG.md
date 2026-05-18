# Changelog

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
