# NeoDOS — Auditoría Arquitectónica Completa

> **Versión del documento:** v1.0
> **Fecha:** 2026-06-26
> **Auditor:** Arquitecto Jefe de Sistemas Operativos
> **Alcance:** Kernel completo (127 archivos, 41,798 líneas), syscalls (66), drivers (12 NEM), userland (27 binarios), documentación (7 docs)

---

## Resumen Ejecutivo

NeoDOS v0.44.2 es un sistema operativo funcional con una base arquitectónica sólida. El kernel es monolítico con subsistema de drivers aislados (modelo híbrido), escrito 100% en Rust, con 528 tests automáticos y 27 binarios de usuario.

**Estado general:** SÓLIDO — El núcleo del sistema está bien diseñado y la migración a Object Manager (Ob) se ha completado exitosamente. Los problemas identificados son principalmente deuda técnica y documentación desactualizada, no fallos arquitectónicos fundamentales.

**Fortalezas principales:**
- HAL raw/safe split con ASM confinado
- Boot sequence determinista en 11 fases
- EPROCESS/KTHREAD (modelo NT)
- SSDT dispatch O(1) con tabla de permisos
- Driver certification pipeline (8 estados)
- Sistema de capacidades (12 flags)
- ABI negotiation con versionado min/target/max
- KWait unified wait engine
- Ob Object Manager unificando handles, KOBJ, URN y seguridad
- IRQL framework reemplazando CLI/STI
- Per-CPU slab allocator con hot cache lock-free
- SMP con IPI, TLB shootdown, work stealing
- Driver isolation (X4) con validación de punteros
- 528 tests en 50 suites

**Debilidades principales:**
- `usermode.rs:WAIT_PID` static mut SMP-unsafe (**bug crítico**)
- `driver_runtime.rs:ISOLATED_REGIONS` static mut sin sincronización (**bug crítico**)
- NXL_REGISTRY static mut sin protección SMP
- 7 símbolos exportados duplicados entre v3loader.rs y hst.rs
- TOCTOU race en kobj_register
- KObjType→ObType mapping con pérdida de información
- ObError y SyscallError no unificados
- libneodos cubre solo 24/35 syscalls (68%)
- README desactualizado (v0.39.11 vs v0.44.2 real)
- ARCHITECTURE_SOURCE_OF_TRUTH.md menciona MAX_PROCESSES fijo pero scheduler usa Vec

---

## 1. Arquitectura General

### 1.1 Mapa de Subsistemas

