# NeoDOS Documentation

> **Version:** v0.50.2 | **Tests:** 665 (kernel) | **ABI:** v8

## Architecture

| Document | Description |
|----------|-------------|
| [Overview](architecture/overview.md) | System architecture, boot flow, GPT layout, subsystem map |
| [Source of Truth](architecture/source-of-truth.md) | Enforceable invariants, MUST/MUST NOT rules |
| [Vision](architecture/vision.md) | Long-term strategy v0.40 → v1.0 |
| [Repository Architecture](architecture/repository.md) | Multi-repo proposal, dependency analysis |

## Boot

| Document | Description |
|----------|-------------|
| [Boot Flow](boot/boot-flow.md) | Bootloader, kernel boot phases, GPT layout |

## Kernel

| Document | Description |
|----------|-------------|
| [Syscalls](kernel/syscalls.md) | Full SSDT table, calling convention, migration status |
| [Object Manager](kernel/objects.md) | Ob types, namespace, operations, handles |
| [Object Manager Design](kernel/obj-arch.md) | Historical design document, migration plan |
| [IPC](kernel/ipc.md) | Pipes, IRP, work queue, event bus |
| [Interrupts](kernel/interrupts.md) | IRQL, IOAPIC, MSI-X, DPC, IPI |
| [HAL](kernel/hal.md) | Hardware abstraction layer, ABI, GDT/IDT |
| [Logging](kernel/logging.md) | Kernel logging infrastructure |

## Scheduler

| Document | Description |
|----------|-------------|
| [Scheduler](scheduler/scheduler.md) | Priorities, aging, SMP, work stealing |

## Memory

| Document | Description |
|----------|-------------|
| [Memory](memory/memory.md) | Buddy allocator, slab, demand paging, mmap |

## Drivers

| Document | Description |
|----------|-------------|
| [Overview](drivers/overview.md) | NEM format, lifecycle, caps, isolation |
| [NEM Spec](drivers/nem-spec.md) | NEM driver format specification |
| [Driver Migration](drivers/driver-migration.md) | Driver migration guide |
| [KCR Compliance](drivers/kcr-compliance.md) | Kernel Certification Requirements |

## Filesystem

| Document | Description |
|----------|-------------|
| [Overview](filesystem/overview.md) | NeoFS, VFS, IoStack, FAT32, page cache |
| [NeoFS v2](filesystem/neofs-v2.md) | NE2 filesystem design, indirect blocks, journaling |
| [VFS Usage Patterns](filesystem/vfs-patterns.md) | VFS patterns and conventions |

## Networking

| Document | Description |
|----------|-------------|
| [Stack](networking/stack.md) | TCP/IP stack, sockets, DHCP, e1000 |
| [Userland](networking/userland.md) | Network userland architecture |

## Security

| Document | Description |
|----------|-------------|
| [Security](security/security.md) | SID, Token, ACL, SAM, SeAccessCheck |

## Registry

| Document | Description |
|----------|-------------|
| [Registry](registry/registry.md) | Cm syscalls, cell-based hive, paths |

## Services

| Document | Description |
|----------|-------------|
| [Power Manager](services/power-manager.md) | Power plans, ACPI, shutdown coordination |

## Userland

| Document | Description |
|----------|-------------|
| [Shell](userland/shell.md) | Commands, pipeline, TAB, user binaries |
| [libneodos](userland/libneodos.md) | User-mode library API |
| [NXE Format](userland/nxe-format.md) | ELF note metadata, TLV tags |
| [NXP Format](userland/nxp-format.md) | Package container format, manifest |
| [NXE Ecosystem](userland/nxe-ecosystem.md) | NXE/NXP format, resources, i18n, tools |
| [NLT/i18n](userland/nlt.md) | NLTv2 format, API, compiler, workflow |
| [Packages](userland/packages.md) | Package system overview |

## Development

| Document | Description |
|----------|-------------|
| [Debugging](development/debugging.md) | GDB setup, debug tips |
| [QEMU Setup](development/qemu.md) | QEMU + OVMF setup guide |
| [VirtualBox](development/virtualbox.md) | VirtualBox setup guide |
| [Configuration](development/configuration.md) | Build configuration options |
| [Testing](development/testing.md) | Test suites, how to add tests |

## Design Proposals

| Document | Description |
|----------|-------------|
| [Font Manager](design/font-manager-design.md) | Font manager design proposal |
| [i18n](design/i18n-design.md) | Internationalization design |
| [NeoCfg](design/neocfg-design.md) | Configuration system design |
| [NeoKBD](design/neokbd-design.md) | Keyboard system design |
| [Registry Improvements](design/registry-improvements.md) | Registry improvements proposal |
| [Shell Improvements](design/shell-improvements.md) | Shell improvements proposal |
| [Users & Security](design/users-security-design.md) | Users and security design |

## Reference

| Document | Description |
|----------|-------------|
| [History](reference/history.md) | Project history |
| [Audit Report](reference/audit-report.md) | Previous architecture audit |
| [Package Manager Architecture](reference/package-manager-arch.md) | Package manager design |

## Project Roadmap

| Document | Description |
|----------|-------------|
| [ROADMAP.md](../ROADMAP.md) | Master roadmap: phases, milestones, priorities |
| [roadmap/improvements.md](../roadmap/improvements.md) | Local task list (synced with GitHub Issues) |
| [CHANGELOG.md](../CHANGELOG.md) | Release changelog |

## Related Projects

| Project | Repository |
|---------|-----------|
| NeoDev | [github.com/NeoDOS-Project/NeoDev](https://github.com/NeoDOS-Project/NeoDev) |
| NeoDOS Dev Server | [github.com/NeoDOS-Project/neodos-dev-server](https://github.com/NeoDOS-Project/neodos-dev-server) |
| NeoTools | [github.com/NeoDOS-Project/NeoTools](https://github.com/NeoDOS-Project/NeoTools) |
