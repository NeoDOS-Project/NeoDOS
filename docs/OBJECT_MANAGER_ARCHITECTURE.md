# NeoDOS Object Manager — Architecture Document v0.1

> **Autor:** Arquitecto Jefe de Sistemas Operativos
> **Versión:** v0.1 (draft)
> **Fecha:** 2026-06-22
> **Estado:** Propuesta para revisión arquitectónica

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
| 60 | `sys_ob_open` | RBX=path_ptr, RCX=access_mask | sys_open parcial | NUEVA |
| 61 | `sys_ob_create` | RBX=path_ptr, RCX=type, RDX=attrs | sys_pipe, sys_mkdir parcial | NUEVA |
| 62 | `sys_ob_query_info` | RBX=fd, RCX=info_class, RDX=buf_ptr, R8=buf_size | sys_kobj_enum, sys_stat | NUEVA |
| 63 | `sys_ob_set_info` | RBX=fd, RCX=info_class, RDX=buf_ptr | — | NUEVA |
| 64 | `sys_ob_enum` | RBX=path_fd, RCX=buf_ptr, RDX=max_entries | sys_readdir extendido | NUEVA |
| 65 | `sys_ob_wait` | RBX=handle_count, RCX=handles_ptr, RDX=wait_type, R8=timeout | sys_waitpid, sys_thread_join, sleep_ex unificado | NUEVA |

### 12.2 Slot Reservation

| RAX | Syscall | Nota |
|-----|---------|------|
| 66–79 | Reservados para Object Manager | 14 slots para futuro |

---

## 13. Syscalls Existentes: Migración y Compatibilidad

### 13.1 Syscalls que se Convierten en Wrappers

| RAX | Syscall | Wrapper de | Fase |
|-----|---------|-----------|------|
| 4 | `sys_read` | ob_open(fd→object_id) + ObOperations::read | v0.45 |
| 10 | `sys_open` | ob_open(path) + ob_query_info si dir | v0.45 |
| 11 | `sys_readfile` | ob_query_info(fd→ObId) + vfs::read | v0.45 |
| 12 | `sys_writefile` | ob_query_info(fd→ObId) + vfs::write | v0.45 |
| 5 | `sys_pipe` | ob_create(path_pipe) + ob_open x2 | v0.45 |
| 13 | `sys_close` | ob_close(handle) — ya existe semánticamente | v0.41 |
| 8 | `sys_readdir` | ob_enum(fd→ob_enum_dir) | v0.45 |
| 22 | `sys_thread_create` | ob_create(thread) | v0.45 |
| 9 | `sys_waitpid` | ob_wait(process, CHILD_EXIT) | v0.45 |
| 23 | `sys_thread_join` | ob_wait(thread, THREAD_EXIT) | v0.45 |
| 48 | `sys_kobj_enum` | ob_enum(global) — wrapper de compat | v0.45 |

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
v0.41 (Prep):
  ─ src/handle.rs: añadir object_id campo
  ─ src/kobj/mod.rs: refactor → ObjectManager module
  ─ src/object/mod.rs: nuevo módulo
  ─ src/syscall/mod.rs: handler_close → ob_close

v0.45 (Ob APIs):
  ─ sys_ob_open (RAX=60)
  ─ sys_ob_create (RAX=61)
  ─ sys_ob_query_info (RAX=62)
  ─ sys_ob_set_info (RAX=63)
  ─ sys_ob_enum (RAX=64)
  ─ sys_ob_wait (RAX=65)
  ─ sys_open wrapper de ob_open
  ─ sys_readdir wrapper de ob_enum

v0.50 (Tools):
  ─ ps.nxe usa ob_enum(Process)
  ─ kill.nxe usa ob_open + ob_set_info
  ─ pri.nxe usa ob_open + ob_set_info
  ─ neoshell usa ob_enum para autocomplete

v1.0 (Stable):
  ─ URN sobre Ob
  ─ Security en ObOpen
  ─ KWait integrado en ObWait
  ─ Documentación API
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

#### Issue OB-001: Módulo base del Object Manager

**Descripción:** Crear `src/object/mod.rs` con las estructuras base: `ObObject`, `ObObjectTable`, `ObOperations` trait, `ObType`, `ObId`, `ObError`. El módulo reemplazará progresivamente a `kobj/mod.rs`.