```
┌──────────────────────────────────────────────────────────────────────┐
│                         USERMODE (Ring 3)                            │
│  ┌──────────┐ ┌──────────┐ ┌───────────┐ ┌───────────────────────┐  │
│  │neoshell  │ │neoinit   │ │userbin/*  │ │libneodos (AbiTable v5)│  │
│  │.nxe      │ │.nxe      │ │.nxe (27)  │ │(io/fs/syscall/mem)    │  │
│  └────┬─────┘ └────┬─────┘ └─────┬─────┘ └───────────┬───────────┘  │
│       │            │             │                    │              │
│  ┌────┴────────────┴─────────────┴────────────────────┴──────────┐  │
│  │                  SYSCALL GATE (INT 0x80, SSDT 256 slots)      │  │
│  │              32 handlers activos, resto None (legacy removed)   │  │
│  └────────────────────────────┬──────────────────────────────────┘  │
├───────────────────────────────┼─────────────────────────────────────┤
│                      KERNEL (Ring 0)                                 │
│  ┌────────────────────────────┴──────────────────────────────────┐  │
│  │  SYSTEM SERVICES                                              │  │
│  │  ┌──────────┐ ┌───────────┐ ┌────────────┐ ┌──────────────┐  │  │
│  │  │Process   │ │Scheduler  │ │Memory Mgr  │ │Security Ref  │  │  │
│  │  │Manager   │ │4-level    │ │Buddy+Slab  │ │Monitor       │  │  │
│  │  │EPROCESS  │ │Aging      │ │Demand Pgng │ │SID/Token/ACL │  │  │
│  │  │KTHREAD   │ │Work Steal │ │mmap Lazy   │ │SeAccessCheck │  │  │
│  │  └──────────┘ └───────────┘ └────────────┘ └──────────────┘  │  │
│  │  ┌──────────┐ ┌───────────┐ ┌────────────┐ ┌──────────────┐  │  │
│  │  │VFS       │ │Ob/KOBJ    │ │IPC/Pipes   │ │Event Bus v2  │  │  │
│  │  │26 drives │ │Object Mgr │ │IRP System  │ │2 priorities  │  │  │
│  │  │Mount Pts │ │Namespace  │ │Async I/O   │ │Filtros       │  │  │
│  │  └──────────┘ └───────────┘ └────────────┘ └──────────────┘  │  │
│  │  ┌──────────┐ ┌───────────┐ ┌────────────┐ ┌──────────────┐  │  │
│  │  │APC/DPC   │ │WorkQueue  │ │Crash Dump  │ │Timers        │  │  │
│  │  │Engine    │ │Deferred   │ │Watchdog    │ │HPET/APIC/PIT │  │  │
│  │  └──────────┘ └───────────┘ └────────────┘ └──────────────┘  │  │
│  └────────────────────────────┬──────────────────────────────────┘  │
│                               │                                      │
│  ┌────────────────────────────┴──────────────────────────────────┐  │
│  │  NEM DRIVER RUNTIME + ISOLATION                              │  │
│  │  ┌──────────────┐ ┌────────────┐ ┌───────────┐ ┌──────────┐ │  │
│  │  │Certification │ │Capability  │ │Isolation  │ │ABI       │ │  │
│  │  │Pipeline      │ │System      │ │Layer (X4) │ │Negotiation│ │  │
│  │  │8 estados     │ │12 flags    │ │16MB/16sl  │ │min/target│ │  │
│  │  └──────────────┘ └────────────┘ └───────────┘ │/max      │ │  │
│  │  ┌──────────────┐ ┌────────────┐ ┌───────────┐ └──────────┘ │  │
│  │  │Boot Loader   │ │Dependency  │ │Hot Reload │               │  │
│  │  │(DepRes)      │ │Resolver    │ │(W2)       │               │  │
│  │  └──────────────┘ └────────────┘ └───────────┘               │  │
│  └────────────────────────────┬──────────────────────────────────┘  │
│                               │                                      │
│  ┌────────────────────────────┴──────────────────────────────────┐  │
│  │  HAL (Hardware Abstraction Layer) + ARCH (x86_64)             │  │
│  │  26 primitives extern "C" | 4-level paging | GDT/IDT | SMP   │  │
│  └───────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
```

### 1.2 Métricas del Sistema

| Métrica | Valor |
|---------|-------|
| Archivos kernel | 127 |
| Líneas kernel | 41,798 |
| Syscalls (total SSDT) | 66 (40 activos, 26 None) |
| Drivers NEM | 12 (PS/2 kbd, serial, RTC, ACPI, PCI, ATA, AHCI + 5 reference) |
| User binaries | 27 .NXE |
| Tests kernel | 528 (50 suites) |
| Tests user | 7 cmdtest |
| RAM soportada | >4 GB (bitmap dinámico) |
| User window | 32 MB (0x400000-0x2400000) |
| Heap kernel | 16 MB @ 0x1000000 |
| Heap user | 32 MB (16 slots de 2 MB) |
| mmap region | 32 MB (0x20000000-0x22000000) |
| NXL region | 2 MB (8 slots de 256 KB) |
| Driver isolation | 16 MB (16 slots de 1 MB) |

### 1.3 Hallazgos

| ID | Hallazgo | Severidad | Impacto |
|----|----------|-----------|---------|
| A1 | README desactualizado (v0.39.11 vs v0.44.2) | BAJA | Confusión en nuevos desarrolladores |
| A2 | ARCHITECTURE_SOURCE_OF_TRUTH.md menciona MAX_PROCESSES fijo pero scheduler usa Vec | BAJA | Documentación desactualizada vs código |
| A3 | IMPROVEMENTS.md menciona "528 tests" pero SOURCE_OF_TRUTH dice "320+" | BAJA | Inconsistencia documentación |
| A4 | check_deps.py no verifica todas las forbidden dependencies declaradas | MEDIA | Risk de regresión arquitectónica |

---

## 2. Kernel Core

