# NeoDOS — Roadmap v4.1 (Vision Arquitectonica)

> This file documents pending improvements and roadmap items for NeoDOS. This document serves as the central roadmap for NeoDOS, capturing all pending improvements, milestones, and architectural tasks. Each entry specifies an ID, related source files, prerequisites, acceptance criteria, and associated tests, providing clear guidance and traceability for developers.

> Version actual: v0.44.3 (528 kernel tests + 27 user-mode binaries, full Ob migration + A4.4 Input + B4.5 VTs).
> Objetivo: v1.0 — executive NT-like arquitectonicamente solido.
> **GUIA:** Leer [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) antes de planificar cualquier cambio.
> Fuente de verdad arquitectonica: [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md)
> Ultima revision: Junio 2026.

**Progreso:** 169 / ~177 items completados (+6 planificados: v0.46+ milestones). Proximo milestone: **v0.46** (Device Tree, VirtIO).

---

## Reglas de ejecucion

1. Una fase no empieza hasta que sus prerequisitos esten marcados **[COMPLETED]**.
2. Cada item pendiente incluye: ID, equivalente NT, archivos, prereqs, criterio de aceptacion, tests.
3. Al completar un item: moverlo a COMPLETED, actualizar `CHANGELOG.md`, `AGENTS.md` y `ARCHITECTURE_SOURCE_OF_TRUTH.md` si cambia un contrato.
4. Validar antes de cerrar: `cargo build` en `neodos-kernel/` + `python3 scripts/auto_test.py` + `scripts/check_deps.py`.

### Checklist por item completado

- [ ] Codigo implementado
- [ ] Tests en `testing.rs` (minimo 1 por invariante)
- [ ] `auto_test.py` pasa
- [ ] `check_deps.py` pasa
- [ ] `CHANGELOG.md` actualizado
- [ ] `AGENTS.md` / `ARCHITECTURE_SOURCE_OF_TRUTH.md` si cambia contrato

---

## COMPLETED (169 items)

### Boot & Core Kernel
1. **x86_64 boot** — entry `_start` en 0x200000, long mode via UEFI bootloader.
2. **GDT/IDT/PIC** — segmentos Ring 0/3, IDT 256 entradas, PIC remapeado IRQ 32–47.
3. **Identity paging 4 GiB** — paginas enormes 2 MB, identidad hasta 4 GB.
4. **Heap allocator** — 16 MB @ 0x1000000, `linked_list_allocator`, Box/Vec/String.
5. **A3. Kernel slab allocator** — 9 size classes (8B–2KB), O(1) alloc/free via per-slot free lists on 4 KB slab pages. Uses `hal::alloc_page()` for page allocation. Falls through to linked-list allocator for >2 KB or >16-byte alignment. 9 self-tests.
6. **A2. Scheduler prioritario** — 4 niveles de prioridad (HIGH/ABOVE_NORMAL/NORMAL/IDLE), time-slicing dinamico (400/200/100/50 ticks), preemption desde Ring 3, aging cada 100 ticks para evitar starvation. 7 tests.
7. **A5. Global page cache (base)** — `buffer/page_cache.rs`: central 4 KB page cache (512 entries x 4 KB = 2 MB) for filesystem file data I/O and mmap file-backed pages. LRU eviction with dirty write-back. Indexed by `(inode, block_num)` with stored `data_lba` for safe flush. Timer-driven flush via `NEED_PAGE_CACHE_FLUSH`. 8 unit tests.
8. **PS/2 keyboard driver** — IRQ1, ring-buffer lock-free 1024 bytes.
9. **Serial console** — COM1, `serial_print!`/`serial_println!`.
10. **Framebuffer console** — GOP 1280x800, font VGA 8x16, `println!`.
11. **X1. Kernel Object Manager (KOBJ)** — `src/kobj/mod.rs`: unified kernel object system with reference counting and common metadata. 64-slot registry, KObjType enum. 8 unit tests.
12. **X5. Deferred work queues** — `src/work_queue.rs`: bottom-half system for deferred execution outside IRQ context. Two-level architecture (high/low priority). Lock-free SPSC ring buffer (64 slots per level). 6 tests.
13. **X6. Async I/O (IRP system)** — `src/irp/mod.rs`: unified I/O Request Packet model. Global 64-slot pool, `IrpQueue` per-device (32 entries), completion callbacks via work queue, scheduler integration. 11 tests.
14. **V1. Global page cache (advanced)** — `src/buffer/page_cache.rs`: hash map O(1) index for `(inode, block_num)` lookups. LRU doubly-linked list for O(1) access updates. Adaptive readahead. 13 tests.
15. **MSI/MSI-X** — `src/interrupts/msi.rs` (232 lines): MSI and MSI-X interrupt support. Direct mode (kernel port I/O) and Delegated mode (Event Bus to `pci.nem`). 256-entry vector allocator. Dynamic IDT dispatch via `msi_dispatch`. Integrated with PCI and NVMe.
16. **C3. HPET / APIC timers** — `src/timers/hpet.rs`, `src/timers/apic.rs`: HPET 1 KHz periodic mode with legacy replacement to IRQ0. Local APIC timer calibrated against HPET, activated as primary source. APIC mode disables HPET legacy replacement and masks PIC IRQ0. Fallback to PIT 18.2 Hz. `sleep_hint()` uses HPET counter.
17. **ASLR v1 (v0.44)** — PIE user binaries (ET_DYN) loaded at random slot base addresses via RDRAND/TSC entropy. `src/elf.rs`: `load_offset` parameter, RELA relocation support (R_X86_64_RELATIVE). `src/arch/x64/paging.rs`: ASLR-aware slot allocator. All 27 user binaries compiled as PIE.

