# NeoDOS — Roadmap de 100 Items

> Versión actual: v0.28.0 (329 kernel tests + 5 user-mode binaries, HPET/APIC Timers, MSI/MSI-X, NVMe, buddy allocator, layout manager, dynamic handles, unlimited handle table, >4 GiB RAM).
> Objetivo: v1.0 — kernel arquitectónicamente sólido, preparado para los próximos 15 años.
> Última revisión: Junio 2026.

---

## COMPLETED (83 items)

### Boot & Core Kernel
1. **x86_64 boot** — entry `_start` en 0x200000, long mode vía UEFI bootloader.
2. **GDT/IDT/PIC** — segmentos Ring 0/3, IDT 256 entradas, PIC remapeado IRQ 32–47.
3. **Identity paging 4 GiB** — páginas enormes 2 MB, identidad hasta 4 GB.
4. **Heap allocator** — 16 MB @ 0x1000000, `linked_list_allocator`, Box/Vec/String.
5. **A3. Kernel slab allocator** — 9 size classes (8B–2KB), O(1) alloc/free via per-slot free lists on 4 KB slab pages. Uses `hal::alloc_page()` for page allocation. Falls through to linked-list allocator for >2 KB or >16-byte alignment. 9 self-tests.
6. **A2. Scheduler prioritario** — 4 niveles de prioridad (HIGH/ABOVE_NORMAL/NORMAL/IDLE), time-slicing dinámico (400/200/100/50 ticks), preemption desde Ring 3, aging cada 100 ticks para evitar starvation. 7 tests. Total: 255 tests.
7. **A5. Global page cache (base)** — `buffer/page_cache.rs`: central 4 KB page cache (512 entries × 4 KB = 2 MB) for filesystem file data I/O and mmap file-backed pages. LRU eviction with dirty write-back. Indexed by `(inode, block_num)` with stored `data_lba` for safe flush. Timer-driven flush via `NEED_PAGE_CACHE_FLUSH`. 8 unit tests. Total: 245 tests.
8. **PS/2 keyboard driver** — IRQ1, ring-buffer lock-free 1024 bytes.
9. **Serial console** — COM1, `serial_print!`/`serial_println!`.
10. **Framebuffer console** — GOP 1280×800, font VGA 8×16, `println!`.
11. **X1. Kernel Object Manager (KOBJ)** — `src/kobj/mod.rs`: unified kernel object system with reference counting and common metadata. 64-slot registry, KObjType enum. 8 unit tests.
12. **X5. Deferred work queues** — `src/work_queue.rs`: bottom-half system for deferred execution outside IRQ context. Two-level architecture (high/low priority). Lock-free SPSC ring buffer (64 slots per level). 6 tests.
13. **X6. Async I/O (IRP system)** — `src/irp/mod.rs`: unified I/O Request Packet model. Global 64-slot pool, `IrpQueue` per-device (32 entries), completion callbacks via work queue, scheduler integration. 11 tests. Total: 284 tests.
14. **V1. Global page cache (advanced)** — `src/buffer/page_cache.rs`: hash map O(1) index for `(inode, block_num)` lookups. LRU doubly-linked list for O(1) access updates. Adaptive readahead. 13 tests.
15. **MSI/MSI-X** — `src/interrupts/msi.rs` (232 lines): MSI and MSI-X interrupt support. Direct mode (kernel port I/O) and Delegated mode (Event Bus to `pci.nem`). 256-entry vector allocator. Dynamic IDT dispatch via `msi_dispatch`. Integrated with PCI and NVMe.
16. **C3. HPET / APIC timers** — `src/timers/hpet.rs`, `src/timers/apic.rs`: HPET 1 KHz periodic mode with legacy replacement to IRQ0. Local APIC timer calibrated against HPET, activated as primary source. APIC mode disables HPET legacy replacement and masks PIC IRQ0. Fallback to PIT 18.2 Hz. `sleep_hint()` uses HPET counter. 320 kernel tests.