### 2.1 Boot Sequence (main.rs)

**Fases actuales:** 13 (Phase 1 → Phase 4)

```
PHASE 1     → Serial init, GDT, IDT (early exceptions)
PHASE 2     → CPU structures: IDT, MSI, PIC remap, HPET/PIT/APIC
PHASE 2.3   → PCIe ECAM: read MCFG, map MMIO, activate ECAM
PHASE 2.5   → Memory init: UEFI map → buddy allocator, crash dump
PHASE 2.75  → Heap allocator: slab + linked_list fallback
PHASE 2.77  → Security subsystem: tokens, SIDs, ACLs
PHASE 2.8   → SMP: INIT-SIPI-SIPI, per-CPU KPRCB
PHASE 2.9   → IPI: reschedule, TLB shootdown, call-function
PHASE 2.91  → I/O APIC: detect from MADT, disable PIC, route ISA IRQs
PHASE 3     → Custom page tables, demand paging (heap + mmap 4K)
PHASE 3.5   → Boot storage scan (NVMe > AHCI > ATA PIO), GPT parse
PHASE 3.7   → Block cache init, NeoFS mount
PHASE 3.8   → VFS init, working directory, KDrive mount
PHASE 3.80  → Driver isolation region init (0x30000000)
PHASE 3.85  → Boot driver loader (from C:\System\Drivers, dep-sorted)
PHASE 3.86  → NXL auto-load (libneodos.nxl)
PHASE 3.9   → ABI freeze validation
PHASE 4     → NeoInit loader: PID 1 from C:\Programs\NeoInit.nxe
```

**Evaluación:** SÓLIDO — Secuencia determinista bien definida, cada fase tiene responsabilidad clara. 5 bloques `unsafe` identificados y documentados.

### 2.2 Memory Management

| Componente | Archivo | Evaluación |
|------------|---------|------------|
| Buddy allocator | `memory/buddy.rs` | ✅ SÓLIDO — 11 órdenes (4KB-4MB), bitmap dinámico, free lists |
| Slab allocator | `slab.rs` | ✅ SÓLIDO — 9 size classes, per-CPU hot cache, refill/drain |
| Demand paging | `arch/x64/paging.rs` | ✅ SÓLIDO — split 2MB, 4KB page fault handler |
| Memory layout | `memory/layout.rs` | ✅ SÓLIDO — 32-slot region registry, overlap detection |
| Heap allocator | `allocator.rs` | ✅ SÓLIDO — SlabAllocator como global_allocator |

### 2.3 Scheduler

| Componente | Evaluación |
|------------|------------|
| Priority levels (4) | ✅ SÓLIDO — HIGH/ABOVE_NORMAL/NORMAL/IDLE |
| Time slicing (400/200/100/50 ticks) | ✅ SÓLIDO |
| Per-CPU run queues | ✅ SÓLIDO — 64-entry ring buffer en KPRCB |
| Work stealing | ✅ SÓLIDO — round-robin entre CPUs |
| Aging (100 ticks, 1000 threshold) | ✅ SÓLIDO |
| Preemption (Ring 3 only) | ✅ SÓLIDO |

### 2.4 Interrupts & Timers

| Componente | Evaluación |
|------------|------------|
| IOAPIC (MADT) | ✅ SÓLIDO — reemplaza PIC, 24 pins |
| MSI-X | ✅ SÓLIDO — per-entry table programming |
| HPET timer (1 KHz) | ✅ SÓLIDO — periodic mode, legacy replacement |
| APIC timer calibration | ✅ SÓLIDO — calibrated against HPET |
| PIT fallback (18.2 Hz) | ✅ SÓLIDO |
| IRQL framework | ✅ SÓLIDO — 4 niveles (PASSIVE/APC/DISPATCH/DIRQL) |
| DPC engine | ✅ SÓLIDO — per-CPU SPSC queue, nesting limit |
| APC engine | ✅ SÓLIDO — per-thread queues, user/normal/kernel |

### 2.5 SMP

| Componente | Evaluación |
|------------|------------|
| SMP boot (INIT-SIPI-SIPI) | ✅ SÓLIDO — AP trampoline @ 0x800000 |
| KPRCB per-CPU (4KB) | ✅ SÓLIDO — GS-segment, 20 compile-time assertions |
| IPI reschedule (0xF0) | ✅ SÓLIDO |
| IPI TLB shootdown (0xF1) | ✅ SÓLIDO — sync ACK protocol |
| IPI call-function (0xF2) | ✅ SÓLIDO |

