# NeoDOS — Roadmap Estratégico y Arquitectural

> Documento maestro de evolución técnica
> Formato: Markdown (.md)
> Objetivo: Consolidar la transición de NeoDOS desde kernel experimental hacia sistema operativo modular estable.
>
> Rama objetivo: v0.20
> Base actual: v0.14.0
> Estado: Arquitectura activa
>
> Última revisión: Mayo 2026

---

# 1. Visión General

NeoDOS ha superado la fase de "toy OS".

Actualmente el proyecto ya dispone de:

* kernel x86_64 funcional
* modo protegido / long mode
* GDT / IDT / paging
* VFS operativo
* multitarea básica
* syscalls funcionales
* memoria dinámica
* soporte ATA/AHCI
* filesystem propio (NeoFS)
* carga de módulos inicial (.NDM)
* shell funcional
* soporte GPT + EFI

El principal objetivo ya NO es añadir funcionalidades rápidamente.

El objetivo real ahora es:

1. estabilizar arquitectura
2. congelar interfaces críticas
3. desacoplar subsistemas
4. reducir deuda técnica
5. consolidar userland
6. preparar extensibilidad real

---

# 2. Objetivo Estratégico v0.20

NeoDOS v0.20 debe convertirse en:

> un sistema operativo modular, extensible y relativamente estable

con:

* ABI de syscalls congelada
* drivers desacoplados
* BlockDevice abstraction
* ELF userland
* módulos cargables
* IPC funcional
* tooling básico
* FS estable y verificable
* entorno de desarrollo externo

La meta NO es competir con Linux o BSD.

La meta es construir:

* una arquitectura coherente
* un kernel mantenible
* una base técnica extensible
* una plataforma experimental seria

---

# 3. Filosofía Arquitectural

> Versión objetivo: 0.20
> Estado actual base: 0.10.x
> Documento de prioridades arquitecturales y estabilización

---

# Filosofía del Proyecto

NeoDOS ya no está en fase de "toy OS".

El objetivo actual NO es añadir funcionalidades rápidamente.
El objetivo real es:

* estabilizar arquitectura
* congelar interfaces críticas
* reducir deuda técnica
* consolidar capas internas
* preparar extensibilidad real

---

# Objetivo Estratégico v0.20

Convertir NeoDOS en:

> un sistema operativo modular, estable y extensible

con:

* ABI de syscalls estable
* VFS consolidado
* drivers desacoplados
* userland usable
* módulos cargables reales
* herramientas de sistema mínimas

---

# PRIORIDAD S — CRÍTICO ABSOLUTO

Estas tareas desbloquean todo el roadmap futuro.

---

## S1. Estabilización de Syscalls

### Estado

Parcialmente funcional.

### Objetivo

Congelar ABI v0.

### Requisitos

* eliminar `unwrap()/expect()` en rutas críticas
* validar ABI userland ↔ kernel
* asegurar stack alignment correcto
* revisar `open/read/write/close`
* asegurar propagación de errores mediante `Result`

### Impacto

Desbloquea:

* ELF
* libneodos
* pipes
* redirección
* shell avanzado
* userland real

---

## S2. BlockDevice Abstraction (#53)

### Estado

**COMPLETADO (v0.15.0).** `BlockDevice` trait en `drivers/block.rs`. `StorageManager` (`drivers/storage_manager.rs`) orquesta init: crea ATA, escanea PCI DMA, prueba AHCI, registra el mejor dispositivo en `BlockDeviceManager`. FAT32 e ISO9660 ya no dependen de globals legacy (`ATA_DRIVER`), usan `BLOCK_DEVICES.get(0)`. Globals legacy eliminados.

### Objetivo

Crear capa unificada:

```rust
trait BlockDevice {
    fn read_blocks(...);
    fn write_blocks(...);
    fn flush(...);
}
```

### Requisitos

* desacoplar FS de drivers
* registrar dispositivos dinámicamente
* soporte uniforme ATA/AHCI/RAMDisk/VirtIO

### Impacto

Desbloquea:

* USB storage
* swap
* journaling
* FSCK
* cifrado
* instalación
* ISO9660
* VirtIO
* defrag

---

## S3. Module ABI v0 (.NDM / .SYS)

### Estado