### Storage
17. **P1. Default file permissions by context** — `NeoDosFs::default_perms_for_filename()` asigna permisos RWXSD según extensión.
18. **ATA PIO driver** — read/write por puertos 0x1F0/0x3F6.
19. **AHCI driver** — DMA polling, PRDT scatter-gather, ATA + ATAPI.
20. **ATA bus-master DMA** — PCI BAR4, buffers alineados, hasta 8 sectores.
21. **NeoFS** — filesystem propio: inodos 256 B, bloques 4 KB, timestamps, permisos, directorios, 75 tests.
22. **FAT32 read** — lectura de sector absoluto desde ESP.
23. **GPT partition parsing** — detecta partición NeoDOS por UUID.
24. **Unified GPT disk image** — `disk_image.img` (ESP FAT32 + NeoDOS FS).
25. **VFS layer** — `FileSystem` trait, `resolve_path()`, FAT32 + NeoDOS + ISO9660.
26. **ISO9660 read** — driver completo con PVD, extent cache, Joliet.
27. **BlockDevice abstraction** — `BlockDevice` trait, `StorageManager` unifica ATA/AHCI.
28. **NVMe driver** — `src/drivers/nvme.rs` (837 lines): NVMe block driver as kernel built-in. PCI detection (class 0x01, subclass 0x08, prog-if 0x02). Admin Queue + I/O Queues with doorbell registers. NVM Read/Write with PRP scatter-gather. Integrated as highest boot priority: NVMe > BootAhci > BootAta.

