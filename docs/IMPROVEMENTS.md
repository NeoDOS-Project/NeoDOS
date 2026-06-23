# NeoDOS — Roadmap v4.0 (Visión Arquitectónica)

> This file documents pending improvements and roadmap items for NeoDOS. This document serves as the central roadmap for NeoDOS, capturing all pending improvements, milestones, and architectural tasks. Each entry specifies an ID, related source files, prerequisites, acceptance criteria, and associated tests, providing clear guidance and traceability for developers.

> Versión actual: v0.44.1 (520 kernel tests + 27 user-mode binaries).
> Objetivo: v1.0 — executive NT-like arquitectónicamente sólido.
> **NUEVA GUÍA:** Leer [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) antes de planificar cualquier cambio.
> Fuente de verdad arquitectónica: [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md)
> Última revisión: Junio 2026.

**Progreso:** 157 / ~170 items completados (+16 planificados: v0.45+ milestones). Próximo milestone: **v0.45** (Registry persistente, Device Tree).

---

## Reglas de ejecución

1. Una fase no empieza hasta que sus prerequisitos estén marcados **[COMPLETED]**.
2. Cada item pendiente incluye: ID, equivalente NT, archivos, prereqs, criterio de aceptación, tests.
3. Al completar un item: moverlo a COMPLETED, actualizar `CHANGELOG.md`, `AGENTS.md` y `ARCHITECTURE_SOURCE_OF_TRUTH.md` si cambia un contrato.
4. Validar antes de cerrar: `cargo build` en `neodos-kernel/` + `python3 scripts/auto_test.py` + `scripts/check_deps.py`.

### Checklist por item completado

- [ ] Código implementado
- [ ] Tests en `testing.rs` (mínimo 1 por invariante)
- [ ] `auto_test.py` pasa
- [ ] `check_deps.py` pasa
- [ ] `CHANGELOG.md` actualizado
- [ ] `AGENTS.md` / `ARCHITECTURE_SOURCE_OF_TRUTH.md` si cambia contrato

---