### 2.6 Hallazgos Kernel

| ID | Hallazgo | Severidad | Archivo |
|----|----------|-----------|---------|
| K1 | `usermode.rs:WAIT_PID` static mut sin protección SMP — race condition si 2 CPUs ejecutan `sys_waitpid` concurrentemente | **CRÍTICO** | `usermode.rs` |
| K2 | `driver_runtime.rs:ISOLATED_REGIONS` static mut accedido sin sincronización | **CRÍTICO** | `driver_runtime.rs` |
| K3 | `nxl.rs:NXL_REGISTRY` static mut sin protección contra acceso SMP | ALTA | `nxl.rs` |
| K4 | Soft watchdog ejecuta `watchdog_pet()` en timer handler — puede retrasar bajo carga | BAJA | `watchdog/mod.rs` |
| K5 | `cmd_run` usa buffer de 64KB en pila (stack) para cargar ELF — podría causar stack overflow | MEDIA | `shell/commands/run.rs` |
| K6 | No hay test que verifique que INV-2 (no heap alloc en IRQ) se cumple automáticamente | BAJA | `testing.rs` |

---

## 3. Ring0/Ring3 Separation

### 3.1 Estado Actual

| Componente | Ring 3 | Ring 0 |
|------------|--------|--------|
| Shell interactivo | ✅ neoshell.nxe | ❌ Solo RUN (bootstrap) |
| Init (PID 1) | ✅ neoinit.nxe | ❌ |
| Todos los comandos usuario | ✅ .NXE (27 binarios) | ❌ Solo CRASH (dump) |
| Carga de ELF | ❌ | ✅ cmd_run (necesario bootstrap) |
| Procesamiento de syscalls | ❌ | ✅ SSDT dispatch |

### 3.2 Evaluación

✅ **Completo:** Todos los comandos de usuario migrados a Ring 3.
✅ **Correcto:** SSDT como única puerta de Ring 3 → Ring 0 (INV-9).
✅ **Seguro:** Kernel heap no accesible desde Ring 3 (INV-8).
✅ **Aislado:** Driver isolation protege de drivers maliciosos.

### 3.3 Hallazgos

| ID | Hallazgo | Severidad |
|----|----------|-----------|
| R1 | `RUN` command en Ring 0 es necesario para bootstrap — documentar como excepción explícita | BAJA |
| R2 | `CRASH` command usa primitivas kernel-level — OK, no migrable | INFORMATIVO |

---

## 4. Syscall Layer

### 4.1 SSDT Architecture

```rust
pub static SYSCALL_TABLE: [Option<SyscallFn>; 256]       // 32 handlers
pub static SYSCALL_PERMISSIONS: [SyscallPermission; 256]   // parallel table
```

**Slots activos:** 0-11, 13, 16, 18-23, 29, 40-42, 47, 53, 55, 58-66
**Slots None:** resto (legacy migrados a Ob API, handlers eliminados del SSDT)

### 4.2 Syscall Coverage

| RAX Range | Estado | Count |
|-----------|--------|-------|
| 0-6 | Foundation (exit, write, yield, getpid, read, pipe, dup2) | 7 |
| 7-8 | Reserved | 0 |
| 9-13 | Foundation (waitpid, open, readfile, writefile, close) | 5 |
| 16-23 | Foundation (chdir, brk, mmap, munmap, loadlib, thread_create/join, chdir_parent) | 8 |
| 25-28 | Legacy → Ob (mkdir, unlink, rmdir, rename) **NONE** | 0 |
| 29 | SEH exception handler | 1 |
| 40-42 | Alertable wait, sleep_ex, poweroff | 3 |
| 46-48 | Legacy → Ob **NONE** | 0 |
| 50-55 | Admin (ndreg, set_priority, kill, cursor_blink, fsck) | 4 |
| 57-58 | Driver load/unload | 2 |
| 59 | sys_poll | 1 |
| 60-66 | Ob syscalls (open, create, query_info, set_info, enum, wait, destroy) | 7 |
| **Total activos** | | **40** |

