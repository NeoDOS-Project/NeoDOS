# NeoDOS — Visión Arquitectónica

> **Autor:** Arquitecto Jefe de Sistemas Operativos
> **Versión del documento:** v1.0
> **Fecha:** 2026-06-21
> **Estado:** Auditoría completa — Plan director

---

## Índice

1. [Resumen Ejecutivo](#1-resumen-ejecutivo)
2. [¿Qué es NeoDOS?](#2-qué-es-neodos)
3. [Filosofía de Diseño](#3-filosofía-de-diseño)
4. [Arquitectura Ideal](#4-arquitectura-ideal)
5. [Diagnóstico: Estado Actual vs. Ideal](#5-diagnóstico-estado-actual-vs-ideal)
6. [Decisiones a Cambiar](#6-decisiones-a-cambiar)
7. [Nuevas Abstracciones Fundamentales](#7-nuevas-abstracciones-fundamentales)
8. [Congelación de Interfaces (ABI/API Freeze)](#8-congelación-de-interfaces-abiapi-freeze)
9. [Roadmap por Versiones](#9-roadmap-por-versiones)
10. [Guía de Contribución Estratégica](#10-guía-de-contribución-estratégica)

---

## 1. Resumen Ejecutivo

NeoDOS es un sistema operativo de 64 bits con arquitectura híbrida (kernel monolítico con subsistema de drivers basado en microkernel). Está escrito en Rust, arranca en UEFI, y ofrece un modelo de procesos estilo NT con planificación prioritaria, drivers en espacio aislado, y un sistema de archivos propio con influencias DOS.

**Fortalezas actuales:**
- Código 100% Rust (sin C en la base)
- HAL con asm aislado en `hal/raw/`
- 537 tests automáticos en kernel
- Certificación de drivers con 7 estados
- Sistema de capacidades para control de acceso granular
- IRQL framework para prioridad de interrupciones
- Soporte SMP con IPI, TLB shootdown, work stealing
- Phase-boot determinista con 11 fases
- Parser ELF64 con validación de espacio de direcciones
- Event Bus centralizado con prioridades y filtros

**Debilidades estructurales:**
- Arrays de tamaño fijo en 8 subsistemas críticos
- Espacio de usuario de solo 4 MB
- Bitmap de buddy allocator limitado a 4 GB
- Sin ASLR
- Sin sys_fork
- Sin modelo de privilegios completo
- Sin gestión de energía
- Sin árbol de dispositivos
- Sin namespace por proceso
- Sin driver de red

**Veredicto:** NeoDOS tiene una base excepcionalmente sólida para un proyecto de su madurez. Las decisiones arquitectónicas son en general correctas, pero algunas ataduras heredadas de la fase prototipo (arrays fijos, espacio de usuario pequeño, ausencia de fork) deben resolverse antes de escalar. El sistema necesita una **fase de maduración** antes de una **fase de expansión**.

---

## 2. ¿Qué es NeoDOS?

### Declaración de Identidad

NeoDOS es un sistema operativo moderno de 64 bits para la plataforma x86-64, diseñado como plataforma de experimentación, aprendizaje e investigación en ingeniería de sistemas operativos. No aspira a ser otro Linux ni otro Windows — aspira a ser un **laboratorio de ideas** donde las decisiones arquitectónicas sean explícitas, rastreables y debatibles.

### ¿Qué problema resuelve?

1. **Complejidad innecesaria de los SO modernos.** Linux tiene >30M de líneas de código. El kernel de un SO debería ser comprensible por una sola persona. NeoDOS demuestra que se puede tener un SO funcional con <50K líneas.

2. **Falta de sistemas didácticos pero reales.** La mayoría de los "SO educativos" son juguetes. NeoDOS es un SO real: multiproceso, SMP, drivers en espacio aislado, sistema de archivos propio, red (futuro).

3. **Rust como lenguaje de sistemas.** Demostrar que se puede escribir un SO completo en Rust sin sacrificar rendimiento ni control de hardware.

### ¿Qué NO es NeoDOS?

- No es un clon de DOS (aunque lleva el nombre por herencia)
- No es un clon de NT (aunque toma prestados conceptos)
- No pretende reemplazar Linux/Windows en producción
- No tiene objetivos de compatibilidad binaria con ningún otro SO

---

## 3. Filosofía de Diseño

### Principios Rectores

1. **Explícito sobre mágico.** Cada asignación de memoria, cada cambio de contexto, cada transición de estado debe ser rastreable en el código. Sin "scheduler magic", sin "memory manager magic".

2. **Capas, no montones.** Separación clara de responsabilidades. HAL es HAL. VFS es VFS. Scheduler es scheduler. Las dependencias prohibidas se documentan y se verifican con herramientas automáticas (`check_deps.py`).

3. **Fallo rápido, fallo claro.** Cuando algo va mal, el sistema debe detenerse con un mensaje que identifique el componente, el error y la posible causa. Sin pánicos genéricos.

4. **El kernel es pequeño.** Todo lo que pueda vivir en Ring 3 debe vivir en Ring 3. El kernel proporciona mecanismos, no políticas.

5. **El driver es un ciudadano de segunda clase.** Los drivers NEM se ejecutan en un espacio aislado, tienen capacidades explícitas, pasan por un pipeline de certificación, y no confían en el kernel (ni el kernel confía en ellos).

6. **La compatibilidad es un contrato.** Una vez que una syscall, una estructura ABI, o un formato de driver se declara estable, no cambia. El versionado semántico se aplica a nivel de kernel, no solo de API.

7. **Los tests son especificación.** 537 tests no son una métrica de calidad — son la especificación ejecutable del comportamiento del kernel. Si no hay test, el comportamiento no está definido.

---

## 4. Arquitectura Ideal

### 4.1 Mapa de Subsistemas y Capas

```
┌─────────────────────────────────────────────────────────────────────┐
│                        USERMODE (Ring 3)                           │
│  ┌─────────┐ ┌──────────┐ ┌───────────┐ ┌──────────────────────┐  │
│  │neoshell │ │neoinit   │ │userbin/*  │ │libneodos (NXL DLL)   │  │
│  │.nxe     │ │.nxe      │ │.nxe       │ │(io/fs/syscall/mem)   │  │
│  └────┬────┘ └────┬─────┘ └─────┬─────┘ └──────────┬───────────┘  │
│       │           │             │                   │              │
│  ┌────┴───────────┴─────────────┴───────────────────┴──────────┐  │
│  │                    SYSCALL GATE (INT 0x80)                  │  │
│  │              SSDT — 256 slots, permission check             │  │
│  └───────────────────────────┬─────────────────────────────────┘  │
├──────────────────────────────┼────────────────────────────────────┤
│                   KERNEL (Ring 0)                                 │
│  ┌───────────────────────────┴─────────────────────────────────┐  │
│  │                    SYSTEM SERVICES                          │  │
│  │  ┌─────────┐ ┌──────────┐ ┌───────────┐ ┌───────────────┐  │  │
│  │  │Process  │ │Scheduler │ │Memory Mgr │ │Security Ref   │  │  │
│  │  │Manager  │ │(4-level) │ │(Buddy+    │ │Monitor        │  │  │
│  │  │EPROCESS │ │Aging     │ │ Slab)     │ │SID/Token/ACL  │  │  │
│  │  │KTHREAD  │ │WorkSteal │ │DemandPgng │ │SeAccessCheck  │  │  │
│  │  └─────────┘ └──────────┘ └───────────┘ └───────────────┘  │  │
│  │  ┌─────────┐ ┌──────────┐ ┌───────────┐ ┌───────────────┐  │  │
│  │  │VFS      │ │KOBJ      │ │IPC/Pipes  │ │Event Bus      │  │  │
│  │  │26 drives│ │Registry  │ │IRP System │ │(2 priorities)  │  │  │
│  │  │MountPts │ │Namespace │ │Async I/O  │ │+ Filters      │  │  │
│  │  └─────────┘ └──────────┘ └───────────┘ └───────────────┘  │  │
│  │  ┌─────────┐ ┌──────────┐ ┌───────────┐ ┌───────────────┐  │  │
│  │  │APC/DPC  │ │WorkQueue │ │Crash Dump │ │Timer/HPET/APIC│  │  │
│  │  │Engine   │ │Deferred  │ │Framework  │ │PIT fallback   │  │  │
│  │  └─────────┘ └──────────┘ └───────────┘ └───────────────┘  │  │
│  └───────────────────────────┬─────────────────────────────────┘  │
│  ┌───────────────────────────┴─────────────────────────────────┐  │
│  │                NEM DRIVER RUNTIME                           │  │
│  │  ┌────────────┐ ┌──────────┐ ┌────────────┐ ┌──────────┐  │  │
│  │  │Certification│ │Capability│ │Isolation   │ │Boot      │  │  │
│  │  │Pipeline    │ │System    │ │Layer (X4)  │ │Loader    │  │  │
│  │  │(8 states)  │ │(12 flags)│ │(16MB/16sl) │ │(DepRes)  │  │  │
│  │  └────────────┘ └──────────┘ └────────────┘ └──────────┘  │  │
│  │  ┌────────────────────────────────────────────────────────┐ │  │
│  │  │         ABSTRAIDO POR CAPAS (ABI v0.4)                 │ │  │
│  │  └────────────────────────────────────────────────────────┘ │  │
│  └───────────────────────────┬─────────────────────────────────┘  │
│  ┌───────────────────────────┴─────────────────────────────────┐  │
│  │  HAL (Hardware Abstraction Layer)                          │  │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐   │  │
│  │  │ hal/raw/     │ │ hal/safe/    │ │ hal/x64/         │   │  │
│  │  │ (asm: STI,   │ │ (Msr trait,  │ │ (extern "C" ABI  │   │  │
│  │  │  CLI, IN/OUT,│ │  read_msr,   │ │  surface, 26 fn) │   │  │
│  │  │  CPUID, TSC) │ │  write_msr)  │ │  without_int     │   │  │
│  │  └──────────────┘ └──────────────┘ └──────────────────┘   │  │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐   │  │
│  │  │ hal/pci/     │ │ interrupts/  │ │ timers/          │   │  │
│  │  │ (ECAM MMIO)  │ │ (IOAPIC,     │ │ (HPET, APIC,     │   │  │
│  │  │              │ │  MSI-X)      │ │  PIT fallback)   │   │  │
│  │  └──────────────┘ └──────────────┘ └──────────────────┘   │  │
│  └─────────────────────────────────────────────────────────────┘  │
│  ┌─────────────────────────────────────────────────────────────┐  │
│  │  ARCH (x86_64) — GDT, IDT, Paging (4-level), SMP trampoline│  │
│  └─────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

### 4.2 Modelo de Objetos del Sistema

Todo recurso del sistema es un **objeto** administrado por el **KOBJ Registry**:

```
KObj (Kernel Object)
├── Type: Process, Thread, Driver, Device, Pipe, File, 
│         EventBus, BlockDevice, Filesystem, MemoryRegion,
│         Symlink, MountPoint, Directory, RegistryKey,
│         Timer, Dpc, Apc, IopCompletion
├── ID (u64, monotónico)
├── RefCount (u32, atómico)
├── Name (hasta 64 bytes UTF-8)
├── Flags (bitmask: persistent, volatile, system, user)
├── CreationTick
├── NativeID (ID interno del subsistema)
└── SecurityDescriptor (SID propietario + ACL)
```

**Principio:** No existe recurso sin objeto. Pipe, archivo abierto, driver, proceso, hilo — todos se registran en KOBJ al crear y se dan de baja al destruir.

### 4.3 Modelo de Procesos (estilo NT, no Unix)

```
EPROCESS (Process)
├── PID (u64)
├── Token (Security)
├── AddressSpace (SegmentInfo[] — loaded segments)
├── HandleTable (HandleEntry[] — fds, pipes, files, devices)
├── KTHREAD[] (1..N threads)
├── CWD (current working directory)
├── Heap (base, break, pages)
├── MmapRegions (VMA list)
├── Priority
├── ParentPID
└── ExitStatus
```

**Principio:** Procesos creados solo por `sys_spawn` (fork NO es necesario en modelo NT). Hilos creados por `sys_thread_create`. Comunicación por pipes, eventos, memoria compartida.

### 4.4 Modelo de Drivers (servicio aislado)

```
NEM Driver
├── Categoría: BOOT | SYSTEM | DEMAND
├── Pipeline: Loaded → Init → Registered → Bound → Active
├── Capacidades (bitmask de 12 flags)
├── Aislamiento (slot en DRIVER_ISO de 1 MB)
├── ABI versionado (min, target, max)
├── Dependencias (resolución topológica)
├── Hot reload (Unloading → Unloaded → Loaded)
└── Event Bus subscription (filtros por tipo/fuente/dispositivo)
```

**Principio:** Driver no es kernel, ni es user-mode. Es un **tercer espacio** con su propio pipeline de certificación, sistema de capacidades, y espacio de direcciones aislado.

### 4.5 Modelo de Seguridad

```
Token (cada proceso)
├── SID (identidad, formato S-R-I-S*)
├── IsAdmin (bool)
├── Groups[] (futuro)
├── Privileges[] (futuro: SeTakeOwnership, SeBackup, etc.)
└── Restricted (bool, para tokens restringidos)

Access Check (SeAccessCheck)
1. ¿Token es admin? → GRANT (bypass)
2. ¿SD es NULL? → DENY
3. ¿DACL vacío? → DENY
4. Iterar ACEs: Deny (DENY) → Allow (GRANT)
5. Sin match → DENY

Security Descriptor (por objeto)
├── Owner SID
├── Group SID
├── DACL (lista de ACEs)
└── SACL (futuro: audit)
```

**Principio:** La seguridad no es opcional. Todo objeto tiene SecurityDescriptor. Todo acceso pasa por SeAccessCheck. El admin bypass es explícito y rastreable.

---

## 5. Diagnóstico: Estado Actual vs. Ideal

### 5.1 Lo que ya está bien (NO TOCAR)

| Componente | Estado | Razón |
|-----------|--------|-------|
| HAL v0.4 raw/safe split | ✅ Excelente | Asm confinado, tipos seguros, MSR trait |
| Boot phases | ✅ Sólido | 11 fases con orden determinista |
| EPROCESS/KTHREAD split | ✅ Correcto | Modelo NT limpio |
| IRQL framework | ✅ Bien | Reemplaza CLI/STI, per-CPU |
| Driver certification pipeline | ✅ Muy bien | 7 estados, transiciones estrictas |
| Capability system | ✅ Bien | 12 flags, defaults por categoría |
| NEM v3 format | ✅ Bien | 80B header, relocs, symbols, ABI |
| ABI negotiation | ✅ Bien | min/target/max version |
| KOBJ registry + ObObjectTable | ✅ Bien | Object Manager con ObId, refcount, close auto-destroy |
| ABI freeze validation | ✅ Bien | Boot-time verification of event/capability/IOAPIC frozen interfaces |
| HandleEntry full Ob | ✅ Bien | All handle types register as Ob objects with close() cleanup |
| Event Bus v2 | ✅ Bien | Lock-free SPSC, filtros, prioridades |
| IRP system | ✅ Bien | Pool de 64 slots, blocking, chaining |
| APC/DPC engines | ✅ Correcto | Per-CPU, nesting limit |
| Page cache + block cache | ✅ Bien | LRU hash map, dirty tracking |
| SMP boot | ✅ Bien | INIT-SIPI-SIPI, AP trampoline |
| IPI infrastructure | ✅ Bien | Reschedule, TLB shootdown, call-function |
| Per-CPU slab | ✅ Bien | GS-segment hot cache, lock-free fast path |
| ELF64 loader | ✅ Bien | 5 validaciones de seguridad |
| Dependency resolver | ✅ Bien | Topological sort, cycle detection |
| KWait Unified Wait Engine | ✅ Bien | 7 WaitReason variants, kwait_block/wake |
| Crash dump framework | ✅ Bien | Lock-free ring, serial dump |
| FSCK | ✅ Bien | Superblock + inode + directory walk |
| Driver isolation (X4) | ✅ Bien | Pointer validation, sandbox mode |

### 5.2 Lo que necesita refactorización (MEJORAR)

| Componente | Problema | Prioridad |
|-----------|----------|-----------|
| Arrays fijos | 8 subsistemas con límites duros | **ALTA** |
| Buddy bitmap 4GB | Frame allocator no escala a >4GB | **ALTA** |
| User window 4MB | Demasiado pequeño para aplicaciones reales | **ALTA** |
| Static buffers en syscalls | BIN_BUF/CMD_BUF de 64KB, no reentrantes | **ALTA** |
| Sin ASLR | Todas las cargas en direcciones fijas | **MEDIA** |
| Scheduler linear scans | O(n) en busca de threads | **MEDIA** |
| Security model incompleto | Sin grupos, sin privilegios, sin audit | **MEDIA** |
| VFS drive letters | Es el modelo NT (\DosDevices\C: → \Device\...) | ✅ Correcto — no tocar |
| Pipe 4KB × 16 | Pequeño y limitado | **MEDIA** |
| SeAccessCheck simplista | Sin herencia de ACE, sin SACL | **BAJA** |

### 5.3 Lo que falta (AÑADIR)

| Componente | Descripción | Prioridad |
|-----------|-------------|-----------|
| Red/Networking | NIC driver + TCP/IP stack | **MEDIA** |
| sys_fork (o clone) | Creación ligera de procesos | **BAJA** (no NT-style) |
| Gestión de energía | Suspend/resume, S-states | **BAJA** |
| Árbol de dispositivos | Enumeración jerárquica | **MEDIA** |
| Registry persistente | Base de datos de configuración | **MEDIA** |
| Per-process namespace | Vistas de sistema de archivos por proceso | **BAJA** |
| Audit trail | Registro de eventos de seguridad | **BAJA** |
| Symlinks en VFS | Enlaces simbólicos en sistema de archivos | **BAJA** |

### 5.4 Matriz de Riesgos Arquitectónicos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|-------------|---------|------------|
| Arrays fijos saturan | Alta (próximos 6 meses) | Alto (pánico en runtime) | Convertir a dinámicos (Vec/ slab) |
| Buddy bitmap overflow | Media (si RAM >4GB) | Alto (kernel no arranca) | Bitmap dinámico por rango |
| User window overflow | Alta (al crecer apps) | Medio (carga ELF falla) | Ampliar a 32MB mínimo |
| Static buffers corruptos | Baja | Alto (data race silencioso) | Reemplazar por allocaciones dinámicas |
| Sin ASLR → exploit | Media (con user-mode) | Medio | Implementar ASLR básico |

---

## 6. Decisiones que Cambiaría

### 6.1 Cambios Inmediatos (v0.40–v0.45)

#### 1. Arrays fijos → Vec dinámico (Breaking change interno)

**Dónde:** EPROCESS[16], KTHREAD[32], pipes[16], driver slots[16], mount points[8], IRP pool[64], memory regions[32], event handlers[64]

**Qué hacer:** Convertir a `Vec<T>` o `Slab<T>` (slab allocator reutilizado). El scheduler sigue teniendo un límite práctico (lo define la memoria disponible, no una constante). Para hot paths, mantener arrays pequeños + overflow a dinámico.

**Riesgo:** Cambio estructural mayor. Requiere reescribir las funciones de búsqueda lineal.

**Beneficio:** Elimina el límite superior en 8 subsistemas. El scheduler escala con la memoria disponible.

#### 2. Buddy bitmap → Estructura que soporte >4GB

**Dónde:** `memory/buddy.rs` — `BITMAP_WORDS = 16384`

**Qué hacer:** Convertir el bitmap de un array fijo a una estructura de rango dinámico:
- Soporte para múltiples rangos de bitmap (cada rango ≤4GB)
- O añadir listas de bloques libres por orden para memoria >4GB
- Alternativa: usar radix tree (como Linux) para el estado de frames

**Riesgo:** Medio. El buddy es estable y testado. Cambiar la estructura de datos requiere re-test.

#### 3. User window: 4MB → 32MB mínimo

**Dónde:** `USER_BASE = 0x400000`, `USER_LIMIT = 0x800000` y paging.rs

**Qué hacer:** Mover USER_LIMIT a `0x2000000` (32MB; 256 slots de 128KB). O mejor: usar un esquema de ventana dinámica donde cada proceso tenga un rango variable.

**Impacto:** Cambia la validación de ELF, la slot allocation, y posiblemente el layout. Breaking para binarios existentes que asumen USER_LIMIT.

#### 4. Static buffers → Allocación dinámica

**Dónde:** `BIN_BUF[65536]`, `CMD_BUF[65536]` en syscall/mod.rs

**Qué hacer:** Usar `Vec<u8>` temporal allocado en heap para cada invocación de spawn/readfile. El costo de allocación es insignificante comparado con la carga de un ELF.

#### 5. SeAccessCheck: iteración completa de ACEs

**Dónde:** `security/access.rs`

**Qué hacer:** Actualmente el algoritmo itera ACEs y retorna en el primer match. Para NT-compatibilidad, debe:
1. Evaluar TODOS los deny ACEs primero
2. Luego evaluar TODOS los allow ACEs
3. Si algún deny match → DENY
4. Si algún allow match → GRANT
5. Ninguno → DENY

### 6.2 Cambios a Medio Plazo (v0.46–v0.50)

#### 6. ASLR (Address Space Layout Randomization)

Implementar ASLR gradual:
1. **Enlace aleatorio** (v1): offset aleatorio en la base de carga del ELF, manteniendo el layout relativo
2. **Pila aleatoria** (v2): posición aleatoria de la pila Ring 3
3. **Heap aleatorio** (v3): posición aleatoria del heap de usuario

#### 7. Namespace de procesos

Cada EPROCESS debería tener su propia vista del namespace global:
- `/dev`, `/proc`, `/sys` virtuales por proceso
- Opens y fds aislados (que ya lo están via handle table)
- Chroot-lite: restringir la resolución de VFS a un subárbol

#### 8. Compatibilidad POSIX básica (opcional)

Si se desea ejecutar software POSIX, añadir:
- `sys_fork` o `sys_clone`
- `sys_execve` (reemplazar imagen de proceso)
- `sys_signal` (o equivalente NT-style)
- `sys_poll` / `sys_select`

**Nota:** No es necesario para la identidad central de NeoDOS. Solo si hay demanda de portar software.

### 6.3 Decisiones de No-Cambio (proteger)

| Decisión | No cambiar porque |
|----------|------------------|
| HAL raw/safe split | Es la base de toda portabilidad |
| SSDT dispatch (O(1)) | Es más rápido y predecible que match |
| Modelo de drivers NEM | Diferencia competitiva de NeoDOS |
| Procesos estilo NT vs Unix | Elección arquitectónica deliberada |
| Rust como único lenguaje | Coherencia del sistema |
| Phase-boot determinista | Depuración y testeo |
| IRQL framework | Reemplaza correctamente CLI/STI |
| Eprocess/Kthread split | Base del modelo de procesos |

---

## 7. Nuevas Abstracciones Fundamentales

### 7.1 Slab<T> — Contenedor de capacidad variable

**Qué es:** Un contenedor que combina la eficiencia de un array de tamaño fijo (para el caso común) con la flexibilidad de un Vec (para overflow). Similar a `smallvec` pero integrado con el slab allocator del kernel.

```rust
pub struct Slab<T> {
    fast: [Option<T>; N],       // Hot path: inline array
    overflow: Vec<T>,           // Slow path: heap allocation
    fast_count: AtomicU16,      // Count in fast array
}
```

**Dónde usar:** EPROCESS (N=16), KTHREAD (N=32), driver slots (N=16), pipes (N=16), handlers (N=64), IRP pool (N=64).

**Beneficio:** En el caso común (≤N elementos), acceso O(1) sin heap. En carga alta, crece sin límite.

### 7.2 Unified Wait Engine (KWait)

**Qué es:** Una abstracción única para toda espera bloqueante, reemplazando los mecanismos ad-hoc actuales (pipe waiting, IRP waiting, thread joining, waitpid STI/HLT loop).

```rust
pub enum WaitReason {
    PipeRead { pipe_id: u16 },
    IrpComplete { irp_id: u32 },
    ThreadJoin { tid: u32 },
    ChildExit { pid: u32 },
    Event { event_type: u32, mask: u64 },
    Timer { timeout_ms: u64 },
    Any(Vec<WaitReason>),  // WaitAny (NT-style)
    All(Vec<WaitReason>),  // WaitAll
}
```

**Dónde usar:**
- `sys_read` en pipe vacío → KWait::PipeRead
- `sys_waitpid` → KWait::ChildExit
- `sys_thread_join` → KWait::ThreadJoin
- Block on IRP completion → KWait::IrpComplete

**Beneficio:** Un solo mecanismo de bloqueo/despertar. Un solo lugar donde el scheduler interactúa con la espera. Posibilidad de WaitAny/WaitAll (como NT `KeWaitForMultipleObjects`).

### 7.3 Device Tree + Resource Manager

**Qué es:** Un árbol jerárquico de dispositivos físicos y lógicos, detectados automáticamente y manejados por el kernel. Cada dispositivo tiene:
- Tipo (PCI, USB, ACPI, Legacy)
- Dirección de bus (bus:dev:func para PCI)
- Recursos asignados (MMIO, IRQ, DMA, I/O ports)
- Driver vinculado (NEM driver que lo maneja)
- Estado (presente, activo, error, ausente)

```rust
pub struct DeviceNode {
    pub device_type: DeviceType,       // Pci, Acpi, Legacy, Virtual
    pub parent: Option<DeviceId>,
    pub children: Vec<DeviceId>,
    pub driver: Option<DriverId>,
    pub resources: ResourceList,       // MMIO ranges, IRQ lines, DMA channels
    pub state: DeviceState,            // Present, Active, Error, Removed
    pub description: [u8; 64],        // Human-readable
}
```

**Dónde usar:**
- PCI bus scan → registra dispositivos PCI
- ACPI → registra dispositivos ACPI (HPET, IOAPIC)
- ISA/Legacy → registra dispositivos heredados (PIT, PIC, PS/2)
- AHCI/ATA → registra discos como hijos del controlador
- NEM drivers → se vinculan a dispositivos en el árbol

**Beneficio:** Reemplaza la detección ad-hoc actual (HPET busca ACPI, PCI busca ECAM, etc.) por un bus manager que construye el árbol durante el boot.

### 7.4 Registry (Base de Datos de Configuración)

**Qué es:** Una base de datos jerárquica tipo Windows Registry con claves, valores, y tipos. Persistente en disco en `C:\System\Config\`.

```
HKEY_LOCAL_MACHINE\
├── Hardware\
│   ├── DeviceTree\     (auto-generado del Device Tree)
│   ├── Memory\         (tamaño, layout, zonas)
│   └── ACPI\           (tablas DSDT/SSDT)
├── System\
│   ├── Drivers\        (configuración de drivers)
│   ├── Services\       (servicios de inicio)
│   └── CurrentControlSet\  (boot configuration)
└── Software\
    └── (user settings)

HKEY_CURRENT_USER\
├── Environment\        (variables de entorno)
├── Keyboard\           (layout, key repeat)
└── (app settings)
```

**Dónde usar:**
- Configuración de drivers (en lugar de boot.cfg)
- Variables de entorno persistentes
- Preferencias de usuario
- Configuración de red

**Beneficio:** Configuración unificada, persistente y accesible vía API. Reemplaza los archivos .cfg actuales con una base de datos estructurada.

### 7.5 I/O Completion Port (IOCP)

**Qué es:** Un mecanismo de notificación de E/S asíncrona para aplicaciones, similar a los IOCP de Windows o los kqueue/epoll de Unix.

```rust
pub struct IoCompletionPort {
    pub port_id: u32,
    pub entries: LockFreeQueue<CompletionEntry>,  // SPSC
    pub associated_handles: Vec<HandleId>,         // IRPs, pipes, events
}
```

**Dónde usar:**
- Aplicaciones de red (servidores)
- E/S de archivos asíncrona
- Timers
- Señales de proceso

**Beneficio:** Permite aplicaciones escalables orientadas a eventos sin threads por conexión. Puerta de entrada para el stack de red.

---

## 8. Congelación de Interfaces (ABI/API Freeze)

### 8.1 Interfaces Congeladas desde v0.40

| Interfaz | Versión | Notas |
|----------|---------|-------|
| HAL ABI v0.4 | v0.40 | 26 funciones extern "C". No añadir, no quitar, no cambiar firma. Solo añadir nuevas v0.5 paralelas. |
| Syscall números 0–30 | v0.40 | RAX 0–30 no se reasignan. Añadir nuevas syscalls en ranuras 31+. |
| BootInfo struct | v0.40 | Contrato UEFI bootloader→kernel. No cambiar campos. Añadir al final si es necesario. |
| NEM header v3 | v0.40 | 80-byte header, 4 sections, relocs, symbols. No cambiar formato. Nuevos campos al final. |
| ABI version scheme | v0.40 | min/target/max semántica congelada. |
| KOBJ entry struct | v0.40 | Estructura exportada a user-mode via sys_kobj_enum. |
| SysDateTime struct | v0.40 | Formato ABI exportado. |
| MemInfo struct | v0.40 | Formato ABI exportado. |
| DirEntryRaw struct | v0.40 | Formato ABI exportado. |
| DriveInfoRaw struct | v0.40 | Formato ABI exportado. |

### 8.2 Interfaces en Congelación Progresiva (v0.42–v0.45)

| Interfaz | Congela en | Notas |
|----------|-----------|-------|
| Event types (0–15) | v0.42 | No reasignar types existentes. Añadir nuevos en 16+. |
| Event struct (56 bytes) | v0.42 | Formato ABI de eventos. |
| Capability flags (1–2048) | v0.42 | No reasignar bits existentes. Añadir en bit 12+. |
| Driver error codes | v0.43 | 12 códigos existentes no se reasignan. ✅ CONGELADO |
| Pipe refcount protocol | v0.43 | Comportamiento de dup2/close con pipes. ✅ CONGELADO |
| IRP pool (64 slots) | v0.43 | Límite fijo. Aumentar solo con nueva versión. ✅ CONGELADO |
| FileSystem trait | v0.44 | Métodos existentes no se modifican. Añadir default. |
| Security SID format | v0.44 | Formato S-R-I-S* congelado. |
| Driver state machine | v0.45 | 8 estados existentes + transiciones. Nuevos estados solo al final. |

### 8.3 Política de Versionado

```
vMAJOR.MINOR.PATCH

MAJOR: Cambio ABI/API incompatible (rompe drivers/binarios existentes)
MINOR: Nueva funcionalidad compatible hacia atrás
PATCH: Bugfixes, optimizaciones, tests
```

**Reglas:**
- v0.40.x: ciclo actual (API inestable, cambios mayores permitidos)
- v1.0.0: primera API estable (todas las interfaces de 8.1 + 8.2 congeladas)
- v1.x.x: API estable, solo cambios compatibles
- v2.0.0: nueva API mayor (con migración documentada)

---

## 9. Roadmap por Versiones

### Fase 1: Maduración (v0.40 – v0.45)
*Duración estimada: 6 meses*

**Objetivo:** Eliminar las limitaciones estructurales que impedirán escalar.

| Versión | Hitos | Impacto |
|---------|-------|---------|
| **v0.40** | — Buddy bitmap dinámico (soporte >4GB RAM) | ✅ Alto |
| | — User window 4MB → 32MB | ⚠️ Breaking |
| | — Static buffers → heap allocation | ✅ Alto |
| | — Priority 0 eliminada del roadmap | |
| **v0.41** | — Slab<T> contenedor para arrays fijos | ✅ Alto |
| | — Scheduler: Vec<EPROCESS>, Vec<KTHREAD> | ⚠️ Breaking |
| | — Pipe: Vec de buffers, 4KB → 16KB por defecto | ✅ Medio |
| | — Driver slots: Vec dinámico | ✅ Medio |
| **v0.42** | — IOAPIC/MSI-X: congelar ABI | ✅ COMPLETADO |
| | — Event Bus: congelar tipos 0–15 | ✅ COMPLETADO |
| | — Capability flags: congelar bits | ✅ COMPLETADO |
| | — Unified Wait Engine (KWait) v1 | ✅ COMPLETADO |
| | — HandleEntry full Ob integration (OB-004) | ✅ COMPLETADO |
| | — ABI freeze validation en boot | ✅ COMPLETADO |
| **v0.43** | — SeAccessCheck NT-compatible, sys_poll(), pipe/IRP freeze | ✅ Completado |
| | — Driver error codes: congelar | ✅ Bajo |
| | — Pipe/IRP: congelar protocolos | ✅ Bajo |
| | — sys_poll() para wait múltiple (via KWait) | ✅ Medio |
| **v0.44** | — FileSystem trait: congelar API | ✅ Bajo |
| | — Security SID: congelar formato | ✅ Bajo |
| | — ASLR v1: base aleatoria para ELF | ✅ Alto |
| | — sys_dup3, sys_fcntl (estilo POSIX) | ✅ Medio |
| **v0.45** | — Driver state machine: congelar | ✅ Bajo |
| | — Registry v1 (persistente en disco) | ✅ Alto |
| | — Refactor completo de VFS (preparar symlinks) | ✅ Medio |

### Fase 2: Expansión (v0.46 – v0.50)
*Duración estimada: 4 meses*

**Objetivo:** Añadir funcionalidades transformadoras.

| Versión | Hitos | Impacto |
|---------|-------|---------|
| **v0.46** | — Device Tree + Resource Manager | ✅ Alto |
| | — PCI: auto-vincular drivers a dispositivos | ✅ Medio |
| | — sys_ioctl() para control de dispositivos | ✅ Medio |
| **v0.47** | — Networking: RTL8139/e1000 NIC driver (NEM) | ✅ Alto |
| | — Stack TCP/IP mínimo (lwIP o similar) | ✅ Alto |
| | — sys_socket / sys_bind / sys_connect | ✅ Alto |
| **v0.48** | — Async I/O: IOCP v1 | ✅ Alto |
| | — sys_accept / sys_send / sys_recv | ✅ Alto |
| | — Servidor HTTP mínimo (demonio) | ✅ Medio |
| **v0.49** | — ASLR v2: pila aleatoria, heap aleatorio | ✅ Medio |
| | — Performance: profile-guided optimization | ✅ Medio |
| | — Benchmarking suite automática | ✅ Medio |
| **v0.50** | — Namespace por proceso (chroot-lite) | ✅ Alto |
| | — Symlinks en VFS | ✅ Medio |
| | — Audit trail (SACL) | ✅ Bajo |

### Fase 3: Estabilización (v0.51 – v1.0.0)
*Duración estimada: 6 meses*

**Objetivo:** Bugfixes, documentación, tests, y preparación para v1.0 estable.

| Versión | Hitos | Impacto |
|---------|-------|---------|
| **v0.51** | — sys_fork (bajo demanda) | ✅ Medio |
| | — sys_signal (mínimo) | ✅ Medio |
| **v0.52** | — Stack de red completo (UDP, DNS, DHCP) | ✅ Alto |
| | — Cliente TFTP / NFS básico | ✅ Medio |
| **v0.53** | — Rendimiento: per-CPU heaps NUMA-aware | ✅ Alto |
| | — Rendimiento: scheduler lock-free | ✅ Alto |
| **v0.54** | — Documentación completa de API | ✅ Alto |
| | — Test coverage >95% de líneas | ✅ Alto |
| **v0.55–0.59** | — Bugfixes, hardening, fuzzing | ✅ Alto |
| **v1.0.0** | — Primera API estable | ⭐ CRÍTICO |

### Después de v1.0

- v1.x: Mantenimiento, nuevas features compatibles
- v2.0: Posible arquitectura microkernel híbrido
- Gestión de energía (S3/S4)
- Virtualización (KVM o Hyper-V enlightenments)
- Multiprocesador NUMA completo
- Gráficos 2D/3D (framebuffer → GPU)

---

## 10. Guía de Contribución Estratégica

### Qué priorizar (orden)

1. **v0.40: Buddy bitmap >4GB** — Sin esto, el kernel no arranca con >4GB RAM
2. **v0.40: User window → 32MB** — Sin esto, las apps no crecen
3. **v0.41: Slab<T> contenedor** — Sin esto, los arrays fijos saturan
4. **v0.42: KWait engine** — Sin esto, el bloqueo ad-hoc es frágil
5. **v0.46: Device Tree** — Sin esto, los drivers dependen de scan ad-hoc
6. **v0.47: Networking** — Con esto, NeoDOS sale al mundo

### Qué evitar (trampas)

- **No añadir features nuevas antes de completar la fase de maduración.** Cada feature nueva se apoya en las abstracciones existentes. Si esas abstracciones son frágiles, la feature será frágil.
- **No implementar sys_fork antes de KWait.** fork necesita wait multiplataforma.
- **No implementar networking antes de Device Tree.** La NIC necesita ser un nodo en el árbol.
- **No implementar ASLR v3 (full randomization) antes de v0.49.** Implementar gradual.

### Filosofía de decisiones

| Situación | Decisión correcta |
|-----------|------------------|
| Duda entre complejidad y simplicidad | Elegir simplicidad. Siempre se puede añadir complejidad después. |
| Duda entre rendimiento y claridad | Elegir claridad primero, optimizar después con benchmarks. |
| Duda entre feature nueva y deuda técnica | Pagar deuda técnica. |
| Duda entre cambiar una abstracción existente o parchearla | Cambiar la abstracción. Los parches se acumulan. |
| Duda entre hacer algo "bien" o "rápido" | Hacerlo "bien". "Rápido" se convierte en permanente. |

---

## Apéndice A: Mapa de Dependencias por Subsistema

```
LEGEND:
─── depends on
-.- may use (optional)
✗── forbidden

HAL ─── Arch (x86_64)
 │
 ├── Memory (buddy, slab, layout)
 │    └── HAL (alloc_page, free_page)
 │
 ├── Interrupts (IOAPIC, MSI-X)
 │    └── HAL (ECAM, port I/O)
 │
 ├── Timers (HPET, APIC, PIT)
 │    └── HAL + Interrupts
 │
 ├── Scheduler ─── KOBJ ─── Security
 │    │
 │    ├── EPROCESS/KTHREAD (Slab alloc)
 │    ├── Wait (KWait engine)
 │    ├── Syscall dispatch (SSDT)
 │    └── ✗── VFS, ✗── Block drivers
 │
 ├── VFS ─── FileSystem trait (NeoDOS FS, FAT32, KDrive)
 │    ├── IoStack ─── BlockDevice trait ─── Drivers (AHCI, ATA, NVMe)
 │    ├── ─ KOBJ (mount points)
 │    └── Page cache ─── Block cache
 │
 ├── Syscall dispatcher
 │    ├── Scheduler (process lifecycle)
 │    ├── VFS (file I/O, directory ops)
 │    ├── Memory (brk, mmap)
 │    ├── Security (permission check)
 │    ├── IPC/Pipes (read/write)
 │    └── Event Bus (driver comms)
 │
 ├── NEM Driver Runtime
 │    ├── Certification pipeline
 │    ├── Capability system
 │    ├── Isolation layer ─── Memory
 │    ├── ABI negotiation
 │    ├── Dependency resolver
 │    └── Boot loader ─── VFS (file read)
 │
 ├── Event Bus
 │    ├── Work queue
 │    └── ─ Scheduler (wake from events)
 │
 ├── IPC/Pipes ─── Scheduler (blocking)
 │
 ├── IRP System ─── Work queue + Scheduler +
 │    └── BlockDevice trait
 │
 ├── APC Engine ─── Scheduler (per-thread queues)
 │
 ├── DPC Engine ─── Work queue + Timers
 │
 └── Shell
      ├── VFS (file ops)
      ├── Scheduler (run/kill/ps)
      ├── Security (admin checks)
      ├── Event Bus (layout)
      └── ✗── AHCI/ATA, ✗── Interrupts, ✗── Timers
```

## Apéndice B: Métricas Objetivo v1.0

| Métrica | Actual | Objetivo v1.0 |
|---------|--------|---------------|
| Líneas de código kernel | ~45.580 | <60K |
| Tests automáticos | 537 (test_case!) | >800 |
| Cobertura de líneas | ~60% | >90% |
| Syscalls (handlers SSDT) | 29 | 50–60 |
| Procesos simultáneos | Ilimitado (Vec dinámica) | Ilimitado |
| RAM máxima | Ilimitado (UEFI dinámico) | 64 GB+ (teórico) |
| Disco máximo (NeoFS) | ~16 TB (u32×4KB) | 2 TB+ |
| Drivers NEM | 9 | 15+ |
| Binarios user-mode | 32 | 30+ |
| Stack de red | TCP/IP + UDP (12 módulos) | TCP/IP + UDP |
| ASLR | v1 (PIE + load_offset) | Sí (v3) |
| Seguridad (ACL) | 5 módulos (SID, Token, ACL, Access) | Sí (completo NT) |

---

*Este documento es la guía arquitectónica para el desarrollo de NeoDOS v0.40 → v1.0. Cualquier decisión que contradiga este documento debe ser debatida en un Architectural Review Board (ARB) antes de implementarse.*
