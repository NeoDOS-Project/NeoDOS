# NeoDOS — Un Sistema Operativo Moderno en Rust para x86-64

[![Version](https://img.shields.io/badge/version-v0.50.4-blue.svg)](CHANGELOG.md)
[![Tests](https://img.shields.io/badge/tests-716-green.svg)](neodos-kernel/src/testing.rs)
[![Rust](https://img.shields.io/badge/rust-nightly-orange.svg)](rust-toolchain.toml)
[![Organization](https://img.shields.io/badge/org-NeoDOS--Project-blueviolet.svg)](https://github.com/NeoDOS-Project)

> **Official repository moved to the [NeoDOS-Project](https://github.com/NeoDOS-Project) organization.**

NeoDOS es un sistema operativo de 64 bits escrito en Rust con arquitectura híbrida: kernel monolítico con subsistema de drivers aislados estilo microkernel. Arranca en UEFI, soporta SMP, tiene un planificador prioritario estilo NT, drivers con certificación y capacidades, un sistema de archivos propio, y un modelo de seguridad ACL-based.

> **Filosofía:** Explícito sobre mágico. Capas, no montones. Fallo rápido, fallo claro. Driver aislado, kernel pequeño.

---

## Arquitectura en 30 segundos

```text
Boot UEFI → Bootloader → Kernel (fases de boot) → NeoInit (PID 1) → NeoShell (Ring 3)
```

El kernel se organiza en 5 capas verticales:

1. **Arch (x86_64)** — GDT, IDT, paginación 4 niveles, SMP trampoline
2. **HAL v0.4** — raw/safe split, 26 primitivas extern "C", asm confinado
3. **System Services** — scheduler (4 prioridades, aging, work stealing), memory (buddy+slab, demand paging), KOBJ, VFS, IPC/pipes, IRP async I/O, Event Bus, seguridad NT6
4. **NEM Driver Runtime** — pipeline de certificación (8 estados), capacidades (12 flags), aislamiento X4 (16 slots × 1 MB), ABI versionado
5. **Syscall SSDT** — SSDT RAX 0-59, tabla de 256 slots, O(1) dispatch, tabla de permisos separada

---

## Estado Actual

| Aspecto | Estado |
| --------- | -------- |
| **Kernel** | v0.50.4 — 716 tests, SSDT RAX 0-59, 24 fases de boot |
| **Drivers NEM** | 7 drivers standalone (PS/2, serial, RTC, ACPI, PCI, ATA, AHCI) + 5 reference |
| **User-mode** | NeoShell Ring 3, 27 binarios .NXE, 2 DLLs .NXL (libneodos, libmath) |
| **Object Manager** | Ob unificado: handles, KOBJ, URN, seguridad (RAX 60-66) |
| **Input** | 4 Virtual Terminals (Alt+F1-F4), per-VT input queues, shadow buffers |
| **Virtual Terminals** | Console state save/restore per VT, framebuffer shadow redraw |
| **SMP** | 16 CPUs, per-CPU KPRCB, INIT-SIPI-SIPI AP bring-up, IPI (reschedule, TLB shootdown, call-function) |
| **Scheduler** | Dispatch commit point (`SELECT → VALIDATE FRAME → COMMIT → DISPATCH`), work stealing, per-CPU runqueues |
| **Seguridad** | NT6 SRM: SID, Token, ACL, ACE, SeAccessCheck |
| **Rendimiento** | HPET → APIC timer 1 KHz, slab con per-CPU hot cache, work stealing |

---

## Project Organization

NeoDOS is now developed under the [NeoDOS-Project](https://github.com/NeoDOS-Project) GitHub organization.

| Repository | Description |
|------------|-------------|
| [NeoDOS](https://github.com/NeoDOS-Project/NeoDOS) | Main repository (kernel, bootloader, drivers, tools, docs) |
| [.github](https://github.com/NeoDOS-Project/.github) | Community health files, issue and PR templates |

---

## Quick Start

NeoDev es una herramienta de desarrollo externa; instálala primero:

```bash
cargo install --git https://github.com/NeoDOS-Project/NeoDev.git

neodev build --image     # bootloader + kernel + user binaries + GPT disk image
neodev run               # QEMU + OVMF, serial, GDB :1234
neodev test              # suite de tests automática (716/716)
neodev list              # descubre los proyectos del workspace
neodev clean             # limpia artefactos de build
```

> Si `neodev test` reporta fallos de filesystem (p. ej. `709/716`) tras un arranque
> interactivo, el `disk_image.img` quedó sucio: reconstruye con `neodev build --image`
> y repite. Ver `docs/development/testing.md`.

---

## Documentación Clave

| Documento | Descripción |
| ----------- | ------------- |
| [Visión Arquitectónica](docs/architecture/vision.md) | Plan director, diagnóstico, roadmap v0.40→v1.0 |
| [Arquitectura](docs/architecture/overview.md) | Arquitectura actual del sistema |
| [Source of Truth](docs/architecture/source-of-truth.md) | Invariantes y contratos arquitectónicos |
| [Boot](docs/boot/boot-flow.md) | Boot sequence, fases, GPT layout |
| [Syscalls](docs/kernel/syscalls.md) | Referencia completa de syscalls |
| [Object Manager](docs/kernel/objects.md) | Ob types, namespace, operaciones |
| [Memory](docs/memory/memory.md) | Buddy allocator, slab, demand paging |
| [Drivers](docs/drivers/overview.md) | NEM format, certificación, capacidades |
| [Filesystem](docs/filesystem/overview.md) | NeoFS, VFS, IoStack |
| [Repository Architecture](docs/architecture/repository.md) | Propuesta de organización multi-repositorio |
| [Documentation Index](docs/README.md) | Índice maestro de documentación |
| [Testing](docs/development/testing.md) | Test suite, harness y caveats |
| [Debug](docs/development/debugging.md) | Guía de depuración con GDB |

---

## Licencia

NeoDOS es software experimental. Úselo bajo su propio riesgo.