**COMPLETADO (v0.10.5).** Header NDM v1 (64 bytes) con magic, version, code/data sections, entry point, api_version, name. Kernel service table (`KernelServiceTableV1`) en `0x4FFFF00` con 12 funciones (console, serial, frame alloc, I/O ports, block I/O). `LOAD` command valida header o cae a raw binary. `generate_driver.py` produce NDM v1. Especificación completa en `docs/MODULE_ABI.md`.

### Objetivo

Congelar ABI modular — **OK**.

### Requisitos

* `#[repr(C)]` — **OK** (`NdModuleHeader`, `KernelServiceTableV1`)
* versionado obligatorio — **OK** (`NDM_ABI_VERSION = 1`, verificado en `from_bytes`)
* export table controlada — **OK** (`KernelServiceTableV1` con function pointers)
* tipos de módulo — **OK** (`ModuleType::Driver/FileSystem/ShellExtension`)
* validación de compatibilidad — **OK** (magic, version, api_version, bounds, overlap)

### Impacto

Desbloquea:

* drivers dinámicos — **habilitado** (demo `driver.ndm` se registra como device handler)
* FS cargables — **pendiente** (necesita registro en VFS)
* modularidad real — **habilitado**
* builds independientes — **habilitado** (contrato binario estable)

---

## S4. Validation & Regression Infrastructure

### Estado

**COMPLETADO (v0.10.5).** Ver `docs/KERNEL_VALIDATION.md`.

### Componentes

* **Trace ring buffer** (`src/trace.rs`): 1024 entradas lock-free, eventos de context switch/syscall/IRQ/scheduler, dump en panic.
* **Panic classification** (`src/panic_classification.rs`): 14 categorías con clasificación por vector+RIP+error code, dump forense (trace + scheduler state).
* **Runtime invariants** (`src/invariants.rs`): IRQ nesting counter, timer-IRQ context switch guard, stack alignment check, macros `kern_assert!` (cfg-gated).
* **Stress harness** (`src/testing.rs`): 8 nuevos tests (scheduler yield/state, syscall rapid getpid/fuzzing/ptr validation, memory alloc/vec/string churn).
* **Regression runner** (`scripts/regression_runner.py`): 100+ iteraciones, detección de panics, informe estructurado con panic signatures.
* **ABI validation**: syscall numbers >19 → u64::MAX; layout assertions compile-time para `NdModuleHeader` y `KernelServiceTableV1`.
* **Build profiles**: `validation` y `stress` features en Cargo.toml.

### Pendiente

* Replay de trace: volcado a NeoDOS FS para análisis offline.
* Stress de IRQ: generación de interrupciones rápidas desde software.
* Cobertura de invariantes: page table corruption, TSS consistency.
* 1000+ iteraciones continuas sin fallo.

---

## ~~S4. Eliminación de Panic Paths (#10)~~

### Estado

**COMPLETADO (v0.11.0).** Cero `.unwrap()` en el kernel. Solo 6 `.expect()` en boot paths (serial, block device init, scheduler init) — todos en código de arranque donde el fallo es necesariamente fatal.

---

# PRIORIDAD A — INFRAESTRUCTURA MAYOR

Estas tareas convierten NeoDOS en un sistema operativo extensible.

---

## A1. libneodos (#54)

### Objetivo

Crear standard library oficial para userland.

### Funcionalidades

* wrappers de syscalls
* macros seguras
* IO
* FS API
* memoria

### Impacto

* simplifica desarrollo
* desacopla userland del ABI crudo
* prepara SDK

---

## A2. ELF Loader (#55)

### Objetivo

Abandonar binarios planos.

### Requisitos

* ELF parsing
* relocalización básica
* segmentos múltiples
* memoria dinámica por proceso

### Impacto

* ejecutables reales
* toolchains modernas
* Rust/C userland serio

---

## A3. IPC / Pipes (#61)

### Objetivo

Pipes reales entre procesos.

### Requisitos

* pipe buffers
* stdin/stdout redirect
* blocking reads
* scheduler integration

### Impacto

* shell moderno
* multitarea usable
* pipelines

---

## A4. FSCK Utility (#37)

### Objetivo

Verificación y reparación de NeoFS.

### Requisitos

* inode validation
* block bitmap validation
* orphan detection
* repair mode

### Impacto

* seguridad de datos
* debugging FS
* recuperación

---

## A5. FAT32 Write Support (#62)

### Objetivo

Escritura real en FAT32.

### Impacto

* interoperabilidad
* instalación dual
* intercambio de archivos

---

## A6. Loadable Drivers (#69)

