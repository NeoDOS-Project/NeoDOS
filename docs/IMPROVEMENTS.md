# NeoDOS — Roadmap de 100 Items

> Versión actual: v0.16.1 (177 tests, 4 user-mode binaries, ELF64 loader, mmap lazy).
> Objetivo: v0.20 — kernel modular, estable, extensible.
> Última revisión: Mayo 2026.

---

## COMPLETED (42 items)

### Boot & Core Kernel
1. **x86_64 boot** — entry `_start` en 0x200000, long mode vía UEFI bootloader.
2. **GDT/IDT/PIC** — segmentos Ring 0/3, IDT 256 entradas, PIC remapeado IRQ 32–47.
3. **Identity paging 4 GiB** — páginas enormes 2 MB, identidad hasta 4 GB.
4. **Heap allocator** — 16 MB @ 0x1000000, `linked_list_allocator`, Box/Vec/String.
5. **PS/2 keyboard driver** — IRQ1, ring-buffer lock-free 1024 bytes.
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

### Userland & Memoria
27. **Demand paging (4 KB)** — frame allocator, split_2mb, heap page fault handler.
28. **sys_brk / sys_mmap** — ajuste program break, asignación zero-filled.
29. **ELF64 loader** — src/elf.rs: carga segmentos PT_LOAD a vaddr, 7 tests.
30. **User-mode processes** — IRETQ a Ring 3, EXIT_RSP/EXIT_RIP, scheduler add_ring3_process.
31. **Kernel private stacks** — TSS.RSP0 por proceso, actualizado en cada context switch.
32. **Syscall table (INT 0x80)** — 14 syscalls: exit, write, yield, getpid, read, waitpid, open, readfile, writefile, close, chdir, getcwd, brk, mmap.
33. **Scheduler blocking** — ProcessState::Blocked, wake_waiters(), idle HLT.

### Shell & Testing
34. **150 kernel self-tests** — 15 suites, comando `test`, 4 user-mode binaries.
35. **4 user-mode test binaries** — HELLO.BIN, SYSTEST.BIN, FILETEST.BIN, ALLTEST.BIN.
36. **Command history** — buffer circular 32, ↑/↓ navegación.
37. **TAB autocomplete** — comandos built-in + archivos del directorio actual.
38. **Keyboard layouts** — KBDUS.klc / KBDSP.klc compilados en build-time.
39. **Shell commands básicos** — HELP, DATE, TIME, VER, DEL, REN, RD, SHUTDOWN, EXIT, LOAD.
40. **S1. Estabilizar syscall ABI** — `SyscallNum` enum + `from_u64()`, `SyscallError` enum (16 codes), `err_to_u64()` negative encoding, `syserr!` macro, `validate_abi()` boot-time assertion, clean `match` dispatch, `[SYS]` log pruning.

---

## PRIORIDAD S — CRÍTICO (10 items)

Estos items desbloquean todo el roadmap futuro.

40. **S2. IPC / Pipes** — pipe buffers en kernel, stdin→stdout redirection, blocking reads, scheduler integration.
42. **S3. Shell output redirection** — `DIR > FILE.TXT`, `ECHO >> FILE.TXT`, `CMD > FILE`.
43. **S4. FAT32 write** — escritura real en FAT32: directorios, archivos, clusters.
44. **S5. FSCK utility** — verificación inodos, block bitmap, orphan detection, repair mode.
45. **S6. libneodos** — standard library: wrappers syscall, IO, FS, memoria, macros seguras.
46. **S7. Process exit: full cleanup** — liberar kernel stack (Box<AlignedKStack>), reciclar slots del scheduler, tabla de archivos abiertos.
47. **S8. PATH resolution** — búsqueda automática de ejecutables en C:\BIN, C:\SYSTEM, etc.
48. **S9. Shell pipe operator** — `CMD1 | CMD2`, conectar stdout→stdin vía pipes.
49. **S10. Batch IF/GOTO/FOR** — parser batch con IF/ELSE, GOTO, FOR, variables.

---

## PRIORIDAD A — INFRAESTRUCTURA (13 items)

50. **A1. Signals userland** — SIGSEGV/SIGTERM/SIGINT, handlers Ring 3, delivery vía IRETQ.
51. **A2. Scheduler prioritario** — prioridades, time slices dinámicos, idle task dedicada.
52. **A3. Kernel slab allocator** — caches por tamaño (inodos, PCB, buffers FS).
53. **A4. DMA dinámico** — PRDT dinámico, multi-block DMA, page pools.
54. **A5. Cache global de bloques** — LRU entre FS, write-back opcional, dirty tracking.
55. **A6. Hard links + symlinks** — enlaces duros NeoFS, symlinks vía VFS.
56. **A7. Compresión transparente** — bloques DEFLATE/LZ4, flags por archivo.
57. **A8. VirtIO block driver** — PCI VirtIO, multi-queue, paravirtualización.
58. **A9. NVMe driver** — queues NVMe, MSI/MSI-X, async completions.
59. **A10. PCIe enumeration** — escaneo completo buses PCIe (no solo bus 0).
60. **A11. MSI/MSI-X** — interrupciones basadas en mensajes, reemplazar PIC.
61. **A12. Ramdisk driver** — dispositivo de bloque en memoria para archivos temporales.