**Archivos:**
- `src/object/mod.rs` (nuevo, ~300 líneas)
- `src/object/types/` (nuevo directorio)

**Estructura inicial:**
```rust
// object/mod.rs
pub mod handle;
pub mod types;
pub mod security;
pub mod namespace;

pub type ObId = u64;

#[repr(u32)]
pub enum ObType { ... }

pub struct ObObject { ... }
pub struct ObOperations { ... }
pub struct ObObjectTable { ... }

pub fn ob_create_object(...) -> Result<ObId, ObError>;
pub fn ob_destroy_object(id: ObId) -> Result<(), ObError>;
pub fn ob_lookup(id: ObId) -> Option<&ObObject>;
pub fn ob_open_object(id: ObId, access: u32) -> Result<(), ObError>;
pub fn ob_close_object(id: ObId) -> Result<(), ObError>;
pub fn ob_reference(id: ObId);
pub fn ob_dereference(id: ObId);
pub fn ob_enum_snapshot() -> Vec<ObObjectSnapshot>;
```

**Criterio de aceptación:**
- `ob_create_object` registra un nuevo objeto con tipo, nombre y ops
- `ob_lookup` obtiene el objeto por ID
- `ob_destroy_object` falla si refcount > 0
- `ob_reference` / `ob_dereference` mantienen conteo
- Tests: 5 (create, lookup, destroy, refcount, double-destroy)

**Prerequisitos:** Ninguno
**Estimación:** ~300 líneas, 2 días

---

#### Issue OB-002: HandleEntry — añadir campo object_id

**Descripción:** Añadir `object_id: u64` al `HandleEntry` actual. No se eliminan `kind` ni `id` todavía — conviven. Modificar `alloc_handle()` para registrar un ObObject si no existe y almacenar su ID.

**Archivos:**
- `src/handle.rs` (~20 líneas modificadas)

**Cambio:**
```rust
pub struct HandleEntry {
    pub object_id: ObId,    // NUEVO — 0 si no migrado
    pub kind: u8,           // legacy — se mantiene
    pub id: u32,            // legacy — se mantiene
    pub extra: u32,         // legacy — se mantiene
    pub offset: u64,        // legacy — se mantiene
}
```

**Criterio:**
- `HandleEntry::closed()` inicializa `object_id = 0`
- `alloc_handle` asigna `object_id` desde ObObject creado internamente
- Tests existentes de handle table pasan sin cambios
- Tests: 2 (migración transparente, closed sentinel)

**Prerequisitos:** OB-001
**Estimación:** ~20 líneas, 0.5 días

---

#### Issue OB-003: KOBJ refactor como ObObjectTable

**Descripción:** Refactorizar `kobj/mod.rs` para que `KObjRegistry` use `ObObjectTable` internamente. `kobj_register()` llama a `ob_create_object()`. `kobj_unregister()` llama a `ob_destroy_object()`. Mantener la API pública de KOBJ exactamente igual.

**Archivos:**
- `src/kobj/mod.rs` (~200 líneas refactorizadas)
- `src/kobj/namespace.rs` (sin cambios — usa ob_insert_object_auto que sigue funcionando)

**Criterio:**
- Todos los tests existentes de KOBJ (8) pasan sin cambios
- `kobj_register` ahora almacena un ObObject completo (no solo metadata)
- `kobj_lookup` funciona igual
- La integración con namespace (ob_insert_object_auto) no se rompe

**Prerequisitos:** OB-001
**Estimación:** ~200 líneas, 1 día

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

**Descripción:** Añadir `object::init()` llamado desde `main.rs` (Phase 3.x) que inicializa el Object Manager, registra los tipos de objeto base, y crea el directorio raíz del namespace Ob.

**Archivos:**
- `src/object/mod.rs` (init, ~18 líneas)
- `src/main.rs` (llamada existente en Phase 2.759)

**Criterio:**
- Al boot, el Object Manager está inicializado con 10 objetos base
- `ob_lookup` funciona antes de que cualquier driver cargue
- La integración con KOBJ namespace funciona (kobj_register crea ObObject automáticamente)
- Tests: 2 (ob_init_root_directory, ob_init_type_entries)

**Prerequisitos:** OB-001, OB-003
**Estimación:** ~20 líneas, 0.5 días

---

### C.3 v0.45 — Object Manager Initial (Issues)

#### Issue OB-010: ObOpen syscall (RAX=60)