### Objetivo

Drivers cargables reales.

### Requisitos

* integración con Module ABI
* registro dinámico
* lifecycle control
* validación de compatibilidad

---

# PRIORIDAD B — USERLAND Y UX

Estas tareas mejoran usabilidad y tooling.

---

## ~~B1. Historial de comandos (#18)~~

**COMPLETADO (v0.15.3).** Buffer circular de 32 entradas, navegación ↑/↓ en el shell. Driver PS/2 emite 0x01 (up) / 0x02 (down) desde `keyboard.rs`.

---

## B2. Redirección de Output (#42)

```bash
DIR > FILE.TXT
```

---

## B3. PATH Resolution (#41)

* búsqueda automática
* ejecución recursiva

---

## B4. Sistema HELP (#89)

* `HELP`
* `/?`
* documentación integrada

---

## B5. NeoEdit (#80)

Editor de texto integrado.

---

## B6. Batch avanzado (#63)

* IF
* GOTO
* variables
* wildcards

---

## B7. RTC + timestamps (#64)

* `ctime`
* `mtime`

---

## B8. Terminales virtuales (#90)

* Alt+F1..F4
* múltiples sesiones

---

# PRIORIDAD C — HARDWARE Y RENDIMIENTO

---

## C1. DMA dinámico (#52)

### Objetivo

Eliminar buffers estáticos.

### Requisitos

* PRDT dinámico
* multi-block DMA
* page pools

---

## C2. Paging optimizado (#29)

### Objetivo

Reutilización de page tables.

---

## C3. Input lock-free (#20)

### Objetivo

Eliminar enable/disable interrupts frecuentes.

---

## C4. VirtIO Drivers (#79)

### Objetivo

Optimización VM.

---

## C5. USB HID estable (#58)

### Objetivo

Teclados USB reales.

---

## C6. USB Mass Storage (#75)

### Objetivo

Pendrives.

---

## C7. ACPI (#68)

### Objetivo

Shutdown/Reboot reales.

---

# PRIORIDAD D — FEATURES AVANZADAS

NO recomendadas antes de v1.0.

---

## D1. Journaling (#77)

### Riesgo

Muy alta complejidad.

---

## D2. Swap (#84)

### Riesgo

Interacción MMU + FS + scheduler.

---

## D3. Network Stack (#67)

### Riesgo

Multiplica complejidad global.

---

## D4. SMP (#74)

### Riesgo

Rompe supuestos actuales del kernel.

---

## D5. GUI (#65)

### Riesgo

Consumo enorme de tiempo.

---

## D6. Encryption (#85)

### Riesgo

Persistencia + recovery + boot.

---

# PRIORIDAD E — ECOSISTEMA

---

## E1. SDK externo (#76)

### Objetivo

Toolchain oficial.

---

## E2. Setup Installer (#87)

### Objetivo

Instalación automatizada.

---

## E3. Package Manager (#95)

### Objetivo

Distribución de software.

---

## E4. CI Integration (#59)

### Objetivo

Testing automático.

---

## E5. Documentation Master Manual (#100)

### Objetivo

Manual oficial.

---

# ROADMAP RECOMENDADO

---

# v0.11 — Stabilization Phase

## Objetivos

* eliminar panic paths
* limpiar warnings críticos
* estabilizar syscalls
* congelar ABI base

---

# v0.12 — Storage Abstraction

## Objetivos

* BlockDevice trait
* desacoplar FS/drivers
* limpieza ATA/AHCI

---

# v0.13 — Modular Kernel

## Objetivos

* Module ABI v0
* drivers cargables
* validación de módulos

---

# v0.14 — Userland Foundation

## Objetivos

* libneodos
* ELF básico
* mejoras shell

---

# v0.15 — Storage Manager

## Objetivos

* StorageManager: discovery PCI + init ATA/AHCI unificado  **✓**
* Eliminar globals legacy (ATA_DRIVER, AHCI_DRIVER)  **✓**
* FAT32/ISO9660 migrados a BlockDeviceManager  **✓**

---

# v0.16 — Filesystem Reliability

## Objetivos

* FSCK
* FAT32 write
* timestamps

---

# v0.17 — Hardware Expansion

## Objetivos

* USB HID
* VirtIO
* ACPI

---

# v0.18 — Ecosystem

## Objetivos

* SDK
* NeoEdit
* HELP
* CI

---

# v0.19 — Real Hardware Preparation

## Objetivos