### Storage
17. **P1. Default file permissions by context** — `NeoDosFs::default_perms_for_filename()` asigna permisos RWXSD segun extension.
18. **ATA PIO driver** — read/write por puertos 0x1F0/0x3F6.
19. **AHCI driver** — DMA polling, PRDT scatter-gather, ATA + ATAPI.
20. **ATA bus-master DMA** — PCI BAR4, buffers alineados, hasta 8 sectores.
21. **NeoFS** — filesystem propio: inodos 256 B, bloques 4 KB, timestamps, permisos, directorios, 75 tests.
22. **FAT32 read** — lectura de sector absoluto desde ESP.
23. **GPT partition parsing** — detecta particion NeoDOS por UUID.
24. **Unified GPT disk image** — `disk_image.img` (ESP FAT32 + NeoDOS FS).
25. **VFS layer** — `FileSystem` trait, `resolve_path()`, FAT32 + NeoDOS + ISO9660.
26. **ISO9660 read** — driver completo con PVD, extent cache, Joliet.
27. **BlockDevice abstraction** — `BlockDevice` trait, `StorageManager` unifica ATA/AHCI.
28. **NVMe driver** — `src/drivers/nvme.rs` (837 lines): NVMe block driver as kernel built-in. PCI detection (class 0x01, subclass 0x08, prog-if 0x02). Admin Queue + I/O Queues with doorbell registers. NVM Read/Write with PRP scatter-gather. Integrated as highest boot priority.

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
38. **Driver Certification Pipeline v1** — estado Loaded->Initialized->Registered->Bound->Active, state machine con transiciones estrictas. 21 tests.
39. **A4. Memory-mapped files** — `MmapRegion` + VMA list per-process, sys_mmap lazy (RAX=19), sys_munmap (RAX=20). 6 tests.
40. **S2. IPC / Pipes** — `src/pipe.rs`: PipeManager con 16 buffers de 4 KB. Per-process handle table dinamico. Syscalls: `sys_pipe` (RAX=5), `sys_dup2` (RAX=6). Blocking reads via `ProcessState::Blocked`. 13 tests.
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
57. **PCI NEM driver** — `drivers/pci/` NEM v3 (SYSTEM). Logs devices, config read/write via events 0x1000-0x1003.
58. **A10. PCIe bus enumeration** — Recursive bridge detection, secondary bus scanning. 3 tests.
59. **A6. ATA NEM standalone driver** — `drivers/ata/` NEM v3 (SYSTEM). Primary+secondary channels, NemBlockDevice registration.
60. **A11. AHCI NEM standalone driver** — `drivers/ahci/` NEM v3 (SYSTEM). DMA polling, ATA+ATAPI, PRDT up to 8 entries.
61. **A12. BootAhci kernel stub** — `boot_ahci.rs` early-boot AHCI. Single port, single command slot, 8-sector PRDT.
62. **X3. Capability system** — `src/drivers/caps.rs`: 64-bit capability bitmap per driver (11 flags). Category inheritance. 11 tests.
63. **Demand paging (4 KB)** — frame allocator, split_2mb, heap page fault handler.
64. **sys_brk / sys_mmap** — ajuste program break, asignacion zero-filled.
65. **ELF64 loader** — `src/elf.rs`: PT_LOAD segment loading, 7 tests.
66. **User-mode processes** — IRETQ a Ring 3, EXIT_RSP/EXIT_RIP, scheduler add_ring3_process.
67. **Kernel private stacks** — TSS.RSP0 por proceso, actualizado en cada context switch.
68. **Syscall table (INT 0x80)** — 22 syscalls: exit, write, yield, getpid, read, waitpid, open, readfile, writefile, close, chdir, getcwd, brk, mmap, munmap, pipe, dup2, loadlib.
69. **Scheduler blocking** — ProcessState::Blocked, wake_waiters(), idle HLT.
70. **S6. libneodos** — `libneodos/`: standard library para Ring 3 Rust processes. Syscall wrappers via `int 0x80`. IO/FS/Mem modules. `print!`/`println!` macros.
71. **301 kernel self-tests** — 36 suites, comando `test`.
72. **5 user-mode test binaries** — HELLO.NXE, SYSTEST.NXE, FILETEST.NXE, ALLTEST.NXE, TEST.NXE.
73. **Command history** — buffer circular 32, up/down navegacion.
74. **TAB autocomplete** — comandos built-in + archivos del directorio actual.
75. **Keyboard layouts** — KBDUS.klc / KBDSP.klc compilados en build-time.
76. **Shell commands basicos** — HELP, DATE, TIME, VER, DEL, REN, RD, SHUTDOWN, EXIT, LOAD.
77. **S1. Estabilizar syscall ABI** — `SyscallNum` enum, `SyscallError` (16 codes), `err_to_u64()`, `validate_abi()`.
78. **B6b. Shared library system (libneodos NXL)** — libneodos como NXL standalone con `AbiTable`. Slot 0 en `0x1e000000`. Auto-load en PHASE 3.86.
79. **Multi-NXL system** — `sys_loadlib` (RAX=21), `LOADLIB` command. libmath.nxl en slot 1 (`0x1e040000`).
80. **X4. Driver Isolation Layer** — `src/drivers/isolation.rs`: 16 MB region (0x30000000-0x31000000), 16 x 1 MB slots. Pointer validation. Sandbox mode. 12 tests.
81. **W2. Hot reload drivers** — `src/drivers/hotreload.rs`: runtime unload/reload. State machine: Active->Unloading->Unloaded->Loaded. EVENT_DRIVER_UNLOAD with timeout. 11 tests.
82. **TEST.EXE — libmath.nxl self-test** — `userbin/test/`: LOAD TEST, BASIC ARITHMETIC, EDGE CASES, STRESS TEST (1M iter), DETERMINISM.
83. **CPUTEST.NXE — CPU stress test binary** — `userbin/cputest/`: tests CPU arithmetic, loops, and basic instruction throughput.
84. **A0.1. Buddy system frame allocator** — `src/memory/buddy.rs`: buddy system de 11 ordenes (4 KB -> 4 MB) con free lists O(log n). Bitmap como validacion. `alloc_frames(order)`/`free_frames(addr, order)`.
85. **A0.2. Dynamic PHYS_MEM_END** — `MemoryMap { total_phys, highest_page }` detectado del memory map UEFI. Frame allocator soporta >4 GB sin modificar constantes.
86. **A0.3. Dynamic memory layout manager** — `src/memory/layout.rs`: `MemoryLayout { regions: [MemoryRegion; 32] }` con `reserve_region()` dinamico y verificacion de solapamientos.
87. **A0.4. Dynamic handle table** — `HandleTable` con `Vec<HandleEntry>` interno. Sin limite fijo. 1024+ handles simultaneos por proceso. Migracion transparente.
88. **Architecture Source of Truth** — `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`: Definicion estricta de invariantes y contratos del sistema (Dave Cutler style) para evitar regresiones de diseno.
89. **MCP Server — Kernel Introspection & VFS Analysis** — `scripts/mcp_server/`: MCP protocol server (JSON-RPC 2.0) with 18 tools for AI-assisted kernel debugging, VFS inspection, and architectural validation. Parser offline de NeoDOS FS, NEM v3, ELF64. 3 resources, 3 prompts. `scripts/mcp-server.sh` launcher.
90. **A4.2. Syscall dispatch table (SSDT)** — `src/syscall/mod.rs`: tabla SSDT `[Option<fn(Registers)>; 256]` con `lazy_static!` reemplaza match monolifico. Tabla paralela `[SyscallPermission; 256]` con admin/ring/caps. Admin syscall RAX=50 (`handler_ndreg`). `validate_abi()` itera SSDT para verificar integridad. Dispatcher table-based con permission check antes de cada llamada. 5 tests.
91. **A1.1. Per-CPU data structures (KPRCB)** — `arch/x64/cpu_local.rs`: Kprcb struct (4 KB page per CPU) con cpu_id, apic_id, current_thread, CpuRunQueue (64-entry ring buffer), PerCpuSlabCache[9], interrupt/context_switch/timer_tick counters, exit trampoline via GS. GS-segment accessors. 20 compile-time offset_of! assertions. 5 tests.
92. **A1.1b. MSR access module** — `arch/x64/msr.rs`: rdmsr/wrmsr, typed accessors (read_gs_base, write_gs_base, is_bsp, rdtsc, rdtscp).
93. **A1.1c. SMP boot (INIT-SIPI-SIPI)** — `arch/x64/smp.rs`: AP trampoline (16->32->64-bit), copy to 0x800000, INIT-SIPI-SIPI sequence, per-CPU GS base, AP entry. 3 tests.
94. **A1.2. Per-CPU run queues + work stealing** — CpuRunQueue in KPRCB. schedule() tries local queue -> work stealing -> global fallback. Threads enqueued on creation/wake/timer. IPI_RESCHEDULE (vector 0xF0). 8 total new tests.
95. **Bug fix: handler_exit deadlock** — Double-locking SCHEDULER mutex when calling wake_thread_joiner(). Inlined wake call.
96. **Bug fix: request_exit_to_kernel()** — Read value as pointer instead of using gs_write_u8.
97. **Bug fix: KPRCB offset constants** — 13 offsets 2 bytes too low due to CpuRunQueue alignment. Fixed with compile-time assertions.
98. **A1.3. Per-CPU slab allocator** — `src/slab.rs` rewritten with per-CPU fast path: 32-object hot caches in KPRCB via GS-segment, O(1) alloc/free without locks. `refill_from_global()` / `drain_to_global()` with global Mutex for cross-CPU replenishment. 5 tests.
99. **A1.4. IPI infrastructure + TLB shootdown** — `arch/x64/ipi.rs`: unified IPI module with `send_ipi()`, `send_ipi_mask()`, `send_ipi_all()`. IPI_TLB_SHOOTDOWN (vector 0xF1) with synchronous ACK protocol. IPI_CALL_FUNCTION (vector 0xF2). 5 tests.
100. **A3.1. Crash dump framework** — `src/crash/mod.rs`: 16 KB CrashDumpHeader, stack walk, GPR snapshot, serial output. `CRASH`/`CRASH DUMP` commands. 5 tests.
101. **B8. cpuinfo.nxe — user-mode CPU info binary** — `userbin/cpuinfo/`: uses `libcpu-nxl` NXL via sys_loadlib. Displays vendor, brand, topology, timers, features.
102. **A4.7. neoshell (Ring 3 shell)** — `userbin/neoshell/`: full-featured Ring 3 interactive shell. Built-in commands: HELP, CLS, ECHO, VER, CWD, DIR, SET, POWEROFF, EXIT. `CD` is a separate Ring 3 tool (`cd.nxe`). DIR uses sys_open+sys_readdir. External commands: PATH scan for `.NXE`, sys_spawn + sys_waitpid. TAB completion (built-ins). History (32 entries). Env vars with SET. CWD prompt. Drive change.
103. **NT5.1. Object directory tree** — `src/kobj/namespace.rs`: transforma el registry plano KOBJ en un arbol jerarquico de objetos con `\` como raiz y directorios estandar (`\Device`, `\DosDevices`, `\Global`, `\Driver`, `\FileSystem`, `\Ob`). Lookup de paths tipo NT con `ob_lookup_path()`, nombres de 24 bytes y `BTreeMap` por nodo. 6 tests.
104. **NT5.2. Symbolic links** — `src/kobj/symlink.rs`: objetos simbolicos que apuntan a otros objetos o paths. Resuelve `\DosDevices\C:` y similares con limite de 10 saltos para evitar loops. 5 tests.
105. **NT5.3. Path resolution API** — `src/kobj/lookup.rs`: API unificada `ob_lookup_by_path()` para paths absolutos y relativos, seguimiento de symlinks, normalizacion y errores `OB_*`. 5 tests.
106. **NT5.4. VFS mount points integration** — `src/vfs/mount.rs`: integracion VFS + namespace de objetos, mount points sobre `\Device`, symlink `\DosDevices\C:` y resolucion de paths NT-style hacia NeoFS/FAT32/ISO9660. 5 tests.
107. **B8.1. DIR.NXE** — `userbin/coredir/`: lista directorio con `sys_open` (dir) + `sys_readdir`. Columnas, `/W` (wide), `/P` (pausa).
108. **B8.3. ECHO.NXE** — `userbin/echo/`: imprime argumentos a stdout via `sys_write`.
109. **B8.4. VER.NXE** — `userbin/ver/`: muestra version del sistema via `sys_get_version` (RAX=43).
110. **B8.6. HELP.NXE** — `userbin/corehelp/`: lista .NXE disponibles escaneando `C:\Programs\*.NXE` con `sys_readdir`.
111. **B8.12. DATETIME.NXE** — `userbin/datetime/`: muestra fecha/hora RTC via `sys_get_datetime` (RAX=44). Flags `/D`, `/T`.
112. **B8.13. MEM.NXE** — `userbin/mem/`: muestra uso de memoria via `sys_get_meminfo` (RAX=45). Migrado de Ring 0.
113. **B8.14. TREE.NXE** — `userbin/tree/`: muestra arbol de directorios con `+--`/`\--`, recursivo hasta 6 niveles. Directorios primero, orden alfabetico case-insensitive. Path opcional (default: CWD).
114. **NeoDOS LSP** — `neodos-lsp/`: Language Server Protocol implementation for NeoDOS development. Full LSP features (completion, goto-def, hover, references, rename, documentSymbol, diagnostics). Background indexing with rayon-parallel parsing. NeoDOS-aware: detects syscall handlers, capability constants, shell command entries, driver states. `dashmap`-backed database. 8 MCP tools for AI-level code analysis. `opencode.json` integration. 34 unit tests.
115. **B8.2. TYPE.NXE** — `userbin/coretype/`: muestra contenido de archivo con `sys_open` + `sys_readfile`. Buffer 512 B.
116. **B8.5. CLS.NXE** — `userbin/corecls/`: limpia pantalla (ANSI escape `\x1b[2J\x1b[H`).
117. **B8.7. COPY.NXE** — `userbin/corecopy/`: copia archivo con `sys_open` + `sys_writefile`. Buffer 4 KB.
118. **B8.8. DEL.NXE** — `userbin/coredel/`: elimina archivo via `sys_unlink`.
119. **B8.9. REN.NXE** — `userbin/coreren/`: renombra via `sys_rename`.
120. **B8.10. MD.NXE** — `userbin/coremd/`: crea directorio via `sys_mkdir`.
121. **B8.11. RD.NXE** — `userbin/corerd/`: elimina directorio vacio via `sys_rmdir`.
122. **B4.1. PATH resolution** — `userbin/neoshell/`: busqueda de `.NXE` en PATH. neoshell itera directorios PATH y ejecuta via `sys_spawn`. Prioridad `.NXE` > `.COM` > `.EXE`.
123. **B8.15. Build + integracion** — `scripts/create_neodos_image.py` compila todos los coretools y neoshell, los copia a `C:\Programs\`.
124. **NT6.1. SID + Access Token** — `src/security/token.rs`, `src/security/sid.rs`: Define la identidad de seguridad de cada proceso y thread mediante SID y token. Token admin por defecto para boot, heredado en spawn. Tests: `token_inherit`, `sid_format`, `token_admin_boot_default`.
125. **NT6.2. ACL/ACE on objects** — `src/security/acl.rs`: Anade descriptors de seguridad a cada objeto del namespace. Define `Ace` (allow/deny, access_mask, SID), `Acl`, `SecurityDescriptor`. Tests: `acl_deny_access`, `acl_allow_access`, `acl_inherit_parent`.
126. **NT6.3. Access check on open** — `src/security/access.rs`: `se_access_check()` compara token SID contra DACL del SD con admin bypass. Tests: `se_access_check_deny`, `se_access_check_allow`, `se_access_check_admin_override`.
127. **NT6.4. Admin vs user token** — `src/security/token.rs`: Separa tokens de sistema y usuario. Syscall 50 requiere admin. 12 tests de seguridad integrados.
128. **NT5.5 Z2. Unified resource namespace (URN) — OB-025 rewrite** — `src/urn/mod.rs`: URN rewrite completo como frontend de Ob. Todos los schemes (`file`, `device`, `registry`, `kobj`) se resuelven mediante `ob_open_path()` en el namespace Ob. `UrnHandle` simplificado a wrapper sobre kernel fd (handle table index). `urn_read`/`urn_write` operan via handle table con VFS. Tests: 15 (8 parse + 2 open error + 1 roundtrip + 3 OB-025 scheme mapping + 1 OB-018 Ob integration).
129. **NT5.6 Z3. Virtual FS objects (K:\ drive)** — `src/vfs/kdrive.rs`: Drive virtual K:\ que expone objetos NT5 internos como archivos de solo lectura via VFS. Directorios: Processes, Drivers, Memory, Interrupts. 12 tests.
130. **A2.1. MMIO ECAM PCI config space** — `src/hal/pci.rs`, `src/drivers/pci.rs`: ECAM-based PCI config space access via MMIO from ACPI MCFG table. Auto-selects ECAM or legacy PIO fallback.
131. **A2.2. IOAPIC + MSI-X como modelo primario** — `src/interrupts/ioapic.rs`, `src/interrupts/msi.rs`: I/O APIC detected from MADT, replaces legacy PIC. MSI-X per-entry table programming. IOAPIC init at PHASE 2.91.
132. **B4.4 B2. ANSI terminal** — `userbin/neoshell/`, framebuffer driver: Emulador de terminal ANSI basico en framebuffer. Interpreta secuencias de escape: color, clear screen, cursor position. Tests: `ansi_color_foreground`, `ansi_cursor_position`, `ansi_clear_screen`.
133. **v0.40 — Buddy bitmap dinamico, User window 32MB, Static buffers->heap** — `src/memory/buddy.rs`: bitmap dinamico (>4GB RAM) en vez de `[u64; 16384]`. `src/arch/x64/paging.rs`, `src/scheduler/address_space.rs`, `src/memory/layout.rs`: user window 4MB->32MB (0x400000..0x2400000), kernel heap reubicado (0x2400000). `kernel.ld`: kernel movido a 0x4000000 (64MB). `src/drivers/boot_ahci.rs`: buffers AHCI heap-allocados. `src/main.rs`: CMD_BUF/BIN_BUF heap-allocados.
134. **v0.41 — Slab<T> contenedor, Scheduler Vec, Pipe buffers dinamicos, ObObjectTable** — `src/slab_container.rs`: Generic Slab<T> contenedor con insert/get/remove. `src/scheduler/mod.rs`: eprocesses/kthreads migrados a Vec dinamico. `src/pipe.rs`: Pipe buffers Box<[u8; 4096]> heap-allocados, MAX_PIPES=16. `src/object/mod.rs`: ObObjectTable base, init_object_manager en boot Phase 2.759, 10 tests. HandleEntry con object_id field. KOBJ delegado en ObObjectTable.
135. **v0.42 — Unified Wait Engine (KWait), ABI Freeze, HandleEntry full Ob integration** — `src/kwait/mod.rs`: KWait engine con WaitReason (7 variantes: PipeRead, IrpComplete, ThreadJoin, ChildExit, Event, Timer, Alertable), `kwait_block()`/`kwait_wake()` unified API, 10 tests. `src/abi_freeze.rs`: Verify frozen event types 0-15, capability flags bits 0-11, IOAPIC API, 4 tests. `src/handle.rs`: Todos los constructores crean objetos Ob via `ob_create_object()`, nuevo metodo `close()` llama `ob_close_object()`, helper methods `is_open()`/`is_pipe()`/etc. Marcas FROZEN v0.42 en eventbus, caps, ioapic. ABI freeze validation en boot Phase 3.9.
136. **v0.43 — SeAccessCheck NT-compatible (ACE order NT-correct)** — `src/security/access.rs`: NT-correct `check_dacl()` evalua primero todos los Deny ACEs, luego todos los Allow ACEs (two-pass). `src/security/acl.rs`: `insert_ace_canonical()` mantiene orden canonico (deny first, allow second). 3 tests nuevos.
137. **v0.43 — sys_poll() (RAX=59)** — `src/syscall/mod.rs`: Nuevo handler `handler_poll()` con PollFd struct (fd, events, revents). POLLIN/POLLOUT/POLLHUP/POLLERR flags. Soporta stdin, stdout/stderr, pipe read/write, files, dirs. SSDT slot 59, permission user-level.
138. **v0.43 — Pipe/IRP protocol freeze** — `src/pipe.rs`: Doc comment con FROZEN ABI v0.43, protocol invariants documentados (read EOF semantics, EPIPE, inc_ref/dec_ref balance, blocking magic 0xFFFF_0000). `src/irp/mod.rs`: Doc comment con FROZEN ABI v0.43, protocol invariants (IRP ID global, pool index id%64, irp_get_params lock discipline, chain semantics).
139. **A3.3. Watchdog subsystem** — `src/watchdog/mod.rs`: Software watchdog basado en HPET. `watchdog_pet()` desde timer tick (1 KHz). 5s timeout -> crash dump con CAUSE_WATCHDOG, EVENT_NMI_WATCHDOG, reset. Re-entry guard MAX_NMI_RECOVERIES=3. 5 tests.
140. **A3.4. SEH + exception dispatcher** — `src/exception/mod.rs`: Mecanismo unificado `exception_dispatch()` para Ring 0 (crash dump+panic) vs Ring 3 (TEB exception handler chain). TEB en 0x7000 con `Teb { teb_self, pid, tid, exception_list }`. sys_set_exception_handler (RAX=29). 5 tests.
141. **B4.2. Shell pipes (`|`)** — `userbin/neoshell/`: pipelines de hasta 16 comandos con pipes nativos via `sys_pipe` + `sys_dup2` + `sys_spawn`. PipeManager con 16 buffers x 4 KB, blocking reads.
142. **B9.1. HELP -> corehelp.nxe** — Ring 0 HELP reducido a stub, `corehelp.nxe` escanea `C:\Programs\*.NXE` buscando marcadores `::HELP::`.
143. **B9.2. SET -> neoshell built-in** — Variables de entorno en Ring 3, Ring 0 SET eliminado.
144. **B9.3. EXIT -> neoshell built-in** — POWEROFF/EXIT en Ring 3 via `sys_poweroff` (RAX=42), Ring 0 EXIT eliminado.
145. **B9.4. PS -> ps.nxe** — Lista procesos via `sys_ob_enum("\Ob\Process")` + `ObQueryInfo(Process)` con datos reales (PID, PPID, prioridad, thread_count, estado). Migrado de `sys_kobj_enum` a Ob.
146. **B9.5. KILL -> kill.nxe** — Termina proceso por PID via `ObOpen` + `ObSetInfo(fd, ProcessTerminate)`. Migrado de `sys_kill_process` a Ob.
147. **B9.6. PRI -> pri.nxe** — Cambia prioridad scheduling via `ObOpen` + `ObSetInfo(fd, ProcessPriority)`. Migrado de `sys_set_priority` a Ob.
148. **B9.8. DRIVES -> drives.nxe** — Lista unidades montadas via `sys_get_drives` (RAX=33). Letra, tipo, etiqueta, tamano. Migrado a Ob en v0.44.2.
149. **B9.10. KEYB -> keyb.nxe** — Cambia layout teclado via `sys_set_keyboard_layout` (RAX=49). US/SP. Migrado a Ob en v0.44.2.
150. **B9.13. CALL -> neoshell built-in** — Ejecuta `.BAT` batch desde Ring 3, replica `commands/call.rs`.
151. **v0.44.1 — libneodos Ob API** — 5 wrappers Ob en `libneodos/src/syscall.rs` con macros asm seguras (temp register copy). `ObBasicInfo`, `ObEnumEntry`, `ObProcessInfo` structs + `ob_access` constants. AbiTable v5 en `libneodos-nxl` y `libneodos/src/export.rs`.
152. **v0.44.1 — ob_open_path auto-create dirs** — `src/object/mod.rs`: `ob_open_path()` crea dir objects on-the-fly para paths namespace que son directorios sin object entry.
153. **v0.44.1 — ob_is_directory()** — `src/kobj/namespace.rs`: publica para detectar directorios namespace sin entry.
154. **v0.44.1 — ProcessTerminate (ObSetInfo class 4)** — `src/syscall/mod.rs`: termina proceso via `ObSetInfo(fd, 4)`. `handler_kill_process` migrable.
155. **v0.44.1 — kobj.nxe migrado a Ob** — usa `ObOpen("\Ob")` + `ObEnum` para mostrar namespace Ob jerarquico.
156. **v0.44.1 — ps.nxe migrado a Ob** — usa `ObOpen("\Ob\Process")` + `ObEnum` + `ObQueryInfo(Process)` por proceso. Datos reales.
157. **v0.44.1 — pri.nxe migrado a Ob** — usa `ObOpen` + `ObSetInfo(ProcessPriority)`.
158. **v0.44.1 — kill.nxe migrado a Ob** — usa `ObOpen` + `ObSetInfo(ProcessTerminate)`.
159. **v0.44.2 — neoshell full Ob migration (OB-040)** — `userbin/neoshell/`: readdir->ob_enum, pipe->ob_create(Pipe), readfile->ob_query_info(ReadContent), spawn->ob_create(Process)+ob_wait, chdir->ob_set_info(SetCwd). All filesystem operations via Ob. Legacy syscalls readfile/writefile/readdir/mkdir/unlink/rmdir/rename/get_volume_label/set_volume_label now disabled in SSDT (None).
160. **v0.44.2 — coredir/tree full Ob migration (OB-041)** — readdir->ob_enum. No legacy syscalls remaining.
161. **v0.44.2 — corecopy/coretype Ob migration (OB-042)** — readfile->ob_query_info(ReadContent), writefile->ob_set_info(WriteContent), unlink->ob_destroy. Legacy readfile/writefile syscalls disabled in SSDT.
162. **v0.44.2 — coredel/coreren/coremd/corerd Ob migration (OB-043)** — unlink->ob_destroy, rename->ob_set_info(VfsRename), mkdir->ob_create(Directory), rmdir->ob_destroy. Legacy unlink/rename/mkdir/rmdir syscalls disabled in SSDT.
163. **v0.44.2 — ndreg/drives Ob migration (OB-044, partial)** — ndreg: ob_open("\Global\Info\Drivers") + ob_query_info(Drivers). drives: ob_open("\Global\Info\Drives") + ob_query_info(Drives). loadnem still pending full Ob migration.
164. **v0.44.2 — datetime/ver/mem/cpuinfo/vol/label Ob migration (OB-045)** — All info syscalls migrated to ob_open("\Global\Info\...") + ob_query_info(). Legacy get_version/get_datetime/get_meminfo/getcpuinfo/get_volume_label disabled in SSDT.
165. **v0.44.2 — sys_ob_destroy RAX 66 (OB-015 extension)** — New syscall handler_ob_destroy: destroys files (VFS remove), directories (VFS rmdir), namespace objects. Registered in SSDT slot 66 with user permission.
166. **v0.44.2 — ob_create(Process) for spawning** — `handler_ob_create` supports type=1 (ObType::Process). spawn via Ob: creates ObObject for process, allocates handle, returns fd. Enables ob_create(Process)+ob_wait pattern. `neoinit` and `neoshell` create child processes via Ob.
167. **v0.44.2 — ob_create(Driver) for driver loading** — `handler_ob_create` supports type=2 (ObType::Driver). Creates ObObject for driver, triggers driver load through the boot loader pipeline. loadnem.nxe uses ob_create(Driver) to load NEM drivers.
168. **v0.44.2 — ob_query_info(ReadContent) class 15** — New info class 15 in `handler_ob_query_info`: reads file content via VFS. Supports both ObObject handles (ObType::Filesystem) and legacy file handles. Used by coretype, corecopy, neoshell (TYPE command).
169. **v0.44.2 — ob_query_info(VolumeLabel) class 16** — New info class 16 in `handler_ob_query_info`: returns volume label string via VFS. Used by vol, label, drives binaries.
170. **v0.44.2 — ob_set_info(WriteContent) class 7** — New info class 7 in `handler_ob_set_info`: writes file content via VFS. Supports both ObObject and legacy handles. Used by corecopy, neoshell (COPY/redirect).
171. **v0.44.2 — ob_set_info(SetCwd) class 8** — New info class 8 in `handler_ob_set_info`: changes current working directory via \Global\Info\Cwd object. Used by cd.nxe and neoshell (CWD command).
172. **v0.44.2 — ob_set_info(SetVolumeLabel) class 9** — New info class 9 in `handler_ob_set_info`: sets volume label via VFS. Used by label.nxe.
173. **v0.44.2 — ob_set_info(VfsRename) class 6** — New info class 6 in `handler_ob_set_info`: renames VFS files/directories. Updates ObObject name to reflect the new path. Used by coreren.nxe.
174. **v0.44.2 — libneodos AbiTable v5 with attrs param** — `libneodos/src/syscall.rs`: all Ob wrappers updated with attrs parameter. New `ObInfoClass` constants (0-16). Wrappers: `ob_open`, `ob_create`, `ob_query_info`, `ob_set_info`, `ob_enum`, `ob_wait`, `ob_destroy`. All 27 user binaries use libneodos Ob API.
175. **v0.44.2 — OB_NAME_LEN 32->128** — `src/object/types.rs`: OB_NAME_LEN increased from 32 to 128. Fixes path truncation for long namespace paths (e.g., "\Global\FileSystem\C:\System\Libraries\libneodos.nxl"). All ObObject name buffers now 128 bytes.
176. **v0.44.2 — SeAccessCheck in ObOpen** — `handler_ob_open` calls `ob_open_path(&path, &token, desired_access)` which performs security check. Token extracted from current EPROCESS, falls back to DEFAULT_ADMIN_TOKEN. AccessDenied returned to user on failure (OB-030).
177. **v0.44.2 — keyb.nxe Ob migration** — ob_open("\Global\Info\Keyboard") + ob_set_info(KeyboardLayout) replaces sys_set_keyboard_layout. Legacy RAX 49 disabled in SSDT.
178. **v0.44.2 — ndreg.nxe Ob migration** — ob_open("\Global\Info\Drivers") + ob_query_info(Drivers) replaces sys_driver_enum. Legacy RAX 56 disabled in SSDT.
179. **v0.44.2 — drives.nxe Ob migration** — ob_open("\Global\Info\Drives") + ob_query_info(Drives) replaces sys_get_drives. Legacy RAX 33 disabled in SSDT.
180. **v0.44.2 — label.nxe Ob migration** — ob_open + ob_query_info(VolumeLabel) + ob_set_info(SetVolumeLabel) replaces sys_get_volume_label/sys_set_volume_label. Legacy RAX 46/54 disabled in SSDT.
181. **v0.44.2 — loadnem.nxe partial Ob migration** — ob_create(Driver) for driver loading. Legacy sys_driver_unload (RAX 58) kept for /U (unload) flag.
182. **v0.44.2 — cmdtest.nxe Ob migration** — All test operations migrated: ob_create(Directory) replaces sys_mkdir, ob_destroy replaces sys_rmdir/sys_unlink, ob_set_info(VfsRename) replaces sys_rename, ob_query_info(ReadContent) replaces sys_readfile.
183. **v0.44.2 — neoinit.nxe Ob spawning** — Uses ob_create(Process) + ob_wait for spawning neoshell with respawn on exit. Legacy sys_spawn (RAX 7) kept for backward compat.
184. **v0.44.3 — A4.4 Input subsystem redesign** — `src/input/` directory con `InputManager`, 4 VT queues (`VtInputQueue` de 4 KB), per-VT input routing. `switch_vt()` via Alt+F1-F4. Console state save/restore per VT (`ConsoleState`), framebuffer shadow redraw (`VtShadowBuffer`). Per-process `vt_num` en EPROCESS, inherited from parent. 8 tests.
185. **v0.44.3 — B4.5 Virtual terminals** — NeoInit spawns shell on VT0. NeoShell banner shows `[VTn]`. `\Global\Info\VtInfo` Ob object for VT number query/set. Syscall 11 (readfile) re-registered in SSDT. handler_read reads from per-VT queue.

---

## ROADMAP PENDIENTE (v0.40 -> v1.0)

> Basado en el analisis completo de `docs/ARCHITECTURAL_VISION.md`.
> **Regla de oro:** No anadir features nuevas antes de completar la fase de maduracion (v0.40-v0.45).
> Cada feature nueva se apoya en abstracciones existentes; si esas abstracciones son fragiles, la feature sera fragil.

---

### Fase 1: Maduracion (v0.40 - v0.45) [COMPLETED]

*Todos los items de la Fase 1 estan completados.*

1. ~~**v0.43** — SeAccessCheck NT-compatible, sys_poll(), Congelar pipe/IRP protocols~~ **COMPLETADO**
2. ~~**v0.44** — ASLR v1 (base aleatoria), Ob syscalls RAX 60-66~~ **COMPLETADO** (v0.44.2: Ob migration completa, todas las syscalls legacy desactivadas)
3. ~~**v0.45** — Ob migration, Device Tree + Resource Manager, Driver state machine freeze~~ **COMPLETADO** (Ob migration completada en v0.44.2; Device Tree y Resource Manager se mueven a v0.46)

---

### Code Quality & Maintenance

### Critical Bugs (SMP-unsafe static mut)

* [ ] **CB1. Fix WAIT_PID static mut SMP-unsafe** | Prereqs: KWait | Files: `src/usermode.rs`
  - **Descripcion:** `WAIT_PID` es un `static mut` en usermode.rs usado para comunicacion entre sys_waitpid y el manejador de terminacion de proceso. En SMP, dos CPUs pueden ejecutar sys_waitpid concurrentemente. Migrar a KWait: `kwait_block(ChildExit(pid))` + `kwait_wake(ChildExit(pid))`.
  - **Severidad:** CRITICO — data corruption en sistemas multicore
  - **Tests:** `smp_waitpid_concurrent`, `smp_waitpid_no_race`

* [ ] **CB2. Fix ISOLATED_REGIONS static mut sin sincronizacion** | Prereqs: -- | Files: `src/drivers/driver_runtime.rs`
  - **Descripcion:** `ISOLATED_REGIONS` es un array estatico mutable accedido desde boot loader, NDREG y hot reload sin Mutex. Envolver en `spin::Mutex<[Option<IsolatedRegion>; MAX_ISOLATED_DRIVERS]>`.
  - **Severidad:** CRITICO — data corruption si 2 CPUs hacen operaciones de driver concurrentes
  - **Tests:** `smp_isolated_region_concurrent_access`

* [ ] **CB3. Fix NXL_REGISTRY static mut sin proteccion SMP** | Prereqs: -- | Files: `src/nxl.rs`
  - **Descripcion:** `NXL_REGISTRY` es array fijo de 8 slots accedido desde sys_loadlib sin sincronizacion. Envolver en `spin::Mutex<[Option<NxlEntry>; 8]>`.
  - **Severidad:** ALTA — dos procesos cargando NXLs concurrentemente pueden corromper el registry
  - **Tests:** `smp_nxl_concurrent_load`

### Documentation & Housekeeping

* [ ] **DH1. Actualizar README.md a v0.44.3** | Prereqs: -- | Files: `README.md`
  - **Descripcion:** README actual muestra v0.39.11 con 320+ tests y 36 syscalls. Actualizar a v0.44.3: 528 tests, 66 syscalls, Ob API, input subsystem, virtual terminals.
  - **Criterio:** README refleja estado real del sistema (version, tests, syscalls, arquitectura)

* [ ] **DH2. Corregir ARCHITECTURE_SOURCE_OF_TRUTH.md** | Prereqs: -- | Files: `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md`
  - **Descripcion:** El documento menciona MAX_PROCESSES como limite fijo, pero el scheduler usa Vec. Menciona 320+ tests (real 528). Boot phases incompletas (falta Phase 3.86 NXL load, Phase 3.9 ABI freeze).
  - **Accion:** Corregir Rule 6.3.1 (MAX_PROCESSES -> Vec dinamico), actualizar test counts, completar boot phase list.

* [ ] **DH3. Completar libneodos syscall wrappers** | Prereqs: -- | Files: `libneodos/src/syscall.rs`
  - **Descripcion:** Faltan wrappers para: `sys_thread_create` (RAX 22), `sys_thread_join` (RAX 23), `sys_sleep_ex` (RAX 41), `sys_poll` (RAX 59), `sys_ob_destroy` (RAX 66), `sys_driver_unload` (RAX 57). Anadir con macros asm igual que los wrappers existentes.
  - **Criterio:** Cada wrapper nuevo tiene test en cmdtest.nxe. 528 kernel tests + cmdtest pasan.
  - **Tests:** 6 nuevos (uno por wrapper)

### Architectural Issues (No criticos)

* [ ] **AI-1. Completar ObInfoClass/ObSetInfoClass enums** | Prereqs: -- | Files: `src/object/types.rs`
  - **Descripcion:** Anadir clases faltantes: `ObInfoClass::ReadContent = 15`, `ObInfoClass::VolumeLabel = 16`, `ObSetInfoClass::ProcessTerminate = 4`, `ObSetInfoClass::VfsRename = 6`, `ObSetInfoClass::WriteContent = 7`, `ObSetInfoClass::SetCwd = 8`, `ObSetInfoClass::SetVolumeLabel = 9`.
  - **Criterio:** Enums reflejan implementacion real del handler. Tests verifican mapping.

* [ ] **AI-2. Consolidar exports duplicados v3loader.rs / hst.rs** | Prereqs: -- | Files: `src/drivers/v3loader.rs`, `src/drivers/hst.rs`
  - **Descripcion:** 7 funciones `hst_*` estan exportadas en ambos archivos. Unificar en hst.rs como unico punto de exportacion. v3loader.rs solo llama a hst.rs.
  - **Criterio:** Zero exports duplicados. Todos los drivers NEM cargan correctamente.

* [ ] **AI-3. Unificar codigos de error ObError y SyscallError** | Prereqs: -- | Files: `src/object/types.rs`, `src/syscall/mod.rs`
  - **Descripcion:** ObError (-1 a -9) y SyscallError (16 codigos) son conjuntos independientes con traduccion manual en handler_ob_*. Unificar en un solo enum NeoDosError reutilizado por ambas capas.
  - **Criterio:** Mapping formal verificado por tests. No hay traduccion manual.

* [ ] **AI-4. Arreglar TOCTOU race en kobj_register** | Prereqs: -- | Files: `src/kobj/mod.rs`
  - **Descripcion:** `kobj_register()` checkea si el object existe (read lock) y luego inserta (write lock) sin atomicidad. Convertir a operacion atomica: `registry.iter_mut().find(|e| e.is_none())` + insert en un solo lock scope.
  - **Criterio:** Dos CPUs registrando el mismo objeto no pueden resultar en duplicados.

* [ ] **AI-5. CQ1. Reorganizar libneodos-nxl en modulos separados** | Prereqs: -- | Files: `libneodos-nxl/src/main.rs` -> `libneodos-nxl/src/{syscall,io,fs,process,mem,info,error}.rs`
  - **Descripcion:** Dividir `libneodos-nxl/src/main.rs` (461 lineas monoliticas) en 7+ modulos separados. Cada modulo agrupa funciones por dominio: `syscall.rs` (raw `int 0x80` wrappers), `io.rs` (stdout/stderr/stdin, _print, _eprint), `fs.rs` (file_open/read/write + sys_mkdir/unlink/rmdir/rename), `process.rs` (pipe/dup2/waitpid/spawn/readdir/chdir/getcwd), `mem.rs` (brk/sbrk/mmap/munmap), `info.rs` (get_version/datetime/meminfo/cpuinfo), `error.rs` (consts + ret helper). `main.rs` solo mantiene `nxl_entry`, el `AbiTable` struct, `EXPORT_TABLE` static, y `nxl_panic`. Zero cambios en ABI: el NXL binario resultante es identico, .export_table en offset 0 con mismos valores. No requiere recompilar user binaries ni cambiar kernel/libneodos/build.
  - **Criterio:** `sha256sum` del NXL antes/despues identica. 528 kernel tests + 27 user binaries funcionan sin cambios.
  - **Tests:** Ninguno nuevo (el binario es identico).

---

### Fase 2: Expansion (v0.46 - v0.50)
*Anadir funcionalidades transformadoras. Ejecucion secuencial dentro de la fase.*

Orden de implementacion dentro de la fase:

1. **v0.46** — Device Tree + Resource Manager completo, PCI auto-vinculacion, VirtIO block driver (BOOT_DRIVER), sys_ioctl()
2. **v0.47** — Networking: NIC driver NEM + TCP/IP stack (B3.1-B3.2)
3. **v0.48** — Async I/O: IOCP v1, sys_accept/send/recv, AHCI NCQ (A5.3), DHCP (B3.3)
4. **v0.49** — ASLR v2 (pila/heap aleatorios), PGO, Benchmarking suite, NTP (B3.4)
5. **v0.50** — Registry hive database (B2.1-B2.5)

> **Regla:** No se pasa a la Fase 3 hasta que v0.50 este completo y todos los tests pasen.

---

### FASE A3 — Fault Tolerance (NT: Bugcheck, KD, SEH)

El kernel actual no sobrevive a fallos estructurados. Ring 3 mata el proceso en cualquier excepcion.

- [ ] **A3.2. Kernel debugger (KD)** | NT: WinDbg kernel-mode debugging | Prereqs: A3.1
  - **Archivos:** `src/debugger/mod.rs`, `src/debugger/breakpoint.rs`, `src/debugger/watchpoint.rs`, `src/shell/commands/debug.rs`, `scripts/kd_client.py` (GDB stub adapter)
  - **Descripcion:** Debugger residente en el kernel para inspeccion interactiva de fallos y ejecucion en vivo. No depende de una GDB externa, pero expone un stub remoto por serial para depuracion desde host cuando haga falta. El objetivo es poder detener el sistema de forma controlada, inspeccionar contexto, modificar puntos de control y reanudar sin perder el estado del bug.
    - **Breakpoints software:** INT3 (0xCC) instruction replacement. `set_breakpoint(addr)` guarda original byte, escribe 0xCC. `#BP` (INT3) handler chequea si breakpoint registrado, pausa kernel si match.
    - **Breakpoints hardware:** 4 registro DR0-DR3 + DR7 (debug control). `set_hw_breakpoint(addr, type: execute|read|write|readwrite, len: 1/2/4/8)` configura DR7. `#DB` (INT1) handler dispara si DR6 flag match.
    - **Pause model:** al dispararse un breakpoint valido, el debugger congela el flujo normal del kernel y entra en estado `Paused`, preservando RIP, RSP, GPRs, CR0-CR4 y el motivo de parada. En ese estado solo se aceptan comandos de depuracion explicitos.
    - **Resume model:** `DEBUG CONTINUE` reanuda exactamente desde la instruccion siguiente al breakpoint o desde el RIP ajustado por watchpoint, sin reentrar en panic ni perder el contexto capturado.
    - **Shell commands:**
      - `DEBUG BREAK <addr>` — set INT3 breakpoint
      - `DEBUG UNBREAK <addr>` — remove
      - `DEBUG WATCH <addr> <type: r|w|rw>` — set hardware watchpoint
      - `DEBUG CONTINUE` — resume ejecucion (solo legal desde breakpoint)
      - `DEBUG REG` — dump GPRs, CR0-4
      - `DEBUG MEM <addr> <len>` — hex dump memoria
      - `DEBUG STACK <depth=16>` — stack trace
      - `DEBUG SCHED` — dump scheduler state (runqueues, current thread)
    - **GDB protocol (serial):** Implementar GDB remote protocol subset (qSupported, vCont, g/G, m/M, Z/z) para que `gdb kernel.elf -ex 'target remote /dev/ttyUSB0'` funcione. El stub solo necesita ser suficiente para stop/resume, lectura de registros y memoria, y gestion basica de breakpoints.
    - **State:** Global `debugger_state: DebuggerState { breakpoints: [Option<BreakpointInfo>; 8], hw_watchpoints: [DrReg; 4], paused_rip: u64, last_stop_reason: StopReason }`. Las estructuras deben vivir en memoria kernel fija y no depender de heap durante la captura.
  - **Criterio:**
    - Breakpoint en `sys_write` entry. Kernel pausa, shell imprime "Breakpoint at 0xXXXX", espera comando.
    - `DEBUG REG` muestra RAX-R15 en ese punto.
    - `DEBUG CONTINUE` reanuda ejecucion (sin panic).
    - Watchpoint en direccion de heap: detiene si algo escribe. Log la instruccion (RIP) que escribio.
    - Un cliente GDB remoto puede conectar, listar registros, leer memoria y continuar sin corromper el estado interno.
  - **Tests:** `kd_breakpoint_set_and_hit`, `kd_breakpoint_invalid_addr`, `kd_watchpoint_write_detect`, `kd_register_snapshot`, `kd_gdb_protocol_qSupported` (5 tests).

---

- ~~**[COMPLETED] A4.4. Input subsystem redesign** | NT: ConDrv (Console Driver) | Prereqs: A4.7~~
  - ~~**Archivos:** `src/input/mod.rs` (reescritura), `src/input/manager.rs` (new), `src/input/vt.rs` (new), integracion `arch/x64/idt.rs` (PS/2 delivery)~~
  - ~~**Descripcion:** Sistema de entrada multiplexado soportando multiples terminales virtuales (VTs) con independencia de input. En vez de enviar bytes directamente al shell activo, el kernel clasifica y enruta el input a una cola por VT, permitiendo que varias sesiones coexistan sin pisarse entre si.~~
    - ~~**Virtual Terminals:** Max 4 VTs (Alt+F1-F4). Cada VT tiene:
      - Input queue (ring buffer 4 KB) independiente
      - Output buffer (attached framebuffer) independiente
      - Foreground pid (proces que recibe input)
      - Session leader (PID 1 NeoInit es sesion leader de todas)~~
    - ~~**Keyboard IRQ (PS/2 IRQ1):** Nueva ruta:
      1. IRQ1 handler lee scancode
      2. Convierte a ASCII (KBDUS/KBDSP layout)
      3. Chequea `active_vt`
      4. Inserta en `vt_queues[active_vt]`
      5. Envia event `EVENT_KEYBOARD_INPUT` al event bus (data0 = byte, data1 = vt_num)~~
    - ~~**VT switching:** Alt+F1 scancode detectado -> `InputManager::switch_vt(1)` -> `active_vt=0` -> framebuffer renderiza VT0, input lo recibe VT0 pid.~~
    - ~~**sys_read(fd=0, buf, len) stdin:** Bloquea en `vt_queues[active_vt].read()` hasta bytes disponibles.~~
    - ~~**Foreground policy:** solo el VT foreground recibe el teclado fisico; los demas conservan su cola y su framebuffer en pausa para poder volver sin perder estado.~~
  - ~~**Criterio:**
    - Alt+F1: pantalla cambia a VT0, teclado entrega a shell en VT0.
    - Alt+F2: pantalla cambia a VT1 (vacia), input sin proc -> silent (no error).
    - Type en VT1, Alt+F1, type en VT0: ambos buffers almacenan independiente.
    - El cambio de VT no altera el proceso foreground salvo que el propio scheduler o shell lo decida.~~
  - ~~**Tests:** `input_vt_switch_framebuffer`, `input_vt_independent_queues`, `input_vt_rapid_switching`, `input_4vt_concurrent_stress`, `input_event_bus_dispatch_vt` (5 tests).~~

---

### FASE A5 — Storage Unification (NT: IoStack)

- [ ] **A5.2. VirtIO block driver (BOOT_DRIVER)**
  Prereqs: A2.1
  * **Archivos:** `src/drivers/virtio_blk.rs` (new, 400-500 lines), integracion `src/drivers/storage.rs`, `src/main.rs` PHASE 3.6 (priority init)
  * **Descripcion:** Controlador de bloques VirtIO para maquinas virtuales QEMU/KVM. Se clasifica como **BOOT_DRIVER**, no como `.NEM`, ya que participa directamente en la cadena de arranque del sistema y debe estar disponible antes del montaje del volumen raiz.
    * **PCI detection:** Bus 0, vendor 0x1AF4 (Red Hat), device 0x1001 (VirtIO Block).
    * **Initialization:**
      1. Read BAR0 (MMIO base)
      2. Write device status: ACKNOWLEDGE | DRIVER
      3. Allocate virtqueue (#0, 32 descriptors)
      4. Register queue physical address
      5. Negotiate legacy/modern features
      6. Write device status: DRIVER_OK
    * **I/O path:** `submit_irp(irp)` ->
      1. Allocate descriptor slot
      2. Fill request header
      3. Configure sector_start, sector_count, buffer address
      4. Notify device (doorbell)
      5. Wait completion (polling or interrupt)
      6. Process used ring
      7. Complete IRP
    * **Supported operations:** READ, WRITE, FLUSH, DISCARD
    * **Storage priority:**
      ```
      NVMe > VirtIO > BootAhci > BootAta
      ```
    * **Boot integration:**
      Available before VFS mount, before NeoInit, before NeoShell, before NEM loader. Used by GPT parser and NeoDOS filesystem loader.
  * **Criterio:**
    * Arrancar NeoDOS en QEMU usando `-drive if=virtio`
    * Deteccion automatica PCI.
    * Inicializacion correcta del dispositivo.
    * GPT parsing via VirtIO.
    * Carga del superblock NeoDOS.
    * Montaje de volumen raiz.
    * Arranque completo de NeoInit y NeoShell.
  * **Tests:**
    `virtio_pci_detect`, `virtio_virtqueue_init`, `virtio_submit_read_write`, `virtio_boot_load_kernel`, `virtio_gpt_parsing`, `virtio_mount_rootfs`, `virtio_boot_neoshell` (7 tests)

- [ ] **A5.3. AHCI NCQ** | NT: Storport Native Command Queuing | Prereqs: A2.2
  - **Archivos:** `src/drivers/boot_ahci.rs` (extend), `src/drivers/ahci/mod.rs` (NEM driver), `src/irp/mod.rs` (tag-based dispatch)
  - **Descripcion:** Native Command Queuing en AHCI permite hasta 32 operaciones simultaneas con finalizacion out-of-order.
    - **NCQ path:**
      1. Host prepara 32 command tables en memoria (FIS buffer per slot).
      2. Escribe descriptores a device: ATA FPDMA QUEUED READ (0x60) / WRITE (0x61)
      3. Device acepta hasta 32 cmds sin esperar completaciones.
      4. Device finaliza out-of-order: escribe SActive register (bit = completado), trigger IRQ.
      5. Host lee Successful NCQ Completion Notification (FIS D2H), extrae tag, localiza IRP via tag.
    - **Tag-based dispatch:** Per-device, map `[Option<IrpId>; 32]` indizado por tag.
    - **Fall back to legacy:** Si device no soporta NCQ (via IDENTIFY), usar single-command path.
  - **Criterio:**
    - 32 read IRPs encolados simultaneamente. Device AHCI completa out-of-order.
    - Time to complete 32 reads: ~0.1 ms (paralelo) vs 3.2 ms (serial). ~30x faster.
    - Stress: NCQ bajo carga, sin comando perdido, IRP_DONE count = 32.
  - **Tests:** `ahci_ncq_32_concurrent_dispatch`, `ahci_ncq_tag_based_completion`, `ahci_ncq_fallback_to_legacy`, `ahci_ncq_out_of_order_completion`, `ahci_ncq_stress_load` (5 tests).

---

### Fase 3: Estabilizacion (v0.51 - v1.0.0)
*Bugfixes, hardening, documentacion, y preparacion para API estable.*

Orden de implementacion dentro de la fase:

1. **v0.51** — sys_fork/clone (bajo demanda), sys_signal minimo, full Input subsystem (A4.4)
2. **v0.52** — Stack de red completo (UDP, DNS, DHCP), TFTP/NFS basico, Virtual Terminals (B4.5)
3. **v0.53** — Rendimiento: per-CPU heaps NUMA-aware, scheduler lock-free, zero-copy pipes (B6.1), COW fork (B6.2)
4. **v0.54-v0.59** — Documentacion API completa, test coverage >95%, fuzzing, module signatures (B5.1), secure boot (B5.3)
5. **v1.0.0** — Primera API estable. Todo lo anterior debe estar COMPLETED.

---

### FASE B — Features (userland + servicios)

Prereqs globales: A4.7 minimo para items userland; NT5/NT6 para items de seguridad.

#### B1. Tracing & Observability

- [ ] **B1.1 Y1. Kernel tracing infrastructure** | Prereqs: A2.4 | Files: `src/trace/mod.rs`
  - **Descripcion:** Ampliar el `TraceBuffer` existente (1024 entries, lock-free ring buffer en `trace.rs`) con trace points registrables dinamicamente. Actualmente el buffer soporta 7 tipos de evento (`ContextSwitch`, `SyscallEnter/Exit`, `IrqEnter/Exit`, `SchedDecision`, `Panic`) con 4 argumentos u64 por entry. Esta mejora anade: registro dinamico de trace points por subsistema (scheduler, VFS, memory, drivers), filtrado por categoria/nivel, y dump formateado via serial con timestamps HPET.
  - **Criterio:** Trace points registrables desde cualquier modulo kernel. Dump via serial legible. Filtrado por categoria funcional.
  - **Tests:** `trace_register_dynamic_point`, `trace_filter_by_category`, `trace_dump_serial_format`.

- [ ] **B1.2 Y2. NeoTrace system** | Prereqs: B1.1 | Files: `userbin/neotrace/`
  - **Descripcion:** Comando de shell Ring 3 `NEOTRACE` que expone la infraestructura de tracing (B1.1) al usuario. Subcomandos: `START` (activa captura global), `STOP` (pausa captura), `DUMP [N]` (vuelca las ultimas N entradas del TraceBuffer a consola), `FILTER <category>` (filtra por categoria). Usa `TRACE.dump()` internamente.
  - **Criterio:** `NEOTRACE START` + ejecutar proceso + `NEOTRACE DUMP 32` muestra ultimas 32 entradas con timestamps.
  - **Tests:** `neotrace_start_stop_toggle`, `neotrace_dump_output`.

#### B2. NeoReg & Configuration Infrastructure

* [ ] **B2.1 Z6. Registry hive database | NT: Cm (Configuration Manager), cell-based hive** | Prereqs: NT5 (Ob), NT6 (SID/ACL), A5.1 (IoStack) | Files: `src/cm/`, `src/cm/hive.rs`, `src/cm/cell.rs`, `src/cm/key.rs`, `src/cm/cache.rs`
  * **Descripcion:**
    Implementar NeoReg, sistema de configuracion jerarquico persistente como el Cm de Windows NT. El diseno sigue el modelo NT de celulas (cells) y bins, con integracion directa en el Object Manager NT5.
    **Cell-based hive format** (en vez de arbol simple):
    ```
    Hive
    +- Base Block (4 KB) — magic "neoR", seq numbers, checksum
    +- Bins (4 KB cada uno)
    |  +- Cell — Key: name, parent_cell, subkeys_list, values_list, class, sec_desc, last_write
    |  +- Cell — Value: name, type (REG_SZ/DWORD/BINARY), data
    |  +- Cell — Security descriptor (SID + ACL, reutilizado entre keys)
    +- Free cells (linked list for reuse)
    ```
    Cada celda tiene un indice dentro del bin, y los bins se numeran secuencialmente.
    **ObNamespace integration** — cada key registry es un objeto en NT5:
    ```
    \Registry
      \Machine           -> KObj::Directory
        \System           -> KObj::Key (backed by SYSTEM.HIV)
        \Drivers          -> KObj::Key (backed by DRIVERS.HIV)
      \User
        \Default          -> KObj::Key (backed by DEFAULT.HIV)
    ```
    `sys_open("\\Registry\\Machine\\System\\BootShell")` funciona via NT5 path resolution.
    **Syscall API** — expuesta como syscalls NT-style:
    ```
    RAX 50  sys_open_key(path)        -> handle (NtOpenKey)
    RAX 51  sys_create_key(path)      -> handle (NtCreateKey)
    RAX 52  sys_query_value(key, name, buf, len) -> value (NtQueryValueKey)
    RAX 53  sys_set_value(key, name, type, data, len) (NtSetValueKey)
    RAX 54  sys_enum_key(key, index, buf) -> subkey name (NtEnumerateKey)
    RAX 55  sys_enum_value(key, index, buf) -> value name (NtEnumerateValueKey)
    RAX 56  sys_delete_key(key)               (NtDeleteKey)
    RAX 57  sys_flush_key(key)                (NtFlushKey)
    RAX 58  sys_load_hive(path, mount_point)  (NtLoadKey, admin)
    RAX 59  sys_unload_hive(mount_point)      (NtUnloadKey, admin)
    ```
  * **Criterio:**
    - Keys y values expuestos como objetos en NT5 namespace
    - `sys_open("\\Registry\\Machine\\System")` devuelve handle a la key raiz de SYSTEM.HIV
    - `sys_set_value(key, "PATH", REG_SZ, "C:\\Programs")` persiste y es recuperable tras reboot
    - Cell cache: 2da lectura de misma key no toca disco (cache hit)
    - Hive persistente: tras reboot, valores anteriores siguen presentes
  * **Tests:**
    `cm_create_key_ob`, `cm_query_value_cache_hit`, `cm_set_value_persist`,
    `cm_enum_keys_multi`, `cm_hive_reload_integrity`, `cm_cell_corruption_isolated`,
    `cm_syscall_open_key`, `cm_syscall_set_get_value` (8 tests)

* [ ] **B2.2-2.5. Registry enhancements** | Prereqs: B2.1
  * **B2.2 Z6. Registry transaction journal | NT: Hive LOG (.LOG1/.LOG2)** | Files: `src/cm/journal.rs`
    Write-Ahead Log (WAL) para cada hive. Sigue el modelo NT de `.LOG` / `.LOG1` / `.LOG2`.
    Recovery al boot: replay del log si seq numbers no coinciden.
  * **B2.3 Z6. Multi-Hive Architecture | NT: SYSTEM/SOFTWARE/SECURITY/DEFAULT hives** | Files: `src/cm/hive.rs`, `src/cm/manager.rs`
    Multiples hives bajo `\Registry` con independencia de carga, persistencia y recovery.
  * **B2.4 Z6. Registry Security | NT: SECURITY.HIVE, Key ACLs (NT6)** | Files: `src/cm/security.rs`
    Control de acceso sobre keys registry usando NT6 Security Reference Monitor (SID + ACL + SeAccessCheck).
  * **B2.5 Z6. Registry notification + load/unload | NT: RegNotifyChangeKeyValue, NtLoadKey, NtUnloadKey** | Files: `src/cm/notify.rs`
    Key change notifications via Event Bus. Hive load/unload for user profiles.

#### B3. Networking

- [ ] **B3.1 D9. Network I/O | NT: Winsock (ws2_32.dll) -> NtCreateFile(\Device\Tcp)** | Prereqs: A4.1, A4.2 | Files: `src/net/`, `src/syscall.rs`
  - **Descripcion:** Modelo NT: el kernel expone `\Device\Tcp` y `\Device\Udp` como objetos de dispositivo en el namespace NT5. La API de red user-mode va en `libneodos/src/net.rs` como wrapper que abre `\Device\Tcp` via `sys_open` y opera via `sys_ioctl` (NtDeviceIoControlFile). No hay syscalls socket-style — se usa el modelo NT de File + IoControl.
  - **Criterio:** User-mode puede hacer `net_open(b"\\Device\\Tcp")`, `net_connect(fd, ip, port)`, `net_send(fd, buf)`. `PING` funciona via ICMP.
  - **Tests:** `net_open_device_tcp`, `net_tcp_connect_send_recv`, `net_icmp_ping`.

- [ ] **B3.2 E3. TCP/IP stack | NT: AFD (Ancillary Function Driver)** | Prereqs: B3.1 | Files: `src/net/`
  - **Descripcion:** Stack de red completo en kernel como driver de dispositivo `\Device\Tcp` y `\Device\Udp`. Capas: Ethernet, ARP (tabla 64 entries, timeout 300s), IPv4 (header parse/build, checksum, TTL), ICMP (echo request/reply), UDP, TCP (3-way handshake, sequence numbers, sliding window 16 KB, retransmit timer, FIN/RST). NIC driver via VirtIO-net o e1000.
  - **Criterio:** `PING 10.0.2.2` recibe reply. TCP connection a host funciona.
  - **Tests:** `tcp_handshake_3way`, `udp_send_recv`, `arp_table_lookup`, `icmp_echo_reply`.

- [ ] **B3.3 D8. DHCP client | NT: DHCP Client Service** | Prereqs: B3.2 | Files: `src/net/dhcp.rs`
  - **Descripcion:** Cliente DHCP (RFC 2131) que obtiene configuracion de red automaticamente al boot.
  - **Criterio:** Al boot con NIC presente, kernel obtiene IP automaticamente sin configuracion manual.
  - **Tests:** `dhcp_discover_offer_sequence`, `dhcp_lease_renewal`.

- [ ] **B3.4 D7. NTP client | NT: W32Time (Windows Time Service)** | Prereqs: B3.2 | Files: `src/net/ntp.rs`
  - **Descripcion:** Cliente NTP (RFC 5905, modo SNTP simplificado) que sincroniza el RTC del sistema con un servidor NTP externo.
  - **Criterio:** Tras boot con red, RTC sincronizado con servidor NTP (offset < 1s).
  - **Tests:** `ntp_request_parse_response`, `ntp_offset_calculation`.

#### B4. Userland Usable System

- [ ] **B4.3 S3. Shell redirection (`>`, `<`, `>>`)** | Prereqs: A4.7 | Files: `userbin/neoshell/`
  - **Descripcion:** Redireccion de I/O en neoshell. Parser detecta tokens `>` (write), `>>` (append), `<` (read). Para `cmd > file`: neoshell abre/crea `file` via syscall Ob, luego spawna `cmd` con `sys_dup2` redirigiendo fd 1 (stdout) al handle del archivo. Para `cmd < file`: abre archivo y redirige fd 0 (stdin). Para `>>`: abre con flag append.
  - **Criterio:** `DIR > output.txt` crea archivo con listado. `TYPE < input.txt` lee de archivo.
  - **Tests:** `redirect_stdout_to_file`, `redirect_stdin_from_file`, `redirect_append`.

- ~~**[COMPLETED] B4.5 B1. Virtual terminals** | Prereqs: A4.4, B4.4 | Files: `userbin/neoshell/`, `src/input/`~~
  - ~~**Descripcion:** Multiplexar el framebuffer y el input en hasta 4 terminales virtuales (VTs). Depende de A4.4 (input subsystem redisenado con `InputManager` y `vt_queues[4]`). Cada VT tiene su propio buffer de framebuffer, cola de input independiente, y PID foreground.~~
  - ~~**Criterio:** Alt+F1 y Alt+F2 muestran shells independientes. Input en un VT no afecta al otro.~~
  - ~~**Tests:** `vt_switch_alt_f1_f2`, `vt_independent_input`, `vt_framebuffer_swap`.~~

- [ ] **B4.6 B6. NeoEdit text editor** | Prereqs: A4.7, B4.4 | Files: `userbin/neoedit/`
  - **Descripcion:** Editor de texto modal Ring 3 (`.NXE`). Usa `ob_open` + `ob_query_info(ReadContent)` para cargar archivos y `ob_set_info(WriteContent)` para guardar. Renderiza via `sys_write` con secuencias ANSI.
  - **Criterio:** `NEOEDIT C:\System\Config\system.cfg` abre, edita, guarda correctamente.
  - **Tests:** `neoedit_open_display`, `neoedit_edit_save`, `neoedit_scroll`.

- [ ] **B4.7 B6b-v2. Shared library per-process binding | NT: Ldr (Loader, PEB->LdrData)** | Prereqs: sys_loadlib | Files: `src/elf.rs`, `libneodos/`
  - **Descripcion:** Evolucionar el sistema NXL actual (slots globales fijos en 0x1E000000-0x1E200000 compartidos entre procesos) a binding per-process. Cada EPROCESS mantiene su propia tabla de NXLs cargadas.
  - **Criterio:** Dos procesos cargan versiones distintas de `libmath.nxl` sin interferencia.
  - **Tests:** `nxl_per_process_isolation`, `nxl_unload_on_exit`, `nxl_version_coexistence`.

- [ ] **B4.8 B7. NeoTOP** | Prereqs: A4.7, A1.5 | Files: `userbin/neotop/`
  - **Descripcion:** Monitor de sistema Ring 3 en tiempo real (`.NXE`). Muestra lista de procesos, uso de CPU por core, estadisticas de memoria, drivers cargados. Refresco cada 1 segundo via `sys_sleep`. Renderiza con ANSI escape codes.
  - **Criterio:** `NEOTOP` muestra procesos activos actualizandose en tiempo real.
  - **Tests:** `neotop_display_processes`, `neotop_refresh_loop`, `neotop_exit_clean`.

- [ ] **B4.9 B11. NeoShell scripting (`.BAT`)** | Prereqs: B4.1, B4.2, B4.3 | Files: `userbin/neoshell/`
  - **Descripcion:** Interprete de scripts batch en neoshell. Soporta archivos `.BAT`/`.CMD` con: `ECHO`, `SET`, `IF %VAR%==valor cmd`, `GOTO :label`, `CALL script.bat`, `FOR`, `REM`, `@`.
  - **Criterio:** Script `.BAT` con IF/GOTO/CALL ejecuta correctamente.
  - **Tests:** `bat_echo_set`, `bat_if_goto`, `bat_call_subroutine`, `bat_for_loop`.

- [ ] **B4.10 B12. Compositor 2D** | Prereqs: B4.4, framebuffer | Files: `userbin/compositor/`
  - **Descripcion:** Compositor de ventanas 2D sobre el framebuffer GOP 1280x800. Modelo: cada ventana tiene un back-buffer, posicion, z-order, titulo. El compositor blittea ventanas en orden z sobre el framebuffer principal. Renderiza a 30 FPS maximo.
  - **Criterio:** Dos ventanas superpuestas, una encima de otra. Mover ventana actualiza framebuffer.
  - **Tests:** `compositor_create_window`, `compositor_z_order`, `compositor_blit_overlap`.

#### B5. Security

- [ ] **B5.1 U1. Module signature validation** | Prereqs: NT6 | Files: `src/drivers/loader.rs`
  - **Descripcion:** Validacion criptografica de modulos `.nem` antes de que entren al runtime del driver loader.
  - **Criterio:** Un `.nem` alterado o sin firma no puede pasar de `Loaded` a `Initialized`.
  - **Tests:** `nem_signature_valid_accepts`, `nem_signature_invalid_rejects`, `nem_signature_tamper_detected`.

- [ ] **B5.2 U3. Driver permission enforcement** | Prereqs: NT6.3, B5.1 | Files: `src/drivers/caps.rs`
  - **Descripcion:** Cruza la capacidad declarada por el driver con el token del proceso que intenta cargarlo y con la ACL del objeto driver en el namespace.
  - **Criterio:** Un driver sin `CAP_ADMIN` no puede abrir objetos protegidos aunque este firmado.
  - **Tests:** `driver_caps_allow_admin`, `driver_caps_deny_user`, `driver_caps_acl_intersection`.

- [ ] **B5.3 U4. Secure boot chain** | Prereqs: B5.1 | Files: `neodos-bootloader/`, `src/boot/secure.rs`
  - **Descripcion:** Encadena la verificacion desde bootloader hasta kernel y drivers para que ningun binario de arranque se ejecute sin validacion previa.
  - **Criterio:** Si falla la verificacion del kernel o de un driver critico, el boot se detiene.
  - **Tests:** `secure_boot_kernel_verified`, `secure_boot_driver_verified`, `secure_boot_fail_closed`.

#### B6. Performance

- [ ] **B6.1 V2. Zero-copy pipes** | Prereqs: A4.5, S2 | Files: `src/pipe.rs`
  - **Descripcion:** Optimiza el camino de pipes para que, cuando el buffer del productor o consumidor este alineado y sea seguro, los datos se pasen por referencia a paginas compartidas o pinneadas en lugar de copiarse byte a byte dentro del kernel.
  - **Criterio:** Un pipeline con buffers alineados evita al menos una copia completa entre procesos.
  - **Tests:** `pipe_zero_copy_aligned_buffers`, `pipe_zero_copy_fallback_copy`, `pipe_zero_copy_integrity`.

- [ ] **B6.2 V3. Copy-on-write fork** | Prereqs: A1.5 | Files: `src/memory/cow.rs`, `src/syscall.rs`
  - **Descripcion:** Implementa `sys_fork` como clonacion perezosa del espacio de direcciones: el hijo comparte paginas con el padre en modo read-only hasta que cualquiera escribe.
  - **Criterio:** Padre e hijo comparten memoria al nacer y divergen solo al escribir.
  - **Tests:** `cow_fork_shares_pages`, `cow_write_triggers_copy`, `cow_fork_isolated_writes`.

#### B9. Shell command migration Ring 0 -> Ring 3 [COMPLETED]

Migracion completa de todos los comandos del kernel shell Ring 0 a `.NXE` en Ring 3.

El Ring 0 solo mantiene:
- **RUN** — bootstrap loader necesario para lanzar el primer binario Ring 3 (NeoInit/neoshell) desde el kernel.
- **CRASH** — crash dump management; es inherentemente kernel-level.

**Completados:**
- HELP -> corehelp.nxe (B9.1)
- SET -> neoshell built-in (B9.2)
- EXIT -> neoshell built-in (B9.3)
- PS -> ps.nxe (B9.4) — migrado a Ob
- KILL -> kill.nxe (B9.5) — migrado a Ob
- PRI -> pri.nxe (B9.6) — migrado a Ob
- DRIVES -> drives.nxe (B9.8) — migrado a Ob
- KEYB -> keyb.nxe (B9.10) — migrado a Ob
- CALL -> neoshell built-in (B9.13)
- LABEL -> label.nxe — migrado a Ob
- FSCK -> fsck.nxe
- NDREG -> ndreg.nxe — migrado a Ob
- LOADNEM -> loadnem.nxe — partial Ob (create(Driver) done, unload via legacy RAX 58)
- KOBJ -> kobj.nxe — migrado a Ob

Los comandos de gestion de archivos (DEL, REN, MD, RD, COPY, TYPE, DIR, TREE, CD, CLS, ECHO, DATE, TIME, VOL, MEM, VER, CPUINFO, DATETIME, VER) tambien estan migrados a Ring 3 como `.NXE`.

---

### X7. NeoDOS Object Manager (Ob) — Unificacion de Handles, KOBJ, URN y Seguridad [COMPLETED v0.44.2]

> **NT Reference:** Ob (Object Manager) — `ObOpen`, `ObCreate`, `ObQueryInfo`, `ObReferenceObject`
> **Documento de diseno:** [`docs/OBJECT_MANAGER_ARCHITECTURE.md`](OBJECT_MANAGER_ARCHITECTURE.md)
> **Version objetivo:** v0.41-v0.44.2
> **Estado:** [DONE] COMPLETADO (v0.44.2)

#### Arquitectura Implementada

El Object Manager (Ob) unifica handles, objetos, seguridad y namespace en una sola abstraccion:

```
ObObject (kernel object)
+-- id: ObId (64-bit)
+-- type: ObType (16 tipos: Process, Driver, Device, Pipe, ..., Timer)
+-- name: [u8; OB_NAME_LEN=128]
+-- refcount: u32
+-- flags: u32
+-- native_id: u64 (back-pointer al recurso real)
+-- ops: Option<&'static dyn ObOperations> (vtable polimorfica)

ObHandle (per-process)
+-- object_id: ObId -> referencia a ObObject
+-- access_mask: u32 (READ|WRITE|EXEC|DELETE)
+-- offset: u64

ObDirectory (namespace)
+-- \Global\, \Device\, \Driver\, \FileSystem\, \Registry\, \Ob\
+-- \Global\Info\ (virtual: Version, DateTime, Memory, CpuInfo, Cwd, Keyboard, Drives, Drivers)
+-- \Ob\Process\ (virtual: PID-indexed)
```

#### Syscalls Ob (RAX 60-66)

| RAX | Syscall | Args | Descripcion |
|-----|---------|------|-------------|
| 60 | `sys_ob_open` | RBX=path, RCX=access | Open named object -> handle (SeAccessCheck integrado) |
| 61 | `sys_ob_create` | RBX=path, RCX=type, RDX=fds_out, R8=attrs | Create named object (Process=1, Driver=2, Pipe=4, Directory=11, Event=13) |
| 62 | `sys_ob_query_info` | RBX=fd, RCX=class, RDX=buf, R8=len | Query object metadata (0-16 classes, incl. ReadContent=15, VolumeLabel=16) |
| 63 | `sys_ob_set_info` | RBX=fd, RCX=class, RDX=buf | Set object metadata (0-9 classes, incl. WriteContent=7, SetCwd=8, SetVolumeLabel=9, VfsRename=6) |
| 64 | `sys_ob_enum` | RBX=dir_fd, RCX=buf, RDX=max | Enumerate directory (VFS-backed + Ob namespace) |
| 65 | `sys_ob_wait` | RBX=count, RCX=handles, RDX=type, R8=to | Wait on objects (multi-type via KWait) |
| 66 | `sys_ob_destroy` | RBX=fd | Destroy/delete object by fd (files, dirs, drivers, namespace objects) |

#### Syscalls Legacy Migrados a Ob

| RAX | Legacy | Estado SSDT | Equivalente Ob |
|-----|--------|-------------|----------------|
| 11 | `sys_readfile` | None | ob_query_info(ReadContent) |
| 12 | `sys_writefile` | None | ob_set_info(WriteContent) |
| 24 | `sys_getcpuinfo` | None | ob_open("\Global\Info\CpuInfo") + ob_query_info |
| 25 | `sys_mkdir` | None | ob_create(Directory) |
| 26 | `sys_unlink` | None | ob_destroy |
| 27 | `sys_rmdir` | None | ob_destroy |
| 28 | `sys_rename` | None | ob_set_info(VfsRename) |
| 33 | `sys_get_drives` | None | ob_open("\Global\Info\Drives") + ob_query_info |
| 43 | `sys_get_version` | None | ob_open("\Global\Info\Version") + ob_query_info |
| 44 | `sys_get_datetime` | None | ob_open("\Global\Info\DateTime") + ob_query_info |
| 45 | `sys_get_meminfo` | None | ob_open("\Global\Info\Memory") + ob_query_info |
| 46 | `sys_get_volume_label` | None | ob_query_info(VolumeLabel) |
| 48 | `sys_kobj_enum` | None | ob_enum |
| 49 | `sys_set_keyboard_layout` | None | ob_open("\Global\Info\Keyboard") + ob_set_info |
| 51 | `sys_set_priority` | None | ob_set_info(ProcessPriority) |
| 52 | `sys_kill_process` | None | ob_set_info(ProcessTerminate) |
| 54 | `sys_set_volume_label` | None | ob_set_info(SetVolumeLabel) |
| 56 | `sys_driver_enum` | None | ob_open("\Global\Info\Drivers") + ob_query_info |
| 57 | `sys_driver_load` | None | ob_create(Driver) |

#### Syscalls Legacy que Permanecen

| RAX | Syscall | Motivo |
|-----|---------|--------|
| 0 | `sys_exit` | Demasiado especifica para abstraer |
| 1 | `sys_write` | Foundation: stdout/stderr/pipe write |
| 2 | `sys_yield` | Foundation: ceder CPU |
| 3 | `sys_getpid` | Foundation: PID actual |
| 4 | `sys_read` | Foundation: stdin/pipe read |
| 5 | `sys_pipe` | Foundation: pipe creation (paralelo a ob_create(Pipe)) |
| 6 | `sys_dup2` | Foundation: redireccion |
| 7 | `sys_spawn` | Foundation: crear proceso (needed by neoinit) |
| 8 | `sys_readdir` | Foundation: directory read (paralelo a ob_enum) |
| 9 | `sys_waitpid` | Foundation: wait child |
| 10 | `sys_open` | Foundation: open file (paralelo a ob_open) |
| 13 | `sys_close` | Foundation: close handle |
| 16 | `sys_chdir` | Foundation: change dir |
| 18 | `sys_brk` | Demasiado especifica |
| 19 | `sys_mmap` | Demasiado especifica |
| 20 | `sys_munmap` | Demasiado especifica |
| 21 | `sys_loadlib` | Demasiado especifica |
| 22 | `sys_thread_create` | Foundation: thread creation |
| 23 | `sys_thread_join` | Foundation: thread join |
| 29 | `sys_set_exception_handler` | Foundation: SEH |
| 40 | `sys_wait_alertable` | Foundation: alertable wait |
| 41 | `sys_sleep_ex` | Foundation: sleep |
| 42 | `sys_poweroff` | Foundation: poweroff |
| 47 | `sys_chdir_parent` | Foundation: parent cwd change |
| 50 | `sys_ndreg` | Internal: driver registry admin |
| 53 | `sys_cursor_blink` | Foundation: cursor control |
| 55 | `sys_fsck` | Foundation: filesystem check |
| 58 | `sys_driver_unload` | Foundation: driver unload |
| 59 | `sys_poll` | Foundation: I/O polling |

#### Metricas Objetivo Alcanzadas

| Metrica | Antes (v0.40) | Despues (v0.44.2) |
|---------|---------------|-------------------|
| HandleEntry tipo-seguro | [PENDING] (kind hardcoded) | [DONE] (ObId ref) |
| KOBJ + handles unificados | [PENDING] | [DONE] |
| Security en open | [PENDING] (solo syscall 50) | [DONE] (todo acceso via SeAccessCheck) |
| URN funcional | Parcial (file + device) | Full (all schemes via Ob) |
| Tipos de objeto | ~8 implicitos | 16 explicitos (ObType enum) |
| Syscalls Ob | 0 | 7 nuevas (RAX 60-66) |
| OB_NAME_LEN | 32 | 128 |

#### Estado por Binario

| Binario | Estado Ob | Syscalls Ob | Syscalls Legacy Restantes |
|---------|-----------|-------------|--------------------------|
| ps | [DONE] COMPLETO | ob_open, ob_enum, ob_query_info | -- |
| kill | [DONE] COMPLETO | ob_open, ob_set_info | -- |
| pri | [DONE] COMPLETO | ob_open, ob_set_info | -- |
| kobj | [DONE] COMPLETO | ob_open, ob_enum | -- |
| neoshell | [DONE] COMPLETO | ob_open, ob_enum, ob_create(Pipe), ob_create(Process), ob_wait, ob_set_info(SetCwd), ob_query_info(ReadContent) | sys_cursor_blink, sys_poweroff |
| cd | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| coredir | [DONE] COMPLETO | ob_open, ob_enum | -- |
| corehelp | [DONE] COMPLETO | ob_open, ob_enum, ob_create(Pipe), ob_query_info(ReadContent), ob_create(Process), ob_wait | -- |
| coretype | [DONE] COMPLETO | ob_open, ob_query_info(ReadContent) | -- |
| tree | [DONE] COMPLETO | ob_open, ob_enum | -- |
| corecopy | [DONE] COMPLETO | ob_open, ob_destroy, ob_query_info(ReadContent), ob_set_info(WriteContent) | -- |
| cmdtest | [DONE] COMPLETO | ob_open, ob_create(Directory), ob_destroy, ob_set_info, ob_query_info(ReadContent) | -- |
| cpuinfo | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| neoinit | [DONE] N/A (PID 1) | ob_create(Process), ob_wait | sys_spawn (needed for bootstrap) |
| datetime | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| ver | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| mem | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| vol | [DONE] COMPLETO | ob_open, ob_query_info(VolumeLabel) | -- |
| coredel | [DONE] COMPLETO | ob_open, ob_destroy | -- |
| coreren | [DONE] COMPLETO | ob_open, ob_set_info(VfsRename) | -- |
| coremd | [DONE] COMPLETO | ob_create(Directory) | -- |
| corerd | [DONE] COMPLETO | ob_open, ob_destroy | -- |
| drives | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| keyb | [DONE] COMPLETO | ob_open, ob_set_info(KeyboardLayout) | -- |
| label | [DONE] COMPLETO | ob_open, ob_query_info(VolumeLabel), ob_set_info(SetVolumeLabel) | -- |
| fsck | [DONE] N/A | -- | sys_fsck (comando de reparacion con argumentos) |
| ndreg | [DONE] COMPLETO | ob_open, ob_query_info | -- |
| loadnem | [DONE] PARCIAL | ob_create(Driver) | sys_driver_unload (RAX 58) para /U |
| echo | [DONE] N/A | -- | (foundation only, solo sys_write) |
| cls | [DONE] N/A | -- | (foundation only, solo sys_write) |

---

### B7. Experimental

- [ ] **B7.1 E4. Full GUI system** | NT: Desktop Window Manager | Prereqs: B4.10 | Files: `userbin/gui/` | Desktop con iconos, menu, ventanas redimensionables.
- [ ] **B7.2 E5. Advanced secure boot (TPM)** | NT: BitLocker / TPM | Prereqs: B5.3 | Files: `src/boot/tpm.rs` | Medicion PCR + sealed storage.
- [ ] **B7.3 E6. Package manager** | NT: MSI / Windows Update | Prereqs: B5.1, A5.1 | Files: `userbin/neopkg/` | Install/remove paquetes `.NPK` firmados.
- [ ] **B7.4 T4. Time-travel debugging** | NT: WinDbg time travel | Prereqs: A3.2, B1.1 | Files: `src/debugger/timetravel.rs` | Replay de trace buffer en debugger.
- [ ] **B7.5 T5. Live kernel patching** | NT: Windows Hotpatch | Prereqs: A2.4, A3.2 | Files: `src/patch/mod.rs` | Hot-patch de funcion kernel sin reboot.
- [ ] **B7.6 T2. Distributed NeoDOS nodes** | NT: DFS | Prereqs: B3.2 | Files: `src/cluster/` | 2 nodos QEMU se descubren y comparten FS read-only.

---

## Matriz de Problemas Arquitectonicos

| # | Problema | Fase | NT ref | Estado | Riesgo si no se hace |
|---|----------|------|--------|--------|---------------------|
| 1 | Frame allocator O(n), 4 GB max | A0.1-A0.2 | Mm | COMPLETED | -- |
| 2 | Direcciones fijas, solapamiento | A0.3 | Mm | COMPLETED | -- |
| 3 | Handle table fijo (16) | A0.4 | Ob handles | COMPLETED | -- |
| 4 | Thread model ausente (1 hilo/proceso) | A1.5 | KTHREAD | COMPLETED | sys_thread_create/join, per-process KTHREADs |
| 5 | Scheduler monoprocesador | A1.1-A1.2 | Ps | COMPLETED | Per-CPU run queues, work stealing SMP |
| 6 | Slab allocator lock global | A1.3 | Lookaside | COMPLETED | Throughput no escala con CPUs |
| 7 | Sin IPI / TLB shootdown | A1.4 | KeIpi | COMPLETED | Data corruption en SMP |
| 8 | IRQL ausente (solo cli/sti) | A2.4 | IRQL | COMPLETED | Per-CPU IRQL levels, IrqMutex, INV-14 |
| 9 | DPC ausente (work queue parche) | A2.5 | DPC | COMPLETED | Per-CPU DPC queues, SPSC ring buffer |
| 10 | PCI port I/O asume x86 | A2.1 | HAL | COMPLETED | ECAM MMIO (MCFG) + PIO fallback |
| 11 | PIC legacy como default | A2.2 | IOAPIC | COMPLETED | IOAPIC init (MADT), PIC disable, MSI-X |
| 12 | HAL mezcla raw y safe | A2.3 | HAL | COMPLETED | asm confinado a hal/ |
| 13 | Sin crash dump ni recovery | A3.1-A3.3 | Bugcheck | COMPLETED | CrashDumpHeader, stack walk |
| 14 | SEH ausente | A3.4 | SEH | COMPLETED | TEB exception handler chain |
| 15 | Stack unwinding inexistente | A3.2 | KD | [PENDING] | Sin backtrace |
| 16 | Shell en Ring 0 | A4.7 | CSRSS | COMPLETED | neoshell.nxe en Ring 3, solo RUN/CRASH |
| 17 | NeoInit no implementado | Z1 | smss.exe | COMPLETED | Doc/codigo divergen |
| 18 | Syscall dispatch manual | A4.2 | SSDT | COMPLETED | SSDT table-based, permission check |
| 19 | ELF loader sin validacion | A4.3 | Ldr | COMPLETED | Triple fault con binarios maliciosos |
| 20 | APC ausente | A4.5 | APC | COMPLETED v0.34.0 | I/O completion en contexto incorrecto |
| 21 | Input sin multiplexion | A4.4 | ConDrv | COMPLETED | -- |
| 22 | FAT32 + NeoFS duplicados | A5.1 | IoStack | COMPLETED | Ambos usan IoStack para I/O |
| 23 | Ob flat (no namespace) | NT5 | Ob | COMPLETED | Hardcode C:, sin symlinks |
| 24 | SRM ausente | NT6 | Se | COMPLETED | SID, Token, ACL, SeAccessCheck |
| 25 | Registry flat -> cell-based hive | B2.1-B2.5 | Cm | [PENDING] | Sin config jerarquica transaccional |
| 26 | Handles/KOBJ/URN/security no unificados | X7 | Ob | COMPLETED v0.44.2 | Unified via ObObjectTable |

---

## Architectural Initiatives

Las siguientes iniciativas arquitectonicas son cambios transversales que afectan a multiples subsistemas y requieren coordinacion.

### AI-1: Clean up ObInfoClass/ObSetInfoClass enums

**Estado:** [PENDING]
**Archivos:** `src/object/types.rs`

El handler `handler_ob_query_info` soporta info classes 0-16, pero el enum `ObInfoClass` en `types.rs` solo define hasta 14 (KeyboardLayout). Similarmente, `ObSetInfoClass` solo define hasta 5 (KeyboardLayout), mientras que el handler soporta clases 4 (ProcessTerminate), 6 (VfsRename), 7 (WriteContent), 8 (SetCwd), 9 (SetVolumeLabel).

**Accion requerida:** Anadir las clases faltantes a los enums para que reflejen la implementacion real:
- `ObInfoClass::ReadContent = 15`, `ObInfoClass::VolumeLabel = 16`
- `ObSetInfoClass::ProcessTerminate = 4`, `ObSetInfoClass::VfsRename = 6`, `ObSetInfoClass::WriteContent = 7`, `ObSetInfoClass::SetCwd = 8`, `ObSetInfoClass::SetVolumeLabel = 9`

### AI-2: Consolidate legacy syscall wrappers

**Estado:** [PENDING]
**Archivos:** `src/syscall/mod.rs`

Tras la migracion a Ob, varias syscalls legacy son wrappers finos que podrian eliminarse:
- `handler_readfile` (RAX 11) y `handler_writefile` (RAX 12) ya estan en None
- `handler_mkdir`/`handler_unlink`/`handler_rmdir`/`handler_rename` (RAX 25-28) ya estan en None
- Sin embargo, `handler_open` (RAX 10), `handler_readdir` (RAX 8), `handler_pipe` (RAX 5) siguen activos y son wrappers de Ob

**Decision:** Mantener las syscalls legacy activas por compatibilidad con binarios antiguos. No eliminar hasta que todos los binarios conocidos usen exclusivamente Ob API (v1.0).

### AI-3: ObObjectTable lock granularity

**Estado:** [PENDING]
**Archivos:** `src/object/mod.rs`

El `ObObjectTable` usa un unico `spin::Mutex` global. Bajo carga de multiple proceso con operaciones Ob concurrentes (open, query, set, destroy), esto puede convertirse en cuello de botella.

**Propuesta:** Migrar a lock striping (16 locks, hash de ObId para elegir lock) o a una `RwLock` para operaciones de solo lectura vs escritura. Evaluar si es necesario tras medir contention real.

### AI-4: Standardize error codes between Ob and syscall layer

**Estado:** [PENDING]
**Archivos:** `src/object/types.rs`, `src/syscall/mod.rs`

Actualmente `ObError` tiene su propio conjunto de codigos (-1 a -9), y `SyscallError` tiene otro conjunto separado. La capa de syscall traduce entre ellos manualmente. Esto puede producir discrepancias (e.g., `ObError::NotFound` -> `SyscallError::NoEnt` y `ObError::InvalidParam` -> `SyscallError::Inval`).

**Propuesta:** Unificar en un solo conjunto de codigos de error reutilizado por ambas capas, o anadir un mapping formal verificado por tests.

---

## Recommended Next Steps

Priorizados por impacto y dependencias:

| Prioridad | Item | Fase | Dependencias | Esfuerzo estimado |
|-----------|------|------|-------------|-------------------|
| 1 | **VirtIO block driver (A5.2)** | v0.46 | A2.1 (ECAM) | 400-500 lineas |
| 2 | **Device Tree + Resource Manager** | v0.46 | NT5, Driver Runtime | 600-800 lineas |
| 3 | **sys_ioctl() and PCI device binding** | v0.46 | A2.1, A2.2 | 300-400 lineas |
| 4 | **Registry hive database (B2.1)** | v0.50 | NT5, NT6, IoStack | 2000-3000 lineas |
| 5 | **Kernel debugger (A3.2)** | v0.51+ | A3.1 | 1500-2000 lineas |
| 6 | **Networking (B3.1-B3.2)** | v0.47 | VirtIO-net, IRP | 3000-5000 lineas |
| 7 | **AHCI NCQ (A5.3)** | v0.48 | A2.2, IRP | 400-600 lineas |
| 8 | **NeoReg transaction journal (B2.2)** | v0.50 | B2.1 | 500-700 lineas |
| 9 | **Shell redirection (B4.3)** | v0.46+ | neoshell | 300-400 lineas |

Items pequenos de baja prioridad pero alto valor:
- **AI-1**: Actualizar enums ObInfoClass/ObSetInfoClass (~15 min)
- **CQ1**: Reorganizar libneodos-nxl en modulos (~2 horas)
- **AI-3**: Evaluar lock contention en ObObjectTable (~1 dia)
- **AI-4**: Unificar codigos de error (~1 dia)

---

## Referencias

- [ARCHITECTURE_SOURCE_OF_TRUTH.md](ARCHITECTURE_SOURCE_OF_TRUTH.md) — invariantes MUST/MUST NOT
- [AGENTS.md](../AGENTS.md) — build, test, convenciones de commit
- [ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md) — vision a largo plazo v0.40 -> v1.0
- [OBJECT_MANAGER_ARCHITECTURE.md](OBJECT_MANAGER_ARCHITECTURE.md) — diseno completo del Object Manager
- [KERNEL.md](KERNEL.md) — documentacion del kernel