**Descripción:** Implementar `sys_ob_open(path, access_mask) → fd`. Internamente:
1. `copy_user_string(path)` → path_str
2. `ob_resolve_path(path_str)` → ObId (usando namespace existente)
3. `se_access_check(current_token, &obj.sd, access_mask)` → check
4. `HandleTable::alloc_handle(HandleEntry { object_id, access_mask })` → fd
5. Registrar objeto en KOBJ si no existe

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_open, ~40 líneas)
- `src/object/mod.rs` (ob_open_path, ~30 líneas)
- `src/syscall/table.rs` (registrar handler)

**Criterio:**
- `ObOpen("\Global\FileSystem\C:\boot.cfg", READ)` → fd
- `ObOpen("\Driver\ps2kbd", READ)` → fd (object existente)
- `ObOpen("\NonExistent", READ)` → -ENOENT
- Tests: 4 (file, driver, non-existent, access denied)

**Prerequisitos:** OB-001, OB-005, NT6 (SeAccessCheck)
**Estimación:** ~70 líneas, 1 día

---

#### Issue OB-011: ObCreate syscall (RAX=61)

**Descripción:** Implementar `sys_ob_create(path, type, attrs) → fd`. Crea un nuevo objeto y lo registra en el namespace. Soporta:
- `ObType::Pipe` → crea un pipe (reemplaza `sys_pipe` legacy)
- `ObType::Directory` → crea directorio en namespace
- `ObType::Event` → crea evento de sincronización (futuro)

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_create, ~50 líneas)
- `src/object/mod.rs` (ob_create_object_path, ~40 líneas)

**Criterio:**
- `ObCreate("\Global\Pipe\my_pipe", Pipe)` → fd (reader) + fd (writer)
- `ObCreate("\Global\MyDir", Directory)` → directory handle
- Tests: 3 (pipe, directory, invalid type)

**Prerequisitos:** OB-010
**Estimación:** ~90 líneas, 1 día

---

#### Issue OB-012: ObQueryInfo syscall (RAX=62)

**Descripción:** Implementar `sys_ob_query_info(fd, info_class, buf, buf_size) → bytes_written`. Consulta metadatos del objeto referenciado por el handle. Classes soportadas: `BasicInfo`, `NameInfo`, `FileInfo`, `ProcessInfo`, `ThreadInfo`, `PipeInfo`, `DeviceInfo`.

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_query_info, ~40 líneas)
- `src/object/types/process.rs` (ObOperations impl, ~50 líneas)
- `src/object/types/file.rs` (ObOperations impl, ~30 líneas)
- `src/object/types/pipe.rs` (ObOperations impl, ~30 líneas)
- `src/object/types/thread.rs` (ObOperations impl, ~30 líneas)
- `src/object/types/device.rs` (ObOperations impl, ~20 líneas)

**Criterio:**
- `ObQueryInfo(fd, BasicInfo)` → type, name, refcount para cualquier objeto
- `ObQueryInfo(fd, FileInfo)` → size, drive, inode
- `ObQueryInfo(fd, ProcessInfo)` → pid, parent, priority, thread_count, state
- `ObQueryInfo(fd, PipeInfo)` → capacity, read_refs, write_refs
- `ObQueryInfo(invalid_fd, BasicInfo)` → -EBADF
- Tests: 8 (5 types × basic + 3 type-specific)

**Prerequisitos:** OB-010, OB-011
**Estimación:** ~200 líneas, 2 días

---

#### Issue OB-013: ObSetInfo syscall (RAX=63)

**Descripción:** Implementar `sys_ob_set_info(fd, info_class, buf)`. Soporta:
- `ProcessPriority` → cambia prioridad de proceso
- `ThreadPriority` → cambia prioridad de thread
- `ObjectName` → renombra objeto
- `SecurityInfo` → cambia SecurityDescriptor

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_set_info, ~30 líneas)
- `src/object/types/process.rs` (set_info, ~20 líneas)
- `src/object/types/thread.rs` (set_info, ~20 líneas)

**Criterio:**
- `ObSetInfo(proc_fd, ProcessPriority, &3)` → cambia prioridad
- `ObSetInfo(thread_fd, ThreadPriority, &1)` → cambia prioridad
- `ObSetInfo(fd, ObjectName, "new_name")` → renombra
- Tests: 4 (priority set, name set, invalid class, invalid fd)