* USB storage
* installer
* recovery tooling

---

# v0.20 — Extensible OS Milestone

## Resultado esperado

NeoDOS debe ser:

* modular
* estable
* extensible
* con userland usable
* con ABI relativamente congelada
* con tooling mínimo funcional

---

# NO RECOMENDADO ANTES DE v1.0

* GUI avanzada
* SMP
* journaling
* swap
* network stack completo
* encryption
* TTF
* sound stack

---

# Funcionalidades Propuestas Futuras

---

# Subsistema de Seguridad

## Secure Boot Experimental

### Objetivo

Permitir validación criptográfica básica de módulos y kernel.

### Propuesta

* hashes SHA-256 para módulos `.NDM`
* firma opcional de drivers
* validación durante carga
* modo developer bypass

### Impacto

* integridad del sistema
* prevención de corrupción accidental
* base para NeoPKG seguro

---

## Sistema de Permisos Básico

### Objetivo

Introducir separación inicial entre procesos y archivos.

### Propuesta

* flags READ/WRITE/EXECUTE
* atributos de archivo
* ownership básico
* permisos mínimos por proceso

### Impacto

* mejora seguridad
* prepara multiusuario futuro

---

# Subsistema de Memoria

## Memory-Mapped Files

### Objetivo

Permitir mapear archivos directamente en memoria.

### Propuesta

* `mmap()` sobre archivos
* lazy loading
* integración con paging

### Impacto

* carga ELF más eficiente
* cache FS avanzada
* optimización IO

---

## Kernel Slab Allocator

### Objetivo

Reducir fragmentación del heap kernel.

### Propuesta

* caches por tamaño
* alloc/free rápidos
* slabs para inodos y estructuras FS

### Impacto

* mejor rendimiento
* menor fragmentación
* menor presión sobre allocator general

---

# Subsistema de Procesos

## Scheduler Prioritario

### Objetivo

Mejorar respuesta bajo carga.

### Propuesta

* prioridades por proceso
* time slices dinámicos
* idle task dedicada

### Impacto

* shell más fluido
* multitarea más usable

---

## Señales Userland

### Objetivo

Manejo controlado de excepciones y eventos.

### Propuesta

* SIGSEGV
* SIGTERM
* SIGINT
* handlers userland

### Impacto

* procesos más robustos
* debugging avanzado

---

# Subsistema de Archivos

## Hard Links y Symbolic Links

### Objetivo

Mejorar flexibilidad del filesystem.

### Propuesta

* enlaces duros
* enlaces simbólicos
* resolución VFS transparente

### Impacto

* estructura UNIX-like opcional
* compatibilidad futura

---

## Compresión Transparente de Archivos

### Objetivo

Reducir consumo de disco.

### Propuesta

* bloques comprimidos opcionales
* DEFLATE/LZ4
* flags por archivo

### Impacto

* imágenes más pequeñas
* mejor aprovechamiento almacenamiento

---

# Subsistema de Desarrollo

## Kernel Debugger Integrado

### Objetivo

Debugging sin GDB externo.

### Propuesta

* breakpoints
* stack traces
* dump de memoria
* inspección de procesos

### Impacto

* debugging más rápido
* menos dependencia externa

---

## Crash Dump System

### Objetivo

Persistir información tras kernel panic.

### Propuesta

* panic dumps
* stack snapshots
* registros CPU
* volcados opcionales a disco

### Impacto

* análisis post-mortem
* reducción de bugs difíciles

---

# Subsistema Gráfico

## Compositor 2D Básico

### Objetivo

Preparar GUI futura.

### Propuesta

* ventanas en memoria
* doble buffering
* clipping básico
* redraw parcial

### Impacto

* base para GUI estable
* rendimiento gráfico mejorado

---

## Driver GPU Lineal

### Objetivo

Abstracción simple de framebuffer.

### Propuesta

* backend GOP/VBE
* primitivas aceleradas simples
* surfaces

### Impacto

* simplifica rendering
* desacopla GUI del hardware

---

# Subsistema de Red

## Socket API Básica

### Objetivo

Preparar networking moderno.

### Propuesta

* sockets UDP/TCP
* bind/listen/connect
* integración syscall

### Impacto

* servicios de red
* transferencia de archivos
* terminal remota

---

## Cliente DHCP

### Objetivo

Configuración automática de red.

### Propuesta

* DHCP discover/request
* configuración IP dinámica

