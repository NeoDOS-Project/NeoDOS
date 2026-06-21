# NeoDOS — Un Sistema Operativo Moderno en Rust para x86-64

[![Version](https://img.shields.io/badge/version-v0.39.11-blue.svg)](CHANGELOG.md)
[![Tests](https://img.shields.io/badge/tests-463-green.svg)](neodos-kernel/src/testing.rs)
[![Rust](https://img.shields.io/badge/rust-nightly-orange.svg)](rust-toolchain.toml)

NeoDOS es un sistema operativo de 64 bits escrito en Rust con arquitectura híbrida: kernel monolítico con subsistema de drivers aislados estilo microkernel. Arranca en UEFI, soporta SMP, tiene un planificador prioritario estilo NT, drivers con certificación y capacidades, un sistema de archivos propio, y un modelo de seguridad ACL-based.

> **Filosofía:** Explícito sobre mágico. Capas, no montones. Fallo rápido, fallo claro. Driver aislado, kernel pequeño.

---

## Arquitectura en 30 segundos

```
Boot UEFI → Bootloader → Kernel (11 fases de boot) → NeoInit (PID 1) → NeoShell (Ring 3)
```

El kernel se organiza en 5 capas verticales:

1. **Arch (x86_64)** — GDT, IDT, paginación 4 niveles, SMP trampoline
2. **HAL v0.4** — raw/safe split, 26 primitivas extern "C", asm confinado
3. **System Services** — scheduler (4 prioridades, aging, work stealing), memory (buddy+slab, demand paging), KOBJ, VFS, IPC/pipes, IRP async I/O, Event Bus, seguridad NT6
4. **NEM Driver Runtime** — pipeline de certificación (8 estados), capacidades (12 flags), aislamiento X4 (16 slots × 1 MB), ABI versionado
5. **Syscall SSDT** — 36 syscalls, tabla de 256 slots, O(1) dispatch, tabla de permisos separada

---

## Estado Actual

| Aspecto | Estado |
|---------|--------|
| **Kernel** | v0.39.11 — 469 tests, 36 syscalls, 11 fases de boot |
| **Drivers NEM** | 7 drivers standalone (PS/2, serial, RTC, ACPI, PCI, ATA, AHCI) |
| **User-mode** | NeoShell Ring 3, 23 binarios .NXE, 3 DLLs .NXL |
| **SMP** | 16 CPUs, per-CPU KPRCB, IPI (reschedule, TLB shootdown, call-function) |
| **Seguridad** | NT6 SRM: SID, Token, ACL, ACE, SeAccessCheck |
| **Rendimiento** | HPET → APIC timer 1 KHz, slab con per-CPU hot cache, work stealing |

---

## Quick Start

```bash
bash scripts/build.sh                    # bootloader + kernel + GPT disk image
bash scripts/build.sh --neodos-image     # + NeoDOS FS image + user binaries
bash scripts/qemu-debug.sh               # QEMU + OVMF, serial a stdout, GDB :1234
gdb -x .gdbinit                          # desde neodos/, conecta a QEMU
python3 scripts/auto_test.py             # Test runner automático headless
```

---

## Documentación Clave

| Documento | Descripción |
|-----------|-------------|
| [Visión Arquitectónica](docs/ARCHITECTURAL_VISION.md) | **NUEVO** — Plan director, diagnóstico, roadmap v0.40→v1.0 |
| [Arquitectura](docs/ARCHITECTURE.md) | Arquitectura actual del sistema |
| [Source of Truth](docs/ARCHITECTURE_SOURCE_OF_TRUTH.md) | Invariantes y contratos arquitectónicos |
| [Kernel](docs/KERNEL.md) | Especificación del kernel |
| [Syscalls](docs/SYSCALLS.md) | Referencia completa de syscalls |
| [Roadmap](docs/IMPROVEMENTS.md) | Items pendientes y completados |
| [Debug](docs/DEBUG.md) | Guía de depuración con GDB |

---

## Licencia

NeoDOS es software experimental. Úselo bajo su propio riesgo.