### Drivers & Dispositivos
29. **Module ABI v0 (.NDM)** — header 64 bytes, kernel service table, LOAD command.
30. **NEM module** — NeoDOS Driver Format v1, 6 tipos, 14 tests parse.
31. **RTC driver** — CMOS RTC, get_datetime(), usado por DATE/TIME.
32. **ACPI driver** — NEM v3 standalone ACPI poweroff driver. PCI PIIX4/ICH9 LPC bridge detection. PM1a SLP_TYP_S5 shutdown. `EVENT_SHUTDOWN` event bus constant.
33. **HAL ABI v0.3** — 26 primitives `extern "C"` (CPU, port I/O, page mem, IRQ, timers).
34. **Device Model + HAL Binding** — 32-slot registry, handles opacos, 5 boot devices.
35. **Event Bus v2** — Dual priority queues (high 16 + normal 64), subscription filters, dynamic payload, backpressure. 17 tests.
36. **Driver Runtime** — DriverInstance con ID/nombre/estado/contadores, built-in callbacks.
37. **NDREG / LOADNEM / NEMLIST** — driver registry CLI.
38. **Driver Certification Pipeline v1** — estado Loaded→Initialized→Registered→Bound→Active, state machine con transiciones estrictas. 21 tests.
39. **A4. Memory-mapped files** — `MmapRegion` + VMA list per-process, sys_mmap lazy (RAX=19), sys_munmap (RAX=20). 6 tests.
40. **S2. IPC / Pipes** — `src/pipe.rs`: PipeManager con 16 buffers de 4 KB. Per-process `handle_table[16]`. Syscalls: `sys_pipe` (RAX=5), `sys_dup2` (RAX=6). Blocking reads via `ProcessState::Blocked`. 13 tests.
41. **S7. Process exit: full cleanup** — `Scheduler::recycle_terminated(pid)` + `cleanup_terminated_process()` reciclan slot y liberan kernel stack.
42. **S5. FSCK utility** — `src/fs/fsck.rs`: superblock, inode table, directory tree validation + repair. 6 tests.
43. **BDL1. NEM v2 ABI fields** — NEM v2 48-byte header with ABI validation fields, driver category, 16-byte name. 9 tests.
44. **BDL2. Boot Driver Loader System** — auto scanning and loading of .nem drivers from `C:\SYSTEM\DRIVERS\BOOT\` and `C:\SYSTEM\DRIVERS\SYSTEM\`. 8 tests.
45. **BDL3. Driver Instance extended** — `DriverCategory`, ABI fields in `DriverInstance`. `register_ext()`.
46. **BDL4. ABI Validation Policy** — validate_abi() checks ABI compatibility window. Boot/System require v2 format.
47. **BDL5. Rust reference .nem drivers** — PS/2 keyboard, framebuffer, storage reference implementations. 32 tests.
48. **BDL6. NDREG updated** — LIST/SHOW display category and ABI range. RUNTIME snapshot.
49. **BDL7. NEM v3 standalone serial driver** — UART 16550A, IRQ4, EVENT_SERIAL_DATA. Dispatch-by-event-type fix.
50. **BDL8. NEM ps2kbd layout switching** — KEYB US|SP via EVENT_KEYB_LAYOUT (type 9).
51. **W1. ABI negotiation layer** — `AbiVersion` struct, `NegotiationResult`, negotiate() with window overlap check. 10 tests.
52. **W4. Driver dependency resolver** — `DependencyGraph` with topological sort, cycle detection. `__dep_` symbols. 13 tests.
53. **Device Model + TSR removal** — Removed legacy devices/mod.rs and tsr/mod.rs. ~530 lines removed.
54. **X2. Unified handle table** — `src/handle.rs`: unified handle table per-process with HandleEntry types. sys_open returns fd.
55. **PS/2 double-character fix** — Fixed duplicate event bus handler registration for keyboard input.
56. **ACPI NEM poweroff driver** — NEM v3 standalone. EVENT_SHUTDOWN (type 12). POWEROFF/SHUTDOWN command.
57. **PCI NEM driver** — `drivers/pci/` NEM v3 (SYSTEM). Logs devices, config read/write via events 0x1000–0x1003.
58. **A10. PCIe bus enumeration** — Recursive bridge detection, secondary bus scanning. 3 tests.
59. **A6. ATA NEM standalone driver** — `drivers/ata/` NEM v3 (SYSTEM). Primary+secondary channels, NemBlockDevice registration.
60. **A11. AHCI NEM standalone driver** — `drivers/ahci/` NEM v3 (SYSTEM). DMA polling, ATA+ATAPI, PRDT up to 8 entries.
61. **A12. BootAhci kernel stub** — `boot_ahci.rs` early-boot AHCI. Single port, single command slot, 8-sector PRDT.
62. **X3. Capability system** — `src/drivers/caps.rs`: 64-bit capability bitmap per driver (11 flags). Category inheritance. 11 tests.
63. **Demand paging (4 KB)** — frame allocator, split_2mb, heap page fault handler.
64. **sys_brk / sys_mmap** — ajuste program break, asignación zero-filled.
65. **ELF64 loader** — `src/elf.rs`: PT_LOAD segment loading, 7 tests.
66. **User-mode processes** — IRETQ a Ring 3, EXIT_RSP/EXIT_RIP, scheduler add_ring3_process.
67. **Kernel private stacks** — TSS.RSP0 por proceso, actualizado en cada context switch.
68. **Syscall table (INT 0x80)** — 22 syscalls: exit, write, yield, getpid, read, waitpid, open, readfile, writefile, close, chdir, getcwd, brk, mmap, munmap, pipe, dup2, loadlib.
69. **Scheduler blocking** — ProcessState::Blocked, wake_waiters(), idle HLT.
70. **S6. libneodos** — `libneodos/`: standard library para Ring 3 Rust processes. Syscall wrappers via `int 0x80`. IO/FS/Mem modules. `print!`/`println!` macros.
71. **301 kernel self-tests** — 36 suites, comando `test`.
72. **5 user-mode test binaries** — HELLO.BIN, SYSTEST.BIN, FILETEST.BIN, ALLTEST.BIN, TEST.BIN.
73. **Command history** — buffer circular 32, ↑/↓ navegación.
74. **TAB autocomplete** — comandos built-in + archivos del directorio actual.
75. **Keyboard layouts** — KBDUS.klc / KBDSP.klc compilados en build-time.
76. **Shell commands básicos** — HELP, DATE, TIME, VER, DEL, REN, RD, SHUTDOWN, EXIT, LOAD.
77. **S1. Estabilizar syscall ABI** — `SyscallNum` enum, `SyscallError` (16 codes), `err_to_u64()`, `validate_abi()`.
78. **B6b. Shared library system (libneodos DLL)** — libneodos como DLL standalone con `AbiTable`. Slot 0 en `0x1e000000`. Auto-load en PHASE 3.86.
79. **Multi-DLL system** — `sys_loadlib` (RAX=21), `LOADLIB` command. libmath.dll en slot 1 (`0x1e040000`).
80. **X4. Driver Isolation Layer** — `src/drivers/isolation.rs`: 16 MB region (0x30000000–0x31000000), 16 × 1 MB slots. Pointer validation. Sandbox mode. 12 tests.
81. **W2. Hot reload drivers** — `src/drivers/hotreload.rs`: runtime unload/reload. State machine: Active→Unloading→Unloaded→Loaded. EVENT_DRIVER_UNLOAD with timeout. 11 tests. Total: 320 kernel tests.
82. **TEST.EXE — libmath.dll self-test** — `userbin/test/`: LOAD TEST, BASIC ARITHMETIC, EDGE CASES, STRESS TEST (1M iter), DETERMINISM. 320 tests + 5 user binaries.
83. **CPUTEST.BIN — CPU stress test binary** — `userbin/cputest/`: tests CPU arithmetic, loops, and basic instruction throughput. Iterative testing across 100 iterations.
84. **A0.1. Buddy system frame allocator** — `src/memory/buddy.rs`: buddy system de 11 órdenes (4 KB → 4 MB) con free lists O(log n). Bitmap como validación. `alloc_frames(order)`/`free_frames(addr, order)`.
85. **A0.2. Dynamic PHYS_MEM_END** — `MemoryMap { total_phys, highest_page }` detectado del memory map UEFI. Frame allocator soporta >4 GB sin modificar constantes.
86. **A0.3. Dynamic memory layout manager** — `src/memory/layout.rs`: `MemoryLayout { regions: [MemoryRegion; 32] }` con `reserve_region()` dinámico y verificación de solapamientos.
87. **A0.4. Dynamic handle table** — `HandleTable` con `Vec<HandleEntry>` interno. Sin límite fijo. 1024+ handles simultáneos por proceso. Migración transparente.

---

## NEO DOS ARCHITECTURAL ROADMAP v2.0

> Diseñado tras auditoría de arquitectura. Prioriza la deuda técnica estructural sobre funcionalidades nuevas.
> Regla: ninguna feature nueva se considera hasta que su capa arquitectónica base esté completa.

---

### FASE A0 — ARQUITECTURA DE MEMORIA (CRÍTICO: previo a todo)

La capa de memoria actual asume 4 GB, frame allocator O(n), y direcciones fijas soldadas. Esto impide escalar a más RAM y a hardware moderno. Hay que reescribir `memory.rs` y `paging.rs` desde cero.

**A0.1. Frame allocator basado en buddy system**
- `src/memory/buddy.rs`: reemplazar el bitmap O(n) `find_first_zero_bit` con un buddy system de 11 órdenes (4 KB → 4 MB).
- `alloc_frames(order)` / `free_frames(addr, order)` — O(log n), fragmentación externa controlada.
- El bitmap actual se mantiene como validación cruzada en debug mode, no como allocator primario.
- **Criterio de aceptación:** 100.000 alloc/free cycles en < 1 ms. Sin fragmentación superior al 5% tras stress test.

**A0.2. Eliminar PHYS_MEM_END hardcodeado**
- Detectar RAM física real desde el memory map de UEFI (BootInfo → memory map entries).
- `PHYS_MEM_END` se vuelve dinámico: `detect_physical_memory()` en boot, almacenado en `MemoryMap { total_phys: u64, highest_page: u64 }`.
- Identity mapping dinámico: solo mapear la RAM que existe, no 4 GB fijos. Soporte para >4 GB mediante páginas de 2 MB adicionales.
- **Criterio de aceptación:** arranque con 8 GB RAM QEMU sin modificar constantes.

**A0.3. Memory layout manager dinámico**
- Reemplazar constantes `USER_BASE=0x400000`, `USER_HEAP_BASE=0x10000000`, `MMAP_BASE=0x20000000`, `DLL_BASE=0x1e000000`, `DRIVER_ISO_BASE=0x30000000` con regiones gestionadas por `MemoryLayout { regions: [MemoryRegion; 16] }` que asigna rangos según demanda.
- `reserve_region(size, align, flags)` → devuelve base address. Soporta expansión y compactación.
- Verificación de solapamiento en cada reserva. `panic!()` si dos regiones chocan.
- **Archivos:** `src/memory/layout.rs`
- **Criterio de aceptación:** cargar un binario de 6 MB sin chocar contra el heap.

**A0.4. Dynamic handle table**
- `[HandleEntry; 16]` → `Vec<HandleEntry>` o linked list con slab pool.
- `alloc_handle()` / `free_handle(idx)` — O(1) amortizado. Límite únicamente por memoria disponible.
- Migración transparente: ningún syscall cambia su firma, solo la implementación interna.
- **Criterio de aceptación:** 1024 handles abiertos simultáneamente en un solo proceso.

---

### FASE A1 — SMP READINESS (CRÍTICO: sin esto no hay escalabilidad)

El scheduler, slab allocator, y el frame allocator asumen monoprocesador. Cada uno tiene un lock global que serializa todo.

**A1.1. Per-CPU data structures**
- CPU-local storage via segmento GS: `cpu_id()`, `current_process()`, `this_cpu().run_queue`, `this_cpu().slab_cache`.
- Macro `PER_CPU(type, name)` para declarar variables por CPU.
- Inicialización: BSP asigna páginas para cada AP, escribe la estructura, apunta GS.base.
- **Archivos:** `arch/x64/smp.rs`, `src/scheduler/cpu_local.rs`
- **Criterio de aceptación:** 4 CPUs en QEMU, cada una con su idle process.

**A1.2. Per-CPU run queues + load balancing**
- `Scheduler { cpus: [CpuScheduler; MAX_CPU] }`, cada `CpuScheduler` con su `VecDeque<Process>` ready.
- `schedule()` solo mira la cola local. Si está vacía: `try_steal()` (roba de otra CPU).
- Load balancing periódico (cada 100 ticks): migrar procesos de CPU sobrecargada a CPU ociosa.
- IPI `IPI_RESCHEDULE` para notificar a otra CPU que un proceso se ha despertado en su cola.
- **Archivos:** `src/scheduler/mod.rs` (reescritura), `src/scheduler/load_balance.rs`
- **Criterio de aceptación:** 4 procesos en 4 CPUs, cada CPU ejecuta uno, sin migración innecesaria.

**A1.3. Per-CPU slab allocator**
- `SlabAllocator` actual con `spin::Mutex` → 9 caches por CPU, sin lock en el path rápido.
- `alloc_local(size)` → slab local. Si vacío: `refill_from_global()` (lock breve en el pool global).
- `free_local(ptr)` → devuelve al slab local. Si hay exceso: `drain_to_global()`.
- **Archivos:** `src/slab.rs` (reescritura completa)
- **Criterio de aceptación:** 8 CPUs haciendo `alloc`/`free` concurrente, throughput 8× respecto a un solo CPU.

**A1.4. IPI infrastructure**
- `send_ipi(cpu_id, vector)` — escribir ICR del Local APIC.
- Handlers: `IPI_RESCHEDULE`, `IPI_TLB_SHOOTDOWN`, `IPI_CALL_FUNCTION`.
- TLB shootdown: cuando un CPU modifica page table, envía IPI a todos los CPUs que tienen el proceso, espera完成.
- **Archivos:** `arch/x64/ipi.rs`, `arch/x64/paging.rs` (TLB shootdown integration)
- **Criterio de aceptación:** munmap de 1 MB en CPU 0 invalida TLB en CPU 1 correctamente.

---

### FASE A2 — HARDWARE ABSTRACTION & INTERRUPTS (ALTA)

La capa HAL es incompleta. PCI asume port I/O, las interrupciones legacy PIC conviven con MSI como parche.

**A2.1. MMIO ECAM PCI config space**
- Reemplazar `outl(0xCF8, ...)` / `inl(0xCFC, ...)` con acceso MMIO al ECAM (Enhanced Configuration Access Mechanism).
- Detectar ECAM base desde ACPI MCFG table. Fallback a port I/O si no disponible.
- `pci_read_config(bus, dev, func, offset, size)` → abstracción que elige ECAM vs port I/O.
- **Archivos:** `src/drivers/pci.rs` (reescritura), `src/hal/pci.rs`
- **Criterio de aceptación:** mismas lecturas PCI que con port I/O, validado en QEMU.

**A2.2. IOAPIC + MSI-X como modelo primario de interrupción**
- Detectar IOAPIC desde ACPI MADT. Configurar redirección de IRQs legacy al IOAPIC.
- Inicializar MSI-X como mecanismo por defecto para dispositivos que lo soporten.
- PIC legacy (8259A) se deshabilita completamente cuando IOAPIC está presente. El PIC solo se usa como fallback en máquinas sin IOAPIC.
- `irq_alloc_vector(device)` → asigna un vector MSI/MSI-X del pool (48-255). `irq_free_vector(vec)`.
- **Archivos:** `src/interrupts/ioapic.rs`, `src/interrupts/msi.rs` (extensión MSI-X), `src/hal/irq.rs`
- **Criterio de aceptación:** NVMe con MSI-X, 2 I/O queues, interrupciones por completion, sin PIC involvement.

**A2.3. HAL v0.4 — arquitectura completa**
- Las 26 primitives actuales están bien pero mezclan HAL puro con helpers de conveniencia.
- Separar: `hal::raw` (asm volatile puro, sin Rust wrappers) vs `hal::safe` (wrappers con type safety).
- Añadir: `hal::read_msr()/write_msr()`, `hal::invpcid()`, `hal::halt_until_interrupt()`.
- La HAL debe ser el ÚNICO código que toca asm. Cero inline asm fuera de `hal/`.
- **Criterio de aceptación:** grep "asm!" fuera de hal/ retorna 0.

---

### FASE A3 — FAULT TOLERANCE & RECOVERY (ALTA)

El kernel actual no sobrevive a ningún fallo. Un bug en cualquier driver → `poweroff()`.

**A3.1. Crash dump framework**
- `src/crash/mod.rs`: panic handler mejorado que captura registros CPU, stack trace (32 frames), scheduler state, y 128 últimos trace points.
- Escritura a partición de crash dump en disco (o área reservada en NeoDOS FS).
- `scripts/crashdump_analyzer.py`: parsea el dump y genera reporte legible.
- Triple fault handler: vector 0 captura estado antes de reset.
- **Criterio de aceptación:** `panic!("test")` → dump legible en serial + disco.

**A3.2. Kernel debugger**
- `src/debugger/mod.rs`: comandos `DEBUG MEM <addr> <len>`, `DEBUG PROC <pid>`, `DEBUG STACK`, `DEBUG SCHED`.
- Software breakpoints: `DEBUG BP <addr>` → INT3. Handler guarda estado, devuelve control al shell.
- Watchpoints: marcar página not-present, page fault handler verifica dirección vigilada.
- **Criterio de aceptación:** breakpoint en `sys_write`, inspección de registros, continue.

**A3.3. Watchdog subsystem**
- HPET watchdog timer (5 second timeout). `watchdog_pet()` desde timer tick handler.
- Si el tick no se ejecuta durante 5s (sistema colgado), watchdog genera NMI o reset.
- NMI handler captura estado antes de reset (registros, RIP, stack).
- **Archivos:** `src/watchdog/mod.rs`, `src/hal/watchdog.rs`

---

### FASE A4 — KERNEL/USER SEPARATION (ALTA)

El shell en Ring 0 es la vulnerabilidad más grave del sistema.

**A4.1. Shell como proceso Ring 3**
- `DosShell` se convierte en un binario userland (`userbin/shell/`) cargado como PID 1 por el kernel.
- Las funciones de shell que necesitan acceso interno (drivers, memoria, testing) se exponen como syscalls.
- El kernel arranca directo a Ring 3 tras PHASE 4. El shell hereda stdin/stdout del kernel.
- Las syscalls de administración (test, ndreg, kobj) se añaden como syscalls privilegiadas (requieren CAP_ADMIN).
- **Archivos:** `userbin/shell/`, nuevas syscalls administrativas en `src/syscall.rs`
- **Criterio de aceptación:** buffer overflow en el parser de comandos → kill del proceso shell, no kernel panic.

**A4.2. Syscall dispatch table (generada)**
- Reemplazar `match rax { 0 => ..., 1 => ..., ... }` con una tabla de punteros a función: `[Option<fn(Registers) -> u64>; 256]`.
- Tabla generada por macro `syscall!(0 => sys_exit, 1 => sys_write, ...)`.
- Verificación en boot: todas las entradas de 0..MAX_SYSCALL son `Some`.
- Permite auditoría centralizada: cada syscall pasa por `check_syscall_permissions()` antes de ejecutar.
- **Criterio de aceptación:** 22 syscalls funcionando idéntico, nueva syscall se añade con una línea.

**A4.3. ELF loader con validación de rangos**
- `load_elf_segment()` verifica `p_vaddr + p_memsz <= USER_WINDOW_END && p_vaddr >= USER_BASE `.
- Verifica que `p_vaddr + p_memsz` no se solapa con kernel, heap, mmap, DLL, o driver isolation regions.
- Rechaza binarios con segmentos que escriben fuera de la ventana de usuario.
- **Archivos:** `src/elf.rs`
- **Criterio de aceptación:** ELF con `p_vaddr=0` → `ERR_INVALID_ELF`, no triple fault.

**A4.4. Input subsystem rediseñado**
- Reemplazar ring-buffer único de 1024 bytes con cola por terminal virtual.
- `InputManager { terminals: [InputQueue; MAX_VT] }`, cada cola con 4 KB.
- Flujo: IRQ teclado → Event Bus → input manager → cola del VT activo → proceso leyendo stdin.
- Soporte para múltiples fuentes: PS/2, USB HID, serial.
- **Archivos:** `src/input.rs` (reescritura)
- **Criterio de aceptación:** 3 VTs, cada una con su buffer de teclado independiente.

---

### FASE A5 — UNIFICACIÓN FS & STORAGE (MEDIA)

Dos implementaciones de FS que no comparten código. Drivers de bloque con lógica duplicada.

**A5.1. Unified block I/O layer**
- `BlockDevice` trait actual → `IoStack { device, cache_slot, crypto, partition_offset }`.
- FAT32 y NeoFS comparten el mismo cache de bloques y la misma lógica de lectura/escritura de sectores.
- `vfs_read_sectors(device, lba, count, buf)` → unified path with cache + crypto + partition offset.
- **Archivos:** `src/vfs/io.rs`, refactor de `src/fs/neofs/` y `src/fs/fat32/`
- **Criterio de aceptación:** FAT32 y NeoFS leen a través del mismo `read_sectors()`.

**A5.2. VirtIO block driver**
- PCI detection (vendor 0x1AF4, device 0x1001). VirtIO transport via BAR0 MMIO.
- Virtqueue ring buffer. `virtio_blk_req` protocolo: Read/Write/Flush/GetId.
- Registra como `NemBlockDevice`. Prioridad: VirtIO > NVMe > BootAhci > BootAta.
- **Criterio de aceptación:** arranque NeoDOS desde disco VirtIO en QEMU.

**A5.3. AHCI NCQ (Native Command Queuing)**
- Expandir Command List AHCI a 32 slots. Pool de slots con asignación dinámica.
- NCQ commands: READ/WRITE FPDMA QUEUED (0x60/0x61). Tags en sector count.
- Out-of-order completion: asociar completion con IRP original mediante tag.
- **Archivos:** `neodos-kernel/src/drivers/boot_ahci.rs` (extensión NCQ)
- **Criterio de aceptación:** 32 IRPs concurrentes a disco AHCI, completan en orden arbitrario.

---

### FASE B — FEATURES (solo cuando A0–A5 estén completas)

Estos items son funcionalidades de usuario que dependen de la infraestructura arquitectónica. No se implementan hasta que A0–A5 estén verificadas.

**B1. Tracing & Observability**
1. Y1. Kernel tracing infrastructure (`src/trace/mod.rs`)
2. Y2. NeoTrace system (`src/shell/commands/neotrace.rs`)
3. Y5. Kernel debugger (fase A3.2 ya cubre esto)

**B2. Service Layer**
4. Z1. NeoInit service manager (PID 1 userland)
5. Z6. System configuration registry (`C:\SYSTEM\CONFIG.REG`)
6. Z2. Unified resource namespace (URN)
7. Z3. Virtual FS objects (`K:\` drive)

**B3. Networking**
8. D9. Socket API (syscalls RAX 30-39)
9. E3. TCP/IP stack (Ethernet, ARP, IPv4, ICMP, UDP, TCP)
10. D8. DHCP client
11. D7. NTP client

**B4. Userland Usable System**
12. S8. PATH resolution
13. S9. Shell pipes (`|`)
14. S3. Shell redirection (`>`, `<`, `>>`)
15. B2. ANSI terminal
16. B1. Virtual terminals
17. B6. NeoEdit text editor
18. B6b-v2. Shared library per-process binding
19. B7. NeoTOP
20. B11. NeoShell scripting (`.BAT`)
21. B12. Compositor 2D

**B5. Security**
22. U1. Module signature validation
23. U3. Driver permission enforcement
24. U4. Secure boot chain

**B6. Performance & SMP**
25. V2. Zero-copy pipes
26. V3. Copy-on-write fork
27. X10. Per-CPU allocators (cubierto en A1.3)
28. X8. SMP-safe kernel (cubierto en A1.1–A1.4)

**B7. Experimental**
29. E4. Full GUI system
30. E5. Advanced secure boot (TPM)
31. E6. Package manager
32. T4. Time-travel debugging
33. T5. Live kernel patching
34. T2. Distributed NeoDOS nodes

---

### RESUMEN ARQUITECTÓNICO (Dave Cutler Checklist)

| # | Problema | Fase | Estado | Riesgo si no se hace |
|---|----------|------|--------|---------------------|
| 1 | Frame allocator O(n), 4 GB max | A0.1–A0.2 | Completado | Buddy system + memory map dinámico |
| 2 | Direcciones fijas, solapamiento | A0.3 | Completado | MemoryLayout con verificación |
| 3 | Handle table fijo (16) | A0.4 | Completado | Vec dinámico sin límite |
| 4 | Scheduler monoprocesador | A1.1–A1.2 | Pendiente | Reescribir scheduler entero para SMP |
| 5 | Slab allocator lock global | A1.3 | Pendiente | Throughput no escala con CPUs |
| 6 | Sin IPI / TLB shootdown | A1.4 | Pendiente | Data corruption en SMP |
| 7 | PCI port I/O asume x86 | A2.1 | Pendiente | No portar a ARM64 ni RISC-V |
| 8 | PIC legacy como default | A2.2 | Pendiente | Límite 15 IRQs, sin MSI-X real |
| 9 | Sin crash dump ni recovery | A3.1–A3.3 | Pendiente | Bugs imposibles de diagnosticar |
| 10 | Shell en Ring 0 | A4.1 | Pendiente | Cualquier bug del shell = kernel panic |
| 11 | Syscall dispatch manual | A4.2 | Pendiente | Cada syscall nueva = más bugs |
| 12 | ELF loader sin validación | A4.3 | Pendiente | Triple fault con binarios maliciosos |
| 13 | Input sin multiplexión | A4.4 | Pendiente | No escalar a múltiples terminales |
| 14 | FAT32 + NeoFS duplicados | A5.1 | Pendiente | Doble mantenimiento, doble bugs |
| 15 | Stack frame unwinding inexistente | A3.2 | Pendiente | Sin backtrace, debugging imposible |