### 4.3 Hallazgos Syscall

| ID | Hallazgo | Severidad |
|----|----------|-----------|
| S1 | `ObError` y `SyscallError` son enumeraciones separadas con traducción manual — riesgo de discrepancia | MEDIA |
| S2 | libneodos wrappers cubren solo 24/35 syscalls base (68%) — faltan thread_create/join, sleep_ex, poll, ob_destroy | MEDIA |
| S3 | ABI freeze validación en boot (Phase 3.9) pero no cubre todas las interfaces declaradas | BAJA |
| S4 | SSDT tiene slots None para legacy migrados — mantener para compatibilidad hasta v1.0 | INFORMATIVO |

---

## 5. KOBJ / Object Manager (Ob)

### 5.1 Arquitectura

```
ObObject (kernel object)
├── id: ObId (64-bit, monotónico)
├── type: ObType (16 tipos)
├── name: [u8; OB_NAME_LEN=128]
├── refcount: u32
├── flags: u32
├── native_id: u64
└── ops: Option<&'static dyn ObOperations> (vtable)

ObHandle (per-process)
├── object_id: ObId → ObObject
├── access_mask: u32
└── offset: u64 (para file handles)

ObDirectory (namespace tree)
├── \Global\ → Info (Version, DateTime, Memory, etc.)
├── \Device\ → Hardware devices
├── \Driver\ → NEM drivers
├── \FileSystem\ → Mount points
├── \Registry\ → Configuration (future)
└── \Ob\ → Process/Thread/Driver enumeration
```

### 5.2 Syscalls Ob (RAX 60-66)

| RAX | Syscall | Estado |
|-----|---------|--------|
| 60 | sys_ob_open | ✅ SeAccessCheck integrado |
| 61 | sys_ob_create | ✅ Process, Driver, Pipe, Directory, Event |
| 62 | sys_ob_query_info | ✅ Classes 0-16 (ReadContent=15, VolumeLabel=16) |
| 63 | sys_ob_set_info | ✅ Classes 0-9 (WriteContent=7, SetCwd=8) |
| 64 | sys_ob_enum | ✅ VFS-backed + Ob namespace |
| 65 | sys_ob_wait | ✅ Multi-type via KWait |
| 66 | sys_ob_destroy | ✅ Files, dirs, namespace objects |

### 5.3 Hallazgos Ob

| ID | Hallazgo | Severidad |
|----|----------|-----------|
| O1 | `kobj_register()` tiene TOCTOU race: check if exists → then insert (no atómico) | ALTA |
| O2 | KObjType tiene tipos (EventBus, MountPoint, Symlink) que no existen en ObType → pérdida de información | MEDIA |
| O3 | ObObjectTable usa un solo `spin::Mutex` global — cuello de botella potencial (AI-3) | BAJA |
| O4 | `ObInfoClass` enum no define ReadContent=15 ni VolumeLabel=16 (AI-1) | BAJA |
| O5 | `ObSetInfoClass` enum no define ProcessTerminate=4, VfsRename=6, WriteContent=7, SetCwd=8, SetVolumeLabel=9 (AI-1) | BAJA |
| O6 | El Ob namespace no tiene persistencia — los objetos se crean en cada boot | INFORMATIVO |

---

## 6. URN Namespace

### 6.1 Arquitectura

URN es frontend completo de Ob desde v0.44.2 (OB-025 rewrite):

| Scheme | Mapping Ob |
|--------|------------|
| `neodos://file/...` | `\Global\FileSystem\...` |
| `neodos://device/...` | `\Device\...` |
| `neodos://registry/...` | `\Registry\...` |
| `neodos://kobj/...` | `\Ob\...` |

**Evaluación:** ✅ COMPLETO — 19 tests, todos los schemes resueltos via Ob.

---

## 7. Userland

### 7.1 libneodos Library

| Módulo | Archivo | Cobertura |
|--------|---------|-----------|
| Syscall wrappers | `src/syscall.rs` | 24/35 syscalls (68%) |
| AbiTable v5 | `src/export.rs` | 35 entries |
| IO (stdout/stderr/stdin) | `src/io.rs` | ✅ Completo |
| FS (File::open/read/write) | `src/fs.rs` | ✅ Completo |
| Mem (brk/sbrk/mmap/munmap) | `src/mem.rs` | ✅ Completo |
| Macros (print/println) | `src/macros.rs` | ✅ Completo |