### Impacto

* networking usable automáticamente

---

# Virtualización

## Guest Additions NeoDOS

### Objetivo

Mejor experiencia en VM.

### Propuesta

* sincronización de ratón
* shared clipboard
* shared folders

### Impacto

* desarrollo más cómodo

---

## Snapshot Awareness

### Objetivo

Detectar restauraciones de VM.

### Propuesta

* UUID boot session
* invalidación cache
* detección rollback

### Impacto

* debugging más fiable

---

# Herramientas Avanzadas

## NeoTOP

### Objetivo

Monitor de procesos y memoria.

### Propuesta

* CPU usage
* memoria
* IO
* scheduler stats

### Impacto

* profiling básico
* debugging runtime

---

## NeoTrace

### Objetivo

Tracing de syscalls.

### Propuesta

* hooks syscall
* logs por proceso
* tracing filtros

### Impacto

* debugging userland
* profiling

---

## NeoPkg Repository Server

### Objetivo

Backend oficial de paquetes.

### Propuesta

* índices firmados
* mirrors
* dependencias

### Impacto

* ecosistema real

---

# Compatibilidad y Portabilidad

## ARM64 Backend

### Objetivo

Portabilidad multi-arquitectura.

### Propuesta

* backend limpio arch/
* MMU ARM64
* exception vectors
* timer ARM generic

### Impacto

* Raspberry Pi
* hardware ARM moderno

---

## RISC-V Experimental

### Objetivo

Arquitectura abierta experimental.

### Propuesta

* boot RV64
* paging Sv39
* SBI support

### Impacto

* investigación kernel
* portabilidad extrema

---

# Calidad y Tooling

## Build Profiles Avanzados

### Objetivo

Separar builds debug/release.

### Propuesta

* debug kernel
* tracing builds
* minimal builds
* testing builds

### Impacto

* desarrollo más rápido
* testing más fiable

---

## Sistema de Benchmarks

### Objetivo

Medir regresiones de rendimiento.

### Propuesta

* IO benchmarks
* syscall latency
* scheduler benchmarks
* FS stress tests

### Impacto

* optimización real
* control de regresiones

---

# Funcionalidades Futuras Experimentales

---

# Inteligencia del Sistema

## Servicio de Telemetría Kernel Interna

### Objetivo

Recopilar métricas internas para debugging avanzado.

### Propuesta

* estadísticas scheduler
* page faults
* uso de memoria
* latencias de syscalls
* rendimiento de drivers

### Impacto

* profiling real
* detección de regresiones
* tuning del kernel

---

## Auto-Recovery de Kernel Panic

### Objetivo

Intentar recuperación parcial tras fallos no críticos.

### Propuesta

* reinicio aislado de drivers
* remount read-only
* recuperación shell mínima
* modo rescue automático

### Impacto

* mayor robustez
* menos pérdida de datos

---

# Subsistema de Almacenamiento

## RAID Software Experimental

### Objetivo

Soporte multidisco básico.

### Propuesta

* RAID0 striping
* RAID1 mirror
* metadata propia
* rebuild simple

### Impacto

* rendimiento
* redundancia
* preparación storage avanzado

---

## NVMe Driver

### Objetivo

Soporte moderno de almacenamiento.

### Propuesta

* queues NVMe
* MSI/MSI-X
* async completions

### Impacto

* rendimiento extremo
* hardware moderno

---

## Cache Global de Bloques

### Objetivo

Reducir IO redundante.

### Propuesta

* LRU cache
* write-back opcional
* dirty tracking
* flush scheduler

### Impacto

* FS más rápido
* menor desgaste SSD

---

# Subsistema de Drivers

## Driver Framework Oficial

### Objetivo

API consistente para drivers.

### Propuesta

* lifecycle estándar
* init/shutdown
* IRQ handlers
* registro dinámico
* descriptor de capacidades

### Impacto

* drivers mantenibles
* ABI estable

---

## Driver Sandboxing Experimental

### Objetivo

Reducir impacto de drivers defectuosos.

### Propuesta

* memoria aislada parcial
* validación de acceso
* watchdog drivers

### Impacto

* menos kernel panics
* debugging más seguro

---

# Subsistema Shell

## Alias y Configuración Persistente

### Objetivo

Customización del entorno.

### Propuesta

* alias
* variables persistentes
* perfiles shell
* autoexec avanzado

### Impacto

* UX mejorada
* scripting flexible

---

