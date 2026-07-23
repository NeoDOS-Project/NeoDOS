# NeoDOS — Roadmap oficial reorganizado

> **Versión del documento:** v2.0
> **Fecha:** 2026-07-15
> **Versión del proyecto:** v0.50-dev
> **Documentos relacionados:** [`docs/architecture/vision.md`](docs/architecture/vision.md),
>   [`docs/architecture/source-of-truth.md`](docs/architecture/source-of-truth.md),
>   [`docs/README.md`](docs/README.md) (índice completo de documentación)

---

## Índice

1. [Resumen Ejecutivo](#1-resumen-ejecutivo)
2. [Auditoría del Roadmap Anterior](#2-auditoría-del-roadmap-anterior)
3. [Estructura del Nuevo Roadmap](#3-estructura-del-nuevo-roadmap)
4. [Fase 0: Consolidación (v0.50)](#4-fase-0-consolidación-v050)
5. [Fase 1: Kernel Maduro (v0.51–v0.55)](#5-fase-1-kernel-maduro-v051v055)
6. [Fase 2: Ecosistema de Usuario (v0.56–v0.60)](#6-fase-2-ecosistema-de-usuario-v056v060)
7. [Fase 3: Seguridad y Estabilidad (v0.61–v0.69)](#7-fase-3-seguridad-y-estabilidad-v061v069)
8. [Fase 4: v1.0 — Primera API Estable](#8-fase-4-v10--primera-api-estable)
9. [Post-1.0: v1.x–v4.x](#9-post-10-v1xv4x)
10. [Deuda Técnica y Auditorías](#10-deuda-técnica-y-auditorías)
11. [Dependencias Críticas](#11-dependencias-críticas)
12. [Priorización Global](#12-priorización-global)
13. [Consistencia Arquitectónica](#13-consistencia-arquitectónica)
14. [Apéndice: Mapa de Migración desde Roadmap Anterior](#14-apéndice-mapa-de-migración)

---

## 1. Resumen Ejecutivo

NeoDOS ha completado su fase de prototipo funcional. El kernel tiene una base sólida:
Object Manager completo (16 ObTypes, 7 syscalls Ob), stack TCP/IP, drivers NEM con
aislamiento, Registry con persistencia, VFS con NeoFS v2, planificación SMP, IRQL,
KWait, Service Manager, y subsistema de internacionalización NLTv2.

El roadmap anterior, concentrado en el antiguo `docs/IMPROVEMENTS.md` (eliminado en la reestructuración de docs), adolecía de:

- Mezcla de prioridades sin criterio uniforme
- Milestones con objetivos poco claros (ej: "v0.52 = VirtIO + Sessions + FS Security")
- Dependencias incompletas o incorrectas
- Ausencia de milestones para herramientas, instalación y executive
- Mezcla de deuda técnica con features nuevas
- Sin planificación post-1.0 más allá de vagas ideas

**Este documento reorganiza completamente el roadmap** en fases y milestones con
objetivos técnicos claros, dependencias verificadas y prioridades justificadas.

---

## 2. Auditoría del Roadmap Anterior

### 2.1 Problemas Detectados

| Problema | Ejemplo | Impacto |
|----------|---------|---------|
| Hitos mezclan subsistemas no relacionados | v0.51 mezcla NeoFS v2 + Shell Phase 2 + SAM + Network tools | Dificulta planificación y review |
| Prioridades inconsistentes | v0.54 etiquetado LOW pero contiene Secure Boot, WAL, DNS | Desorienta contribuidores |
| Dependencias incorrectas | NFSv2-BTREE sin prereq, pero depende del page cache | Riesgo de bloqueo |
| Falta milestone herramientas oficiales | NeoDev, NLT, NXP dispersos sin coordinación | Herramientas crecen sin plan |
| Sin milestone instalación | No hay install.nxe, bootstrap, creación de NeoFS | No se puede distribuir |
| Sin milestone Executive | Service Manager existe pero Configuration Manager, Session Manager no tienen plan | Arquitectura NT incompleta |
| Deuda técnica mezclada con nuevas features | CLEANUP-1..35 en medio de milestones funcionales | Confunde prioridades |
| Roadmap post-1.0 inexistente | Solo ideas vagas en ARCHITECTURAL_VISION.md | Sin dirección a largo plazo |
| docs/roadmap.md (eliminado) | Estaba desactualizado (v0.49) | Información contradictoria |

### 2.2 Tareas Duplicadas

| Tarea 1 | Tarea 2 | Resolución |
|---------|---------|------------|
| AUDIT-17 (user address space) | A3.2 (kernel debugger) | No duplicadas, pero mismo milestone |
| CLEANUP-19 (arrays fijos) | Slab<T> contenedor (ya completado) | Slab<T> ya implementado en v0.41 |
| VFS-6.4 (Async VFS via IRP) | IOCP (ARCHITECTURAL_VISION.md §7.5) | Unificar como IOCP |
| B6.1 (zero-copy pipes) | Pipe 4KB×16 (sección 5.2 ARCHITECTURAL_VISION) | Misma tarea |

### 2.3 Hitos Demasiado Grandes

- **v0.51**: 5 subsistemas distintos (NeoFS v2, Shell, SAM, Network tools, Admin tools)
- **v0.52**: 4 subsistemas (VirtIO, Sessions, FS Security, Performance)
- **v0.53**: 4 subsistemas (Security, Registry, KD, Userland)

### 2.4 Tareas Demasiado Pequeñas

- CLEANUP-1..35: cada una es trivial pero hay 35. Deberían agruparse.
- I18N-P4..P12: muchas variantes del mismo sistema que deberían ser sub-tareas.
- VIO-CON, VIO-RNG, VIO-SCSI, etc.: dispositivos VirtIO individuales como tareas separadas.

---

## 3. Estructura del Nuevo Roadmap

El roadmap se organiza en **Fases** que contienen **Milestones** con objetivos
técnicos claros. Cada milestone agrupa tareas de un **único subsistema** siempre
que sea posible.

### Convenciones

```
P = Prioridad: CRÍTICA | ALTA | MEDIA | BAJA | EXPERIMENTAL
D = Dependencias: lista de IDs de milestones/tareas
```

### Mapa de Versiones

```
v0.50 ── Fase 0: Consolidación (objetivo: completar iniciativas en curso)
v0.51–v0.55 ── Fase 1: Kernel Maduro (objetivo: eliminar deuda técnica crítica)
v0.56–v0.60 ── Fase 2: Ecosistema de Usuario (objetivo: herramientas y executive)
v0.61–v0.69 ── Fase 3: Seguridad y Estabilidad (objetivo: hardening pre-1.0)
v1.0 ── Fase 4: Primera API Estable
v1.x ── Mantenimiento y features compatibles
v2.x ── Networking y virtualización
v3.x ── GUI y experiencia de usuario
v4.x ── Escalabilidad enterprise
```

---

## 4. Fase 0: Consolidación (v0.50)

**Objetivo:** Completar las iniciativas ya iniciadas y estabilizar el milestone actual.

### M0.1 — Shell tokenizer + Power Phase 2 + Hardening (v0.50)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| SH-TOKEN+QUOTE | Shell tokenizer con quoting/escaping | ALTA | — |
| SH-REDIR | Redirección Shell (>, <, >>, 2>) | ALTA | SH-TOKEN+QUOTE |
| NFSv2-SYSCALL | sys_ob_snapshot (RAX 77) | ALTA | NFSv2-BTREE, NFSv2-SNAPSHOT |
| PM-PHASE2 | Power Manager kernel core (ObType=21, Registry) | ALTA | — |
| AUDIT-32 | 5+ `.expect()` panic paths → Result | ALTA | — |
| AUDIT-33 | Boot/init hardening (panic → Result/fallback) | ALTA | — |
| AUDIT-34 | Validación rutas críticas syscall/interrupt | ALTA | — |
| AUDIT-35 | Registry persistence hardening (flush atómico) | MEDIA | CM-FIX |
| AUDIT-36 | Userland build/linker pipeline para .NXE | ALTA | — |
| AUDIT-37 | Suite tests integración boot/registry/shell | MEDIA | — |

**Justificación:** Este milestone ya estaba definido como v0.50. Debe completarse
antes de cualquier planificación futura. Las tareas de hardening (AUDIT-32..37)
son críticas porque corrigen puntos de fallo que causarían pánico en producción.

---

## 5. Fase 1: Kernel Maduro (v0.51–v0.55)

**Objetivo:** Eliminar deuda técnica estructural, completar subsistemas kernel
incompletos, y estabilizar la base para el ecosistema de usuario.

### M1.1 — NeoFS v2 Completion (v0.51)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| NFSv2-BTREE | B-tree persistente genérico (COW) | ALTA | — |
| NFSv2-FREELIST | Free list + merge adyacentes | ALTA | — |
| NFSv2-SNAPSHOT | Snapshot table (64 circular) | ALTA | NFSv2-BTREE |
| NFSv2-MKFS | mkfs.neodos tool | MEDIA | NFSv2-FREELIST |
| VFS-2.2 | Refactorizar FSCK a trait | MEDIA | — |

**Objetivo:** Completar NeoFS v2 con todas sus capacidades planeadas.

### M1.2 — Shell Phase 2 (v0.51)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| SH-EDITOR+HISTORY | Line editor ANSI + history persistente | ALTA | SH-TOKEN+QUOTE |
| SH-ENV+PIPE | Expansión %VAR% + pipeline wait | ALTA | SH-TOKEN+QUOTE |
| SH-SEP+COMPL+BATCH | Semicolon + completion + batch scripting | MEDIA | SH-ENV+PIPE, SH-TOKEN+QUOTE |

**Objetivo:** Shell de usuario completo con scripting batch estilo NT.

### M1.3 — SAM Foundation + Network Tools (v0.51)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| USR-P1a | ObType::Session=19 + SAM built-in users | ALTA | — |
| USR-P1b | Token: integrity_level + creation_time | ALTA | USR-P1a |
| USR-P1c | SAM persistence to Registry hive | MEDIA | USR-P1a |
| USR-P1d | SeAccessCheck: fix empty DACL + group SIDs | ALTA | USR-P1b |
| USR-P1e | ObSetInfoClass::ChangePassword (31) | MEDIA | USR-P1c |
| NET-1.9 | ipconfig.nxe | MEDIA | NET-1.8 |
| NET-1.10 | ping.nxe | MEDIA | NET-1.8 |
| B3.4 | NTP client | BAJA | NET-1.8 |
| ADM-4 | neotask (gestor de tareas) | MEDIA | — |
| ADM-1+2 | neotop v0.2 + neostat | MEDIA | — |
| ADM-5+6 | neocfg + neofs | MEDIA | — |

**Objetivo:** Base del modelo de seguridad NT (SAM) y herramientas de red y administración.
Agrupado porque SAM es prerrequisito del resto de seguridad.

### M1.4 — VirtIO Architecture (v0.52)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| VIO-ARCH | Virtqueue abstraction + modern PCI transport | ALTA | A2.1 (ECAM) |
| VIO-NET | VirtIO Network (0x1000) NEM driver | ALTA | VIO-ARCH |
| VIO-BLK2 | VirtIO Block NEM driver | ALTA | VIO-ARCH |
| VIO-9P | VirtIO 9P filesystem | MEDIA | VIO-ARCH |
| VIO-INPUT | VirtIO Input (keyboard, mouse) | MEDIA | VIO-ARCH |

**Objetivo:** Soporte VirtIO completo como plataforma de virtualización estándar.
VIO-ARCH es prerrequisito de todos los demás. VIO-NET y VIO-BLK2 son críticos para
entornos QEMU/KVM.

### M1.5 — Sessions + FS Security (v0.52)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| USR-P2a | SessionManager + ob_create(Session) | ALTA | USR-P1a |
| USR-P2b | SessionInfo + SessionLock/Logoff | ALTA | USR-P2a |
| USR-P2c | TokenInfo + session_id inheritance | MEDIA | USR-P2a |
| USR-P2d | neologon.nxe login binary | ALTA | USR-P2b, USR-P2c |
| USR-P2e | NeoInit spawns neologon | ALTA | USR-P2d |
| USR-P3a | DirEntryV2: owner_sid field | ALTA | USR-P1b |
| USR-P3b | VFS permission checking function | ALTA | USR-P1d, USR-P3a |
| USR-P3c | Wire VFS checks in syscall handlers | ALTA | USR-P3b |
| USR-P3d | Default permissions by extension | MEDIA | USR-P3c |
| VFS-2.2 | Refactorizar FSCK | MEDIA | — |

**Objetivo:** Modelo de sesiones NT completo + seguridad en sistema de archivos.

### M1.6 — Power Manager Phase 3 + Zero-copy (v0.52)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| PM-PHASE3 | Power syscall dispatch + Event Bus types | MEDIA | PM-PHASE2 |
| B6.1 | Zero-copy pipes | MEDIA | — |

**Agrupado con M1.5** por ser tareas pequeñas de subsistemas diferentes que
comparten versión.

### M1.7 — Registry Phase 2 + Integrity Levels (v0.53)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| CM-DIRTY | Registry per-cell dirty tracking | ALTA | — |
| CM-MULTI | Registry multi-hive (SOFTWARE, SECURITY, DEFAULT) | ALTA | — |
| USR-P4a | Registry ACL checking module | ALTA | USR-P1d |
| USR-P4b | Wire sec_desc_cell on key creation | ALTA | USR-P4a |
| USR-P4c | ACL checks in Cm syscall handlers | ALTA | USR-P4b |
| USR-P4d | User profile hive auto-mount | MEDIA | USR-P4c |
| USR-P5a | Integrity level in SeAccessCheck | ALTA | USR-P1b |
| USR-P5b | SetIntegrityLevel + IntegrityLevel query | MEDIA | USR-P5a |
| USR-P5c | Privilege enforcement in admin syscalls | ALTA | USR-P1b |

**Objetivo:** Registry con multi-hive, ACLs y dirty tracking. Modelo de integridad NT.

### M1.8 — Module Signing + KD + Shared Libraries (v0.53)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| B5.1 | Module signature validation | ALTA | — |
| B5.2 | Driver permission enforcement | ALTA | B5.1 |
| A3.2 | Kernel debugger (KD) GDB stub | MEDIA | — |
| B4.6 | NeoEdit text editor | BAJA | — |
| B4.7 | Shared library per-process binding | MEDIA | sys_loadlib |

**Objetivo:** Firma de módulos para seguridad en carga de drivers. Herramientas
de desarrollo (KD) y usuario (NeoEdit).

### M1.9 — Power Phase 4 + User Commands + DNS (v0.54)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| PM-PHASE4 | Service Manager shutdown + libneodos + shell | MEDIA | PM-PHASE3 |
| USR-P6a | WHOAMI command | BAJA | USR-P2c |
| USR-P6b | PASSWD command | BAJA | USR-P2d |
| USR-P6c | WHO + LOGOFF commands | BAJA | USR-P2b |
| USR-P6d | SU command | BAJA | USR-P2d, USR-P2e |
| USR-P6e | RUNAS command | BAJA | USR-P6d |
| NET-DNS | DNS resolver (stub + cache) | MEDIA | NET-1.9 |
| B1.1 | Kernel tracing infrastructure | MEDIA | — |
| B1.2 | NeoTrace system | BAJA | B1.1 |
| ADM-3 | neolog (visor event log) | BAJA | B1.1 |

**Agrupación pragmática:** Tareas de baja prioridad de múltiples subsistemas que
completan funcionalidades ya parcialmente implementadas.

### M1.10 — Registry WAL + Secure Boot + Tracing (v0.55)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| CM-WAL | Registry WAL (write-ahead logging) | MEDIA | CM-DIRTY |
| CM-LIB | Registry libneodos wrappers (7 missing) | BAJA | — |
| CM-REGEDIT | regedit.nxe | BAJA | CM-LIB |
| B5.3 | Secure boot chain | BAJA | B5.1 |
| PM-PHASE5 | Power Manager polish + full tests | BAJA | PM-PHASE4 |
| VFS-3.2 | `\DosDevices` dinámico | BAJA | — |
| VFS-5.3 | Write-back ordenado | BAJA | — |
| VFS-6.1..6.4 | VFS Features (overlay, attr, notifications, async) | BAJA | — |
| VFS-7.1..7.3 | VFS Performance (lock, lookup cache, path cache) | BAJA | — |

**Objetivo:** Registry con recuperación ante fallos (WAL), secure boot, y
características VFS avanzadas.

---

## 6. Fase 2: Ecosistema de Usuario (v0.56–v0.60)

**Objetivo:** Completar el ecosistema de herramientas oficiales, el modelo Executive
NT, y el sistema de instalación.

### M2.1 — NXE/NXP Ecosystem Completion (v0.56)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| NXE-ECO-12 | NXE metadata auto-generation in build pipeline | MEDIA | NXE-ECO-1 |
| NXE-ECO-13 | `\Resource\<app>\` virtual Ob namespace | MEDIA | NXE-ECO-5 |
| NXE-ECO-14 | NXE file header validation in kernel | BAJA | — |
| NXE-ECO-15 | Digital signature verification infrastructure | BAJA | — |
| I18N-P2 | Migrar apps core a tr_id!() | MEDIA | I18N-P1 |
| I18N-P4 | format_str() con placeholders {0} | MEDIA | I18N-P1 |
| I18N-P5 | i18n_available_locales() | BAJA | I18N-P1 |
| I18N-P6 | Per-user locale (Registry) | BAJA | I18N-P1, USR-P1 |

**Objetivo:** Ecosistema NXE/NXP completo con herramientas, recursos, y traducciones.

### M2.2 — Executive Manager (v0.57)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| EXEC-CM | Configuration Manager (consolidación Registry + boot settings) | ALTA | CM-MULTI |
| EXEC-SM | Session Manager (gestión de sesiones de usuario) | ALTA | USR-P2a |
| EXEC-OM | Object Namespace Manager (virtualización de namespace por proceso) | MEDIA | — |
| EXEC-PM | Power Manager final (políticas, planos, eventos) | MEDIA | PM-PHASE5 |

**Objetivo:** Componentes Executive del modelo NT: Configuration Manager,
Session Manager, Object Namespace, Power Manager como servicios de sistema.

### M2.3 — Herramientas Oficiales (v0.58)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| TOOL-NEODEV | NeoDev v2 (build, image, ISO, run, test, QEMU backends) | ALTA | — |
| TOOL-NEODEV-VBOX | VirtualBox backend para NeoDev | MEDIA | TOOL-NEODEV |
| TOOL-NEODEV-DISCOVERY | Auto-descubrimiento de proyectos mejorado | MEDIA | TOOL-NEODEV |
| TOOL-NEODEV-LEGACY | Sustitución completa de scripts heredados | ALTA | TOOL-NEODEV |
| TOOL-NEOCFG | neocfg (Panel de Control) completar módulos | MEDIA | ADM-5 |
| TOOL-NEOMEM | neomem v0.2 | BAJA | — |
| TOOL-NEOTOP | neotop v0.2+ (per-thread CPU, I/O, network) | BAJA | ADM-1 |
| TOOL-NEOTASK | neotask (gestor de tareas) completar | BAJA | ADM-4 |
| TOOL-NEOLOG | neolog (visor event log) | BAJA | ADM-3 |
| TOOL-NXINFO | nxinfo completar (modos, checks, JSON) | MEDIA | NXE-ECO-2 |
| TOOL-NXPKG | nxpkg completar (extract, verify, info) | MEDIA | NXE-ECO-3 |
| TOOL-NXDUMP | nxdump (hex dump, ELF, relocs, strings) | BAJA | NXE-ECO-4 |
| TOOL-NXRES | nxres (explorador de recursos) | BAJA | NXE-ECO-7 |
| TOOL-NXLOCALE | nxlocale (gestor de idiomas) | BAJA | NXE-ECO-8 |
| TOOL-NXVERIFY | nxverify (verificador de integridad) | BAJA | NXE-ECO-9 |

**Objetivo:** Todas las herramientas oficiales del proyecto en un estado pulido y
documentado. NeoDev como herramienta única de desarrollo.

### M2.4 — Instalación y Bootstrap (v0.59)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| INSTALL-NXE | install.nxe (asistente de instalación) | ALTA | NXP-ECO |
| INSTALL-NEOFS | Creación de NeoFS desde cero | ALTA | NFSv2-MKFS |
| INSTALL-BOOTSTRAP | Bootstrap inicial (GPT + partición + NeoFS) | ALTA | INSTALL-NEOFS |
| INSTALL-CONFIG | Configuración inicial (teclado, idioma, admin) | ALTA | USR-P1 |
| INSTALL-PACKAGES | Despliegue de paquetes base | MEDIA | NXP-ECO |

**Objetivo:** Sistema instalable desde cero con asistente interactivo.

### M2.5 — NLT i18n + Regional Formats (v0.60)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| I18N-P7 | Compresión NLT (LZSS/LZ4) | BAJA | I18N-P1 |
| I18N-P8 | UTF-16 support | BAJA | I18N-P1 |
| I18N-P9 | Pluralización | BAJA | I18N-P1 |
| I18N-P10 | Formatos regionales (fechas, monedas) | BAJA | I18N-P1 |
| I18N-P11 | Soporte RTL/bidi | BAJA | I18N-P1 |
| I18N-P12 | Firmas digitales en NLT | BAJA | I18N-P1 |

**Objetivo:** Sistema de internacionalización completo con formatos regionales,
pluralización y soporte de escritura RTL.

---

## 7. Fase 3: Seguridad y Estabilidad (v0.61–v0.69)

**Objetivo:** Hardening completo pre-v1.0. Auditorías, fuzzing, documentación,
y estabilización de la ABI.

### M3.1 — Security Hardening (v0.61–v0.62)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| SEC-AUDIT-FULL | Auditoría de seguridad completa del kernel | CRÍTICA | — |
| SEC-FUZZ-SYSCALLS | Fuzzing de todas las syscalls (0–77) | CRÍTICA | — |
| SEC-FUZZ-DRIVERS | Fuzzing de interfaz HST de drivers NEM | ALTA | — |
| SEC-ASLR-V2 | ASLR v2: pila aleatoria + heap aleatorio | ALTA | ASLR v1 |
| SEC-ASLR-V3 | ASLR v3: full randomization (PIE + stack + heap + mmap) | MEDIA | SEC-ASLR-V2 |
| SEC-NX | Non-executable stack enforcement | ALTA | — |
| SEC-NX-HEAP | Non-executable heap enforcement | ALTA | — |

### M3.2 — Performance (v0.63)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| PERF-SCHED-LOCKFREE | Scheduler lock-free (per-CPU run queues) | ALTA | — |
| PERF-SLAB-NUMA | Per-CPU heaps NUMA-aware | MEDIA | — |
| PERF-BENCH-SUITE | Benchmarking suite automática | MEDIA | — |
| PERF-PGO | Profile-guided optimization | BAJA | PERF-BENCH-SUITE |

### M3.3 — Documentación y Test Coverage (v0.64–v0.65)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| DOCS-API-COMPLETE | Documentación completa de API (syscalls, libneodos, drivers) | CRÍTICA | — |
| DOCS-SUBSYSTEMS | Documentación de todos los subsistemas kernel | ALTA | — |
| DOCS-TUTORIALS | Tutoriales: escribir driver, crear app, contribuir | ALTA | — |
| TEST-COVERAGE-80 | Coverage >80% de líneas | CRÍTICA | — |
| TEST-COVERAGE-95 | Coverage >95% de líneas | ALTA | TEST-COVERAGE-80 |

### M3.4 — Bugfixes y Hardening (v0.66–v0.69)

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| AUDIT-FUZZ-ROUND2 | Segunda ronda de fuzzing post-correcciones | CRÍTICA | SEC-FUZZ-SYSCALLS |
| BUG-ALL | Corrección de todos los bugs detectados | CRÍTICA | — |
| HARDEN-STATIC-BUFS | Eliminar todos los buffers estáticos globales | ALTA | — |
| HARDEN-OOB | Auditoría de bounds checking en todas las syscalls | ALTA | — |
| ABI-FREEZE-FINAL | Congelación final de ABI para v1.0 | CRÍTICA | — |

---

## 8. Fase 4: v1.0 — Primera API Estable

| ID | Tarea | Prioridad | Dependencias |
|----|-------|-----------|--------------|
| V1.0-RELEASE | Release v1.0.0 | CRÍTICA | Todo lo anterior |
| V1.0-ABI-FROZEN | Todas las interfaces congeladas (syscalls, NEM, eventos, capacidades) | CRÍTICA | ABI-FREEZE-FINAL |
| V1.0-DOCS | Documentación de release, changelog, guía de migración | CRÍTICA | DOCS-API-COMPLETE |
| V1.0-TESTS | Suite completa de tests (800+) | CRÍTICA | TEST-COVERAGE-95 |
| V1.0-NXE-COMPAT | Todos los binarios de usuario compilados contra ABI final | ALTA | V1.0-ABI-FROZEN |

**Objetivo:** Primera versión con API estable. Todo lo que se congela en v1.0
no cambia hasta v2.0. Contrato: drivers NEM v3, syscalls 0–77, formato NXE/NXP,
formato NeoFS v2.

---

## 9. Post-1.0: v1.x–v4.x

### v1.x — Mantenimiento y Features Compatibles

| Versión | Enfoque | Ejemplos |
|---------|---------|----------|
| v1.1 | Bugfixes post-release | Correcciones de la comunidad |
| v1.2 | Nuevas syscalls compatibles | Nuevos ObInfoClass, ObSetInfoClass |
| v1.3 | Nuevos drivers | VirtIO GPU, VirtIO Sound |
| v1.4 | Rendimiento | Optimizaciones sin breaking change |

### v2.x — Red y Virtualización

| Versión | Enfoque | Ejemplos |
|---------|---------|----------|
| v2.0 | API mayor v2 (con migración) | Posible microkernel híbrido |
| v2.1 | Networking avanzado | IPv6, VLAN, bonding, firewall |
| v2.2 | Virtualización | KVM enlightenments, Hyper-V |
| v2.3 | Servicios de red | NeoLDAP, NeoDNS (servidor), NeoDHCP (servidor) |
| v2.4 | Administración remota | WinRM-style, RPC sobre TCP |

### v3.x — GUI y Experiencia de Usuario

| Versión | Enfoque | Ejemplos |
|---------|---------|----------|
| v3.0 | API mayor v3 | GUI framework nativo |
| v3.1 | Compositor 2D | Ventanas, controles básicos |
| v3.2 | NeoStore | Tienda de aplicaciones NXP |
| v3.3 | Gráficos 3D | Aceleración GPU básica (Vulkan?) |
| v3.4 | Experiencia de escritorio | File manager, taskbar, start menu |

### v4.x — Escalabilidad Enterprise

| Versión | Enfoque | Ejemplos |
|---------|---------|----------|
| v4.0 | API mayor v4 | NUMA completo, clustering |
| v4.1 | Alta disponibilidad | Failover, replicación |
| v4.2 | Gestión de energía enterprise | S3/S4, wake-on-LAN |
| v4.3 | Virtualización de almacenamiento | SAN, iSCSI, NVMe-oF |
| v4.4 | Seguridad avanzada | TPM 2.0, Secure Boot completo, measured boot |

---

## 10. Deuda Técnica y Auditorías

### TD.1 — Refactorización de Arrays Fijos (completado en v0.41)

El contenedor `Slab<T>` y la migración a `Vec<T>` en scheduler, pipes, y driver
slots ya está completada. Verificar que no queden arrays fijos residuales.

### TD.2 — Buddy Bitmap Dinámico (completado en v0.40)

El bitmap del buddy allocator ya es dinámico. Verificar cobertura de tests.

### TD.3 — Static Buffers Globales (v0.66)

| Tarea | Archivos | Prioridad |
|-------|----------|-----------|
| Eliminar BIN_BUF[65536] static | `src/syscall/mod.rs` | ALTA |
| Eliminar CMD_BUF[65536] static | `src/syscall/mod.rs` | ALTA |
| Eliminar buffers fijos en crash dump | `src/crash/mod.rs` | MEDIA |
| Eliminar buffers fijos en stack trace | `src/crash/mod.rs` | MEDIA |
| Eliminar buffers fijos en serial | `src/arch/x64/serial.rs` | BAJA |

### TD.4 — CLEANUP-1..35 (Fase 2: v0.56–v0.58)

Agrupar los 35 items de cleanup en paquetes de trabajo:

| Paquete | Items | Archivos afectados |
|---------|-------|-------------------|
| CLEANUP-DEADCODE | CLEANUP-1,2,3,4,9,10,11,12,13,14 | Múltiples |
| CLEANUP-DUPLICATES | CLEANUP-5,6,7,15,16,18,24,25,26,27,28 | Múltiples |
| CLEANUP-REFACTOR | CLEANUP-8,17,19,20,21,22,23,29,30,31,32,33,34,35 | Múltiples |

### TD.5 — Objeto Manager Consolidación (v0.57)

| Tarea | Prioridad |
|-------|-----------|
| AI-1: Completar ObInfoClass/ObSetInfoClass enums | MEDIA |
| AI-2: Consolidar legacy syscall wrappers | BAJA |
| AI-3: ObObjectTable lock granularity (lock striping) | BAJA |
| AI-4: Arreglar TOCTOU race en kobj_register | BAJA |

### TD.6 — Estabilización ABI (v0.69)

| Tarea | Prioridad |
|-------|-----------|
| SSDT-DRVUNLOAD: Migrar sys_driver_unload a Ob API | MEDIA |
| Verificar todas las interfaces congeladas tienen tests | CRÍTICA |
| Validar que ningún struct ABI ha cambiado desde v0.50 | CRÍTICA |

---

## 11. Dependencias Críticas

### 11.1 Dependencias Entre Milestones

```
M0.1 (v0.50): Sin dependencias externas (es el milestone actual)
  │
  ├──► M1.1 (NeoFS v2): Depende de page cache (ya existe)
  │
  ├──► M1.2 (Shell Phase 2): Depende de SH-TOKEN+QUOTE (completado)
  │
  ├──► M1.3 (SAM + Net): Depende de M0.1 (hardening)
  │
  └──► M1.4 (VirtIO): Depende de A2.1 (ECAM, ya completado)
         │
         ├──► M1.7 (Registry Phase 2): Depende de CM-FIX (completado)
         │
         ├──► M1.8 (Module Signing): Depende de M1.3 (SAM)
         │
         └──► M2.1 (NXE/NXP): Independiente (herramientas host)
                │
                ├──► M2.2 (Executive): Depende de M1.7 + M1.5
                │
                ├──► M2.3 (Tools): Independiente (herramientas host)
                │
                ├──► M2.4 (Install): Depende de M2.1 + M1.1
                │
                └──► M3.x (Security): Depende de casi todo lo anterior
                       │
                       └──► v1.0: Depende de M3.x completo
```

### 11.2 Dependencias Incorrectas Eliminadas

| Antes | Después | Razón |
|-------|---------|-------|
| NFSv2-BTREE ← NFSv2-SYSCALL | NFSv2-SYSCALL ← NFSv2-BTREE | La syscall depende del B-tree, no al revés |
| USR-P1c ← USR-P1a | USR-P1c ← USR-P1a (correcta) | — |
| USR-P2a ← USR-P1a (OK) | — | — |
| I18N-P2 ← I18N-P1 (OK) | — | — |
| PM-PHASE4 ← PM-PHASE3 | PM-PHASE4 ← PM-PHASE3 (OK) | — |

### 11.3 Dependencias Añadidas

| Tarea | Nueva Dependencia | Razón |
|-------|------------------|-------|
| AUDIT-35 (Registry hardening) | CM-FIX | No tiene sentido endurecer antes de estabilizar |
| B5.2 (Driver permission) | USR-P1d | Usa SeAccessCheck, que se arregla en USR-P1d |
| USR-P4d (User profile hive) | USR-P1c | Necesita SAM para obtener SID del usuario |
| NET-DNS | NET-1.9 | DNS usa configuración de interfaz de ipconfig.nxe |

---

## 12. Priorización Global

### CRÍTICA (Bloqueante para v1.0)

| ID | Razón |
|----|-------|
| AUDIT-32..37 | Previenen pánicos en producción |
| SEC-AUDIT-FULL | Sin auditoría no se puede afirmar que el sistema es seguro |
| SEC-FUZZ-SYSCALLS | Sin fuzzing hay bugs desconocidos |
| DOCS-API-COMPLETE | Sin documentación la API no es utilizable |
| TEST-COVERAGE-80 | Sin cobertura no se puede garantizar estabilidad |
| ABI-FREEZE-FINAL | Sin congelación no hay v1.0 |
| V1.0-* | Release blocking |

### ALTA

| ID | Razón |
|----|-------|
| NeoFS v2 completo | Sin snapshot y B-tree, el FS está incompleto |
| Shell Phase 2 | Sin scripting, el shell es limitado |
| SAM foundation | Sin SAM no hay modelo de usuarios |
| Sessions (USR-P2) | Sin sesiones no hay multi-usuario |
| FS Security (USR-P3) | Sin permisos en VFS, la seguridad es incompleta |
| VirtIO ARCH+NET+BLK | Sin VirtIO, el rendimiento en VM es pobre |
| IRP per-process (B4.7) | Sin NXL per-process, las librerías no escalan |
| Registry multi-hive | Sin multi-hive, el Registry no es NT-completo |
| Integrity levels | Sin IL, el modelo de seguridad es básico |
| Module signing | Sin firmas, la carga de drivers es insegura |
| Configuration Manager | Sin CM, el Executive está incompleto |
| NeoDev v2 | Sin NeoDev completo, el build es frágil |
| Installation | Sin instalación, NeoDOS no es distribuible |

### MEDIA

| ID | Razón |
|----|-------|
| Power Manager Phase 3–5 | Funcionalidad importante pero no bloqueante |
| NTP client | Bueno tener, no crítico |
| DNS resolver | Necesario para apps de red |
| NXE metadata auto-generation | Automatización, no funcionalidad |
| VirtIO 9P + Input | Periféricos secundarios |
| Kernel debugger | Herramienta de desarrollo |
| NXE ecosystem completion | Importante pero no bloqueante para v1.0 |
| CMS multi-hive | Registry incompleto sin multi-hive |

### BAJA

| ID | Razón |
|----|-------|
| NXE-ECO-14..22 (fases futuras) | No bloqueantes |
| VirtIO GPU/Sound/SCSI | Periféricos especializados |
| I18N-P7..P12 | Features avanzadas de i18n |
| USR-P6 (user commands) | Comandos de usuario (WHOAMI, SU, etc.) |
| VFS-6.x, VFS-7.x | Features VFS avanzadas |
| Compositor 2D | Post-1.0 |
| NeoStore | Post-1.0 |
| GUI | Post-1.0 (v3.x) |

### EXPERIMENTAL

| ID | Razón |
|----|-------|
| B7.1 (Full GUI system) | Requiere mucho diseño |
| B7.2 (TPM Secure Boot) | Dependencia de hardware |
| B7.3 (Package manager) | Post-1.0 |
| B7.4 (Time-travel debugging) | Muy complejo |
| B7.5 (Live kernel patching) | Muy complejo |
| B7.6 (Distributed NeoDOS) | Muy ambicioso |
| B6.2 (COW fork) | Contradice modelo NT |

---

## 13. Consistencia Arquitectónica

### 13.1 Tareas que Contradicen la Arquitectura NT-like

| Tarea | Problema | Alternativa Propuesta |
|-------|----------|----------------------|
| B6.2 (COW fork) | fork es modelo Unix, no NT. NeoDOS usa sys_spawn | Despriorizar. Si se necesita, implementar como sys_clone estilo NT (CreateProcess) |
| VFS-6.4 (Async VFS via IRP) | Ya existe IOCP en el diseño (§7.5 ARCHITECTURAL_VISION) | Implementar IOCP en lugar de async IRP directo |
| sys_poll (RAX 59) | Ya existe sys_ob_wait con KWait | sys_poll debe delegar en KWait, no implementar lógica propia |

### 13.2 Tareas Alineadas con la Filosofía NT

| Tarea | Principio NT |
|-------|-------------|
| Object Manager | Central: todo recurso es un objeto |
| SAM + Sessions | Modelo de seguridad NT (SID, Token, ACL) |
| Registry multi-hive | HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER |
| Executive Manager | NT Executive: Configuration Manager, Session Manager |
| \DosDevices namespace | Namespace NT: \Global??, \DosDevices\ |
| Service Manager | SCM (Service Control Manager) |
| Driver signing | Driver signature enforcement (Vista+) |
| Integrity levels | Mandatory Integrity Control (Vista+) |

### 13.3 Decisiones Arquitectónicas Protegidas

| Decisión | No cambiar porque |
|----------|-------------------|
| No sys_fork | fork no es NT. Crear procesos via sys_spawn |
| NEM driver isolation | Diferencia competitiva de NeoDOS |
| Rust como único lenguaje | Coherencia del código base |
| SSDT dispatch (O(1)) | Más rápido y predecible que match |
| IRQL framework | Reemplaza correctamente CLI/STI |
| Process Manager en kernel | Modelo monolítico NT |

---

## 14. Apéndice: Mapa de Migración desde Roadmap Anterior

### Items que Cambian de Versión

| Item Anterior | Versión Anterior | Nueva Versión | Razón |
|---------------|------------------|---------------|-------|
| NFSv2-BTREE | v0.51 | v0.51 (M1.1) | Sin cambio |
| SH-EDITOR+HISTORY | v0.51 | v0.51 (M1.2) | Sin cambio |
| VIO-ARCH | v0.52 | v0.52 (M1.4) | Sin cambio |
| USR-P2a | v0.52 | v0.52 (M1.5) | Sin cambio |
| USR-P4a | v0.53 | v0.53 (M1.7) | Sin cambio |
| B5.1 | v0.53 | v0.53 (M1.8) | Sin cambio |
| NET-DNS | v0.54 | v0.54 (M1.9) | Sin cambio |
| CM-WAL | v0.54 | v0.55 (M1.10) | No es crítico para v0.54 |
| B5.3 | v0.54 | v0.55 (M1.10) | Depende de B5.1 |
| I18N-P2 | v0.54 | v0.56 (M2.1) | No es crítico, mejor junto a NXE |
| NXE-ECO-12 | backlog | v0.56 (M2.1) | Agrupado con ecosistema NXE |
| Executive (nuevo) | — | v0.57 (M2.2) | Nuevo milestone |
| Herramientas (nuevo) | — | v0.58 (M2.3) | Nuevo milestone |
| Instalación (nuevo) | — | v0.59 (M2.4) | Nuevo milestone |
| I18N-P7..P12 | v0.54 | v0.60 (M2.5) | Baja prioridad |
| SEC-ASLR-V2 | v0.49 | v0.61 (M3.1) | Seguridad pre-1.0 |
| Performance | v0.53 | v0.63 (M3.2) | Post-seguridad |
| Documentación | v0.54 | v0.64 (M3.3) | Post-rendimiento |
| CLEANUP-1..35 | v0.54+ | Múltiples (TD.4) | Refactorización continua |

### Items Eliminados

| Item | Razón |
|------|-------|
| B6.2 (COW fork) | Contradice modelo NT. Reemplazar por sys_clone si es necesario |
| PKG-1 (NeoGet v1) | Diferido a post-1.0 (NeoStore) |
| B7.1..B7.6 | Mover a experimental post-1.0 |
| VFS-6.4 (Async via IRP) | Reemplazar por IOCP (ARCHITECTURAL_VISION §7.5) |

### Items Promocionados

| Item | De | A | Razón |
|------|-----|-----|-------|
| CM-DIRTY | MEDIUM | ALTA | Necesario para multi-hive |
| CM-MULTI | MEDIUM | ALTA | Necesario para Registry completo |
| USR-P4a | MEDIUM | ALTA | Seguridad del Registry |
| Executive (nuevo) | — | ALTA | Componente central del modelo NT |
| NeoDev v2 | MEDIUM | ALTA | Herramienta oficial de desarrollo |
| Installation | — | ALTA | Bloqueante para distribución |

---

---

## Apéndice B: Sincronización con GitHub

> **A partir de la reorganización del flujo de trabajo, GitHub Issues es el sistema
> oficial de planificación, seguimiento e histórico del proyecto.**

### Estructura

```
roadmap/
├── improvements.md        # Ideas locales → convertidas a Issues por la IA
├── labels.yaml            # Definición de labels de GitHub
├── milestones.yaml        # Definición de milestones (versiones)
├── issue_templates/       # Templates para crear Issues
│   ├── feature.md
│   ├── bug.md
│   ├── task.md
│   └── completed-feature.md
scripts/
└── sync-roadmap.sh        # Sincronización idempotente con GitHub
    └── lib/
        ├── github.sh      # Wrapper de gh + gh api
        ├── labels.sh      # Gestión de labels
        ├── milestones.sh  # Gestión de milestones
        └── issues.sh      # Gestión de issues
```

### Comandos

```bash
scripts/sync-roadmap.sh sync       # Sincroniza todo (idempotente)
scripts/sync-roadmap.sh labels     # Crea/actualiza labels
scripts/sync-roadmap.sh milestones # Crea/actualiza milestones
scripts/sync-roadmap.sh issues     # Crea/actualiza issues
scripts/sync-roadmap.sh changelog  # Genera changelog desde milestones
scripts/sync-roadmap.sh check      # Verifica configuración
```

### Flujo

1. Las nuevas ideas se añaden a `roadmap/improvements.md`.
2. La IA o `sync-roadmap.sh` convierten cada idea en una GitHub Issue.
3. El desarrollo diario referencia Issues en commits y PRs.
4. Las releases son Milestones; cuando todas sus Issues están cerradas,
   la versión está terminada.
5. El changelog se genera automáticamente desde Issues cerradas por Milestone.

*Este documento es la hoja de ruta oficial del proyecto NeoDOS. Reemplaza al
antiguo `docs/roadmap.md` (eliminado) como referencia de planificación.
El detalle granular de cada tarea se mantiene en `roadmap/improvements.md`
(que se sincroniza con GitHub Issues).*