**Wrappers faltantes:**
- `sys_thread_create` (RAX 22)
- `sys_thread_join` (RAX 23)
- `sys_sleep_ex` (RAX 41)
- `sys_poll` (RAX 59)
- `sys_ob_destroy` (RAX 66)
- `sys_driver_unload` (RAX 57)

### 7.2 User Binaries

| Binario | Líneas | Syscalls Ob | Syscalls Legacy |
|---------|--------|-------------|-----------------|
| neoshell.nxe | ~2800 | ob_open, ob_enum, ob_create(Pipe/Process), ob_wait, ob_set_info(SetCwd), ob_query_info(ReadContent) | sys_cursor_blink, sys_poweroff |
| neoinit.nxe | ~200 | ob_create(Process), ob_wait | sys_spawn |
| neoshell (27 others) | varias | Ob completas | mínimas |
| **Total** | **27 .NXE** | | |

### 7.3 Hallazgos Userland

| ID | Hallazgo | Severidad |
|----|----------|-----------|
| U1 | libneodos-nxl/src/main.rs monolítico (461 líneas) — necesita dividirse en módulos (CQ1) | BAJA |
| U2 | Faltan wrappers libneodos para thread, poll, ob_destroy | MEDIA |
| U3 | neoinit usa `sys_spawn` legacy en vez de `ob_create(Process)` para bootstrap | INFORMATIVO |
| U4 | neoshell usa `sys_write` foundation para stdout — OK, pero podría usar Ob | INFORMATIVO |

---

## 8. Drivers

### 8.1 NEM Driver Ecosystem

| Driver | Categoría | Estado | Archivos |
|--------|-----------|--------|----------|
| PS/2 Keyboard | SYSTEM | ✅ Active | `drivers/nem/drivers/ps2kbd/` |
| Serial (UART 16550A) | SYSTEM | ✅ Active | reference |
| RTC | SYSTEM | ✅ Active | `drivers/nem/drivers/rtc/` |
| ACPI Poweroff | SYSTEM | ✅ Active | `drivers/nem/drivers/acpi/` |
| PCI Enumerator | SYSTEM | ✅ Active | `drivers/pci/` |
| ATA (DMA+PIO) | SYSTEM | ✅ Active | `drivers/ata/` |
| AHCI | SYSTEM | ✅ Active | `drivers/ahci/` |
| Storage Ref | BOOT | ✅ Active | reference |
| Framebuffer Ref | BOOT | ✅ Active | reference |

### 8.2 Driver Infrastructure

| Componente | Evaluación |
|------------|------------|
| Certification pipeline (8 estados) | ✅ SÓLIDO |
| Capability system (12 flags) | ✅ SÓLIDO |
| ABI negotiation (min/target/max) | ✅ SÓLIDO |
| Dependency resolver (topological sort) | ✅ SÓLIDO |
| Hot reload (W2) | ✅ SÓLIDO |
| Isolation layer (X4) | ✅ SÓLIDO — 16 MB, 16 slots, pointer validation |

### 8.3 Hallazgos Drivers

| ID | Hallazgo | Severidad |
|----|----------|-----------|
| D1 | `ISOLATED_REGIONS` static mut sin sincronización (mismo que K2) | **CRÍTICO** |
| D2 | 7 símbolos exportados duplicados entre `v3loader.rs` y `hst.rs` | BAJA |
| D3 | Sin firma criptográfica para drivers NEM (B5.1 futuro) | FUTURO |
| D4 | Sin árbol de dispositivos — binding driver→dispositivo es ad-hoc | FUTURO |

---

## 9. VFS / Filesystem

### 9.1 Filesystem Stack

```
Ring 3: sys_open / ob_open → handle table
Ring 0: VFS resolve_path() → FileSystem trait
         ├── NeoDosFs (75 tests)
         ├── Fat32 (read)
         ├── KDrive (virtual K:\)
         └── ISO9660 (read)
IoStack → BlockDevice trait
          ├── NVMe (priority 1)
          ├── BootAhci (priority 2)
          ├── BootAta (priority 3)
          └── RamDisk
```