---

## PRIORIDAD B — USERLAND & UX (14 items)

63. **B1. Terminales virtuales** — Alt+F1..F4, shells independientes, TSS activo.
64. **B2. ANSI escape** — colores 16, cursor control, clear, VT100 subset.
65. **B3. Scrollback buffer** — buffer circular VGA, navegación Shift+↑/↓.
66. **B4. Alias y configuración** — alias persistentes, perfil shell desde AUTOEXEC.BAT.
67. **B5. Shell multilínea** — continuaciones `^`, historial persistente en disco.
68. **B6. NeoEdit** — editor de texto integrado estilo edit.com.
69. **B7. NeoTOP** — monitor procesos: CPU, memoria, scheduler stats.
70. **B8. NeoTrace** — tracing syscalls por proceso, logs en NeoFS.
71. **B9. BMP/PNG viewer** — visor de imágenes sobre framebuffer.
72. **B10. WAV/PCM audio** — mixer simple, buffer ring, PC speaker / SB16.
73. **B11. NeoShell script language** — parser propio: variables, funciones, loops, arrays.
74. **B12. Compositor 2D** — ventanas en memoria, doble buffer, clipping.
75. **B13. Driver GPU lineal** — abstracción framebuffer, primitivas aceleradas.
76. **B14. Swap** — disco como memoria secundaria, page-out/page-in.

---

## PRIORIDAD C — HARDWARE (7 items)

77. **C1. USB HID funcional** — UHCI/EHCI, teclados USB reales (PIIX3 fix).
78. **C2. USB mass storage** — pendrives vía UHCI/EHCI + SCSI.
79. **C3. HPET / APIC timers** — alta precisión, reemplazar PIT.
80. **C4. Paging optimizado** — reutilización page tables, TLB flush selectivo.
81. **C5. Input lock-free** — eliminar cli/sti frecuentes en ring buffer.
82. **C6. AHCI NCQ** — Native Command Queuing, múltiples comandos simultáneos.
83. **C7. USB UHCI completo** — driver UHCI funcional (actualmente no escribe FLBASEADD).

---

## PRIORIDAD D — ECOSISTEMA (10 items)

84. **D1. SDK externo** — cargo-neodos, GCC cross, documentación ABI userland.
85. **D2. CI Integration** — GitHub Actions: build + QEMU test + regression.
86. **D3. Build profiles** — debug/release/minimal/test con features separadas.
87. **D4. Benchmark system** — IOPS, syscall latency, scheduler, FS stress.
88. **D5. Kernel debugger** — breakpoints, stack traces, dump memoria, inspect procesos.
89. **D6. Crash dump** — persistir panic dumps a NeoFS, análisis post-mortem.
90. **D7. NTP client** — sincronización horaria vía UDP.
91. **D8. DHCP client** — configuración automática IP/gateway/DNS.
92. **D9. Socket API** — UDP/TCP, bind/listen/connect, syscall integration.
93. **D10. POSIX compatibility** — wrappers POSIX sobre syscalls NeoDOS.

---

## PRIORIDAD E — EXPERIMENTAL (7 items)

94. **E1. ARM64 backend** — MMU ARM64, exception vectors, generic timer.
95. **E2. SMP** — multi-CPU, IPIs, locking atómico, scheduler balanceado.
96. **E3. Network stack** — TCP/IP completo, drivers NIC (e1000, RTL8139).
97. **E4. GUI básica** — ventanas, ratón, iconos, barra de tareas.
98. **E5. Secure boot** — módulos firmados, validación SHA-256, modo developer.
99. **E6. Package manager** — repositorio, dependencias, instalación automatizada.
100. **E7. Real hardware boot** — probar y corregir arranque en hardware real (no solo QEMU).

---

## Resumen

| Estado | Items | Prioridades |
|--------|-------|-------------|
| COMPLETED | 42 | — |
| S — Crítico | 9 | Pipes, Redirection, FAT32 write, FSCK, libneodos, cleanup, PATH, pipe operator, batch |
| A — Infraestructura | 12 | Signals, scheduler, slab, DMA, cache, links, compression, VirtIO, NVMe, PCIe, MSI, ramdisk |
| B — Userland & UX | 14 | Virtual terminals, ANSI, scrollback, NeoEdit, NeoTOP, NeoShell, compositor, swap |
| C — Hardware | 7 | USB HID, USB storage, HPET, paging, lock-free input, NCQ, UHCI |
| D — Ecosistema | 10 | SDK, CI, benchmarks, debugger, crash dump, NTP, DHCP, sockets, POSIX |
| E — Experimental | 7 | ARM64, SMP, network, GUI, secure boot, package manager, real hardware |
| **Total** | **100** | |