**Prerequisitos:** OB-012
**Estimación:** ~70 líneas, 1 día

---

#### Issue OB-014: ObEnum syscall (RAX=64)

**Descripción:** Implementar `sys_ob_enum(dir_fd, buf, max_entries) → count`. Enumera los objetos contenidos en un directorio del namespace Ob. Reemplaza funcionalidad de `sys_readdir` y `sys_kobj_enum`.

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_enum, ~40 líneas)
- `src/object/mod.rs` (ob_enum_directory, ~30 líneas)
- `src/object/namespace.rs` (enumerate, ~30 líneas)

**Criterio:**
- `ObEnum(root_fd)` → lista `\Global`, `\Device`, `\Driver`, `\FileSystem`, `\Registry`
- `ObEnum(device_fd)` → lista dispositivos registrados
- Compat: `sys_kobj_enum(RAX=48)` wrapper de `ObEnum` con filtro global
- Tests: 4 (root, nested, empty, invalid fd)

**Prerequisitos:** OB-010, NT5 (namespace)
**Estimación:** ~100 líneas, 1 día

---

#### Issue OB-015: sys_open como wrapper de ObOpen

**Descripción:** Refactorizar `handler_open` para que internamente llame a `ob_open_path()` y luego adapte el resultado al formato legacy si es necesario. El comportamiento visible para user-mode no cambia.

**Archivos:**
- `src/syscall/mod.rs` (handler_open, ~50 líneas refactorizadas)

**Criterio:**
- `sys_open("C:\file.txt", 0)` devuelve el mismo fd que antes
- `sys_open("C:\nonexistent", 0)` devuelve -ENOENT
- `sys_open("C:\dir", 0)` devuelve handle de directorio
- Tests: 3 (file, dir, non-existent)

**Prerequisitos:** OB-010
**Estimación:** ~50 líneas, 1 día

---

#### Issue OB-016: sys_pipe como wrapper de ObCreate

**Descripción:** Refactorizar `handler_pipe` para que cree un objeto Pipe via `ob_create_object(ObType::Pipe)` y luego abra reader/writer handles.

**Archivos:**
- `src/syscall/mod.rs` (handler_pipe, ~30 líneas refactorizadas)
- `src/pipe.rs` (ObOperations impl para Pipe, ~40 líneas)

**Criterio:**
- `sys_pipe(fds)` funciona exactamente igual que antes
- El pipe se registra como ObObject en el Object Manager
- Tests: 2 (pipe create, pipe read/write via Ob)

**Prerequisitos:** OB-011
**Estimación:** ~70 líneas, 1 día

---

#### Issue OB-017: sys_readfile/sys_writefile como wrappers Ob

**Descripción:** Refactorizar `handler_readfile` y `handler_writefile` para obtener la información de drive/inode via `ObQueryInfo(fd, FileInfo)` en lugar de leer directamente del HandleEntry.

**Archivos:**
- `src/syscall/mod.rs` (handler_readfile, handler_writefile, ~40 líneas refactorizadas)

**Criterio:**
- `sys_readfile(fd, buf, len)` funciona exactamente igual
- `sys_writefile(fd, buf, len)` funciona exactamente igual
- Tests: 2 (read, write)

**Prerequisitos:** OB-012
**Estimación:** ~40 líneas, 0.5 días

---

#### Issue OB-018: URN — file scheme usa ObOpen

**Descripción:** Refactorizar `urn_open` para el scheme `file` para que use `ob_open_path` internamente en lugar de `vfs.resolve_path` directamente. Esto unifica el camino de apertura.

**Archivos:**
- `src/urn/mod.rs` (~30 líneas refactorizadas)

**Criterio:**
- `urn_open("neodos://file/C:/boot.cfg")` devuelve UrnHandle con ObId válida
- Tests existentes de URN (11) pasan sin cambios
- Tests: 1 (URN file → Ob)

**Prerequisitos:** OB-010
**Estimación:** ~30 líneas, 0.5 días

---

### C.4 v0.50 — Migración de Herramientas (Issues)

#### Issue OB-020: ObWait syscall (RAX=65) + KWait integration

**Descripción:** Implementar `sys_ob_wait(handle_count, handles, wait_type, timeout)`. Integra con KWait (Unified Wait Engine) para esperar en múltiples objetos simultáneamente.