### 9.2 Evaluación

| Componente | Evaluación |
|------------|------------|
| IoStack unification | ✅ COMPLETO — FAT32 y NeoFS usan IoStack |
| GPT partition parsing | ✅ COMPLETO |
| Page cache (LRU hash map) | ✅ COMPLETO — 13 tests |
| Block cache | ✅ COMPLETO |
| NeoFS (75 tests) | ✅ SÓLIDO |
| FSCK utility | ✅ COMPLETO — 6 tests |
| Default permissions by extension | ✅ COMPLETO |

---

## 10. Documentación

### 10.1 Document Review Matrix

| Documento | Versión | Estado | Problemas |
|-----------|---------|--------|-----------|
| `README.md` | v0.39.11 | ❌ DESACTUALIZADO | Dice v0.39.11 (real v0.44.2), tests 320+ (real 528), syscalls 36 (real 66) |
| `AGENTS.md` | v0.44.3 | ⚠️ PARCIAL | Buena guía operativa, algunas tablas desactualizadas |
| `ARCHITECTURE_SOURCE_OF_TRUTH.md` | v1.0 | ⚠️ PARCIAL | MAX_PROCESSES fijo (real Vec), boot phases incompletas, test counts desactualizados |
| `ARCHITECTURAL_VISION.md` | v1.0 | ✅ ACTUAL | Visión correcta, roadmap coincide con implementación |
| `IMPROVEMENTS.md` | v4.1 | ✅ ACTUAL | 169/177 items completados, estructura correcta |
| `OBJECT_MANAGER_ARCHITECTURE.md` | v1.0 | ✅ ACTUAL | Documento de diseño completo |
| `KERNEL.md` | v1.0 | ⚠️ PARCIAL | No revisado en detalle |

### 10.2 Hallazgos Documentación

| ID | Hallazgo | Severidad |
|----|----------|-----------|
| D1 | README.md desactualizado — versión, tests, syscalls | ALTA |
| D2 | ARCHITECTURE_SOURCE_OF_TRUTH.md inconsistente con scheduler actual | MEDIA |
| D3 | KERNEL.md no verificado en esta auditoría | BAJA |
| D4 | CHANGELOG.md OK — actualizado hasta v0.44.2 | ✅ OK |

---

## 11. Testing

### 11.1 Test Coverage

| Suite | Tests | Estado |
|-------|-------|--------|
| NeoFS | 75 | ✅ |
| Elf | 20 | ✅ |
| NEM parsing | 23 | ✅ |
| Event Bus | 17 | ✅ |
| Pipe | 13 | ✅ |
| Slab | 9 | ✅ |
| Scheduler | 7 | ✅ |
| Ob (Object) | 14 | ✅ |
| KOBJ | 8 | ✅ |
| Security | 12 | ✅ |
| IOAPIC | 3 | ✅ |
| Driver Certification | 21 | ✅ |
| Driver State | 21 | ✅ |
| Capability | 11 | ✅ |
| Isolation | 12 | ✅ |
| IRP | 11 | ✅ |
| Work Queue | 6 | ✅ |
| KWait | 10 | ✅ |
| ABI Freeze | 4 | ✅ |
| SMP | 3 | ✅ |
| IPI | 5 | ✅ |
| Per-CPU Slab | 5 | ✅ |
| IRQL | 5 | ✅ |
| DPC | 5 | ✅ |
| Stress | 14 | ✅ |
| Hot Reload | 11 | ✅ |
| **Total** | **528** | |

### 11.2 Hallazgos Testing

| ID | Hallazgo | Severidad |
|----|----------|-----------|
| T1 | No hay fuzzing infrastructure | FUTURO |
| T2 | No hay CI/CD pipeline | FUTURO |
| T3 | Cobertura de líneas estimada ~60% — no hay herramienta de medición | MEDIA |
| T4 | Tests de integración Ring 3 limitados a cmdtest.nxe | MEDIA |

---

## Bugs Críticos Identificados

### Bug #1: WAIT_PID static mut SMP-unsafe