## COMPLETED (154 items)

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
17. **ASLR v1 (v0.44)** — PIE user binaries (ET_DYN) loaded at random slot base addresses via RDRAND/TSC entropy. `src/elf.rs`: `load_offset` parameter, RELA relocation support (R_X86_64_RELATIVE). `src/arch/x64/paging.rs`: ASLR-aware slot allocator. All 30+ user binaries compiled as PIE (`. = 0`, `-pie`, `relocation-model=pie`). 520 kernel tests (+5 PIE-specific ELF tests).

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
40. **S2. IPC / Pipes** — `src/pipe.rs`: PipeManager con 16 buffers de 4 KB. Per-process handle table dinámico. Syscalls: `sys_pipe` (RAX=5), `sys_dup2` (RAX=6). Blocking reads via `ProcessState::Blocked`. 13 tests.
41. **S7. Process exit: full cleanup** — `Scheduler::recycle_terminated(pid)` + `cleanup_terminated_process()` reciclan slot y liberan kernel stack.
42. **S5. FSCK utility** — `src/fs/fsck.rs`: superblock, inode table, directory tree validation + repair. 6 tests.
43. **BDL1. NEM v2 ABI fields** — NEM v2 48-byte header with ABI validation fields, driver category, 16-byte name. 9 tests.
44. **BDL2. Boot Driver Loader System** — auto scanning and loading of .nem drivers from `C:\System\Drivers\`. 8 tests.
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
72. **5 user-mode test binaries** — HELLO.NXE, SYSTEST.NXE, FILETEST.NXE, ALLTEST.NXE, TEST.NXE.
73. **Command history** — buffer circular 32, ↑/↓ navegación.
74. **TAB autocomplete** — comandos built-in + archivos del directorio actual.
75. **Keyboard layouts** — KBDUS.klc / KBDSP.klc compilados en build-time.
76. **Shell commands básicos** — HELP, DATE, TIME, VER, DEL, REN, RD, SHUTDOWN, EXIT, LOAD.
77. **S1. Estabilizar syscall ABI** — `SyscallNum` enum, `SyscallError` (16 codes), `err_to_u64()`, `validate_abi()`.
78. **B6b. Shared library system (libneodos NXL)** — libneodos como NXL standalone con `AbiTable`. Slot 0 en `0x1e000000`. Auto-load en PHASE 3.86.
79. **Multi-NXL system** — `sys_loadlib` (RAX=21), `LOADLIB` command. libmath.nxl en slot 1 (`0x1e040000`).
80. **X4. Driver Isolation Layer** — `src/drivers/isolation.rs`: 16 MB region (0x30000000–0x31000000), 16 × 1 MB slots. Pointer validation. Sandbox mode. 12 tests.
81. **W2. Hot reload drivers** — `src/drivers/hotreload.rs`: runtime unload/reload. State machine: Active→Unloading→Unloaded→Loaded. EVENT_DRIVER_UNLOAD with timeout. 11 tests. Total: 320 kernel tests.
82. **TEST.EXE — libmath.nxl self-test** — `userbin/test/`: LOAD TEST, BASIC ARITHMETIC, EDGE CASES, STRESS TEST (1M iter), DETERMINISM. 320 tests + 5 user binaries.
83. **CPUTEST.NXE — CPU stress test binary** — `userbin/cputest/`: tests CPU arithmetic, loops, and basic instruction throughput. Iterative testing across 100 iterations.
84. **A0.1. Buddy system frame allocator** — `src/memory/buddy.rs`: buddy system de 11 órdenes (4 KB → 4 MB) con free lists O(log n). Bitmap como validación. `alloc_frames(order)`/`free_frames(addr, order)`.
85. **A0.2. Dynamic PHYS_MEM_END** — `MemoryMap { total_phys, highest_page }` detectado del memory map UEFI. Frame allocator soporta >4 GB sin modificar constantes.
86. **A0.3. Dynamic memory layout manager** — `src/memory/layout.rs`: `MemoryLayout { regions: [MemoryRegion; 32] }` con `reserve_region()` dinámico y verificación de solapamientos.
87. **A0.4. Dynamic handle table** — `HandleTable` con `Vec<HandleEntry>` interno. Sin límite fijo. 1024+ handles simultáneos por proceso. Migración transparente.
88. **Architecture Source of Truth** — `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`: Definición estricta de invariantes y contratos del sistema (Dave Cutler style) para evitar regresiones de diseño.
89. **MCP Server — Kernel Introspection & VFS Analysis** — `scripts/mcp_server/`: MCP protocol server (JSON-RPC 2.0) with 18 tools for AI-assisted kernel debugging, VFS inspection, and architectural validation. Parser offline de NeoDOS FS, NEM v3, ELF64. 3 resources, 3 prompts. `scripts/mcp-server.sh` launcher.
90. **A4.2. Syscall dispatch table (SSDT)** — `src/syscall/mod.rs`: tabla SSDT `[Option<fn(Registers) -> u64>; 256]` con `lazy_static!` reemplaza match monolítico. Tabla paralela `[SyscallPermission; 256]` con admin/ring/caps. Admin syscall RAX=50 (`handler_ndreg`). `validate_abi()` itera SSDT para verificar integridad. Dispatcher table-based con permission check antes de cada llamada. 5 tests: `syscall_table_sparse_dispatch`, `syscall_permission_admin_check`, `syscall_enosys_unknown`, `syscall_table_validation_boot`, `syscall_add_new_easy`.
91. **A1.1. Per-CPU data structures (KPRCB)** — `arch/x64/cpu_local.rs`: Kprcb struct (4 KB page per CPU) con cpu_id, apic_id, current_thread, CpuRunQueue (64-entry ring buffer), PerCpuSlabCache[9], interrupt/context_switch/timer_tick counters, exit trampoline via GS. GS-segment accessors. 20 compile-time offset_of! assertions. 5 tests.
92. **A1.1b. MSR access module** — `arch/x64/msr.rs`: rdmsr/wrmsr, typed accessors (read_gs_base, write_gs_base, is_bsp, rdtsc, rdtscp).
93. **A1.1c. SMP boot (INIT-SIPI-SIPI)** — `arch/x64/smp.rs`: AP trampoline (16→32→64-bit), copy to 0x800000, INIT-SIPI-SIPI sequence, per-CPU GS base, AP entry. 3 tests.
94. **A1.2. Per-CPU run queues + work stealing** — CpuRunQueue in KPRCB. schedule() tries local queue → work stealing → global fallback. Threads enqueued on creation/wake/timer. IPI_RESCHEDULE (vector 0xF0). 8 total new tests.
95. **Bug fix: handler_exit deadlock** — Double-locking SCHEDULER mutex when calling wake_thread_joiner(). Inlined wake call.
96. **Bug fix: request_exit_to_kernel()** — Read value as pointer instead of using gs_write_u8.
97. **Bug fix: KPRCB offset constants** — 13 offsets 2 bytes too low due to CpuRunQueue alignment. Fixed with compile-time assertions.
98. **A1.3. Per-CPU slab allocator** — `src/slab.rs` rewritten with per-CPU fast path: 32-object hot caches in KPRCB via GS-segment, O(1) alloc/free without locks. `refill_from_global()` / `drain_to_global()` with global Mutex for cross-CPU replenishment. Per-CPU slab accessor functions in `cpu_local.rs` (gs_read_u16/gs_write_u16, this_cpu_slab_alloc_local/free_local). 5 tests: `per_cpu_slab_alloc_free_concurrent`, `per_cpu_refill_drain_batching`, `slab_scaling_8cpu`, `slab_under_irql_dispatch`, `slab_stress_100k`.
99. **A1.4. IPI infrastructure + TLB shootdown** — `arch/x64/ipi.rs`: unified IPI module with `send_ipi()`, `send_ipi_mask()`, `send_ipi_all()`. IPI_TLB_SHOOTDOWN (vector 0xF1) with synchronous ACK protocol and shared `TlbShootdownPayload`. IPI_CALL_FUNCTION (vector 0xF2) with `CallFunctionCb` dispatch. TLB shootdown integrated into `paging.rs` (heap_free_page, heap_free_range, mmap_free_page, mmap_free_range, set_page_user_accessible). `ack_irq()` fixed to send APIC EOI for all vectors >= 32 (was only vector 32). Scheduler sends IPI_RESCHEDULE on cross-CPU thread enqueue. 5 tests: `ipi_constants`, `ipi_tlb_shootdown_struct`, `ipi_call_function_struct`, `ipi_tlb_shootdown_local_only`, `ipi_call_function_no_targets`.
100. **A3.1. Crash dump framework** — `src/crash/mod.rs`: 16 KB CrashDumpHeader, stack walk, GPR snapshot, serial output. `CRASH`/`CRASH DUMP` commands. 5 tests.
101. **B8. cpuinfo.nxe — user-mode CPU info binary** — `userbin/cpuinfo/`: uses `libcpu-nxl` NXL via sys_loadlib. Displays vendor, brand, topology, timers, features. sys_getcpuinfo (RAX=24) kernel + user wrappers.
102. **A4.7. neoshell (Ring 3 shell)** — `userbin/neoshell/`: full-featured Ring 3 interactive shell. Built-in commands: HELP, CLS, ECHO, VER, CWD, DIR, SET, POWEROFF, EXIT. `CD` is a separate Ring 3 tool (`cd.nxe`) that changes the parent shell cwd via `sys_chdir_parent`. DIR uses sys_open+sys_readdir. External commands: PATH scan for `.NXE`, sys_spawn + sys_waitpid. TAB completion (built-ins). History (32 entries). Env vars with SET. CWD prompt. Drive change.
103. **NT5.1. Object directory tree** — `src/kobj/namespace.rs`: transforma el registry plano KOBJ en un árbol jerárquico de objetos con `\` como raíz y directorios estándar (`\Device`, `\DosDevices`, `\Global`, `\Driver`, `\FileSystem`, `\Ob`). Lookup de paths tipo NT con `ob_lookup_path()`, nombres de 24 bytes y `BTreeMap` por nodo. 6 tests.
104. **NT5.2. Symbolic links** — `src/kobj/symlink.rs`: objetos simbólicos que apuntan a otros objetos o paths. Resuelve `\DosDevices\C:` y similares con límite de 10 saltos para evitar loops. 5 tests.
105. **NT5.3. Path resolution API** — `src/kobj/lookup.rs`: API unificada `ob_lookup_by_path()` para paths absolutos y relativos, seguimiento de symlinks, normalización y errores `OB_*`. 5 tests.
106. **NT5.4. VFS mount points integration** — `src/vfs/mount.rs`: integración VFS + namespace de objetos, mount points sobre `\Device`, symlink `\DosDevices\C:` y resolución de paths NT-style hacia NeoFS/FAT32/ISO9660. 5 tests.
107. **B8.1. DIR.NXE** — `userbin/coredir/`: lista directorio con `sys_open` (dir) + `sys_readdir`. Columnas, `/W` (wide), `/P` (pausa).
108. **B8.3. ECHO.NXE** — `userbin/echo/`: imprime argumentos a stdout via `sys_write`.
109. **B8.4. VER.NXE** — `userbin/ver/`: muestra versión del sistema via `sys_get_version` (RAX=43).
110. **B8.6. HELP.NXE** — `userbin/corehelp/`: lista .NXE disponibles escaneando `C:\Programs\*.NXE` con `sys_readdir`.
111. **B8.12. DATETIME.NXE** — `userbin/datetime/`: muestra fecha/hora RTC via `sys_get_datetime` (RAX=44). Flags `/D`, `/T`.
112. **B8.13. MEM.NXE** — `userbin/mem/`: muestra uso de memoria via `sys_get_meminfo` (RAX=45). Migrado de Ring 0.
113. **B8.14. TREE.NXE** — `userbin/tree/`: muestra árbol de directorios con `├──`/`└──`, recursivo hasta 6 niveles. Directorios primero, orden alfabético case-insensitive. Path opcional (default: CWD).
114. **NeoDOS LSP** — `neodos-lsp/`: Language Server Protocol implementation for NeoDOS development. Full LSP features (completion, goto-def, hover, references, rename, documentSymbol, diagnostics). Background indexing with rayon-parallel parsing. NeoDOS-aware: detects syscall handlers, capability constants, shell command entries, driver states. `dashmap`-backed database. 8 MCP tools for AI-level code analysis. `opencode.json` integration. 34 unit tests.
115. **B8.2. TYPE.NXE** — `userbin/coretype/`: muestra contenido de archivo con `sys_open` + `sys_readfile`. Búfer 512 B.
116. **B8.5. CLS.NXE** — `userbin/corecls/`: limpia pantalla (ANSI escape `\x1b[2J\x1b[H`).
117. **B8.7. COPY.NXE** — `userbin/corecopy/`: copia archivo con `sys_open` + `sys_writefile`. Búfer 4 KB.
118. **B8.8. DEL.NXE** — `userbin/coredel/`: elimina archivo via `sys_unlink`.
119. **B8.9. REN.NXE** — `userbin/coreren/`: renombra via `sys_rename`.
120. **B8.10. MD.NXE** — `userbin/coremd/`: crea directorio via `sys_mkdir`.
121. **B8.11. RD.NXE** — `userbin/corerd/`: elimina directorio vacío via `sys_rmdir`.
122. **B4.1. PATH resolution** — `userbin/neoshell/`: búsqueda de `.NXE` en PATH. neoshell itera directorios PATH y ejecuta via `sys_spawn`. Prioridad `.NXE` > `.COM` > `.EXE`.
123. **B8.15. Build + integración** — `scripts/create_neodos_image.py` compila todos los coretools y neoshell, los copia a `C:\Programs\`.
124. **NT6.1. SID + Access Token** — `src/security/token.rs`, `src/security/sid.rs`: Define la identidad de seguridad de cada proceso y thread mediante SID y token. Token admin por defecto para boot, heredado en spawn. Tests: `token_inherit`, `sid_format`, `token_admin_boot_default`.
125. **NT6.2. ACL/ACE on objects** — `src/security/acl.rs`: Añade descriptors de seguridad a cada objeto del namespace. Define `Ace` (allow/deny, access_mask, SID), `Acl`, `SecurityDescriptor`. Tests: `acl_deny_access`, `acl_allow_access`, `acl_inherit_parent`.
126. **NT6.3. Access check on open** — `src/security/access.rs`: `se_access_check()` compara token SID contra DACL del SD con admin bypass. Tests: `se_access_check_deny`, `se_access_check_allow`, `se_access_check_admin_override`.
127. **NT6.4. Admin vs user token** — `src/security/token.rs`: Separa tokens de sistema y usuario. Syscall 50 requiere admin. 12 tests de seguridad integrados.
128. **NT5.5 Z2. Unified resource namespace (URN) — OB-025 rewrite** — `src/urn/mod.rs`: URN rewrite completo como frontend de Ob. Todos los schemes (`file`, `device`, `registry`, `kobj`) se resuelven mediante `ob_open_path()` en el namespace Ob. `UrnHandle` simplificado a wrapper sobre kernel fd (handle table index). `urn_read`/`urn_write` operan via handle table con VFS. Tests: 15 (8 parse + 2 open error + 1 roundtrip + 3 OB-025 scheme mapping + 1 OB-018 Ob integration).
129. **NT5.6 Z3. Virtual FS objects (K:\ drive)** — `src/vfs/kdrive.rs`: Drive virtual K:\ que expone objetos NT5 internos como archivos de solo lectura via VFS. Directorios: Processes, Drivers, Memory, Interrupts. 12 tests.
130. **A2.1. MMIO ECAM PCI config space** — `src/hal/pci.rs`, `src/drivers/pci.rs`: ECAM-based PCI config space access via MMIO from ACPI MCFG table. Auto-selects ECAM or legacy PIO fallback. Tests: 5.
131. **A2.2. IOAPIC + MSI-X como modelo primario** — `src/interrupts/ioapic.rs`, `src/interrupts/msi.rs`: I/O APIC detected from MADT, replaces legacy PIC. MSI-X per-entry table programming. IOAPIC init at PHASE 2.91. Tests: 5.
132. **B4.4 B2. ANSI terminal** — `userbin/neoshell/`, framebuffer driver: Emulador de terminal ANSI básico en framebuffer. Interpreta secuencias de escape: color, clear screen, cursor position. Tests: `ansi_color_foreground`, `ansi_cursor_position`, `ansi_clear_screen`.
133. **v0.40 — Buddy bitmap dinámico, User window 32MB, Static buffers→heap** — `src/memory/buddy.rs`: bitmap dinámico (>4GB RAM) en vez de `[u64; 16384]`. `src/arch/x64/paging.rs`, `src/scheduler/address_space.rs`, `src/memory/layout.rs`: user window 4MB→32MB (0x400000..0x2400000), kernel heap reubicado (0x2400000). `kernel.ld`: kernel movido a 0x4000000 (64MB). `src/drivers/boot_ahci.rs`: búferes AHCI heap-allocados. `src/main.rs`: CMD_BUF/BIN_BUF heap-allocados. 479 tests.
134. **v0.41 — Slab&lt;T&gt; contenedor, Scheduler Vec, Pipe buffers dinámicos, ObObjectTable** — `src/slab_container.rs`: Generic Slab&lt;T&gt; contenedor con insert/get/remove. `src/scheduler/mod.rs`: eprocesses/kthreads migrados a Vec dinámico. `src/pipe.rs`: Pipe buffers Box&lt;[u8; 4096]&gt; heap-allocados, MAX_PIPES=16. `src/object/mod.rs`: ObObjectTable base, init_object_manager en boot Phase 2.759, 10 tests. HandleEntry con object_id field. KOBJ delegado en ObObjectTable. 487 tests.
135. **v0.42 — Unified Wait Engine (KWait), ABI Freeze, HandleEntry full Ob integration** — `src/kwait/mod.rs`: KWait engine con WaitReason (7 variantes: PipeRead, IrpComplete, ThreadJoin, ChildExit, Event, Timer, Alertable), `kwait_block()`/`kwait_wake()` unified API, 10 tests. `src/abi_freeze.rs`: Verify frozen event types 0–15, capability flags bits 0–11, IOAPIC API, 4 tests. `src/handle.rs`: Todos los constructores crean objetos Ob via `ob_create_object()`, nuevo método `close()` llama `ob_close_object()`, helper methods `is_open()`/`is_pipe()`/etc. Marcas FROZEN v0.42 en eventbus, caps, ioapic. ABI freeze validation en boot Phase 3.9. 509 tests.
136. **v0.43 — SeAccessCheck NT-compatible (ACE order NT-correct)** — `src/security/access.rs`: NT-correct `check_dacl()` evalua primero todos los Deny ACEs, luego todos los Allow ACEs (two-pass). `src/security/acl.rs`: `insert_ace_canonical()` mantiene orden canónico (deny first, allow second). 3 tests nuevos: `se_deny_first_allow_after_deny`, `se_deny_first_mixed_aces`, `se_insert_ace_canonical`. 509 tests.
137. **v0.43 — sys_poll() (RAX=59)** — `src/syscall/mod.rs`: Nuevo handler `handler_poll()` con PollFd struct (fd, events, revents). POLLIN/POLLOUT/POLLHUP/POLLERR flags. Soporta stdin, stdout/stderr, pipe read/write, files, dirs. `src/pipe.rs`: 3 nuevas funciones públicas `pipe_peek_read_ready()`, `pipe_peek_write_closed()`, `pipe_peek_read_closed()` para poll sin bloqueo. SSDT slot 59, permission user-level.
138. **v0.43 — Pipe/IRP protocol freeze** — `src/pipe.rs`: Doc comment con FROZEN ABI v0.43, protocol invariants documentados (read EOF semantics, EPIPE, inc_ref/dec_ref balance, blocking magic 0xFFFF_0000). `src/irp/mod.rs`: Doc comment con FROZEN ABI v0.43, protocol invariants (IRP ID global, pool index id%64, irp_get_params lock discipline, chain semantics).
139. **A3.3. Watchdog subsystem** — `src/watchdog/mod.rs`: Software watchdog basado en HPET. `watchdog_pet()` desde timer tick (1 KHz). 5s timeout → crash dump con CAUSE_WATCHDOG, EVENT_NMI_WATCHDOG, reset. Re-entry guard MAX_NMI_RECOVERIES=3. 5 tests.
137. **A3.4. SEH + exception dispatcher** — `src/exception/mod.rs`: Mecanismo unificado `exception_dispatch()` para Ring 0 (crash dump+panic) vs Ring 3 (TEB exception handler chain). TEB en 0x7000 con `Teb { teb_self, pid, tid, exception_list }`. sys_set_exception_handler (RAX=29). 5 tests.
138. **B4.2. Shell pipes (`|`)** — `userbin/neoshell/`: pipelines de hasta 16 comandos con pipes nativos vía `sys_pipe` + `sys_dup2` + `sys_spawn`. PipeManager con 16 buffers × 4 KB, blocking reads.
139. **B9.1. HELP → corehelp.nxe** — Ring 0 HELP reducido a stub, `corehelp.nxe` escanea `C:\Programs\*.NXE` buscando marcadores `::HELP::`.
140. **B9.2. SET → neoshell built-in** — Variables de entorno en Ring 3, Ring 0 SET eliminado.
141. **B9.3. EXIT → neoshell built-in** — POWEROFF/EXIT en Ring 3 vía `sys_poweroff` (RAX=42), Ring 0 EXIT eliminado.
142. **B9.4. PS → ps.nxe** — Lista procesos vía `sys_ob_enum("\Ob\Process")` + `ObQueryInfo(Process)` con datos reales (PID, PPID, prioridad, thread_count, estado). Migrado de `sys_kobj_enum` a Ob.
143. **B9.5. KILL → kill.nxe** — Termina proceso por PID vía `ObOpen` + `ObSetInfo(fd, ProcessTerminate)`. Migrado de `sys_kill_process` a Ob.
144. **B9.6. PRI → pri.nxe** — Cambia prioridad scheduling vía `ObOpen` + `ObSetInfo(fd, ProcessPriority)`. Migrado de `sys_set_priority` a Ob.
145. **B9.8. DRIVES → drives.nxe** — Lista unidades montadas vía `sys_get_drives` (RAX=33). Letra, tipo, etiqueta, tamaño.
146. **B9.10. KEYB → keyb.nxe** — Cambia layout teclado vía `sys_set_keyboard_layout` (RAX=49). US/SP.
147. **B9.13. CALL → neoshell built-in** — Ejecuta `.BAT` batch desde Ring 3, replica `commands/call.rs`.
148. **v0.44.1 — libneodos Ob API** — 5 wrappers Ob en `libneodos/src/syscall.rs` con macros asm seguras (temp register copy). `ObBasicInfo`, `ObEnumEntry`, `ObProcessInfo` structs + `ob_access` constants. AbiTable v5 en `libneodos-nxl` y `libneodos/src/export.rs`.
149. **v0.44.1 — ob_open_path auto-create dirs** — `src/object/mod.rs`: `ob_open_path()` crea dir objects on-the-fly para paths namespace que son directorios sin object entry.
150. **v0.44.1 — ob_is_directory()** — `src/kobj/namespace.rs`: pública para detectar directorios namespace sin entry.
151. **v0.44.1 — ProcessTerminate (ObSetInfo class 4)** — `src/syscall/mod.rs`: termina proceso via `ObSetInfo(fd, 4)`. `handler_kill_process` migrable.
152. **v0.44.1 — kobj.nxe migrado a Ob** — usa `ObOpen("\Ob")` + `ObEnum` para mostrar namespace Ob jerárquico.
153. **v0.44.1 — ps.nxe migrado a Ob** — usa `ObOpen("\Ob\Process")` + `ObEnum` + `ObQueryInfo(Process)` por proceso. Datos reales.
154. **v0.44.1 — pri.nxe migrado a Ob** — usa `ObOpen` + `ObSetInfo(ProcessPriority)`.
155. **v0.44.1 — kill.nxe migrado a Ob** — usa `ObOpen` + `ObSetInfo(ProcessTerminate)`.

---

## ROADMAP PENDIENTE (v0.40 → v1.0)

> Basado en el análisis completo de `docs/ARCHITECTURAL_VISION.md`.
> **Regla de oro:** No añadir features nuevas antes de completar la fase de maduración (v0.40–v0.45).
> Cada feature nueva se apoya en abstracciones existentes; si esas abstracciones son frágiles, la feature será frágil.

---

### 🟢 Fase 1: Maduración (v0.40 – v0.45)
*Resolver limitaciones estructurales antes de expandir. Prioridad máxima.*

Orden de implementación dentro de la fase:

1. ~~**v0.43** — SeAccessCheck NT-compatible, sys_poll(), Congelar pipe/IRP protocols~~ **COMPLETADO**
2. ~~**v0.44** — ASLR v1 (base aleatoria), Ob syscalls RAX 60–64~~ **COMPLETADO** (v0.44.1: 4 binarios migrados a Ob: ps, kobj, pri, kill)**
3. **v0.45** — Device Tree + Resource Manager, Driver state machine freeze

> **Regla:** No se pasa a la Fase 2 hasta que v0.45 esté completo y todos los tests pasen.

---

### 🟢 Code Quality & Maintenance

* [ ] **CQ1. Reorganizar libneodos-nxl en módulos separados** | Prereqs: — | Files: `libneodos-nxl/src/main.rs` → `libneodos-nxl/src/{syscall,io,fs,process,mem,info,error}.rs`
  - **Descripción:** Dividir `libneodos-nxl/src/main.rs` (461 líneas monolíticas) en 7+ módulos separados. Cada módulo agrupa funciones por dominio: `syscall.rs` (raw `int 0x80` wrappers), `io.rs` (stdout/stderr/stdin, _print, _eprint), `fs.rs` (file_open/read/write + sys_mkdir/unlink/rmdir/rename), `process.rs` (pipe/dup2/waitpid/spawn/readdir/chdir/getcwd), `mem.rs` (brk/sbrk/mmap/munmap), `info.rs` (get_version/datetime/meminfo/cpuinfo), `error.rs` (consts + ret helper). `main.rs` solo mantiene `nxl_entry`, el `AbiTable` struct, `EXPORT_TABLE` static, y `nxl_panic`. Zero cambios en ABI: el NXL binario resultante es idéntico, .export_table en offset 0 con mismos valores. No requiere recompilar user binaries ni cambiar kernel/libneodos/build.
  - **Criterio:** `sha256sum` del NXL antes/después idéntica. 520 kernel tests + 27 user binaries funcionan sin cambios.
  - **Tests:** Ninguno nuevo (el binario es idéntico).

---

### 🟡 Fase 2: Expansión (v0.46 – v0.50)
*Añadir funcionalidades transformadoras. Ejecución secuencial dentro de la fase.*

Orden de implementación dentro de la fase:

1. **v0.46** — Device Tree + Resource Manager completo, PCI auto-vinculación, sys_ioctl(), VirtIO (A5.2), Input subsystem (A4.4)
2. **v0.47** — Networking: NIC driver NEM + TCP/IP stack (B3.1–B3.2)
3. **v0.48** — Async I/O: IOCP v1, sys_accept/send/recv, AHCI NCQ (A5.3), DHCP (B3.3)
4. **v0.49** — ASLR v2 (pila/heap aleatorios), PGO, Benchmarking suite, NTP (B3.4)
5. **v0.50** — **ObOpen/ObCreate/ObQueryInfo/ObSetInfo/ObEnum (RAX 60–64)**, Namespace por proceso (chroot-lite), Symlinks en VFS, Audit trail SACL, Shell pipes (B4.2), Redirection (B4.3), VT (B4.5)

---

### FASE A3 — Fault Tolerance (NT: Bugcheck, KD, SEH)

El kernel actual no sobrevive a fallos estructurados. Ring 3 mata el proceso en cualquier excepción.

- [ ] **A3.2. Kernel debugger (KD)** | NT: WinDbg kernel-mode debugging | Prereqs: A3.1
  - **Archivos:** `src/debugger/mod.rs`, `src/debugger/breakpoint.rs`, `src/debugger/watchpoint.rs`, `src/shell/commands/debug.rs`, `scripts/kd_client.py` (GDB stub adapter)
  - **Descripción:** Debugger residente en el kernel para inspección interactiva de fallos y ejecución en vivo. No depende de una GDB externa, pero expone un stub remoto por serial para depuración desde host cuando haga falta. El objetivo es poder detener el sistema de forma controlada, inspeccionar contexto, modificar puntos de control y reanudar sin perder el estado del bug.
    - **Breakpoints software:** INT3 (0xCC) instruction replacement. `set_breakpoint(addr)` guarda original byte, escribe 0xCC. `#BP` (INT3) handler chequea si breakpoint registrado, pausa kernel si match.
    - **Breakpoints hardware:** 4 registro DR0–DR3 + DR7 (debug control). `set_hw_breakpoint(addr, type: execute|read|write|readwrite, len: 1/2/4/8)` configura DR7. `#DB` (INT1) handler dispara si DR6 flag match.
    - **Pause model:** al dispararse un breakpoint válido, el debugger congela el flujo normal del kernel y entra en estado `Paused`, preservando RIP, RSP, GPRs, CR0–CR4 y el motivo de parada. En ese estado solo se aceptan comandos de depuración explícitos.
    - **Resume model:** `DEBUG CONTINUE` reanuda exactamente desde la instrucción siguiente al breakpoint o desde el RIP ajustado por watchpoint, sin reentrar en panic ni perder el contexto capturado.
    - **Shell commands:**
      - `DEBUG BREAK <addr>` — set INT3 breakpoint
      - `DEBUG UNBREAK <addr>` — remove
      - `DEBUG WATCH <addr> <type: r|w|rw>` — set hardware watchpoint
      - `DEBUG CONTINUE` — resume ejecución (solo légal desde breakpoint)
      - `DEBUG REG` — dump GPRs, CR0–4
      - `DEBUG MEM <addr> <len>` — hex dump memoria
      - `DEBUG STACK <depth=16>` — stack trace
      - `DEBUG SCHED` — dump scheduler state (runqueues, current thread)
    - **GDB protocol (serial):** Implementar GDB remote protocol subset (qSupported, vCont, g/G, m/M, Z/z) para que `gdb kernel.elf -ex 'target remote /dev/ttyUSB0'` funcione. El stub solo necesita ser suficiente para stop/resume, lectura de registros y memoria, y gestión básica de breakpoints.
    - **State:** Global `debugger_state: DebuggerState { breakpoints: [Option<BreakpointInfo>; 8], hw_watchpoints: [DrReg; 4], paused_rip: u64, last_stop_reason: StopReason }`. Las estructuras deben vivir en memoria kernel fija y no depender de heap durante la captura.
  - **Criterio:**
    - Breakpoint en `sys_write` entry. Kernel pausa, shell imprime "Breakpoint at 0xXXXX", espera comando.
    - `DEBUG REG` muestra RAX–R15 en ese punto.
    - `DEBUG CONTINUE` reanuda ejecución (sin panic).
    - Watchpoint en dirección de heap: detiene si algo escribe. Log la instrucción (RIP) que escribió.
    - Un cliente GDB remoto puede conectar, listar registros, leer memoria y continuar sin corromper el estado interno.
  - **Tests:** `kd_breakpoint_set_and_hit`, `kd_breakpoint_invalid_addr`, `kd_watchpoint_write_detect`, `kd_register_snapshot`, `kd_gdb_protocol_qSupported` (5 tests).

---

- [ ] **A4.4. Input subsystem rediseñado** | NT: ConDrv (Console Driver) | Prereqs: A4.7
  - **Archivos:** `src/input/mod.rs` (reescritura), `src/input/manager.rs` (new), `src/input/vt.rs` (new), integración `arch/x64/idt.rs` (PS/2 delivery)
  - **Descripción:** Sistema de entrada multiplexado soportando múltiples terminales virtuales (VTs) con independencia de input. En vez de enviar bytes directamente al shell activo, el kernel clasifica y enruta el input a una cola por VT, permitiendo que varias sesiones coexistan sin pisarse entre sí.
    - **Virtual Terminals:** Máx 4 VTs (Alt+F1–F4). Cada VT tiene:
      - Input queue (ring buffer 4 KB) independiente
      - Output buffer (attached framebuffer) independiente
      - Foreground pid (proces que recibe input)
      - Session leader (PID 1 NeoInit es sesion leader de todas)
    - **InputManager structure:**
      ```rust
      struct InputManager {
          vt_queues: [InputQueue; 4],  // 4 KB each
          active_vt: u8,  // 0-3
          vt_pids: [Option<u32>; 4],   // foreground pid per VT
      }
      ```
    - **Keyboard IRQ (PS/2 IRQ1):** Nueva ruta:
      1. IRQ1 handler lee scancode
      2. Convierte a ASCII (KBDUS/KBDSP layout)
      3. Chequea `active_vt`
      4. Inserta en `vt_queues[active_vt]`
      5. Envía event `EVENT_KEYBOARD_INPUT` al event bus (data0 = byte, data1 = vt_num)
    - **VT switching:** Alt+F1 scancode detectado → `InputManager::switch_vt(1)` → `active_vt=0` → framebuffer renderiza VT0, input lo recibe VT0 pid.
    - **sys_read(fd=0, buf, len) stdin:** Bloquea en `vt_queues[active_vt].read()` hasta bytes disponibles.
    - **Fuentes adicionales (paralelizable):** Serial (COM1), USB HID (cuando UHCI/OHCI maduros) → cada fuente puede entregar a `InputManager::push_byte(vt, byte)` independientemente.
    - **Foreground policy:** solo el VT foreground recibe el teclado físico; los demás conservan su cola y su framebuffer en pausa para poder volver sin perder estado.
    - **Rendering contract:** cambiar de VT implica swap de back-buffer, no recreación de terminal, para conservar scrollback y estado visual.
  - **Criterio:**
    - Alt+F1: pantalla cambia a VT0, teclado entrega a shell en VT0.
    - Alt+F2: pantalla cambia a VT1 (vacía), input sin proc → silent (no error).
    - Type en VT1, Alt+F1, type en VT0: ambos buffers almacenan independiente.
    - Stress: generar keystrokes rápidos en 4 VTs simultáneamente, ninguno pierde bytes.
    - El cambio de VT no altera el proceso foreground salvo que el propio scheduler o shell lo decida.
  - **Tests:** `input_vt_switch_framebuffer`, `input_vt_independent_queues`, `input_vt_rapid_switching`, `input_4vt_concurrent_stress`, `input_event_bus_dispatch_vt` (5 tests).

---

### FASE A5 — Storage Unification (NT: IoStack)

- [ ] #### A5.2. VirtIO block driver (BOOT_DRIVER)
Prereqs: A2.1
* **Archivos:** `src/drivers/virtio_blk.rs` (new, 400–500 lines), integración `src/drivers/storage.rs`, `src/main.rs` PHASE 3.6 (priority init)
* **Descripción:** Controlador de bloques VirtIO para máquinas virtuales QEMU/KVM. Se clasifica como **BOOT_DRIVER**, no como `.NEM`, ya que participa directamente en la cadena de arranque del sistema y debe estar disponible antes del montaje del volumen raíz.
  * **PCI detection:** Bus 0, vendor 0x1AF4 (Red Hat), device 0x1001 (VirtIO Block).
  * **Initialization:**
    1. Read BAR0 (MMIO base)
    2. Write device status: ACKNOWLEDGE | DRIVER
    3. Allocate virtqueue (#0, 32 descriptors)
    4. Register queue physical address
    5. Negotiate legacy/modern features
    6. Write device status: DRIVER_OK
  * **I/O path:** `submit_irp(irp)` →
    1. Allocate descriptor slot
    2. Fill request header
    3. Configure sector_start, sector_count, buffer address
    4. Notify device (doorbell)
    5. Wait completion (polling or interrupt)
    6. Process used ring
    7. Complete IRP
  * **Supported operations:**
    * READ
    * WRITE
    * FLUSH
    * DISCARD
  * **Storage priority:**
    ```text
    NVMe > VirtIO > BootAhci > BootAta
    ```
  * **Boot integration:**
    * Available before VFS mount
    * Available before NeoInit
    * Available before NeoShell
    * Available before NEM loader
    * Used by GPT parser
    * Used by NeoDOS filesystem loader
  * **Driver classification:**
    ```text
    DriverClass::Boot
    ```
  * **Future compatibility:**
    Diseñar el driver usando el futuro Driver ABI interno para facilitar una migración posterior a módulos preembebidos tipo:
    ```text
    kernel.bin
     ├─ virtio_blk.nem
     ├─ ahci.nem
     └─ nvme.nem
    ```
    cargados desde memoria durante early boot. No implementar todavía.
* **Criterio:**
  * Arrancar NeoDOS en QEMU usando:
    ```text
    -drive if=virtio
    ```
  * Detección automática PCI.
  * Inicialización correcta del dispositivo.
  * GPT parsing vía VirtIO.
  * Carga del superblock NeoDOS.
  * Montaje de volumen raíz.
  * Arranque completo de NeoInit y NeoShell.
  * Lectura de 1 MB < 50 ms.

* **Tests:**
  * `virtio_pci_detect`
  * `virtio_virtqueue_init`
  * `virtio_submit_read_write`
  * `virtio_boot_load_kernel`
  * `virtio_gpt_parsing`
  * `virtio_mount_rootfs`
  * `virtio_boot_neoshell`
  * **(7 tests)**

- [ ] **A5.3. AHCI NCQ** | NT: Storport Native Command Queuing | Prereqs: A2.2
  - **Archivos:** `src/drivers/boot_ahci.rs` (extend), `src/drivers/ahci/mod.rs` (NEM driver), `src/irp/mod.rs` (tag-based dispatch)
  - **Descripción:** Native Command Queuing en AHCI permite hasta 32 operaciones simultáneas con finalización out-of-order.
    - **Legacy AHCI (v0.14):** Single command slot, operaciones serializadas (issue, poll, complete, issue siguiente). Latency: 1 cmd = 0.1 ms * 32 cmds = 3.2 ms para 32 reads.
    - **NCQ path:**
      1. Host prepara 32 command tables en memoria (FIS buffer per slot).
      2. Escribe descriptores a device:
         - Comando: ATA FPDMA QUEUED READ (0x60) / WRITE (0x61)
         - CDB[0] = code, [1–8] = LBA–48, [9] = COUNT (sectors), [14] = TAG (0–31)
      3. Device acepta hasta 32 cmds sin esperar completaciones.
      4. Device finaliza out-of-order: escribe SActive register (bit = completado), trigger IRQ.
      5. Host lee Successful NCQ Completion Notification (FIS D2H), extrae tag, localiza IRP via tag.
      6. Cada IRP completion genera eventos (IRP_DONE) independientemente.
    - **Tag-based dispatch:** Per-device, map `[Option<IrpId>; 32]` indizado por tag. Al completar, lookup rápido O(1).
    - **Fall back to legacy:** Si device no soporta NCQ (vía IDENTIFY), usar single-command path (v0.14).
  - **Criterio:**
    - 32 read IRPs encolados simultáneamente. Device AHCI completa out-of-order (no esperan serialización).
    - Time to complete 32 reads: ~0.1 ms (paralelo) vs 3.2 ms (serial). ~30x faster.
    - Stress: NCQ bajo carga, sin comando perdido, IRP_DONE count = 32.
  - **Tests:** `ahci_ncq_32_concurrent_dispatch`, `ahci_ncq_tag_based_completion`, `ahci_ncq_fallback_to_legacy`, `ahci_ncq_out_of_order_completion`, `ahci_ncq_stress_load` (5 tests).

---

### 🔴 Fase 3: Estabilización (v0.51 – v1.0.0)
*Bugfixes, hardening, documentación, y preparación para API estable.*

Orden de implementación dentro de la fase:

1. **v0.51** — sys_fork/clone (bajo demanda), sys_signal mínimo, **ObWait (RAX 65) + KWait integration**, **HandleEntry full migration (kind→ObId)**
2. **v0.52** — Stack de red completo (UDP, DNS, DHCP), TFTP/NFS básico, **Security integration in ObOpen**
3. **v0.53** — Rendimiento: per-CPU heaps NUMA-aware, scheduler lock-free, zero-copy pipes (B6.1), COW fork (B6.2)
4. **v0.54–0.59** — Documentación API completa, test coverage >95%, fuzzing, module signatures (B5.1), secure boot (B5.3), **Ob API documentation + legacy syscall compat verified**
5. **v1.0.0** — Primera API estable. Todo lo anterior debe estar COMPLETED.

---

### FASE B — Features (userland + servicios)

Prereqs globales: A4.7 mínimo para items userland; NT5/NT6 para items de seguridad.

#### B1. Tracing & Observability

- [ ] **B1.1 Y1. Kernel tracing infrastructure** | Prereqs: A2.4 | Files: `src/trace/mod.rs`
  - **Descripción:** Ampliar el `TraceBuffer` existente (1024 entries, lock-free ring buffer en `trace.rs`) con trace points registrables dinámicamente. Actualmente el buffer soporta 7 tipos de evento (`ContextSwitch`, `SyscallEnter/Exit`, `IrqEnter/Exit`, `SchedDecision`, `Panic`) con 4 argumentos u64 por entry. Esta mejora añade: registro dinámico de trace points por subsistema (scheduler, VFS, memory, drivers), filtrado por categoría/nivel, y dump formateado via serial con timestamps HPET. El buffer circular de 4 KB se mantiene lock-free para uso desde contexto IRQ.
  - **Criterio:** Trace points registrables desde cualquier módulo kernel. Dump via serial legible. Filtrado por categoría funcional.
  - **Tests:** `trace_register_dynamic_point`, `trace_filter_by_category`, `trace_dump_serial_format`.

- [ ] **B1.2 Y2. NeoTrace system** | Prereqs: B1.1 | Files: `userbin/neotrace/`
  - **Descripción:** Comando de shell Ring 3 `NEOTRACE` que expone la infraestructura de tracing (B1.1) al usuario. Subcomandos: `START` (activa captura global), `STOP` (pausa captura), `DUMP [N]` (vuelca las últimas N entradas del TraceBuffer a consola), `FILTER <category>` (filtra por categoría). Usa `TRACE.dump()` internamente. No existe variante Ring 0; el acceso operatorio solo se hace desde `neoshell` o binarios `userbin`.
  - **Criterio:** `NEOTRACE START` + ejecutar proceso + `NEOTRACE DUMP 32` muestra últimas 32 entradas con timestamps.
  - **Tests:** `neotrace_start_stop_toggle`, `neotrace_dump_output`.



#### B2. NeoReg & Configuration Infrastructure

* [ ] **B2.1 Z6. Registry hive database | NT: Cm (Configuration Manager), cell-based hive** | Prereqs: NT5 (Ob), NT6 (SID/ACL), A5.1 (IoStack) | Files: `src/cm/`, `src/cm/hive.rs`, `src/cm/cell.rs`, `src/cm/key.rs`, `src/cm/cache.rs`
  * **Descripción:**
    Implementar NeoReg, sistema de configuración jerárquico persistente como el Cm de Windows NT. El diseño sigue el modelo NT de células (cells) y bins, con integración directa en el Object Manager NT5.
    **Cell-based hive format** (en vez de árbol simple):
    ```text
    Hive
    ├─ Base Block (4 KB) — magic "neoR", seq numbers, checksum
    ├─ Bins (4 KB cada uno)
    │  ├─ Cell — Key: name, parent_cell, subkeys_list, values_list, class, sec_desc, last_write
    │  ├─ Cell — Value: name, type (REG_SZ/DWORD/BINARY), data
    │  └─ Cell — Security descriptor (SID + ACL, reutilizado entre keys)
    └─ Free cells (linked list for reuse)
    ```
    Cada celda tiene un índice dentro del bin, y los bins se numeran secuencialmente. Esto permite crecimiento incremental y recovery por bin, no por hive completo.
    **ObNamespace integration** — cada key registry es un objeto en NT5:
    ```text
    KObj type KEY
    \Registry
      \Machine           → KObj::Directory
        \System           → KObj::Key (backed by SYSTEM.HIV)
          \BootShell      → KObj::Key
        \Drivers          → KObj::Key (backed by DRIVERS.HIV)
      \User
        \Default          → KObj::Key (backed by DEFAULT.HIV)
    ```
    `sys_open("\\Registry\\Machine\\System\\BootShell")` funciona via NT5 path resolution. Key objects tienen SecurityDescriptor del NT6 SRM.
    **Cell cache** — hash table LRU de 512 celdas en memoria. Evita leer disco en cada acceso. Las celdas sucias (dirty) se marcan y se flushed periódicamente.
    **Syscall API** — expuesta como syscalls NT-style:
    ```text
    RAX 50  sys_open_key(path)        → handle (NtOpenKey)
    RAX 51  sys_create_key(path)      → handle (NtCreateKey)
    RAX 52  sys_query_value(key, name, buf, len) → value (NtQueryValueKey)
    RAX 53  sys_set_value(key, name, type, data, len) (NtSetValueKey)
    RAX 54  sys_enum_key(key, index, buf) → subkey name (NtEnumerateKey)
    RAX 55  sys_enum_value(key, index, buf) → value name (NtEnumerateValueKey)
    RAX 56  sys_delete_key(key)               (NtDeleteKey)
    RAX 57  sys_flush_key(key)                (NtFlushKey)
    RAX 58  sys_load_hive(path, mount_point)  (NtLoadKey, admin)
    RAX 59  sys_unload_hive(mount_point)      (NtUnloadKey, admin)
    ```
    Valores: `REG_NONE=0`, `REG_SZ=1`, `REG_DWORD=4`, `REG_BINARY=3`, `REG_MULTI_SZ=7`.
    **Boot sequence:**
    ```text
    PHASE 3.87  Init Cm subsystem
    PHASE 3.88  Mount SYSTEM.HIV → \Registry\Machine\System
    PHASE 3.89  Mount SOFTWARE.HIV → \Registry\Machine\Software
    PHASE 3.90  Mount DRIVERS.HIV → \Registry\Machine\Drivers
    PHASE 3.91  Mount DEFAULT.HIV → \Registry\User\Default
    ```
  * **Criterio:**
    - Keys y values expuestos como objetos en NT5 namespace
    - `sys_open("\\Registry\\Machine\\System")` devuelve handle a la key raíz de SYSTEM.HIV
    - `sys_set_value(key, "PATH", REG_SZ, "C:\\Programs")` persiste y es recuperable tras reboot
    - Cell cache: 2da lectura de misma key no toca disco (cache hit)
    - Hive persistente: tras reboot, valores anteriores siguen presentes
    - Format cell-based: corrupción de un bin no invalida el hive completo
  * **Tests:**
    `cm_create_key_ob`, `cm_query_value_cache_hit`, `cm_set_value_persist`,
    `cm_enum_keys_multi`, `cm_hive_reload_integrity`, `cm_cell_corruption_isolated`,
    `cm_syscall_open_key`, `cm_syscall_set_get_value` (8 tests)

* [ ] **B2.2 Z6. Registry transaction journal | NT: Hive LOG (.LOG1/.LOG2)** | Prereqs: B2.1 | Files: `src/cm/journal.rs`
  * **Descripción:**
    Write-Ahead Log (WAL) para cada hive. Sigue el modelo NT de `.LOG` / `.LOG1` / `.LOG2`:
    ```text
    SYSTEM.HIV     — hive principal
    SYSTEM.LOG     — WAL primario (cambios desde último flush)
    SYSTEM.LOG1    — WAL secundario (NT usa .LOG1 como respaldo)
    ```
    Formato: cada entrada de log es un `JournalEntry { seq, op: CmOp, cell_id, old_data, new_data, crc32 }`.
    Las operaciones son: `CreateKey`, `DeleteKey`, `SetValue`, `DeleteValue`, `RenameKey`.
    Recovery al boot: si `SYSTEM.HIV.seq != SYSTEM.LOG.seq`, replay todas las entradas desde la última secuencia confirmada. Si `.LOG` está corrupto (crc32 mismatch), probar `.LOG1`. En NT, si ambos logs fallan, se carga el hive en modo read-only.
  * **Criterio:** Apagado durante `reg_set_value` — al boot se replay el log y el valor aparece. Ambos logs corruptos → hive carga en read-only.
  * **Tests:** `cm_journal_commit_replay`, `cm_journal_corrupt_log1_fallback`, `cm_journal_crc_mismatch`, `cm_journal_dual_recovery` (4 tests)

* [ ] **B2.3 Z6. Multi-Hive Architecture | NT: SYSTEM/SOFTWARE/SECURITY/DEFAULT hives** | Prereqs: B2.1, B2.2 | Files: `src/cm/hive.rs`, `src/cm/manager.rs`
  * **Descripción:**
    Múltiples hives bajo `\Registry` con independencia de carga, persistencia y recovery:
    ```text
    \Registry\Machine\System     → C:\SYSTEM\CONFIG\SYSTEM.HIV
    \Registry\Machine\Software   → C:\SYSTEM\CONFIG\SOFTWARE.HIV
    \Registry\Machine\Drivers    → C:\SYSTEM\CONFIG\DRIVERS.HIV
    \Registry\User\Default       → C:\SYSTEM\CONFIG\DEFAULT.HIV
    \Registry\Machine\Security   → C:\SYSTEM\CONFIG\SECURITY.HIV (B2.4)
    ```
    `HiveManager` central gestiona carga cíclica, resolución de rutas y carga diferida (lazy load). Fallback: hive faltante → empty, hive corrupto → intenta recovery → backup → empty. Cada hive tiene su propio cache de celdas y su propio journal.
  * **Criterio:** `\Registry\Machine\System` resuelve SYSTEM.HIV. SOFTWARE.HIV corrupto → SYSTEM.HIV sigue funcionando.
  * **Tests:** `cm_multi_hive_resolve`, `cm_hive_independent_recovery`, `cm_hive_lazy_load`, `cm_missing_hive_empty` (4 tests)

* [ ] **B2.4 Z6. Registry Security | NT: SECURITY.HIVE, Key ACLs (NT6)** | Prereqs: B2.3, NT6 | Files: `src/cm/security.rs`
  * **Descripción:**
    Control de acceso sobre keys registry usando el NT6 Security Reference Monitor directamente (SID + ACL + SeAccessCheck), no un sistema ad-hoc. Cada key object tiene un `SecurityDescriptor` (owner SID, group SID, DACL). Al abrir una key, `SeAccessCheck` valida el token del caller contra la DACL de la key. Hive `SECURITY.HIV` en `\Registry\Machine\Security` para políticas.
    Protecciones iniciales:
    ```text
    \Registry\Machine\System     → SYSTEM:FULL_CONTROL, USER:READ
    \Registry\Machine\Drivers    → SYSTEM:FULL_CONTROL, USER:READ
    \Registry\Machine\Security   → SYSTEM:FULL_CONTROL (USER: denied)
    \Registry\User\Default       → USER:FULL_CONTROL
    ```
  * **Criterio:** `sys_set_value` en `\Registry\Machine\Security` desde USER → `-EACCES`. `sys_open_key` en `\Registry\User\Default` desde USER → OK.
  * **Tests:** `cm_security_system_write_allowed`, `cm_security_user_read_denied`, `cm_security_admin_bypass`, `cm_security_acl_inheritance` (4 tests)

* [ ] **B2.5 Z6. Registry notification + load/unload | NT: RegNotifyChangeKeyValue, NtLoadKey, NtUnloadKey** | Prereqs: B2.3, Event Bus | Files: `src/cm/notify.rs`
  * **Descripción:**
    **Key change notifications:** cuando una key o valor cambia, se publica un evento `EVENT_REG_KEY_CHANGED (type 0x2001)` al Event Bus con `data0=key_id`, `data1=change_type`. Permite que drivers y procesos user-mode reaccionen a cambios de configuración sin polling. `sys_notify_reg_key(key_handle, subscribe)` (RAX=60) registra interés; el kernel entrega eventos via el mecanismo de APC (A4.5).
    **Hive load/unload:** `sys_load_hive(path, mount_point)` (RAX=58, admin) carga un hive externo (ej. perfil de usuario) bajo una ruta en `\Registry`. `sys_unload_hive(mount_point)` (RAX=59, admin) lo descarga y persiste. Útil para profiles de usuario: `sys_load_hive("C:\\Users\\Alejandro\\NTUSER.HIV", "\\Registry\\User\\Alejandro")`.
  * **Criterio:** `reg_set_value(key, ...)` → subscriber recibe `EVENT_REG_KEY_CHANGED`. `sys_load_hive(path, mount)` carga hive externo y keys aparecen bajo `\Registry\User\Alejandro`.
  * **Tests:** `cm_notify_key_change_apc`, `cm_notify_subscribe_unsubscribe`, `cm_load_hive_external`, `cm_unload_hive_persist`, `cm_load_hive_admin_required` (5 tests)


* [ ] **B3.1. TCP/IP stack** | NT: AFD (Ancillary Function Driver for WinSock) | Prereqs: NIC driver
          ↓
  Check SYSTEM.LOG
  ```
  Si existe transacción incompleta:
  ```text
  Replay Journal
         ↓
  Rebuild Hive
         ↓
  Continue Boot
  ```
---
* **Protección de corrupción**
  Añadir checksum:
  ```rust
  crc32
  ```
  tanto en:
  ```text
  SYSTEM.HIV
  SYSTEM.LOG
  ```
  Detectar:
  * escritura parcial
  * truncado
  * corrupción simple
---
* **Optimización**
  Mantener:
  ```text
  1 journal activo
  ```
  No implementar todavía:
  ```text
  LOG1
  LOG2
  snapshots
  checkpoints
  incremental recovery
  ```
---
* **Herramientas**
  Añadir:
  ```text
  REGCHECK.NXE
  ```
  Ejemplo:
  ```text
  REGCHECK
  ```
  Salida:
  ```text
  Hive: OK
  Journal: CLEAN
  ```
  o:
  ```text
  Hive: OK
  Journal: RECOVERED
  ```
* **Criterio**
  * Apagado durante escritura no corrompe hive.
  * Recovery automático al boot.
  * Journal se limpia tras commit.
  * Checksum validado correctamente.
  * Hive siempre queda consistente.

* **Tests**
  ```text
  registry_journal_create
  registry_journal_commit
  registry_journal_replay
  registry_recover_after_crash
  registry_checksum_validation
  registry_hive_consistency
  ```

---

#### B3. Networking

- [ ] **B3.1 D9. Network I/O | NT: Winsock (ws2_32.dll) → NtCreateFile(\Device\Tcp)** | Prereqs: A4.1, A4.2 | Files: `src/net/`, `src/syscall.rs`
  - **Descripción:** Modelo NT: el kernel expone `\Device\Tcp` y `\Device\Udp` como objetos de dispositivo en el namespace NT5. La API de red user-mode va en `libneodos/src/net.rs` como wrapper que abre `\Device\Tcp` via `sys_open` y opera via `sys_ioctl` (NtDeviceIoControlFile). No hay syscalls socket-style — se usa el modelo NT de File + IoControl. `sys_ioctl` (RAX=14) se extiende con códigos como `IOCTL_TCP_CONNECT`, `IOCTL_TCP_SEND`, `IOCTL_TCP_RECV`. El kernel tiene un stack TCP/IP interno (Ethernet, ARP, IPv4, ICMP, UDP, TCP) que se conecta al NIC via IRP. Winsock-like API en user-mode: `net_open()`, `net_bind()`, `net_connect()`, `net_send()`, `net_recv()`, `net_close()`.
  - **Criterio:** User-mode puede hacer `net_open(b"\\Device\\Tcp")`, `net_connect(fd, ip, port)`, `net_send(fd, buf)`. `PING` funciona via ICMP.
  - **Tests:** `net_open_device_tcp`, `net_tcp_connect_send_recv`, `net_icmp_ping`.

- [ ] **B3.2 E3. TCP/IP stack | NT: AFD (Ancillary Function Driver)** | Prereqs: B3.1 | Files: `src/net/`
  - **Descripción:** Stack de red completo en kernel como driver de dispositivo `\Device\Tcp` y `\Device\Udp`. Capas: Ethernet (frame TX/RX, MAC addressing), ARP (tabla 64 entries, request/reply, timeout 300s), IPv4 (header parse/build, checksum, TTL decrement, fragmentation básica), ICMP (echo request/reply para `PING`), UDP (connectionless, checksum opcional), TCP (3-way handshake, sequence numbers, sliding window 16 KB, retransmit timer, FIN/RST). NIC driver via VirtIO-net o e1000 (QEMU). Se prueba con QEMU `-netdev user,hostfwd=tcp::8080-:80`.
  - **Criterio:** `PING 10.0.2.2` recibe reply. TCP connection a host funciona.
  - **Tests:** `tcp_handshake_3way`, `udp_send_recv`, `arp_table_lookup`, `icmp_echo_reply`.

- [ ] **B3.3 D8. DHCP client | NT: DHCP Client Service** | Prereqs: B3.2 | Files: `src/net/dhcp.rs`
  - **Descripción:** Cliente DHCP (RFC 2131) que obtiene configuración de red automáticamente al boot. Envía DHCPDISCOVER broadcast (UDP 68→67), recibe DHCPOFFER, envía DHCPREQUEST, recibe DHCPACK. Configura: IP address, subnet mask, default gateway, DNS server. Lease renewal timer via HPET. En QEMU `-netdev user`, el DHCP server integrado de QEMU asigna 10.0.2.15.
  - **Criterio:** Al boot con NIC presente, kernel obtiene IP automáticamente sin configuración manual.
  - **Tests:** `dhcp_discover_offer_sequence`, `dhcp_lease_renewal`.

- [ ] **B3.4 D7. NTP client | NT: W32Time (Windows Time Service)** | Prereqs: B3.2 | Files: `src/net/ntp.rs`
  - **Descripción:** Cliente NTP (RFC 5905, modo SNTP simplificado) que sincroniza el RTC del sistema con un servidor NTP externo. Envía NTP request (UDP puerto 123), parsea respuesta (timestamps T1–T4), calcula offset y round-trip delay, ajusta RTC via `rtc_bridge.rs`. Servidor configurable en registry o `C:\System\Config\system.cfg`. Sincronización periódica cada 3600 segundos.
  - **Criterio:** Tras boot con red, RTC sincronizado con servidor NTP (offset < 1s).
  - **Tests:** `ntp_request_parse_response`, `ntp_offset_calculation`.

---

#### B4. Userland Usable System

- [ ] **B4.3 S3. Shell redirection (`>`, `<`, `>>`)** | Prereqs: A4.7 | Files: `userbin/neoshell/`
  - **Descripción:** Redirección de I/O en neoshell. Parser detecta tokens `>` (write), `>>` (append), `<` (read). Para `cmd > file`: neoshell abre/crea `file` via `sys_open`, luego spawna `cmd` con `sys_dup2` redirigiendo fd 1 (stdout) al handle del archivo. Para `cmd < file`: abre archivo y redirige fd 0 (stdin). Para `>>`: abre con flag append. Tras `sys_waitpid`, cierra el handle del archivo via `sys_close`.
  - **Criterio:** `DIR > output.txt` crea archivo con listado. `TYPE < input.txt` lee de archivo.
  - **Tests:** `redirect_stdout_to_file`, `redirect_stdin_from_file`, `redirect_append`.
- [ ] **B4.5 B1. Virtual terminals** | Prereqs: A4.4, B4.4 | Files: `userbin/neoshell/`, `src/input/`
  - **Descripción:** Multiplexar el framebuffer y el input en hasta 4 terminales virtuales (VTs). Depende de A4.4 (input subsystem rediseñado con `InputManager` y `vt_queues[4]`). Cada VT tiene su propio buffer de framebuffer (back-buffer 1280×800), cola de input independiente, y PID foreground. Alt+F1–F4 cambia VT activo: el kernel copia el back-buffer del VT seleccionado al framebuffer visible y redirige IRQ1 (PS/2) a la cola correspondiente. NeoInit spawna una instancia de neoshell por VT.
  - **Criterio:** Alt+F1 y Alt+F2 muestran shells independientes. Input en un VT no afecta al otro.
  - **Tests:** `vt_switch_alt_f1_f2`, `vt_independent_input`, `vt_framebuffer_swap`.

- [ ] **B4.6 B6. NeoEdit text editor** | Prereqs: A4.7, B4.4 | Files: `userbin/neoedit/`
  - **Descripción:** Editor de texto modal Ring 3 (`.NXE`). Usa `sys_open` + `sys_readfile` para cargar archivos y `sys_writefile` para guardar. Interfaz: barra de estado (nombre archivo, línea, columna), área de edición con scroll vertical, comandos Ctrl+S (save), Ctrl+Q (quit), Ctrl+G (goto line). Renderiza via `sys_write` con secuencias ANSI (B4.4) para posicionar cursor y aplicar colores (syntax highlighting básico para `.CFG`, `.BAT`). Buffer interno: array de líneas `Vec<String>` limitado a 64 KB.
  - **Criterio:** `NEOEDIT C:\System\Config\system.cfg` abre, edita, guarda correctamente.
  - **Tests:** `neoedit_open_display`, `neoedit_edit_save`, `neoedit_scroll`.

- [ ] **B4.7 B6b-v2. Shared library per-process binding | NT: Ldr (Loader, PEB->LdrData)** | Prereqs: A1.5, sys_loadlib | Files: `src/elf.rs`, `libneodos/`
  - **Descripción:** Evolucionar el sistema NXL actual (slots globales fijos en 0x1E000000–0x1E200000 compartidos entre procesos) a binding per-process. Cada EPROCESS mantiene su propia tabla de NXLs cargadas (`nxl_table: [Option<NxlBinding>; 8]` en `Eprocess`). `sys_loadlib` (RAX=21) mapea la NXL en el address space del proceso caller (no global). Al hacer `sys_exit`, se desmapean las NXLs del proceso. Esto permite versiones diferentes de la misma NXL en procesos distintos.
  - **Criterio:** Dos procesos cargan versiones distintas de `libmath.nxl` sin interferencia.
  - **Tests:** `nxl_per_process_isolation`, `nxl_unload_on_exit`, `nxl_version_coexistence`.

- [ ] **B4.8 B7. NeoTOP** | Prereqs: A4.7, A1.5 | Files: `userbin/neotop/`
  - **Descripción:** Monitor de sistema Ring 3 en tiempo real (`.NXE`). Muestra: lista de EPROCESS/KTHREAD (PID, TID, estado, prioridad, CPU ticks, memoria), uso de CPU por core (contadores del KPRCB via `sys_getcpuinfo`), estadísticas de memoria (buddy allocator free frames, slab usage, page cache hit ratio), drivers cargados. Refresco cada 1 segundo via `sys_sleep`. Renderiza con ANSI escape codes. Columnas ordenables. Ctrl+Q para salir.
  - **Criterio:** `NEOTOP` muestra procesos activos actualizándose en tiempo real. Ctrl+Q sale limpiamente.
  - **Tests:** `neotop_display_processes`, `neotop_refresh_loop`, `neotop_exit_clean`.

- [ ] **B4.9 B11. NeoShell scripting (`.BAT`)** | Prereqs: B4.1, B4.2, B4.3 | Files: `userbin/neoshell/`
  - **Descripción:** Intérprete de scripts batch en neoshell. Soporta archivos `.BAT`/`.CMD` con: `ECHO` (imprimir), `SET` (variables), `IF %VAR%==valor cmd` (condicional), `GOTO :label` (salto), `CALL script.bat` (subrutina), `FOR %%i IN (*.txt) DO cmd` (iteración), `REM` (comentarios), `@` (silenciar echo). Parser lee línea a línea via `sys_readfile`. Variables expandidas con `%VAR%`. Exit code del último comando en `%ERRORLEVEL%`.
  - **Criterio:** Script `.BAT` con IF/GOTO/CALL ejecuta correctamente. `%ERRORLEVEL%` refleja exit codes.
  - **Tests:** `bat_echo_set`, `bat_if_goto`, `bat_call_subroutine`, `bat_for_loop`.

- [ ] **B4.10 B12. Compositor 2D** | Prereqs: B4.4, framebuffer | Files: `userbin/compositor/`
  - **Descripción:** Compositor de ventanas 2D sobre el framebuffer GOP 1280×800. Modelo: cada ventana tiene un back-buffer (ancho×alto×4 bytes BGRA), posición (x,y), z-order, título. El compositor blittea ventanas en orden z sobre el framebuffer principal. Soporte: mover ventanas (drag título), redimensionar (drag bordes), minimizar, cerrar. Input: mouse events (futuro PS/2 mouse driver o emulación con teclado). IPC: procesos envían draw commands via pipe o shared memory al compositor. Renderiza a 30 FPS máximo (33ms refresh via HPET timer).
  - **Criterio:** Dos ventanas superpuestas, una encima de otra. Mover ventana actualiza framebuffer.
  - **Tests:** `compositor_create_window`, `compositor_z_order`, `compositor_blit_overlap`.

#### B5. Security

- [ ] **B5.1 U1. Module signature validation** | Prereqs: NT6 | Files: `src/drivers/loader.rs` | Done when: `.nem` sin firma válida rechazado en load.
  - **Descripción:** Validación criptográfica de módulos `.nem` antes de que entren al runtime del driver loader. El loader debe verificar cabecera, hash del payload y firma con la clave pública esperada antes de reservar memoria o registrar símbolos. Si la firma falla, el módulo se marca `Faulted` y no llega a inicializarse.
  - **Criterio:** Un `.nem` alterado o sin firma no puede pasar de `Loaded` a `Initialized`.
  - **Tests:** `nem_signature_valid_accepts`, `nem_signature_invalid_rejects`, `nem_signature_tamper_detected`.
- [ ] **B5.2 U3. Driver permission enforcement** | Prereqs: NT6.3, B5.1 | Files: `src/drivers/caps.rs` | Done when: caps verificadas contra token + ACL del driver object.
  - **Descripción:** Cruza la capacidad declarada por el driver con el token del proceso que intenta cargarlo y con la ACL del objeto driver en el namespace. Esto evita que un binario firmado pero no autorizado acceda a recursos de alto privilegio o a operaciones de I/O que no le corresponden.
  - **Criterio:** Un driver sin `CAP_ADMIN` no puede abrir objetos protegidos aunque esté firmado.
  - **Tests:** `driver_caps_allow_admin`, `driver_caps_deny_user`, `driver_caps_acl_intersection`.
- [ ] **B5.3 U4. Secure boot chain** | Prereqs: B5.1 | Files: `neodos-bootloader/`, `src/boot/secure.rs` | Done when: kernel + drivers verificados antes de ejecutar.
  - **Descripción:** Encadena la verificación desde bootloader hasta kernel y drivers para que ningún binario de arranque se ejecute sin validación previa. La idea es que el bootloader sea el primer punto de confianza, verifique el kernel, y luego el kernel continúe verificando los módulos que carga durante Phase 3.85.
  - **Criterio:** Si falla la verificación del kernel o de un driver crítico, el boot se detiene en vez de seguir con código no confiable.
  - **Tests:** `secure_boot_kernel_verified`, `secure_boot_driver_verified`, `secure_boot_fail_closed`.

#### B6. Performance

- [ ] **B6.1 V2. Zero-copy pipes** | Prereqs: A4.5, S2 | Files: `src/pipe.rs` | Done when: pipe read/write sin copia kernel intermedia para buffers alineados.
  - **Descripción:** Optimiza el camino de pipes para que, cuando el buffer del productor o consumidor esté alineado y sea seguro, los datos se pasen por referencia a páginas compartidas o pinneadas en lugar de copiarse byte a byte dentro del kernel. El fallback sigue siendo la copia tradicional si no se cumplen las condiciones de seguridad.
  - **Criterio:** Un pipeline con buffers alineados evita al menos una copia completa entre procesos.
  - **Tests:** `pipe_zero_copy_aligned_buffers`, `pipe_zero_copy_fallback_copy`, `pipe_zero_copy_integrity`.
- [ ] **B6.2 V3. Copy-on-write fork** | Prereqs: A1.5 | Files: `src/memory/cow.rs`, `src/syscall.rs` | Done when: `sys_fork` duplica address space con COW pages.
  - **Descripción:** Implementa `sys_fork` como clonación perezosa del espacio de direcciones: el hijo comparte páginas con el padre en modo read-only hasta que cualquiera escribe, momento en el que se dispara un page fault y se materializa una copia privada. Esto reduce mucho el coste de crear procesos y prepara el terreno para shells y utilidades estilo UNIX.
  - **Criterio:** Padre e hijo comparten memoria al nacer y divergen solo al escribir.
  - **Tests:** `cow_fork_shares_pages`, `cow_write_triggers_copy`, `cow_fork_isolated_writes`.



#### B9. Shell command migration Ring 0 → Ring 3

Migrar los comandos del kernel shell Ring 0 (`src/shell/commands/`) a `.NXE` en Ring 3. El Ring 0 solo mantiene `RUN` (bootstrap) y `CRASH` (crash dump). Se añaden 11 syscalls (RAX 29–39). Post-migración: toda interacción de operador pasa por `neoshell.nxe` vía PATH dispatch.

**Nota sobre HELP:** coexisten Ring 0 (`commands/help.rs`) y Ring 3 (`corehelp.nxe`). El Ring 0 se transforma en un stub mínimo que solo muestra los comandos kernel (RUN, CRASH) y redirige al usuario a usar `HELP` desde `neoshell` para la lista completa de .NXE disponibles.

**Permanecen en Ring 0:**
- **RUN** — bootstrap loader necesario para lanzar el primer binario Ring 3 (NeoInit/neoshell) desde el kernel.
- **CRASH** — crash dump management; es inherentemente kernel-level (manejo de page faults, triple faults, stack traces).

**Completados (movidos a COMPLETED):** HELP, SET, EXIT, PS, KILL, PRI, DRIVES, KEYB, CALL, LABEL, FSCK, NDREG, LOADNEM — todos migrados a Ring 3 como `.NXE`.

**Permanecen en Ring 0:**
- **RUN** — bootstrap loader necesario para lanzar el primer binario Ring 3 (NeoInit/neoshell) desde el kernel.
- **CRASH** — crash dump management; es inherentemente kernel-level (manejo de page faults, triple faults, stack traces).

---

### 🔷 X7. NeoDOS Object Manager (Ob) — Unificación de Handles, KOBJ, URN y Seguridad

> **NT Reference:** Ob (Object Manager) — `ObOpen`, `ObCreate`, `ObQueryInfo`, `ObReferenceObject`
> **Documento de diseño:** [`docs/OBJECT_MANAGER_ARCHITECTURE.md`](OBJECT_MANAGER_ARCHITECTURE.md)
> **Prerequisitos:** v0.40 (buddy >4GB, user window 32MB, static buffers → heap), A0.4 (dynamic handle table), NT5 (Ob namespace + symlinks), NT6 (SID/ACL/SeAccessCheck), X5 (work queue), X6 (IRP system)
> **Versión objetivo:** v0.41–v1.0
> **Estado:** 🔴 No iniciado

#### ⚠️ Problema Actual

El kernel de NeoDOS tiene **tres abstracciones paralelas** para gestionar recursos del sistema:

| Abstracción | Propósito | Limitaciones |
|------------|-----------|-------------|
| **HandleEntry** (handle.rs) | Referencia por proceso a archivos, pipes, dispositivos | Tipos hardcoded (10 valores), `id` polimórfico, sin metadatos |
| **KObjEntry** (kobj/mod.rs) | Registry global de metadata de objetos | Solo metadatos — no hay operaciones ni refcount real |
| **UrnHandle** (urn/mod.rs) | Acceso unificado tipo URL | Paralelo a handles, schemes sin implementar |

**Consecuencias:**

1. **Acoplamiento:** Cada syscall que opera sobre un handle (close, dup2, exit, kill, pipe) debe conocer los tipos internos y despachar manualmente — 5+ handlers modificados para añadir un nuevo tipo.

2. **Duplicación de cleanup:** La liberación de recursos se distribuye entre `handler_exit` (~230 líneas), `kill_pid` (~70 líneas), `sys_close`, y `PIPE_MANAGER`.

3. **Sin query/estandarización:** Dado un handle fd, no hay forma de preguntar "¿qué tipo de objeto es?" o "¿cuáles son sus metadatos?" sin conocer internamente el tipo.

4. **Sin seguridad por objeto:** SeAccessCheck existe pero solo se usa en syscall 50. No se verifica acceso en `sys_open`, `sys_readfile`, `sys_writefile`.

5. **URN desconectado:** No hay camino de UrnHandle → HandleEntry → fd real.

#### 🏗️ Diseño Propuesto

**Object Manager (Ob):** Abstracción única que unifica handles, objetos, seguridad y namespace.

```
ObObject (kernel object)
├── id: ObId (hereda KObjId)
├── type: ObType (Process, Thread, File, Pipe, Device, ...)
├── name: [u8; 256]
├── sd: SecurityDescriptor
├── refcount: u32
├── ops: &'static ObOperations (vtable polimórfica)
└── context: *mut c_void (back-pointer al recurso real)

ObHandle (per-process)
├── object_id: ObId → referencia a ObObject
├── access_mask: u32 (READ|WRITE|EXEC|DELETE)
└── offset: u64

ObDirectory (namespace)
├── \Global\, \Device\, \Driver\, \FileSystem\, \Registry\
└── \Process\ (virtual: PID-indexed)
```

**Syscalls nuevas (RAX 60–65):**

| RAX | Syscall | Args | Descripción |
|-----|---------|------|-------------|
| 60 | `sys_ob_open` | RBX=path, RCX=access | Open named object → handle |
| 61 | `sys_ob_create` | RBX=path, RCX=type, RDX=attrs | Create named object |
| 62 | `sys_ob_query_info` | RBX=fd, RCX=class, RDX=buf, R8=len | Query object metadata |
| 63 | `sys_ob_set_info` | RBX=fd, RCX=class, RDX=buf | Set object metadata |
| 64 | `sys_ob_enum` | RBX=dir_fd, RCX=buf, RDX=max | Enumerate directory |
| 65 | `sys_ob_wait` | RBX=count, RCX=handles, RDX=type, R8=to | Wait on objects |

**Syscalls existentes que migran (wrappers internos):**

| RAX | Syscall | Pasa a ser wrapper de | Fase |
|-----|---------|----------------------|------|
| 10 | `sys_open` | ObOpen + ObQueryInfo | v0.45 |
| 11 | `sys_readfile` | ObQueryInfo → vfs read | v0.45 |
| 12 | `sys_writefile` | ObQueryInfo → vfs write | v0.45 |
| 5 | `sys_pipe` | ObCreate(Pipe) + ObOpen x2 | v0.45 |
| 13 | `sys_close` | ObClose(handle) | v0.41 |
| 8 | `sys_readdir` | ObEnum(dir) | v0.45 |
| 9 | `sys_waitpid` | ObWait(Process, CHILD_EXIT) | v0.45 |
| 22 | `sys_thread_create` | ObCreate(Thread) + ObOpen | v0.45 |
| 23 | `sys_thread_join` | ObWait(Thread, THREAD_EXIT) | v0.45 |
| 48 | `sys_kobj_enum` | ObEnum(global) | v0.45 |

**Syscalls que permanecen (no migran):** exit, yield, getpid, spawn, chdir, getcwd, brk, mmap, munmap, loadlib, get_cpuinfo, get_version, get_datetime, get_meminfo, poweroff, cursor_blink — son demasiado específicas o internas para abstraer como objetos.

#### 📋 Dependencias

| # | Dependencia | Tipo | Estado |
|---|-------------|------|--------|
| 1 | Handle table dinámico (A0.4) | Hard | Completado v0.40 |
| 2 | KOBJ registry + namespace (NT5) | Hard | Completado v0.40 |
| 3 | SID/ACL/SeAccessCheck (NT6) | Hard | Completado v0.40 |
| 4 | KWait Unified Wait Engine | Soft | v0.42 |
| 5 | SecurityDescriptor en objetos | Soft | NT6 ya existe, falta integrar |

#### 📐 Arquitectura de Módulos

```
src/
├── object/                    # NUEVO: Object Manager
│   ├── mod.rs                # ObObjectTable, ObObject, ObOperations
│   ├── handle.rs             # HandleEntry refactorizado (ObId + access_mask)
│   ├── types/                # Implementaciones de ObOperations por tipo
│   │   ├── process.rs        # Process operations
│   │   ├── thread.rs         # Thread operations
│   │   ├── file.rs           # File operations
│   │   ├── pipe.rs           # Pipe operations
│   │   ├── device.rs         # Device operations
│   │   └── driver.rs         # Driver operations
│   ├── security.rs           # Integración SeAccessCheck en ObOpen
│   └── namespace.rs          # ObDirectory (refactor de kobj/namespace.rs)
│
├── kobj/                      # RENOMBRAR → object/ (legacy wrappers)
│   └── mod.rs                # Stub de compatibilidad
│
├── handle.rs                  # REFACTOR → object/handle.rs
│
└── urn/                       # REWRITE → frontend de Ob
    └── mod.rs                # ObOpen + ObEnum tras Ob
```

#### 📦 Impacto en Archivos Existentes

| Archivo | Cambio | Líneas estimadas |
|---------|--------|-----------------|
| `src/syscall/mod.rs` | 5 handlers refactorizados, 6 nuevos | +400/−150 |
| `src/handle.rs` | Añadir object_id, mantener compat | +20/−10 |
| `src/kobj/mod.rs` | Refactor como ObObjectTable + stubs | +200/−100 |
| `src/kobj/namespace.rs` | Integrar con ObDirectory | +50/−30 |
| `src/urn/mod.rs` | Rewrite como frontend de Ob | +80/−100 |
| `src/scheduler/mod.rs` | EPROCESS/KTHREAD register en Ob | +30/−10 |
| `src/pipe.rs` | Pipe operations para Ob | +40/−10 |
| `src/security/access.rs` | Integrar check en ObOpen | +30/−5 |

**Total estimado:** ~850 líneas nuevas, ~415 modificadas, ~255 eliminadas.

#### 🛡️ Seguridad

Cada ObObject tiene un `SecurityDescriptor`. `ObOpen` ejecuta `SeAccessCheck` antes de crear el handle. El handle almacena la `access_mask` concedida. Las operaciones posteriores verifican que la mask del handle cubra el acceso solicitado.

Flujo:
```rust
ObOpen(path, desired_access)
  → ob_resolve_path(path) → ObId
  → se_access_check(token, &obj.sd, desired_access)
  → if GRANT: handle = HandleTable::push(ObId, desired_access)
  → if DENY: return ACCESS_DENIED
```

#### 🧪 Tests Planificados

| Categoría | Tests | Descripción |
|-----------|-------|-------------|
| Object lifecycle | 6 | create, open, close, refcount, double-close, not-found |
| Operations | 8 | query_info (4 types), set_info, enum, wait |
| Namespace | 5 | resolve, create dir, symlink resolve, case-insensitive, virtual Proc |
| Security | 6 | access GRANT, DENY, admin bypass, invalid mask, token-check, handle-mask |
| Legacy compat | 8 | sys_open wrapper, sys_close wrapper, sys_readdir, sys_pipe, sys_readfile, sys_writefile, sys_waitpid, sys_thread_join |
| URN integration | 4 | file → Ob, device → Ob, registry stub, roundtrip |
| Stress | 3 | 1000 objects, concurrent open/close, mixed types |
| **Total** | **40** | |

#### 📈 Métricas Objetivo (v1.0)

| Métrica | Actual | Objetivo |
|---------|--------|----------|
| HandleEntry tipo-seguro | ❌ (kind hardcoded) | ✅ (ObId ref) |
| KOBJ + handles unificados | ❌ | ✅ |
| Security en open | ❌ (solo syscall 50) | ✅ (todo acceso) |
| URN funcional | Parcial (file + device) | Full (all schemes) |
| Tipos de objeto | ~8 implícitos | 15+ explícitos |
| Syscalls Ob | 0 | 6 nuevas |

#### ⚠️ Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|-------------|---------|------------|
| Performance regression en hot path (read/write) | Media | Alto | Benchmarks antes/después, eliminar indirección en hot path si es necesario |
| Rotura de compatibilidad con binarios existentes | Baja | Alto | Wrappers mantienen firma exacta, no eliminar syscalls legacy |
| Complejidad excesiva del dispatch | Media | Medio | ObOperations es opcional — los objetos simples pueden tener ops = None |
| Deadlock en refcount cross-module | Baja | Alto | Refcount atómico, sin locks en hot path |
| Migración demasiado lenta | Media | Medio | Priorizar por impacto: close primero (fácil), pipe después (complejo) |

---

#### B7. Experimental

- [ ] **B7.1 E4. Full GUI system** | NT: Desktop Window Manager | Prereqs: B4.10 | Files: `userbin/gui/` | Desktop con iconos, menú, ventanas redimensionables.
- [ ] **B7.2 E5. Advanced secure boot (TPM)** | NT: BitLocker / TPM | Prereqs: B5.3 | Files: `src/boot/tpm.rs` | Medición PCR + sealed storage.
- [ ] **B7.3 E6. Package manager** | NT: MSI / Windows Update | Prereqs: B5.1, A5.1 | Files: `userbin/neopkg/` | Install/remove paquetes `.NPK` firmados.
- [ ] **B7.4 T4. Time-travel debugging** | NT: WinDbg time travel | Prereqs: A3.2, B1.1 | Files: `src/debugger/timetravel.rs` | Replay de trace buffer en debugger.
- [ ] **B7.5 T5. Live kernel patching** | NT: Windows Hotpatch | Prereqs: A2.4, A3.2 | Files: `src/patch/mod.rs` | Hot-patch de función kernel sin reboot.
- [ ] **B7.6 T2. Distributed NeoDOS nodes** | NT: DFS | Prereqs: B3.2 | Files: `src/cluster/` | 2 nodos QEMU se descubren y comparten FS read-only.

---

| # | Problema | Fase | NT ref | Estado | Riesgo si no se hace |
|---|----------|------|--------|--------|---------------------|
| 1 | Frame allocator O(n), 4 GB max | A0.1–A0.2 | Mm | Completado | — |
| 2 | Direcciones fijas, solapamiento | A0.3 | Mm | Completado | — |
| 3 | Handle table fijo (16) | A0.4 | Ob handles | Completado | — |
| 4 | Thread model ausente (1 hilo/proceso) | A1.5 | KTHREAD | Completado | sys_thread_create/join, per-process KTHREADs |
| 5 | Scheduler monoprocesador | A1.1–A1.2 | Ps | Completado | Per-CPU run queues, work stealing SMP |
| 6 | Slab allocator lock global | A1.3 | Lookaside | Completado | Throughput no escala con CPUs |
| 7 | Sin IPI / TLB shootdown | A1.4 | KeIpi | Completado | Data corruption en SMP |
| 8 | IRQL ausente (solo cli/sti) | A2.4 | IRQL | Completado | Per-CPU IRQL levels, IrqMutex, INV-14 page fault check |
| 9 | DPC ausente (work queue parche) | A2.5 | DPC | Completado | Per-CPU DPC queues, SPSC ring buffer, DIRQL→DISPATCH dispatch |
| 10 | PCI port I/O asume x86 | A2.1 | HAL | **Completado** | ECAM MMIO (MCFG) + PIO fallback |
| 11 | PIC legacy como default | A2.2 | IOAPIC | **Completado** | IOAPIC init (MADT), PIC disable, MSI-X |
| 12 | HAL mezcla raw y safe | A2.3 | HAL | COMPLETED | asm confinado a hal/ |
| 13 | Sin crash dump ni recovery | A3.1–A3.3 | Bugcheck | Completado | CrashDumpHeader 16 KB, stack walk, serial dump |
| 14 | SEH ausente | A3.4 | SEH | Completado | TEB exception handler chain, sys_set_exception_handler |
| 15 | Stack unwinding inexistente | A3.2 | KD | Pendiente | Sin backtrace |
| 16 | Shell en Ring 0 | A4.7 | CSRSS | Completado | neoshell.nxe en Ring 3, Ring 0 solo RUN/CRASH |
| 17 | NeoInit no implementado | Z1 | smss.exe | Completado | Doc/código divergen |
| 18 | Syscall dispatch manual | A4.2 | SSDT | Completado | SSDT table-based dispatch con permission check |
| 19 | ELF loader sin validación | A4.3 | Ldr | Completado | Triple fault con binarios maliciosos |
| 20 | APC ausente | A4.5 | APC | Completado v0.34.0 | I/O completion en contexto incorrecto |
| 21 | Input sin multiplexión | A4.4 | ConDrv | Pendiente | No escalar a múltiples terminales |
| 22 | FAT32 + NeoFS duplicados | A5.1 | IoStack | Completado | Ambos usan IoStack para I/O |
| 23 | Ob flat (no namespace) | NT5 | Ob | Completado | Hardcode C:, sin symlinks |
| 24 | SRM ausente | NT6 | Se | Completado | SID, Token, ACL, SeAccessCheck implementados |
| 25 | Registry flat → cell-based hive + Ob integration | B2.1–B2.5 | Cm | Pendiente | Sin config jerárquica transaccional, notificaciones, ni load/unload |
| 26 | Handles/KOBJ/URN/security no unificados | X7 | Ob | Pendiente | Tipos hardcoded, dispatch manual, cleanup duplicado, sin security en objetos |

---

## Referencias

- [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md) — invariantes MUST/MUST NOT
- [AGENTS.md](../AGENTS.md) — build, test, convenciones de commit