## Shell Multilínea

### Objetivo

Comandos complejos.

### Propuesta

* continuaciones `^`
* edición multilinea
* historial persistente

### Impacto

* scripting más potente

---

## NeoShell Script Language

### Objetivo

Lenguaje propio de automatización.

### Propuesta

* parser dedicado
* variables
* funciones
* loops
* arrays simples

### Impacto

* automatización avanzada
* tooling interno

---

# Subsistema Multimedia

## BMP/PNG Viewer

### Objetivo

Visualización básica de imágenes.

### Propuesta

* BMP decoder
* PNG decoder ligero
* framebuffer rendering

### Impacto

* tooling gráfico inicial

---

## WAV/PCM Audio Stack

### Objetivo

Audio userland básico.

### Propuesta

* mixer simple
* PCM playback
* streaming buffer

### Impacto

* multimedia básica

---

# Compatibilidad

## POSIX Compatibility Layer

### Objetivo

Portabilidad parcial de software UNIX.

### Propuesta

* wrappers POSIX
* open/read/write compatibles
* estructura tipo libc

### Impacto

* facilitar porting
* tooling externo

---

## Linux Syscall Translation Experimental

### Objetivo

Ejecutar binarios Linux simples.

### Propuesta

* capa syscall translation
* ELF Linux subset
* runtime compatibility

### Impacto

* ecosistema experimental

---

# Subsistema de Tiempo

## High Resolution Timers

### Objetivo

Timers precisos.

### Propuesta

* HPET
* APIC timers
* nanosecond timing

### Impacto

* scheduler mejorado
* multimedia
* networking

---

## NTP Client

### Objetivo

Sincronización horaria.

### Propuesta

* UDP NTP
* sync RTC
* drift correction

### Impacto

* timestamps fiables

---

# Subsistema de Consola

## ANSI Escape Support

### Objetivo

Terminal moderna.

### Propuesta

* colores ANSI
* cursor control
* clear screen
* VT100 subset

### Impacto

* tooling moderno
* mejor UX

---

## Scrollback Buffer

### Objetivo

Historial visual de terminal.

### Propuesta

* buffer circular VGA
* navegación scroll
* búsqueda futura

### Impacto

* debugging más cómodo

---

# Subsistema de Kernel

## Live Kernel Reload Experimental

### Objetivo

Recargar partes del kernel sin reboot.

### Propuesta

* módulos hot-reload
* reinicio parcial subsistemas
* invalidación segura

### Impacto

* desarrollo rapidísimo

---

## Capability-Based Security

### Objetivo

Permisos granulares.

### Propuesta

* capabilities por proceso
* acceso restringido a drivers
* permisos syscall

### Impacto

* seguridad moderna

---

## Async Kernel Tasks

### Objetivo

Operaciones no bloqueantes.

### Propuesta

* async IO
* deferred work queues
* background flushers

### Impacto

* mejor rendimiento IO

---

# Subsistema Distribuido

## Remote Shell

### Objetivo

Administración remota.

### Propuesta

* terminal TCP
* autenticación básica
* consola remota

### Impacto

* gestión headless

---

## Cluster Experimental

### Objetivo

Comunicación entre NeoDOS hosts.

### Propuesta

* mensajes nodo ↔ nodo
* jobs distribuidos
* FS compartido experimental

### Impacto

* investigación distribuida

---

# Herramientas de Desarrollo

## NeoProfiler

### Objetivo

Profiler kernel/userland.

### Propuesta

* CPU hotspots
* syscall profiling
* frame timing

### Impacto

* optimización seria

---

## NeoCoverage

### Objetivo

Cobertura de tests.

### Propuesta

* instrumentación kernel
* reports automáticos
* integración CI

### Impacto

* calidad del código

---

## Kernel Fuzzing

### Objetivo

Detección automática de bugs.

### Propuesta

* syscall fuzzing
* FS fuzzing
* malformed ELF fuzzing

### Impacto

* estabilidad extrema

---

# Subsistema Experimental Futuro

## Hypervisor NeoDOS

### Objetivo

Virtualización propia.

### Propuesta

* VT-x / AMD-V
* guest execution
* minimal VM monitor

### Impacto

* investigación avanzada

---

## Microkernel Research Branch

### Objetivo

Explorar arquitectura híbrida.

### Propuesta

* mover drivers userland
* IPC avanzado
* servicios aislados

### Impacto

* laboratorio arquitectural