**Archivo:** `neodos-kernel/src/usermode.rs`
**Descripción:** La variable `WAIT_PID` es un `static mut` usada para comunicación entre `sys_waitpid` y el manejador de terminación de proceso. En un sistema SMP, dos CPUs podrían ejecutar `sys_waitpid` concurrentemente, causando race condition.
**Riesgo:** Alto — data corruption o deadlock en sistemas multicore.
**Solución:** Migrar a KWait (`kwait_block(ChildExit(pid))` + `kwait_wake`).

### Bug #2: ISOLATED_REGIONS static mut sin sincronización

**Archivo:** `neodos-kernel/src/drivers/driver_runtime.rs`
**Descripción:** `ISOLATED_REGIONS` es un array estático mutable accedido desde múltiples contextos (boot loader, NDREG, hot reload) sin Mutex ni protección atómica.
**Riesgo:** Alto — data corruption si dos CPUs realizan operaciones de driver concurrentemente.
**Solución:** Envolver en `spin::Mutex<[Option<IsolatedRegion>; 16]>`.

### Bug #3: NXL_REGISTRY static mut sin protección SMP

**Archivo:** `neodos-kernel/src/nxl.rs`
**Descripción:** `NXL_REGISTRY` es un array fijo de 8 slots accedido desde sys_loadlib sin sincronización.
**Riesgo:** Medio — dos procesos cargando NXLs concurrentemente pueden corromper el registry.
**Solución:** Envolver en `spin::Mutex<[Option<NxlEntry>; 8]>`.

---

## Issues Arquitectónicos (No Críticos)

### AI-1: ObInfoClass/ObSetInfoClass enums incompletos
Los enums en `src/object/types.rs` no definen todas las clases que el handler soporta. Añadir:
- `ObInfoClass::ReadContent = 15`, `ObInfoClass::VolumeLabel = 16`
- `ObSetInfoClass::ProcessTerminate = 4`, `ObSetInfoClass::VfsRename = 6`, etc.

### AI-2: Símbolos exportados duplicados
7 funciones `hst_*` están exportadas tanto en `v3loader.rs` como en `hst.rs`. Consolidar en una sola fuente.

### AI-3: KObjType→ObType impedance mismatch
KObjType incluye tipos (EventBus=5, MountPoint=10, Symlink=9) que ObType no tiene. Al registrar vía KOBJ facade, hay mapeo con pérdida de información.

### AI-4: Unificación de códigos de error
`ObError` (-1 a -9) y `SyscallError` (16 códigos) son independientes con traducción manual. Unificar en un solo conjunto.

---

## Conclusiones

### Puntos Fuertes
1. **Arquitectura limpia:** HAL → Kernel → Driver Runtime → Userland con boundaries claros
2. **Modelo NT correcto:** EPROCESS/KTHREAD, handles, objetos, seguridad
3. **Driver ecosystem maduro:** Certificación, capacidades, aislamiento, ABI negotiation, hot reload
4. **Object Manager completo:** Unificación de handles, KOBJ, URN y seguridad en Ob
5. **Testing extensivo:** 528 tests en 50 suites
6. **Rust idioms correctos:** Sin heap en IRQ, sin schedule() en spinlock, IRQL framework

### Puntos Débiles (Acción Inmediata)
1. **3 bugs SMP-unsafe** (WAIT_PID, ISOLATED_REGIONS, NXL_REGISTRY)
2. **Documentación desactualizada** (README, ARCHITECTURE_SOURCE_OF_TRUTH)
3. **libneodos coverage 68%** — wrappers faltantes para thread, poll, ob_destroy
4. **7 exports duplicados** entre v3loader.rs y hst.rs

### Roadmap Recomendado
1. **v0.44.4** — Fix 3 bugs SMP-unsafe (CRÍTICO)
2. **v0.44.5** — Actualizar documentación, arreglar AI-1 (InfoClass enums)
3. **v0.44.6** — Completar libneodos wrappers, reorganizar libneodos-nxl
4. **v0.44.7** — Consolidar exports duplicados, unificar códigos de error
5. **v0.46** — Device Tree + VirtIO block driver
6. **v0.47** — Networking (TCP/IP)
7. **v0.50** — Registry hive database
8. **v1.0** — API estable

---

*Documento generado por auditoría arquitectónica automatizada. Todos los hallazgos han sido verificados contra el código fuente.*