**Soporte inicial:**
- `WAIT_TYPE_ANY` → despierta en el primer objeto señalado
- `WAIT_TYPE_ALL` → despierta cuando todos están señalados
- Timeout en milisegundos (0 = no timeout)
- Objetos soportados: Process (exit), Thread (terminate), Pipe (data available), Timer (expire)

**Archivos:**
- `src/syscall/mod.rs` (handler_ob_wait, ~60 líneas)
- `src/object/mod.rs` (ob_wait_objects, ~50 líneas)
- `src/kwait/` (integración existente, ~30 líneas)

**Criterio:**
- `ObWait([proc_handle], WAIT_TYPE_ANY, INFINITE)` → espera a que el proceso termine
- `ObWait([pipe_handle], WAIT_TYPE_ANY, 1000)` → timeout tras 1s
- Compat: `sys_waitpid(RAX=9)` wrapper de `ObWait(Process, CHILD_EXIT)`
- Compat: `sys_thread_join(RAX=23)` wrapper de `ObWait(Thread, THREAD_EXIT)`
- Tests: 5 (process wait, thread wait, pipe wait, timeout, wait-any multi)

**Prerequisitos:** OB-010, v0.42 KWait
**Estimación:** ~140 líneas, 2 días

---

#### Issue OB-021: ps.nxe migrado a ObEnum

**Descripción:** Modificar `userbin/ps/` para usar `sys_ob_enum` (o su wrapper libneodos `ob_open + ob_enum`) filtrando por `ObType::Process` en lugar de `sys_kobj_enum`.

**Archivos:**
- `userbin/ps/src/main.rs` (~20 líneas modificadas)

**Criterio:**
- `PS` desde neoshell muestra los mismos procesos que antes
- Tests: 1 (ps_output_via_ob)

**Prerequisitos:** OB-014 (ObEnum)
**Estimación:** ~20 líneas, 0.5 días

---

#### Issue OB-022: kill.nxe migrado a Ob

**Descripción:** Modificar `userbin/kill/` para usar `sys_ob_set_info(proc_fd, ProcessTerminate)` en lugar de `sys_kill_process`.

**Archivos:**
- `userbin/kill/src/main.rs` (~15 líneas modificadas)
- `src/syscall/mod.rs` (handler_set_priority → ob_set_info, ~10 líneas)

**Criterio:**
- `KILL 5` termina PID 5 (funcionalidad idéntica)
- `sys_kill_process(RAX=52)` wrapper de `ObSetInfo`
- Tests: 2 (kill, kill non-existent)

**Prerequisitos:** OB-013 (ObSetInfo)
**Estimación:** ~25 líneas, 0.5 días

---

#### Issue OB-023: pri.nxe migrado a Ob

**Descripción:** Modificar `userbin/pri/` para usar `sys_ob_set_info(proc_fd, ProcessPriority, &new_priority)` en lugar de `sys_set_priority`.

**Archivos:**
- `userbin/pri/src/main.rs` (~15 líneas modificadas)

**Criterio:**
- `PRI 5 0` cambia prioridad (comportamiento idéntico)
- `sys_set_priority(RAX=51)` wrapper de `ObSetInfo`
- Tests: 2 (priority set, invalid level)

**Prerequisitos:** OB-013
**Estimación:** ~15 líneas, 0.5 días

---

#### Issue OB-024: HandleEntry — eliminar kind+id legacy

**Descripción:** Eliminar los campos `kind` y `id` del `HandleEntry`. Migrar todos los consumidores (handler_exit, kill_pid, dup2, pipe, etc.) a usar `object_id`.

**Archivos:**
- `src/handle.rs` (~20 líneas)
- `src/syscall/mod.rs` (~100 líneas en 5+ handlers)
- `src/scheduler/mod.rs` (~50 líneas en kill_pid, exit)

**Criterio:**
- El handle table solo almacena `object_id`, `access_mask`, `offset`
- Todos los handlers existentes funcionan sin `kind`
- Tests: 5 (close, dup2, pipe exit cleanup, kill cleanup, all types)

**Prerequisitos:** OB-002, OB-004, OB-015, OB-016, OB-017
**Estimación:** ~170 líneas, 2 días

---

#### ~~Issue OB-025: URN rewrite completo como frontend de Ob~~ **[COMPLETED]**