---

## NeoAI Service Layer

### Objetivo

Automatización inteligente interna.

### Propuesta

* shell assistant
* diagnóstico automático
* análisis logs
* scripting inteligente

### Impacto

* tooling futurista

---

# Resumen Estratégico

## Prioridad máxima real

1. Syscalls
2. BlockDevice abstraction
3. Module ABI **✓ v0.10.5**
4. Panic elimination
5. libneodos
6. ELF
7. IPC
8. FSCK

---

# Meta realista

Si NeoDOS alcanza:

* ABI estable
* drivers modulares
* ELF
* pipes
* SDK
* FS estable

entonces ya deja definitivamente la categoría de "proyecto experimental" y entra en:

> sistema operativo extensible real

# Guía de Desarrollo: Qué hacer antes de cada nueva función

Antes de implementar cualquier nueva funcionalidad en NeoDOS, se debe seguir este protocolo obligatorio para mantener estabilidad arquitectural.

---

## 1. Evaluación de impacto

Antes de escribir código:

* ¿Afecta al kernel core?
* ¿Afecta a syscalls?
* ¿Afecta a VFS o FS?
* ¿Afecta a drivers o hardware?
* ¿Afecta a ABI o estructuras compartidas?

Si la respuesta es sí en cualquiera de estos:
👉 requiere diseño previo obligatorio

---

## 2. Clasificación de la función

Toda nueva función debe clasificarse como:

* 🟢 Infraestructura (kernel base)
* 🟡 Sistema (drivers / FS / scheduler)
* 🔵 Userland (shell / tools)
* 🟣 Experimental (no estable)

Esto define el nivel de revisión requerido.

---

## 3. Diseño previo obligatorio (PRE-DESIGN)

Antes de implementar:

* definir inputs/outputs
* definir structs implicados
* definir errores (`Result<T, E>`)
* definir interacción con syscalls
* definir impacto en memoria

Si no se puede explicar en pseudocódigo:
👉 no se implementa aún

---

## 4. Revisión de dependencias

Cada función debe verificar:

* dependencias de otros módulos
* acoplamiento con drivers
* uso de `unsafe`
* uso de global state

Regla:

> si depende de más de 2 subsistemas → probablemente mal diseñada

---

## 5. Regla de aislamiento

Toda nueva función debe intentar cumplir:

* mínimo acceso global
* no depender de `static mut`
* no modificar estado global sin lock
* no romper ABI

---

## 6. Validación de seguridad

Antes de integrar:

* ¿puede causar panic?
* ¿puede causar undefined behavior?
* ¿puede corromper FS?
* ¿puede romper scheduler?

Si sí → requiere sandbox o revisión manual

---

## 7. Test mental obligatorio (dry-run)

Simular ejecución:

* caso normal
* caso error
* caso edge (disk full, null pointer, IRQ interrupt)

Si falla en simulación mental:
👉 no implementar todavía

---

## 8. Compatibilidad ABI

Toda función nueva debe respetar:

* calling conventions
* struct alignment (`repr(C)` si aplica)
* syscall contract
* module ABI versioning

---

## 9. Revisión de rendimiento

Antes de implementar:

* coste en syscalls
* coste en memoria
* coste en IO
* posibles bloqueos

Optimización prematura está prohibida,
pero regresiones graves deben evitarse.

---

## 10. Integración progresiva

Ninguna función debe integrarse directamente en producción kernel:

* primero stub
* luego implementación parcial
* luego integración completa
* luego optimización

---

---

# Bugs — Histórico

## ~~GPF intermitente en syscall_handler_asm → iretq (#GPF)~~

### Estado

**RESUELTO.** Cada proceso tiene su propia pila Ring‑0 privada (`Process.kernel_stack_top`).
`TSS.RSP0` se actualiza en cada context switch (`syscall.rs:109`) y al lanzar un proceso
(`usermode.rs:102`), eliminando la sobrescritura de frames entre procesos.
Cada proceso necesitaría su propio `AlignedStack` de ~16 KB, y el TSS.RSP0 se actualizaría
en cada context switch. Esto eliminaría por completo la raza.

### Impacto

* estabilidad 100 % en tests
* elimina la necesidad de la mitigación de idle‑only
* permite preempción real entre procesos Ring‑3 en el timer handler

---

# Regla final

> Si una función no puede ser explicada, aislada y simulada antes de implementarse, no pertenece todavía al kernel.
