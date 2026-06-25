# NeoDOS Object Manager — Architecture Document v0.2

> **Autor:** Arquitecto Jefe de Sistemas Operativos
> **Versión:** v0.2
> **Fecha:** 2026-06-23
> **Estado:** Documento de referencia v0.44.1 — implementado parcialmente

---

## Índice

1. [Resumen Ejecutivo](#1-resumen-ejecutivo)
2. [Diagnóstico del Estado Actual](#2-diagnóstico-del-estado-actual)
3. [Principios de Diseño](#3-principios-de-diseño)
4. [Arquitectura Propuesta: Ob (Object Manager)](#4-arquitectura-propuesta-ob-object-manager)
5. [ObObject: El Recurso Universal](#5-obobject-el-recurso-universal)
6. [ObHandle: Referencia por Proceso](#6-obhandle-referencia-por-proceso)
7. [ObDirectory: Namespace Jerárquico](#7-obdirectory-namespace-jerárquico)
8. [ObOperations: Despacho Polimórfico](#8-oboperations-despacho-polimórfico)
9. [Security Integration](#9-security-integration)
10. [URN Integration](#10-urn-integration)
11. [Catálogo de Object Types](#11-catálogo-de-object-types)
12. [Nuevas Syscalls](#12-nuevas-syscalls)
13. [Syscalls Existentes: Migración y Compatibilidad](#13-syscalls-existentes-migración-y-compatibilidad)
14. [Mapa de Dependencias](#14-mapa-de-dependencias)
15. [Decisiones Congeladas](#15-decisiones-congeladas)

---

## 1. Resumen Ejecutivo

NeoDOS tiene un kernel funcional con **40 syscalls**, un **KOBJ registry** plano, un **sistema de handles** basado en tipos hardcoded, y una **URN namespace** que opera en paralelo. La plataforma ha madurado hasta un punto donde la desconexión entre estos tres sistemas es un riesgo arquitectónico.

**El problema:** Handles, KOBJ y URN existen como sistemas separados que:
- No comparten una visión unificada de "recurso"
- No permiten consultar metadatos de un recurso a partir de un handle
- Duplican lógica de ciclo de vida (cleanup en exit vs close vs pipe manager)
- No integran seguridad (el SecurityDescriptor existe pero no se verifica en cada acceso)

**La solución:** Un Object Manager (Ob) al estilo NT que unifica handles, objetos, seguridad y namespace bajo una sola abstracción. No se reescribe nada — se migra progresivamente.

---

## 2. Diagnóstico del Estado Actual

### 2.1 Handle Table (src/handle.rs)

```rust
pub struct HandleEntry {
    pub kind: u8,     // HANDLE_CLOSED, HANDLE_FILE, HANDLE_PIPE_READ, ...
    pub id: u32,      // pipe_id, inode, device_id (polimórfico)
    pub extra: u32,   // drive_index, etc.
    pub offset: u64,  // file/readdir position
}
```

**Problemas:**
- `kind` es un enum hardcoded de 10 valores — añadir un nuevo tipo requiere cambios en `handler_close`, `handler_exit`, `handler_kill`, `handler_dup2`
- `id` es polimórfico (inode o pipe_id o device_id según el tipo) — sin tipo seguro
- No hay forma de obtener metadatos del objeto desde un handle (necesitas conocer el tipo y hacer lookup manual)
- No hay referencia cruzada con KOBJ

### 2.2 KOBJ Registry (src/kobj/mod.rs)

```rust
pub struct KObjEntry {
    pub id: KObjId,
    pub refcount: u32,
    pub obj_type: KObjType,
    pub name: [u8; 24],
    pub flags: u32,
    pub creation_tick: u64,
    pub native_id: u64,
}
```

**Problemas:**
- Es metadata-only — no tiene un puntero al recurso real (native_id no es suficiente)
- No hay operaciones asociadas (query info, set info, wait)
- No hay despacho por tipo — es una colección plana
- El refcount no está sincronizado con el handle table (un handle puede existir sin refcount en KOBJ)

### 2.3 URN (src/urn/mod.rs)

```rust
pub struct UrnHandle {
    pub scheme: UrnScheme,
    pub drive: u8,
    pub inode: u32,
    pub offset: u64,
    pub device_ob_path: String,
}
```

**Problemas:**
- Es un sistema paralelo — UrnHandle no es un HandleEntry
- registry y kobj schemes son stubs (no implementados)
- No hay integración con handles del scheduler (no puedes pasar un UrnHandle a sys_read)

### 2.4 Security (src/security/)

```rust
pub struct SecurityDescriptor {
    pub owner: Option<Sid>,
    pub group: Option<Sid>,
    pub dacl: Option<Acl>,
}
```

**Problemas:**
- SecurityDescriptor existe en código pero no está integrado en KOBJ entries ni en HandleEntry
- SeAccessCheck se usa en muy pocos puntos (solo syscall 50)
- No hay acceso comprobado en ObOpen / sys_open

### 2.5 Dependencias y Acoplamiento

| Problema | Impacto | Subsistemas afectados |
|----------|---------|----------------------|
| Tipos de handle hardcoded | Alto | syscall/mod.rs (5+ handlers), scheduler/mod.rs, pipe.rs |
| KOBJ no vinculado a handles | Alto | sys_kobj_enum devuelve datos separados |
| URN paralelo a handles | Medio | urn.rs, syscall dispatch |
| Security no integrado | Alto | token.rs, kobj, handle dispatch |
| Cleanup duplicado | Medio | handler_exit (230 líneas), kill_pid (70 líneas), sys_close |

---

## 3. Principios de Diseño

1. **Todo es un objeto.** No existe recurso del sistema que no sea representable como ObObject. Un pipe, un archivo abierto, un driver, un proceso — todos son objetos.

2. **Handle → Objeto → Operación.** Todo handle referencia un ObObject. Toda operación sobre un handle pasa por el Object Manager.

3. **Seguridad en cada acceso.** Cada ObOpen verifica acceso. Cada operación posterior usa la access_mask del handle.

4. **Migración progresiva.** No se rompen syscalls existentes. Las syscalls viejas se convierten en wrappers que internamente llaman al Object Manager.

5. **URN es un frontend de Ob.** `neodos://file/C:/foo` resuelve a `ObOpen("\Global\FileSystem\C:\foo")`.

---

## 4. Arquitectura Propuesta: Ob (Object Manager)

```
┌────────────────────────────────────────────────────────────────┐
│                       USERMODE (Ring 3)                        │
│  libneodos: ObOpen, ObCreate, ObQueryInfo, ObSetInfo, ObEnum  │
│                         │                                      │
│  ┌──────────────────────┴──────────────────────────────────┐   │
│  │                 SYSCALL LAYER (INT 0x80)                │   │
│  │  Ob syscalls (RAX 60–69)   +   Legacy wrappers (0–53)  │   │
│  └──────────────────────┬──────────────────────────────────┘   │
├─────────────────────────┼──────────────────────────────────────┤
│                    KERNEL (Ring 0)                             │
│  ┌──────────────────────┴──────────────────────────────────┐   │
│  │                 OBJECT MANAGER (Ob)                      │   │
│  │                                                         │   │
│  │  ┌──────────────────────────────────────────────────┐   │   │
│  │  │  ObObjectTable                                   │   │   │
│  │  │  ├── Vec<Option<ObObject>> (slab alloc)          │   │   │
│  │  │  ├── ObOpen(path, access) → Handle               │   │   │
│  │  │  ├── ObCreate(type, path, attrs) → Handle        │   │   │
│  │  │  ├── ObClose(handle)                             │   │   │
│  │  │  ├── ObQueryInfo(handle) → ObBasicInfo           │   │   │
│  │  │  ├── ObSetInfo(handle, info)                     │   │   │
│  │  │  ├── ObEnum(path) → Vec<ObEntry>                 │   │   │
│  │  │  └── ObWait(handle, reason) → KWait integration  │   │   │
│  │  └──────────────────────────────────────────────────┘   │   │
│  │                                                         │   │
│  │  Current KOBJ registry → refactored as ObObjectTable    │   │
│  │  Current HandleTable → stores ObObjectId + access_mask  │   │
│  │  Current URN → frontend over ObOpen/ObEnum              │   │
│  └──────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────┘
```

---

## 5. ObObject: El Recurso Universal

### 5.1 Estructura

```rust
/// Identificador único de objeto (hereda de KOBJ)
pub type ObId = u64;

/// Tipos de objeto (extensible)
#[repr(u32)]
pub enum ObType {
    // Core system
    Process = 1,
    Thread = 2,
    // I/O
    File = 3,
    Directory = 4,
    Pipe = 5,
    Device = 6,
    // IPC & Sync
    Event = 7,
    Semaphore = 8,
    Timer = 9,
    // Registry
    RegistryKey = 10,
    RegistryValue = 11,
    // Drivers & Kernel
    Driver = 12,
    SymbolicLink = 13,
    Token = 14,
    MemorySection = 15,
    // Virtual
    MountPoint = 16,
    DirectoryObject = 17,
}

/// Operaciones polimórficas por tipo de objeto
pub struct ObOperations {
    pub query_info: fn(ObId, ObInfoClass, &mut [u8]) -> Result<usize, ObError>,
    pub set_info: fn(ObId, ObInfoClass, &[u8]) -> Result<(), ObError>,
    pub close: fn(ObId) -> Result<(), ObError>,
    pub wait: fn(ObId, ObWaitReason) -> Result<ObWaitResult, ObError>,
}

/// Objeto base del Object Manager
pub struct ObObject {
    pub id: ObId,
    pub obj_type: ObType,
    pub name: [u8; 256],
    pub sd: SecurityDescriptor,
    pub refcount: u32,
    pub flags: ObFlags,
    pub creation_tick: u64,
    pub ops: Option<&'static ObOperations>,
    pub context: *mut core::ffi::c_void,  // back-pointer
}
```

### 5.2 Ciclo de Vida

```
ObCreate (or native creation)
    ↓
ObObjectTable::insert(ObObject)
    ↓
ObOpen(path, access)
    ↓
HandleTable::push { object_id, access_mask }
    ↓
... operations via dispatch object_id → ops →
    ↓
ObClose(handle)
    ↓
HandleTable::remove
    ↓
if refcount == 0 → cleanup native + ObObjectTable::remove
```

### 5.3 Relación con KOBJ Actual

KOBJ actual (`KObjEntry`, `KObjRegistry`) se **refactoriza** como `ObObjectTable`:
- `kobj_register` → `ob_create_object` (más parámetros)
- `kobj_unregister` → `ob_destroy_object` (con refcount check)
- `kobj_lookup` → `ob_lookup`
- `kobj_iter_snapshot` → `ob_enum_snapshot`

Los tests existentes de KOBJ (8) se mantienen y amplían.

---

## 6. ObHandle: Referencia por Proceso

### 6.1 Nuevo HandleEntry

```rust
pub struct HandleEntry {
    pub object_id: ObId,       // Referencia al ObObject
    pub access_mask: u32,      // ObAccess::READ | WRITE | EXEC | DELETE
    pub offset: u64,           // Posición (file, pipe, dir)
    pub flags: u16,            // HANDLE_FLAG_INHERIT, HANDLE_FLAG_PROTECT
}
```

### 6.2 Migración desde HandleEntry actual

**Fase 1 (v0.41):**
- Añadir `object_id: u64` al HandleEntry actual (campo nuevo)
- Toda creación de handle registra un ObObject si no existe
- Los handlers existentes pueden seguir usando `kind` + `id`

**Fase 2 (v0.45):**
- Eliminar `kind` y `id` del HandleEntry (ahora es solo object_id)
- Todo acceso va via object_id → ObObject → ObOperations
- Los handlers legacy se refactorizan para usar la dispatch table

### 6.3 Access Mask

```rust
pub mod ObAccess {
    pub const READ: u32    = 1 << 0;
    pub const WRITE: u32   = 1 << 1;
    pub const EXECUTE: u32 = 1 << 2;
    pub const DELETE: u32  = 1 << 3;
    pub const ALL: u32     = READ | WRITE | EXECUTE | DELETE;
}
```

---

## 7. ObDirectory: Namespace Jerárquico

### 7.1 Árbol de Directorios

El namespace existente en `kobj/namespace.rs` (`ObNamespace`) se refactoriza como capa superior de Ob:

```
\Global\                    — Objetos globales compartidos
\Device\                    — Dispositivos físicos/lógicos
\Driver\                    — NEM drivers
\FileSystem\                — Filesystems montados
  \C:\                      — Drive letters (symlinks a \Device\Harddisk...)
\Registry\                  — Registry keys (futuro)
\Process\                   — Virtual, PID-indexed
  \1\                       — Proceso PID 1
    \Threads\               — Threads del proceso
\ObjectTypes\               — Virtual, lista de tipos registrados
\SymbolicLinks\             — Symlinks del namespace
```

### 7.2 Implementación

- `ObNamespace` actual (BTreeMap-based) se mantiene como backend
- Se añade `ob_insert_path(path, object_id)` y `ob_resolve_path(path) → ObId`
- Los symlinks existentes funcionan igual
- Se añade un directorio virtual `\Process\<PID>\` que se genera dinámicamente

---

## 8. ObOperations: Despacho Polimórfico

### 8.1 Modelo

Cada tipo de objeto registra una tabla de operaciones. El Object Manager despacha llamadas según el tipo:

```rust
fn ob_query_info(object_id: ObId, class: ObInfoClass, buf: &mut [u8]) -> Result<usize, ObError> {
    let obj = ob_table.lookup(object_id)?;
    match obj.ops {
        Some(ops) => (ops.query_info)(object_id, class, buf),
        None => Err(ObError::NotSupported),
    }
}
```

### 8.2 Clases de Información

```rust
pub enum ObInfoClass {
    // All objects
    BasicInfo,         // ObBasicInfo: type, name, refcount, flags
    NameInfo,          // ObNameInfo: full name string
    SecurityInfo,      // SecurityDescriptor
    // Type-specific
    FileInfo,          // size, drive, inode
    ProcessInfo,       // pid, parent, priority, thread_count, state
    ThreadInfo,        // tid, pid, state, priority
    PipeInfo,          // capacity, refcounts
    DeviceInfo,        // device_id, driver_name
    RegistryInfo,      // key path, value count
}
```

### 8.3 Implementadores Iniciales

| ObType | query_info | set_info | close | wait |
|--------|-----------|----------|-------|------|
| Process | pid, parent, priority, threads | priority | cleanup_terminated_process | waitpid |
| Thread | tid, pid, state, priority | priority | recycle_thread | thread_join |
| File | inode, drive, size, mode | offset seek | close handle | — |
| Pipe | read_refs, write_refs, capacity | — | dec_read/write_ref | pipe block |
| Device | device_id, handler | ioctl | — | — |
| Driver | state, caps, abi | — | unload | — |

---

## 9. Security Integration

### 9.1 Flujo de Acceso

```
ObOpen(path, desired_access)
    ↓
ob_resolve_path(path) → ObId
    ↓
obj = ob_table.lookup(ObId)
    ↓
result = se_access_check(current_token, &obj.sd, desired_access)
    ↓
if GRANT: handle = HandleTable::push(ObId, desired_access)
if DENY:  return ObError::AccessDenied
```

### 9.2 Handles y Access Mask

Cada handle almacena la access_mask concedida en open. Las operaciones posteriores verifican:

```rust
fn check_access(handle: HandleEntry, required: u32) -> Result<(), ObError> {
    if handle.access_mask & required == required {
        Ok(())
    } else {
        Err(ObError::AccessDenied)
    }
}
```

### 9.3 Security Descriptor por Objeto

Cada ObObject tiene un `SecurityDescriptor` (owner SID + DACL). Los defaults por tipo:

| Object Type | Default Owner | Default DACL |
|-------------|--------------|--------------|
| Process | Creator SID | Creator: ALL |
| Thread | Creator SID | Creator: ALL |
| File | Creator SID | Creator: ALL, SYSTEM: ALL |
| Pipe | Creator SID | Creator: READ/WRITE |
| Driver | SYSTEM | SYSTEM: ALL |
| RegistryKey | SYSTEM | SYSTEM: ALL, USER: READ |

---

## 10. URN Integration

La URN actual (`neodos://<scheme>/<path>`) se convierte en un frontend de Ob:

| URN Scheme | Ob Path |
|-----------|---------|
| `neodos://file/C:/path` | `ob_open("\Global\FileSystem\C:\path")` |
| `neodos://device/Harddisk0` | `ob_open("\Device\Harddisk0")` |
| `neodos://registry/Machine/System` | `ob_open("\Registry\Machine\System")` |
| `neodos://kobj/Driver/ahci` | `ob_open("\Driver\ahci")` |

`UrnHandle` se elimina. `urn_open()` internamente llama a `ObOpen()` y devuelve un handle regular (fd).

---

## 11. Catálogo de Object Types

### 11.1 Types Actuales (Migran a Ob)

| ObType | Recurso actual | Syscall asociada |
|--------|---------------|------------------|
| Process | EPROCESS | spawn, exit, getpid, kill, set_priority |
| Thread | KTHREAD | thread_create, thread_join |
| File | NeoFS inode (via HandleEntry) | open, readfile, writefile, close |
| Directory | NeoFS directory inode | open (dir), readdir |
| Pipe | PipeManager slot | pipe, read (pipe), write (pipe), close, dup2 |
| Device | device_id (DeviceHandler) | register_device, ioctl |
| Driver | DriverInstance | ndreg syscalls (future) |
| SymbolicLink | ObSymlink (namespace.rs) | — |

### 11.2 Types Futuros (Post-v0.50)

| ObType | Descripción | Syscall |
|--------|-------------|---------|
| RegistryKey | Clave del registry | ob_open_key, ob_set_value, ob_query_value |
| Event | Evento de sincronización | ob_create_event, ob_set_event, ob_wait |
| Timer | Timer de notificación | ob_create_timer, ob_set_timer |
| Semaphore | Semáforo de conteo | ob_create_semaphore, ob_release_semaphore |
| MemorySection | Región de memoria compartida | ob_create_section, ob_map_section |
| Token | Security token | ob_duplicate_token |

---

## 12. Nuevas Syscalls

### 12.1 Slot Allocation

| RAX | Syscall | Args | Reemplaza | Estado |
|-----|---------|------|-----------|--------|
| 60 | `sys_ob_open` | RBX=path_ptr, RCX=access_mask | sys_open parcial | **IMPLEMENTADA (v0.44)** |
| 61 | `sys_ob_create` | RBX=path_ptr, RCX=type, RDX=attrs | sys_pipe, sys_mkdir parcial | **IMPLEMENTADA (v0.44.1)** |
| 62 | `sys_ob_query_info` | RBX=fd, RCX=info_class, RDX=buf_ptr, R8=buf_size | sys_kobj_enum, sys_stat | **IMPLEMENTADA (v0.44.1)** |
| 63 | `sys_ob_set_info` | RBX=fd, RCX=info_class, RDX=buf_ptr | — | **IMPLEMENTADA (v0.44.1)** |
| 64 | `sys_ob_enum` | RBX=path_fd, RCX=buf_ptr, RDX=max_entries | sys_readdir extendido | **IMPLEMENTADA (v0.44.1)** |
| 65 | `sys_ob_wait` | RBX=handle_count, RCX=handles_ptr, RDX=wait_type, R8=timeout | sys_waitpid, sys_thread_join, sleep_ex unificado | **IMPLEMENTADA (v0.44.1)** |
| 66 | `sys_ob_destroy` | RBX=fd | sys_unlink, sys_rmdir | **IMPLEMENTADA (v0.44.2)** |

### 12.2 Slot Reservation

| RAX | Syscall | Nota |
|-----|---------|------|
| 67–79 | Reservados para Object Manager | 13 slots para futuro |

---

## 13. Syscalls Existentes: Migración y Compatibilidad

### 13.1 Syscalls que se Convierten en Wrappers

| RAX | Syscall | Wrapper de | Fase | Estado |
|-----|---------|-----------|------|--------|
| 4 | `sys_read` | ob_open(fd→object_id) + ObOperations::read | v0.45 | PENDIENTE |
| 10 | `sys_open` | ob_open(path) + ob_query_info si dir | v0.45 | **PARCIAL** — Ob namespace paths migrados, legacy paths crean ObObject |
| 11 | `sys_readfile` | ob_query_info(fd→ObId) + vfs::read | v0.45 | **COMPLETADO** — resuelve vía ob_lookup |
| 12 | `sys_writefile` | ob_query_info(fd→ObId) + vfs::write | v0.45 | **COMPLETADO** — resuelve vía ob_lookup |
| 5 | `sys_pipe` | ob_create(path_pipe) + ob_open x2 | v0.45 | **COMPLETADO** — crea ObType::Pipe con ObOperations |
| 13 | `sys_close` | ob_close(handle) — ya existe semánticamente | v0.41 | **COMPLETADO** |
| 8 | `sys_readdir` | ob_enum(fd→ob_enum_dir) | v0.45 | PENDIENTE |
| 22 | `sys_thread_create` | ob_create(thread) | v0.45 | PENDIENTE |
| 9 | `sys_waitpid` | ob_wait(process, CHILD_EXIT) | v0.45 | **COMPLETADO** — handler_ob_wait soporta ChildExit |
| 23 | `sys_thread_join` | ob_wait(thread, THREAD_EXIT) | v0.45 | PENDIENTE |
| 48 | `sys_kobj_enum` | ob_enum(global) — wrapper de compat | v0.45 | PENDIENTE (slot 48 = None) |

### 13.2 Syscalls que Permanecen sin Cambios Significativos

| RAX | Syscall | Motivo |
|-----|---------|--------|
| 0 | `sys_exit` | Demasiado kernel-interna para abstraer |
| 1 | `sys_write` | Console write no es un objeto (todavía) |
| 2 | `sys_yield` | Demasiado básica |
| 3 | `sys_getpid` | Es un acceso directo a EPROCESS |
| 6 | `sys_dup2` | Opera solo sobre el handle table |
| 7 | `sys_spawn` | Demasiado compleja para Ob (carga ELF + setup) |
| 16 | `sys_chdir` | Opera solo sobre EPROCESS CWD |
| 17 | `sys_getcwd` | Ídem |
| 18 | `sys_brk` | Memoria interna del proceso |
| 19 | `sys_mmap` | Mapeo de memoria |
| 20 | `sys_munmap` | Desmapeo |
| 21 | `sys_loadlib` | Carga de DLL |
| 24+ | Informational | get_cpuinfo, get_version, etc. |

### 13.3 Compatibilidad

- **Cambio de firma:** Ninguno. Los wrappers mantienen la firma exacta.
- **ABI structs:** `DirEntryRaw`, `KObjEntryRaw`, `MemInfo` se mantienen como compat. Los nuevos syscalls Ob usan structs nuevos.
- **libneodos:** Se añaden wrappers Ob. Los wrappers existentes se refactorizan para llamar a Ob internamente.

---

## 14. Mapa de Dependencias

```
Object Manager (Ob)
├── KOBJ actual → refactorizado como ObObjectTable
├── HandleTable → almacena ObId + access_mask
├── Security (SID, ACL, SeAccessCheck)
├── KWait (Unified Wait Engine) → ObWait
├── URN → frontend de Ob (dependencia invertida)
├── VFS → File ob_type implementa FileSystem trait
├── Scheduler → Process/Thread ob_types
├── Pipe → Pipe ob_type
└── Driver Runtime → Driver ob_type

Dependencias PROHIBIDAS:
✗── Ob → AHCI / ATA / NVMe (drivers de bloque)
✗── Ob → Console (framebuffer)
✗── Ob → HAL
```

---

## 15. Decisiones Congeladas

### 15.1 ABI Congelado

| Elemento | Versión | Notas |
|----------|---------|-------|
| ObId = u64 (hereda KObjId) | v0.45 | No cambiar tamaño |
| ObType enum (valores 1–17) | v0.45 | No reasignar tipos existentes |
| ObAccess mask bits | v0.45 | No reasignar bits 0–3 |
| ObInfoClass enum | v0.45 | Nuevas clases se añaden al final |
| Syscall RAX 60–65 | v0.45 | No reasignar números |

### 15.2 No-Cambios

| Decisión | Motivo |
|----------|--------|
| No eliminar syscalls legacy | Compatibilidad con binarios existentes |
| No cambiar HandleEntry de golpe | Migración progresiva |
| No integrar Console como objeto | Es demasiado temprano y no aporta valor |
| No hacer Ob per-process namespace | Post-v0.50 |
| No eliminar KOBJ API | Ob extiende, no reemplaza |

---

## Apéndice A: Mapa de Migración (Syscall por Syscall)

```
~~v0.41 (Prep):~~ ✅ COMPLETADO
  ~~─ src/handle.rs: añadir object_id campo~~ ✅
  ~~─ src/kobj/mod.rs: refactor → ObjectManager module~~ ✅
  ~~─ src/object/mod.rs: nuevo módulo~~ ✅
  ~~─ src/syscall/mod.rs: handler_close → ob_close~~ ✅

~~v0.45 (Ob APIs):~~ ✅ COMPLETADO (v0.44.1)
  ~~─ sys_ob_open (RAX=60)~~ ✅
  ~~─ sys_ob_create (RAX=61)~~ ✅
  ~~─ sys_ob_query_info (RAX=62)~~ ✅
  ~~─ sys_ob_set_info (RAX=63)~~ ✅
  ~~─ sys_ob_enum (RAX=64)~~ ✅
  ~~─ sys_ob_wait (RAX=65)~~ ✅
  ─ sys_open wrapper de ob_open 🔶 PARCIAL (Ob namespace paths ok)
  ─ sys_readdir wrapper de ob_enum ❌ PENDIENTE

~~v0.50 (Tools):~~ ✅ COMPLETADO
  ~~─ ps.nxe usa ob_enum(Process)~~ ✅
  ~~─ kill.nxe usa ob_open + ob_set_info~~ ✅
  ~~─ pri.nxe usa ob_open + ob_set_info~~ ✅
  ~~─ kobj.nxe usa ob_open + ob_enum~~ ✅
  ─ neoshell usa ob_enum para autocomplete ⏳ PENDIENTE

v0.52 (All Binaries F1–F2): Alta prioridad
  ─ neoinit (PID 1): spawn+wait via Ob ✅ ← CRÍTICO
  ─ neoshell: readdir→ob_enum, spawn→ob_create(Process)+ob_wait, pipe→ob_create(Pipe) ✅
  ─ coredir, tree: readdir→ob_enum ✅
  ─ corecopy, coretype: readfile→ob_query_info, writefile→ob_set_info ✅
  ─ cd: getcwd→ob_open("\Global\Info\Cwd")+ob_query_info ✅

v0.55 (All Binaries F3–F4): Media prioridad
  ─ coredel, coreren, coremd, corerd: VFS ops via Ob ✅
  ─ ndreg, loadnem: driver_enum/load/unload via Ob namespace ✅
  ─ fsck, drives: fsck/drives via Ob namespace ✅
  ─ vol, label, keyb: volume/label/keyboard via Ob ✅

v0.58 (All Binaries F5–F7): Baja prioridad
  ─ datetime, ver, mem, cpuinfo: info syscalls via Ob ✅
  ─ Binarios de test: migrados a Ob ✅

v1.0 (Stable):
  ─ URN sobre Ob 🔶 PARCIAL (device scheme migrado, file scheme parcial, registry/kobj stubs)
  ─ Security en ObOpen 🔶 PARCIAL (SeAccessCheck en ob_open_path, no en todas las rutas)
  ─ KWait integrado en ObWait 🔶 PARCIAL (solo ChildExit)
  ─ Documentación API ⏳ PENDIENTE
```

---

## Apéndice B: Ejemplo de Flujo Completo

### Abrir un archivo y leerlo (hoy)

```
sys_open("C:\file.txt", 0)
  → vfs.resolve_path → (drive, inode)
  → HandleEntry { kind: HANDLE_FILE, id: inode, extra: drive, offset: 0 }
  → return fd

sys_readfile(fd, buf, 512)
  → HandleEntry { kind: HANDLE_FILE, id: inode, extra: drive, offset }
  → vfs.read(drive, inode, offset, buf)
  → HandleEntry.offset += bytes_read
  → return bytes_read
```

### Abrir un archivo y leerlo (con Ob)

```
sys_open("C:\file.txt", 0)   [wrapper]
  → ob_internal_open("\Global\FileSystem\C:\file.txt", OB_ACCESS_READ)
    → ob_resolve_path → ObId (file object)
    → ob_table.lookup(ObId) → ObObject { type: File, ops: &FILE_OPS }
    → se_access_check(current_token, &obj.sd, OB_ACCESS_READ) → GRANT
    → HandleEntry { object_id: ObId, access_mask: READ, offset: 0 }
  → return fd

sys_readfile(fd, buf, 512)   [wrapper]
  → handle = HandleTable[fd]
  → check_access(handle, OB_ACCESS_READ) → OK
  → ob_table.lookup(handle.object_id) → ObObject
  → (FILE_OPS.read)(handle.object_id, handle.offset, buf, 512)
    → vfs.read(drive, inode, offset, buf)
  → HandleTable[fd].offset += bytes_read
  → return bytes_read

sys_close(fd)   [wrapper]
  → handle = HandleTable[fd]
  → ob_table.lookup(handle.object_id) → ObObject
  → (FILE_OPS.close)(handle.object_id)
    → (no-op for file, dec refcount for pipe)
  → HandleTable[fd] = CLOSED
  → if ObObject.refcount == 0: ob_table.remove(ObId)
```

### Diferencia clave

Hoy: el handler de syscall conoce el tipo y despacha manualmente.
Con Ob: el handler obtiene el objeto del Object Manager y delega en `ObOperations`.

La complejidad no desaparece — se **centraliza** en las implementaciones de `ObOperations`, que son fáciles de testear, seguras, y extensibles sin cambiar el dispatch.

---

## Apéndice C: Plan de Implementación Detallado

### C.1 Visión General por Versiones

```
v0.41 ── Preparación interna (sin romper ABI)
  ├── src/object/mod.rs (ObObject, ObObjectTable, ObOperations trait)
  ├── HandleEntry: añadir object_id field (mantener kind+id compat)
  ├── KOBJ refactor: KObjRegistry → ObObjectTable (wrappers compat)
  ├── sys_close → primer wrapper Ob
  ├── init_object_manager() en boot phase
  └── 10+ tests de objeto base

v0.45 ── Object Manager initial (nuevas APIs)
  ├── ObOpen (RAX=60), ObCreate (RAX=61)
  ├── ObQueryInfo (RAX=62), ObSetInfo (RAX=63)
  ├── ObEnum (RAX=64)
  ├── HandleEntry: kind+id → object_id (breaking change interno)
  ├── sys_open → wrapper de ObOpen
  ├── sys_readfile/writefile → wrappers de Ob
  ├── sys_pipe → wrapper de ObCreate
  ├── sys_readdir → wrapper de ObEnum
  ├── sys_kobj_enum → wrapper de ObEnum
  ├── 6 ObOperations implementadas (Process, Thread, File, Pipe, Device, Driver)
  ├── URN: file scheme usa ObOpen
  └── 20+ tests de syscalls Ob

v0.50 ── Migración de herramientas
  ├── ObWait (RAX=65) + KWait integration
  ├── ps.nxe → usa ObEnum(Process)
  ├── kill.nxe → usa ObOpen proc + ObSetInfo
  ├── pri.nxe → usa ObOpen proc + ObSetInfo
  ├── neoshell → ObEnum para autocomplete de objetos
  ├── URN rewrite completo como frontend de Ob
  ├── HandleEntry: eliminar kind+id legacy
  ├── Security: SeAccessCheck en ObOpen
  └── 30+ tests de herramientas

v1.0 ── Arquitectura estable
  ├── Security completo: todo ObOpen verifica ACL
  ├── KWait full integration en ObWait
  ├── Legacy syscalls: todos los wrappers verificados
  ├── Documentación API completa
  ├── Test coverage >90% en Ob module
  └── 40+ tests totales Ob
```

### C.2 v0.41 — Preparación Interna (Issues)

#### Issue OB-001: Módulo base del Object Manager **[COMPLETED]**

**Descripción:** Crear `src/object/mod.rs` con las estructuras base: `ObObject`, `ObObjectTable`, `ObOperations` trait, `ObType`, `ObId`, `ObError`. El módulo reemplazará progresivamente a `kobj/mod.rs`.

**Archivos:**
- `src/object/mod.rs` (~670 líneas, implementado)
- `src/object/types.rs` (~136 líneas, implementado)

**Estructura final:**
```rust
// object/mod.rs
pub mod types;

pub type ObId = u64;

#[repr(u32)]
pub enum ObType { ... }   // 15 tipos

pub trait ObOperations: Send + Sync { ... }
pub struct ObObject { ... }
pub struct ObObjectTable { ... }

pub fn ob_create_object(...) -> Result<ObId, ObError>;
pub fn ob_destroy_object(id: ObId) -> Result<(), ObError>;
pub fn ob_lookup(id: ObId) -> Option<&ObObject>;
pub fn ob_open_object(id: ObId, access: u32) -> Result<(), ObError>;
pub fn ob_close_object(id: ObId) -> Result<(), ObError>;
pub fn ob_reference(id: ObId);
pub fn ob_dereference(id: ObId);
pub fn ob_enum_snapshot() -> Vec<ObObjectSnapshot>;
pub fn ob_open_path(...) -> Result<ObId, ObError>;
```

**Criterio de aceptación ✅:**
- `ob_create_object` registra un nuevo objeto con tipo, nombre y ops
- `ob_lookup` obtiene el objeto por ID
- `ob_destroy_object` falla si refcount > 0
- `ob_reference` / `ob_dereference` mantienen conteo
- Tests: 5+ (create, lookup, destroy, refcount, double-destroy, open_path, access_denied)

**Implementado en:** v0.41 – `src/object/mod.rs` + `src/object/types.rs`

---

#### Issue OB-002: HandleEntry — añadir campo object_id **[COMPLETED]** (OB-024 completó la migración)

**Descripción:** Añadir `object_id: u64` al `HandleEntry` actual. El campo `kind` fue eliminado completamente en OB-024.

**Archivos:**
- `src/handle.rs` (~285 líneas, implementado)

**Estructura final:**
```rust
pub struct HandleEntry {
    pub object_id: ObId,    // ObId del ObObject, sentinel para stdio
    pub offset: u64,        // posición para file-like objects
}
```

El tipo se identifica mediante sentinelas en `object_id` (ObId::MAX, MAX-1, MAX-2 para stdin/stdout/stderr) u `ob_lookup().obj_type` para objetos reales.

**Criterio ✅:**
- `HandleEntry::closed()` inicializa `object_id = 0`
- Los constructores (file, pipe_read, pipe_write, device, dir) registran ObObject automáticamente
- Tests existentes pasan sin cambios

**Implementado en:** v0.41 (object_id) + v0.44.1 (eliminación de kind)

---

#### Issue OB-003: KOBJ refactor como ObObjectTable **[COMPLETED]**

**Descripción:** KOBJ refactorizado para usar `ObObjectTable` internamente. `kobj_register()` llama a `ob_create_object()`. `kobj_unregister()` llama a `ob_destroy_object()`. La API pública de KOBJ se mantiene para compatibilidad.

**Archivos:**
- `src/kobj/mod.rs` (API compat sobre ObObjectTable)
- `src/kobj/namespace.rs` (sin cambios — funciona con ObObject IDs)

**Criterio ✅:**
- Todos los 8 tests existentes de KOBJ pasan sin cambios
- `kobj_register` almacena un ObObject completo (no solo metadata)
- `kobj_lookup` funciona igual
- La integración con namespace (ob_insert_object_auto) no se rompe

**Implementado en:** v0.41

---

#### Issue OB-004: sys_close como primer wrapper Ob **[COMPLETED]**

**Descripción:** Refactorizar `handler_close` para que llame a `ob_close_object(handle.object_id)` antes de marcar el handle como CLOSED. Esto es seguro porque:
- `ob_close_object` para archivos es no-op (solo decrementa refcount y auto-destroy)
- `ob_close_object` para pipes decrementa refcount y libera si llega a 0
- Elimina la lógica manual de `match entry.kind` en handler_close

**Archivos:**
- `src/syscall/mod.rs` (handler_close, ~10 líneas)
- `src/object/mod.rs` (ob_close_object auto-destroy, ~5 líneas)

**Criterio:**
- `sys_close` en pipe decrementa refcount via ObObject (comportamiento idéntico)
- `sys_close` en file decrementa refcount via ObObject (no-op, mantiene compat)
- `ob_close_object` auto-destroy al llegar a refcount 0
- Tests: 4 (ob_close_object_auto_destroy, ob_close_object_keeps_alive_with_refs, handler_close_file, handler_close_pipe)

**Prerequisitos:** OB-002, OB-003
**Estimación:** ~15 líneas, 0.5 días

---

#### Issue OB-005: init_object_manager en boot phase **[COMPLETED]**

**Descripción:** `object::init()` llamado desde `main.rs` (Phase 2.759) que inicializa el Object Manager, registra los tipos de objeto base, y crea el directorio raíz del namespace Ob.

**Archivos:**
- `src/object/mod.rs` (init_object_manager, ~40 líneas)
- `src/main.rs` (llamada en Phase 2.759)

**Criterio ✅:**
- Al boot, el Object Manager está inicializado con 9 directorios tipo (\Global, \Driver, \Device, \Pipe, etc.)
- `ob_lookup` funciona antes de que cualquier driver cargue
- `kobj_register` crea ObObject automáticamente en el namespace
- Tests: 2 (root directory entries, type entries)

**Implementado en:** v0.41

---

### C.3 v0.45 — Object Manager Initial (Issues)

#### Issue OB-010: ObOpen syscall (RAX=60) **[COMPLETED]**

**Descripción:** `sys_ob_open(path, access_mask) → fd`. Implementado con:
1. `copy_user_string(path)` → path_str
2. `ob_open_path(path_str, &token, desired_access)` → ObId (namespace + VFS fallback)
3. `se_access_check(current_token, &obj.sd, desired_access)` → check
4. `HandleTable::alloc_handle(HandleEntry::ob_object(object_id, access_mask))` → fd

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_open registrado en slot 60)
- `src/object/mod.rs` (ob_open_path ~60 líneas con namespace + VFS + security)

**Criterio ✅:**
- `ObOpen("\Global\FileSystem\C:\boot.cfg", READ)` → fd
- `ObOpen("\Driver\ps2kbd", READ)` → fd (object existente)
- `ObOpen("\NonExistent", READ)` → -ENOENT
- SeAccessCheck integrado: `ob_open_path` verifica token contra SD
- Tests: 4 (existing object, not found, access denied, non-existent namespace)

**Implementado en:** v0.44

---

#### Issue OB-011: ObCreate syscall (RAX=61) **[COMPLETED]**

**Descripción:** `sys_ob_create(path, type, attrs) → fd`. Implementado con `ob_create_object_path()` que soporta:
- `ObType::Pipe` → crea pipe + fd reader/writer
- `ObType::Directory` → crea directorio en namespace

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_create registrado en slot 61)
- `src/object/mod.rs` (ob_create_object_path, ~60 líneas)

**Criterio ✅:**
- `ObCreate("\Global\Pipe\my_pipe", Pipe)` → crea pipe + devuelve handles
- `ObCreate("\Global\MyDir", Directory)` → directory handle
- Namespace insert con creación automática de directorios padre
- Tests: 3 (pipe, directory, invalid type)

**Implementado en:** v0.44.1

---

#### Issue OB-012: ObQueryInfo syscall (RAX=62) **[COMPLETED]**

**Descripción:** `sys_ob_query_info(fd, info_class, buf, buf_size) → bytes_written`. Clases de información soportadas: `BasicInfo`, `NameInfo`, `FileInfo`, `ProcessInfo`, `ThreadInfo`, `PipeInfo`, `DeviceInfo`.

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_query_info registrado en slot 62)
- `src/object/types.rs` (ObInfoClass enum con 7 clases)

**Criterio ✅:**
- `ObQueryInfo(fd, BasicInfo)` → type, name, refcount
- `ObQueryInfo(fd, FileInfo)` → size, drive, inode (vía ob_lookup)
- `ObQueryInfo(fd, ProcessInfo)` → pid, parent, priority, thread_count, state
- `ObQueryInfo(fd, PipeInfo)` → pipe metadata
- `ObQueryInfo(invalid_fd, BasicInfo)` → -EBADF

**Implementado en:** v0.44.1

---

#### Issue OB-013: ObSetInfo syscall (RAX=63) **[COMPLETED]**

**Descripción:** `sys_ob_set_info(fd, info_class, buf)`. Soporta:
- `ProcessPriority` → cambia prioridad de proceso
- `ThreadPriority` → cambia prioridad de thread
- `ObjectName` → renombra objeto
- `SecurityInfo` → cambia SecurityDescriptor

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_set_info registrado en slot 63)
- `src/object/types.rs` (ObSetInfoClass enum)

**Criterio ✅:**
- `ObSetInfo(proc_fd, ProcessPriority, &3)` → cambia prioridad
- `ObSetInfo(fd, ObjectName, "new_name")` → renombra
- SecurityDescriptor modificable vía SecurityInfo class
- Tests: 4 (priority, name, invalid class, invalid fd)

**Implementado en:** v0.44.1

---

#### Issue OB-014: ObEnum syscall (RAX=64) **[COMPLETED]**

**Descripción:** `sys_ob_enum(dir_fd, buf, max_entries) → count`. Enumera objetos del namespace Ob mediante `ob_enum_directory()`.

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_enum registrado en slot 64)
- `src/object/mod.rs` (ob_enum_directory, ~40 líneas)
- `src/object/types.rs` (ObEnumEntry struct ABI-stable)

**Criterio ✅:**
- `ObEnum(root_fd)` → lista directorios del namespace
- `ObEnum(device_fd)` → lista dispositivos registrados
- `sys_kobj_enum(RAX=48)` → actualmente None (pendiente wrapper)
- Tests: 4 (root, nested, empty, invalid fd)

**Implementado en:** v0.44.1

---

#### Issue OB-015: sys_open como wrapper de ObOpen **[COMPLETED]**

**Descripción:** `handler_open` usa `ob_open_path()` para TODAS las rutas: namespace paths (`\...`) van directas, drive-letter paths (`C:\...`) se convierten a `\Global\FileSystem\C:\...` antes de resolver.

**Archivos:**
- `src/syscall/mod.rs` (handler_open, refactorizado ~linea 1038)

**Criterio ✅:**
- ✅ `sys_open("\Driver\ps2kbd", 0)` → ObOpen path completo
- ✅ `sys_open("C:\System\boot.cfg", 0)` → ObOpen via `\Global\FileSystem\C:\System\boot.cfg`
- ✅ `sys_open("C:\nonexistent", 0)` → -ENOENT (fallback a VFS legacy)
- ✅ `sys_open("C:\dir", 0)` → handle de directorio con ObObject
- ✅ Security check en ob_open_path para todas las rutas

**Implementado en:** v0.44.2

---

#### Issue OB-016: sys_pipe como wrapper de ObCreate **[COMPLETED]**

**Descripción:** `handler_pipe` crea un objeto `ObType::Pipe` via `ob_create_object()` con `PIPE_OPS`, comparte el mismo `ob_id` entre reader y writer handles.

**Archivos:**
- `src/syscall/mod.rs` (handler_pipe, ~linea 853)
- `src/pipe.rs` (crate::pipe::PIPE_OPS como ObOperations)

**Criterio ✅:**
- `sys_pipe(fds)` funciona exactamente igual que antes
- El pipe se registra como ObObject con refcount: 1 (create) + 2 (handles) → drop create = 2 refs
- Namespace actual: nombre generado "PIPE{id}" (no path-based)

**Implementado en:** v0.44.1

---

#### Issue OB-017: sys_readfile/sys_writefile como wrappers Ob **[COMPLETED]**

**Descripción:** `handler_readfile` y `handler_writefile` resuelven el fd mediante `ob_lookup(entry.object_id)` para extraer drive (desde `flags`) e inode (desde `native_id`).

**Archivos:**
- `src/syscall/mod.rs` (handler_readfile ~linea 1157, handler_writefile ~linea 1214)

**Criterio ✅:**
- `sys_readfile(fd, buf, len)` funciona exactamente igual
- `sys_writefile(fd, buf, len)` funciona exactamente igual
- El I/O de datos sigue yendo por VFS (Ob es capa de handles/namespace, no de block I/O)

**Implementado en:** v0.44.1

---

#### Issue OB-018: URN — Todos los schemes via ObOpen **[COMPLETED]**

**Descripción:** `urn_open` para TODOS los schemes (`file`, `device`, `registry`, `kobj`) resuelve mediante `ob_open_path()` en el namespace Ob.

**Archivos:**
- `src/urn/mod.rs` (~340 líneas)
- `src/kobj/namespace.rs` (init_object_namespace añade \Registry)

**Criterio ✅:**
- ✅ Device scheme: `urn_open("neodos://device/Harddisk0")` → `ob_open_path("\Device\Harddisk0")`
- ✅ File scheme: `urn_open("neodos://file/C:/file.txt")` → `ob_open_path("\Global\FileSystem\C:\file.txt")`
- ✅ Registry scheme: `urn_open("neodos://registry/Machine/System")` → `ob_open_path("\Registry\Machine\System")`
- ✅ KObj scheme: `urn_open("neodos://kobj/Driver/ahci")` → `ob_open_path("\Ob\Driver\ahci")`
- ✅ Namespace \Registry creado en init_object_namespace
- Tests: 19 pasan

**Implementado en:** v0.44.2

---

### C.4 v0.50 — Migración de Herramientas (Issues)

#### Issue OB-020: ObWait syscall (RAX=65) + KWait integration **[COMPLETED]**

**Descripción:** `handler_ob_wait` implementado con integración KWait completa. Soporta `ChildExit`, `PipeRead`, `Event`, `Timer`. Pipe/ThreadJoin migrados de ad-hoc magic a KWait.

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_wait registrado en slot 65, ~linea 3407)
- `src/kwait/` (kwait_block/kwait_wake para 7 wait reasons)
- `src/pipe.rs` (block_current_for_pipe usa KWait)
- `src/scheduler/mod.rs` (block_current_for_thread usa KWait)

**Soporte actual:**
- ✅ `WAIT_TYPE_ANY` para Process (via `kwait_block(ChildExit { pid })`)
- ✅ `WAIT_TYPE_ANY` para Pipe (via `kwait_block(PipeRead { pipe_id })` + non-blocking peek)
- ✅ `WAIT_TYPE_ANY` para Event (via `kwait_block(Event { event_type })`)
- ✅ `WAIT_TYPE_ANY` para Timer (via `kwait_block(Timer { timeout_ms })`)
- ⏳ `WAIT_TYPE_ALL` → devuelve `NoSys` (multi-handle no implementado)
- ⏳ Timeout → parámetro aceptado pero no procesado (0 = infinite)

**Criterio ✅:**
- ✅ `ObWait([proc_handle], WAIT_TYPE_ANY, 0)` → ChildExit via KWait
- ✅ `ObWait([pipe_handle], WAIT_TYPE_ANY, 0)` → PipeRead via KWait (non-blocking peek first)
- ✅ `ObWait([event_handle], WAIT_TYPE_ANY, 0)` → Event via KWait
- ✅ Pipe blocking: `block_current_for_pipe` y `wake_pipe_readers` usan KWait
- ✅ ThreadJoin: `block_current_for_thread` y `wake_thread_joiner` usan KWait
- ✅ `handler_thread_join(RAX=23)` refactorizado a KWait

**Implementado en:** v0.44.2

---

#### Issue OB-021: ps.nxe migrado a ObEnum **[COMPLETED]**

**Descripción:** `userbin/ps/` usa `sys_ob_enum` (vía libneodos) en lugar de `sys_kobj_enum`.

**Archivos:**
- `userbin/ps/src/main.rs` (migrado a ObEnum)

**Criterio ✅:**
- `PS` desde neoshell muestra los mismos procesos que antes
- Usa `sys_ob_enum` con filtro de ObType::Process

**Implementado en:** v0.44.1

---

#### Issue OB-022: kill.nxe migrado a Ob **[COMPLETED]**

**Descripción:** `userbin/kill/` usa `sys_ob_set_info(proc_fd, ...)` en lugar de `sys_kill_process`.

**Archivos:**
- `userbin/kill/src/main.rs` (migrado a ObSetInfo)

**Criterio ✅:**
- `KILL 5` termina PID 5 (funcionalidad idéntica)
- `sys_kill_process(RAX=52)` → None actualmente (se invoca directamente)

**Implementado en:** v0.44.1

---

#### Issue OB-023: pri.nxe migrado a Ob **[COMPLETED]**

**Descripción:** `userbin/pri/` usa `sys_ob_set_info(proc_fd, ProcessPriority, ...)` en lugar de `sys_set_priority`.

**Archivos:**
- `userbin/pri/src/main.rs` (migrado a ObSetInfo)

**Criterio ✅:**
- `PRI 5 0` cambia prioridad (comportamiento idéntico)
- `sys_set_priority(RAX=51)` → None actualmente

**Implementado en:** v0.44.1

---

#### Issue OB-024: HandleEntry — eliminar kind+id legacy **[COMPLETED]**

**Descripción:** HandleEntry ya no tiene campo `kind`. Solo almacena `object_id: ObId` + `offset: u64`. El tipo se identifica mediante sentinelas ObId (para stdio) y `ob_lookup().obj_type` para objetos reales.

**Archivos:**
- `src/handle.rs` (HandleEntry simplificado)
- `src/syscall/mod.rs` (todos los handlers migrados a object_id)
- `src/scheduler/mod.rs` (kill_pid, exit migrados)

**Criterio ✅:**
- HandleTable solo almacena `object_id` + `offset`
- Sentinelas: `HANDLE_STDIN = ObId::MAX`, `HANDLE_STDOUT = MAX-1`, `HANDLE_STDERR = MAX-2`
- Constructores: `file()`, `pipe_read()`, `pipe_write()`, `device()`, `dir()` registran ObObject automáticamente
- Todos los handlers funcionan sin `kind`

**Implementado en:** v0.44.1

---

#### ~~Issue OB-025: URN rewrite como frontend de Ob~~ **[COMPLETED]**

**Descripción:** URN es un frontend completo de Ob. Todos los 4 schemes (`file`, `device`, `registry`, `kobj`) resuelven mediante `ob_open_path()` en el namespace Ob.

**Archivos:**
- `src/urn/mod.rs` (~340 líneas)

**Criterio ✅:**
- ✅ File scheme: `urn_open("neodos://file/C:/file.txt")` → `ob_open_path("\Global\FileSystem\C:\file.txt")`
- ✅ Device scheme: `urn_open("neodos://device/Harddisk0")` → `ob_open_path("\Device\Harddisk0")`
- ✅ Registry scheme: `urn_open("neodos://registry/Machine/System")` → `ob_open_path("\Registry\Machine\System")`
- ✅ KObj scheme: `urn_open("neodos://kobj/Driver/ahci")` → `ob_open_path("\Ob\Driver\ahci")`
- ✅ 19 tests pasan

**Implementado en:** v0.44.2

---

### C.5 v1.0 — Arquitectura Estable (Issues)

#### Issue OB-030: Security completo en ObOpen **[COMPLETED]**

**Descripción:** `SeAccessCheck` integrado en `ob_open_path()` y en todas las rutas legacy de VFS: `sys_open` (vía `\Global\FileSystem\...`), `sys_spawn` (ACCESS_EXECUTE), `sys_mkdir` (ACCESS_WRITE), `sys_unlink`, `sys_rmdir` (ACCESS_DELETE), `sys_rename` (ACCESS_WRITE|DELETE).

**Archivos:**
- `src/object/mod.rs` (ob_open_path con se_access_check)
- `src/syscall/mod.rs` (check_legacy_path_access helper, ~linea 1366)

**Criterio ✅:**
- ✅ `ob_open_path` sin acceso → ACCESS_DENIED
- ✅ Admin bypass funciona
- ✅ Token de usuario no puede abrir objetos SYSTEM-only
- ✅ `sys_spawn(path, ...)` chequea ACCESS_EXECUTE via Ob
- ✅ `sys_mkdir(path)` chequea ACCESS_WRITE via Ob
- ✅ `sys_unlink / sys_rmdir` chequea ACCESS_DELETE via Ob
- ✅ `sys_rename` chequea ACCESS_WRITE | DELETE via Ob
- ✅ Todos los chequeos son no-intrusivos: sin SD → acceso concedido (backward compatible)
- Tests: 16 + todas las rutas legacy cubiertas

**Implementado en:** v0.44.2

---

#### Issue OB-031: KWait full integration en ObWait **[COMPLETED]**

**Descripción:** KWait completamente integrado. Todas las operaciones de bloqueo (PipeRead, ThreadJoin, ChildExit, Event, Timer, IrpComplete, Alertable) usan KWait. Ad-hoc magics (`0xFFFF_0000`, `0x8000_0000`) eliminados.

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_wait, handler_thread_join, handler_exit)
- `src/pipe.rs` (block_current_for_pipe, wake_pipe_readers via KWait)
- `src/scheduler/mod.rs` (block_current_for_thread, wake_thread_joiner via KWait)
- `src/kwait/mod.rs` (7 wait reasons, ABI frozen v0.42)

**Criterio ✅:**
- ✅ `ObWait([proc_handle], WAIT_TYPE_ANY, INFINITE)` → ChildExit via KWait
- ✅ `ObWait([pipe_handle], WAIT_TYPE_ANY, 0)` → PipeRead via KWait (non-blocking peek)
- ✅ `ObWait([event_handle], WAIT_TYPE_ANY, 0)` → Event via KWait
- ✅ `ObWait([timer_handle], WAIT_TYPE_ANY, 0)` → Timer via KWait
- ✅ Pipe blocking usa KWait (no ad-hoc 0xFFFF_0000 magic)
- ✅ ThreadJoin usa KWait (no ad-hoc 0x8000_0000 magic)
- ✅ `handler_thread_join(RAX=23)` refactorizado a KWait
- ⏳ Multi-handle y WAIT_TYPE_ALL → NoSys (próxima iteración)

**Implementado en:** v0.44.2

---

#### Issue OB-032: Documentación API completa

**Descripción:** Documentar toda la API del Object Manager:
- Estructuras ABI (ObBasicInfo, ObEntryRaw, ObWaitResult)
- Syscalls (RAX 60–65) con calling convention
- Códigos de error (ObError enum)
- Guía de migración para tooling

**Archivos:**
- `docs/OBJECT_MANAGER_ARCHITECTURE.md` (completar secciones)
- `AGENTS.md` (tabla de syscalls actualizada)

**Criterio:**
- La documentación cubre todas las syscalls Ob
- La guía de migración es utilizable por un desarrollador nuevo

**Prerequisitos:** OB-030, OB-031
**Estimación:** 2 días

---

### C.6 Resumen de Esfuerzo y Estado Actual

| Versión | Issues | Estado | Tests |
|---------|--------|--------|-------|
| v0.41 (Prep) | 5 | ✅ **COMPLETADO todo** | 12 |
| v0.45 (Ob APIs) | 9 | ✅ **9 COMPLETADOS** | 31 |
| v0.50 (Tools) | 8 | ✅ **8 COMPLETADOS** | 19 |
| v0.52 (Binarios F1–F2) | 7 | ❌ **PENDIENTE** | 0 |
| v0.55 (Binarios F3–F4) | 7 | ❌ **PENDIENTE** | 0 |
| v0.58 (Binarios F5–F7) | 5 | ❌ **PENDIENTE** | 0 |
| v1.0 (Stable) | 3 | 🔶 **3 parciales** (Security, KWait, docs) | 9 |
| **Total** | **43** | **13 completos, 6 parciales, 24 pendientes** | **69** |

### Estado por Issue

| Issue | Versión | Estado | Notas |
|-------|---------|--------|-------|
| OB-001 | v0.41 | ✅ COMPLETADO | src/object/mod.rs + types.rs |
| OB-002 | v0.41 | ✅ COMPLETADO | object_id en HandleEntry |
| OB-003 | v0.41 | ✅ COMPLETADO | KOBJ sobre ObObjectTable |
| OB-004 | v0.41 | ✅ COMPLETADO | sys_close via ob_close_object |
| OB-005 | v0.41 | ✅ COMPLETADO | init_object_manager en boot |
| OB-010 | v0.45 | ✅ COMPLETADO | sys_ob_open (RAX=60) |
| OB-011 | v0.45 | ✅ COMPLETADO | sys_ob_create (RAX=61) |
| OB-012 | v0.45 | ✅ COMPLETADO | sys_ob_query_info (RAX=62) |
| OB-013 | v0.45 | ✅ COMPLETADO | sys_ob_set_info (RAX=63) |
| OB-014 | v0.45 | ✅ COMPLETADO | sys_ob_enum (RAX=64) |
| OB-015 | v0.45 | ✅ COMPLETADO | Ob namespace paths migrados + legacy C:\... via \Global\FileSystem\ bridge |
| OB-016 | v0.45 | ✅ COMPLETADO | sys_pipe via ob_create_object |
| OB-017 | v0.45 | ✅ COMPLETADO | readfile/writefile via ob_lookup |
| OB-018 | v0.45 | ✅ COMPLETADO | URN file scheme via ob_open_path, registry/kobj implementados |
| OB-020 | v0.50 | ✅ COMPLETADO | ObWait con ChildExit, PipeRead, Event, Timer via KWait |
| OB-021 | v0.50 | ✅ COMPLETADO | ps.nxe migrado a ObEnum |
| OB-022 | v0.50 | ✅ COMPLETADO | kill.nxe migrado a ObSetInfo |
| OB-023 | v0.50 | ✅ COMPLETADO | pri.nxe migrado a ObSetInfo |
| OB-024 | v0.50 | ✅ COMPLETADO | HandleEntry sin kind+id |
| OB-025 | v0.50 | ✅ COMPLETADO | URN frontend completo de Ob (file, device, registry, kobj) |
| OB-030 | v1.0 | ✅ COMPLETADO | SeAccessCheck en ob_open_path + legacy paths (spawn, mkdir, unlink, rmdir, rename) |
| OB-031 | v1.0 | ✅ COMPLETADO | KWait full integration: PipeRead, ThreadJoin migrados de ad-hoc magic |
| OB-032 | v1.0 | 🔶 PARCIAL | Documentación de API actualizada, falta doc completa de structs |
| **OB-040** | v0.52 | 🔶 PARCIAL | neoshell: readdir+pipe→Ob, spawn→ob_create+ob_wait pendiente |
| **OB-041** | v0.52 | ✅ COMPLETADO | coredir, tree: readdir→ob_enum |
| **OB-042** | v0.52 | 🔶 PARCIAL | corecopy: unlink→ob_destroy ✅. coretype/corecopy: readfile/writefile sin equivalente Ob |
| **OB-043** | v0.55 | ✅ COMPLETADO | coredel/coreren/coremd/corerd: VFS ops via Ob |
| **OB-044** | v0.55 | 🔶 PARCIAL | ndreg/drives ✅, fsck ⛔ N/A, loadnem pendiente (sys_driver_load/unload sin equivalente Ob) |
| **OB-045** | v0.58 | ✅ COMPLETADO | datetime/ver/mem/cpuinfo: info via Ob |
| **OB-046** | v0.52 | ✅ COMPLETADO | Processos registrados como ObObjects en namespace \Process\<pid> |
| **OB-047** | v0.58 | ❌ PENDIENTE | Binarios de test: migración completa a Ob |

### C.7 Dependencias entre Issues — Estado Actual

```
v0.41: ✅ COMPLETED
OB-001 ─┬── OB-002 ── OB-004
         └── OB-003 ── OB-005

v0.45: ✅ 9/9 COMPLETED
OB-005 ── OB-010 ──┬── OB-011 ── OB-016 ✅
                    ├── OB-012 ──┬── OB-013 ✅
                    │             └── OB-017 ✅
                    ├── OB-014 ── OB-015 ✅
                    └── OB-018 ✅
OB-030 ── (check_legacy_path_access en sys_open/spawn/mkdir/unlink/rmdir/rename)

v0.50: ✅ 8/8 COMPLETED
OB-012 ── OB-020 (ObWait) ✅
OB-014 ── OB-021 (ps) ✅
OB-013 ──┬── OB-022 (kill) ✅
         └── OB-023 (pri) ✅
OB-024 (HandleEntry cleanup) ✅
OB-018 ── OB-025 (URN) ✅
OB-031 (KWait full integration) ✅
OB-046 (neoinit processes as ObObjects) ✅

v0.52 (F1–F2, alta prioridad): ❌ PENDIENTE
OB-014 ── OB-040 (neoshell autocomplete)
OB-014 ── OB-041 (coredir, tree → ob_enum)
OB-012 + OB-013 ── OB-042 (corecopy, coretype → ob_query/set_info)
~~OB-011 + OB-020 ── OB-046 (neoinit spawn+wait — PID 1)~~ ✅

v0.55 (F3–F4, media prioridad): ✅ PARCIAL
OB-011 + OB-013 ── ~~OB-043 (FS ops via Ob)~~ ✅
OB-014 ── OB-044 (loadnem: driver load/unload sin equivalente Ob) ❌
OB-014 ── OB-044 (driver/fs/drive via Ob namespace)

v0.58 (F5–F7, baja prioridad): ✅ PARCIAL
OB-012 ── ~~OB-045 (info syscalls via Ob)~~ ✅
OB-047 (test binaries — cmdtest parcialmente migrado)

v1.0: ✅ COMPLETED
OB-010 ── OB-030 (Security) ✅
OB-020 ── OB-031 (KWait full) ✅
OB-030 + OB-031 ── OB-032 (Documentación) 🔶
```

### C.8 Plan de Migración Completo: Todos los Binarios a Ob

**Objetivo:** Todos los 35 binarios de usuario deben usar exclusivamente syscalls Ob
(RAX 60–65) para operaciones sobre objetos del sistema (archivos, directorios,
procesos, pipes, dispositivos, drivers, etc.), eliminando las syscalls legacy
equivalentes.

#### Fases de Migración

| Fase | Binarios | Syscalls Legacy a Eliminar | Syscall Ob Equivalente |
|------|----------|---------------------------|----------------------|
| **F1** — YA MIGRADOS | ps, kill, pri, kobj | sys_kobj_enum, sys_kill_process, sys_set_priority | ob_open, ob_enum, ob_set_info, ob_query_info |
| **F2** — ALTA PRIORIDAD | neoinit, neoshell, coredir, tree, corehelp, coretype, corecopy | sys_readdir, sys_readfile, sys_writefile, sys_open_with_flags, sys_spawn, sys_pipe, sys_waitpid | ob_open, ob_enum, ob_query_info, ob_wait |
| **F3** — GESTIÓN FS | coredel, coreren, coremd, corerd, label, vol | sys_unlink, sys_rename, sys_mkdir, sys_rmdir, sys_get_volume_label, sys_set_volume_label | ob_open + ob_set_info o wrapper de VFS via Ob |
| **F4** — DRIVERS/SISTEMA | ndreg, loadnem, fsck, drives, keyb | sys_driver_enum, sys_driver_load, sys_driver_unload, sys_fsck, sys_get_drives, sys_set_keyboard_layout | ob_open_path + ob_enum en namespace \Driver\ y \Device\ |
| **F5** — INFO LECTURA | cpuinfo, datetime, ver, mem | sys_getcpuinfo, sys_get_datetime, sys_get_version, sys_get_meminfo | ob_open("\Global\Info\...") + ob_query_info |
| **F6** — BINARIOS DE TEST | hello, systest, filetest, alltest, cputest, cmdtest | sys_open, sys_readfile, sys_writefile, sys_mkdir, sys_rmdir, sys_unlink, sys_rename | ob_open, ob_create, ob_enum, wrappers Ob |
| **F7** — TRIVIALES | echo, cls | Ninguna (solo foundation) | No requiere cambios |

#### Estado Actual por Binario

| Binario | Estado Ob | Syscalls Ob | Syscalls Legacy Restantes |
|---------|-----------|-------------|--------------------------|
| **ps** | ✅ COMPLETO | ob_open, ob_enum, ob_query_info | — |
| **kill** | ✅ COMPLETO | ob_open, ob_set_info | — |
| **pri** | ✅ COMPLETO | ob_open, ob_set_info | — |
| **kobj** | ✅ COMPLETO | ob_open, ob_enum | — |
| **neoshell** | 🔶 PARCIAL | ob_open, ob_enum, ob_create(Pipe) | sys_readfile, sys_spawn, sys_waitpid, sys_chdir, sys_cursor_blink, sys_poweroff |
| **cd** | ✅ COMPLETO | ob_open, ob_query_info | — |
| **coredir** | ✅ COMPLETO | ob_open, ob_enum | — |
| **corehelp** | 🔶 PARCIAL | ob_open, ob_enum, ob_create(Pipe) | sys_readfile, sys_spawn, sys_waitpid |
| **coretype** | 🔶 PARCIAL | ob_open | sys_readfile |
| **tree** | ✅ COMPLETO | ob_open, ob_enum | — |
| **corecopy** | 🔶 PARCIAL | ob_open, ob_destroy | sys_open_with_flags, sys_readfile, sys_writefile |
| **cmdtest** | 🔶 PARCIAL | ob_open, ob_create(Directory), ob_destroy, ob_set_info | sys_open_with_flags, sys_readfile, sys_writefile |
| **cpuinfo** | ✅ COMPLETO | ob_open, ob_query_info | — |
| **neoinit** | ⛔ N/A (PID 1) | — | sys_spawn (no migrable — creación de procesos no es objeto) |
| **datetime** | ✅ COMPLETO | ob_open, ob_query_info | — |
| **ver** | ✅ COMPLETO | ob_open, ob_query_info | — |
| **mem** | ✅ COMPLETO | ob_open, ob_query_info | — |
| **vol** | ❌ PENDIENTE | — | sys_get_volume_label |
| **coredel** | ✅ COMPLETO | ob_open, ob_destroy | — |
| **coreren** | ✅ COMPLETO | ob_open, ob_set_info | — |
| **coremd** | ✅ COMPLETO | ob_create(Directory) | — |
| **corerd** | ✅ COMPLETO | ob_open, ob_destroy | — |
| **drives** | ✅ COMPLETO | ob_open, ob_query_info | — |
| **keyb** | ✅ COMPLETO | ob_open, ob_set_info | — |
| **label** | ❌ PENDIENTE | — | sys_get_volume_label, sys_set_volume_label |
| **fsck** | ⛔ N/A | — | sys_fsck (no migrable — comando de reparación con argumentos) |
| **ndreg** | ✅ COMPLETO | ob_open, ob_query_info | — |
| **loadnem** | ❌ PENDIENTE | — | sys_driver_load, sys_driver_unload |
| **echo** | ✅ N/A | — | (foundation only, solo sys_write) |
| **cls** | ✅ N/A | — | (foundation only, solo sys_write) |

#### Issues de Migración de Binarios

| Issue | Binario | Syscall Legacy→Ob | Depende de | Prioridad |
|-------|---------|-------------------|-----------|-----------|
| OB-040 | neoshell | ~~readdir~~→~~ob_enum~~, ~~pipe~~→~~ob_create(Pipe)~~, readfile→ob_open+query, spawn→ob_create(Process)+ob_wait | OB-011, OB-014, OB-020 | ALTA |
| ~~OB-041~~ | coredir, tree | readdir→ob_enum | OB-014 | ✅ COMPLETADO |
| OB-042 | corecopy, coretype | readfile→ob_query_info, writefile→ob_set_info, ~~unlink~~→~~ob_destroy~~ | OB-012, OB-013 | ALTA |
| OB-046 | neoinit (PID 1) | spawn→ob_create(Process)+ob_wait | OB-011, OB-020 | **CRÍTICA** |
| ~~OB-043~~ | coredel, coreren, coremd, corerd | unlink→ob_destroy, rename→ob_set_info, mkdir→ob_create(Directory), rmdir→ob_destroy | OB-011, OB-013 | ✅ COMPLETADO |
| OB-044 | ndreg, loadnem, fsck, drives | driver_enum→ob_enum("\Driver\"), fsck→ob_query_info(DriveInfo), get_drives→ob_enum("\Device\") | OB-014 | MEDIA |
| OB-045 | datetime, ver, mem, cpuinfo | get_datetime→ob_open("\Global\Info\DateTime")+query, get_version→ob_query_info | OB-010, OB-012 | BAJA |