**Descripción:** Reescribir `urn/mod.rs` para que todos los schemes sean frontends de Ob:
- `urn_open("neodos://file/...")` → `ob_open("\Global\FileSystem\...")`
- `urn_open("neodos://device/...")` → `ob_open("\Device\...")`
- `urn_open("neodos://registry/...")` → `ob_open("\Registry\...")`
- `urn_open("neodos://kobj/...")` → `ob_open(path lookup via Ob)`
- `UrnHandle` se simplifica a un wrapper sobre `fd`

**Archivos:**
- `src/urn/mod.rs` (~80 líneas refactorizadas)

**Criterio:**
- Todos los 11 tests existentes de URN pasan
- `urn_open` devuelve un fd normal (no UrnHandle separado)
- Tests: 3 (file, device, roundtrip)

**Prerequisitos:** OB-010, OB-011, OB-014
**Estimación:** ~80 líneas, 1 día

---

### C.5 v1.0 — Arquitectura Estable (Issues)

#### Issue OB-030: Security completo en ObOpen

**Descripción:** Integrar `SeAccessCheck` en todas las rutas de `ObOpen`. Cada objecto tiene un `SecurityDescriptor`. Cada open verifica que el token del caller tenga el acceso solicitado.

**Archivos:**
- `src/object/security.rs` (~60 líneas)
- `src/object/mod.rs` (ob_open_path, ~20 líneas integración)

**Criterio:**
- `ObOpen` sin acceso → ACCESS_DENIED
- Admin bypass funciona (como hoy)
- Token de usuario no puede abrir objetos SYSTEM-only
- Tests: 5 (admin grant, user deny, admin bypass, invalid token, no SD)

**Prerequisitos:** OB-010, NT6
**Estimación:** ~80 líneas, 1 día

---

#### Issue OB-031: KWait full integration en ObWait

**Descripción:** Integrar completamente ObWait con KWait para soportar todas las razones de espera: PipeRead, ThreadJoin, ChildExit, TimerExpire, EventSet.

**Archivos:**
- `src/object/mod.rs` (~50 líneas)
- `src/kwait/` (~30 líneas integración)

**Criterio:**
- `ObWait([pipe_fd, timer_fd], WAIT_TYPE_ANY, 5000)` → despierta al primer evento
- `ObWait([proc_fd, thread_fd], WAIT_TYPE_ALL, INFINITE)` → espera ambos
- Tests: 4 (wait-any, wait-all, timeout, interrumpido por APC)

**Prerequisitos:** OB-020, v0.42 KWait
**Estimación:** ~80 líneas, 1 día

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

### C.6 Resumen de Esfuerzo

| Versión | Issues | Líneas nuevas | Líneas modificadas | Tests nuevos | Días estimados |
|---------|--------|--------------|-------------------|-------------|---------------|
| v0.41 | 5 | ~550 | ~50 | 12 | 4.5 |
| v0.45 | 9 | ~680 | ~140 | 31 | 9 |
| v0.50 | 6 | ~480 | ~80 | 17 | 6.5 |
| v1.0 | 3 | ~210 | ~30 | 9 | 4 |
| **Total** | **23** | **~1920** | **~300** | **69** | **24 días** |

### C.7 Dependencias entre Issues

```
v0.41:
OB-001 (Object base) ─┬── OB-002 (Handle object_id) ── OB-004 (close wrapper)
                      └── OB-003 (KOBJ refactor) ────── OB-005 (boot init)

v0.45:
OB-005 ── OB-010 (ObOpen) ──┬── OB-011 (ObCreate) ── OB-016 (pipe wrapper)
                             ├── OB-012 (ObQueryInfo) ─┬── OB-013 (ObSetInfo)
                             │                          └── OB-017 (file wrappers)
                             ├── OB-014 (ObEnum) ── OB-015 (open wrapper)
                             └── OB-018 (URN file)

v0.50:
OB-012 ── OB-020 (ObWait + KWait)
OB-014 ── OB-021 (ps migrado)
OB-013 ──┬── OB-022 (kill migrado)
         └── OB-023 (pri migrado)
OB-024 (HandleEntry cleanup) ── depende de: OB-004, OB-015, OB-016, OB-017
OB-018 ── OB-025 (URN rewrite)

v1.0:
OB-010 ── OB-030 (Security ObOpen)
OB-020 ── OB-031 (KWait full)
OB-030 + OB-031 ── OB-032 (Documentación)
```
