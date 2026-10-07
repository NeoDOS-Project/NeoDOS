# NeoDOS Documentation

> **Version:** v0.51.4 | **Tests:** 825 (kernel) | **ABI:** v8

## Architecture

| Document | Description |
|----------|-------------|
| [Overview](architecture/overview.md) | System architecture, boot flow, GPT layout, subsystem map |
| [Source of Truth](architecture/source-of-truth.md) | Enforceable invariants, MUST/MUST NOT rules |
| [Vision](architecture/vision.md) | Long-term strategy v0.40 → v1.0 |
| [Repository Architecture](architecture/repository.md) | Multi-repo proposal, dependency analysis |
| [AHCI Stability](architecture/ahci-stability.md) | AHCI stability improvements, multi-sector read batching |
| [Disk Layout](architecture/disk-layout.md) | NT-style disk layout proposal (WIP, not implemented) |
| [Package Manager Architecture](architecture/package-manager-arch.md) | Package manager design |

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
| [NTP Daemon](services/ntpd.md) | ntpd architecture, config, lifecycle, limitations |

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
| [Contributing](development/contributing.md) | Contribution guide, workflow, conventions |
| [Triple Fault Audit](development/triple-fault-audit.md) | Diagnóstico técnico y auditoría del reinicio por Triple Fault |
| [GPF netd Audit — Fase 1](development/audit-gpf-netd-fase1-2026-08-30.md) | Auditoría forense Fase 1: GPF en `iretq` al seleccionar `netd` — verificada instrucción-a-instrucción |
| [GPF netd Audit — Completa](development/audit-gpf-netd-2026-08-30.md) | Auditoría completa GPF netd A-K + validación forense 2026-09-17 |
| [Graphify](development/graphify.md) | Herramienta auxiliar: grafo de conocimiento para agentes IA (workspace, crates, syscalls, drivers) |
| [Network Recovery — Forensic Report](development/net-recovery-2026-09-26.md) | e1000 NEM ABI / DMA ring alignment / DHCP service recovery (netd, SMP) |
| [Network Recovery — VirtualBox Validation](development/net-recovery-vbox-validation-2026-09-26.md) | Bridged e1000 validation of the network stack in VirtualBox |

## Investigation

| Document | Description |
|----------|-------------|
| [SMP Bring-Up](investigation/smp-bring-up-report.md) | SMP bring-up investigation |
| [Phase 13 — AP Scheduling Design](investigation/phase13-ap-scheduling-design.md) | Design for AP dispatch (real SMP scheduling) |
| [Phase 13 — AP timer `iretq` #GP Forensics](investigation/phase13-ap-timer-iretq-gpf-forensics.md) | Forensic investigation of the AP timer `iretq` #GP |
| [F-01/F-02 Adversarial Audit](investigation/f01-f02-adversarial-audit.md) | SMP current identity, zombie reap, stack UAF |
| [KBD Input Queue Investigation](investigation/kbd-input-queue-investigation.md) | Keyboard input queue investigation |
| [KBD Investigation Report](investigation/kbd-investigation-report.md) | Keyboard subsystem investigation report |
| [KBD Pipeline — Fase 0](investigation/kbd-pipeline-fase0.md) | Black-box + white-box keyboard pipeline |
| [KBD SMP Queue Validation](investigation/kbd-smp-queue-validation.md) | Keyboard SMP queue validation report |
| [#293 SMP4 shell GPF](investigation/smp4-shell-gpf-2026-09-27.md) | GPF in the shell `iretq` of `syscall_handler_asm` |
| [#331 SMP>1 exit TLB-shootdown self-deadlock](investigation/smp331-exit-tlb-shootdown-self-deadlock.md) | Recursive `SCHEDULER` lock in process-exit page free |
| [#338 Ring-0 `Ready` frame publication](investigation/smp338-ring0-ready-frame-2026-09-29.md) | netcfg stalls after one iteration |
| [#340 e1000 RX init / DHCP first-DISCOVER loss](investigation/netd-dhcp-first-discover-2026-09-27.md) | RX init order, `DD+len=0` descriptors |
| [#340 netd / `sys_yield` / scheduling](investigation/netd-sys-yield-340-2026-09-30.md) | netd scheduling investigation (2026-09-30) |
| [#343 Service Manager lock-order inversion](investigation/smp343-service-lock-order-deadlock.md) | `PAGE_CACHE` / `BLOCK_DEVICES` lock-order inversion |
| [#346 SMP>1 idle-thread frame corruption](investigation/smp346-idle-frame-corruption.md) | `SS=0x15` / stack canary |
| [#348 idle-stack canary unsound](investigation/smp348-idle-stack-canary.md) | `check_kernel_stack_canary` on the 4 KiB idle stack |
| [#474 SMP2 Ring-0 INVALID_OPCODE](investigation/issue-474-smp2-ring0-invalid-opcode.md) | `rip=0x148` wild `iretq` |
| [#476 VirtualBox SMP2 INVALID_OPCODE](investigation/issue-476-vbox-invalid-opcode.md) | `rip` in another thread's stack |
| [#476 IRETQ frame audit](investigation/issue-476-iretq-frame-audit.md) | `IRETQ_BAD_FRAME` |
| [#476 foreign `rsp` revert-desync fix](investigation/issue-476-kprcb-revert-desync-fix.md) | Foreign `rsp` stored into a saved context |
| [#476 close syscall KPRCB/RSP window](investigation/issue-476-close-window-fix.md) | Close the ownership window |
| [#476 allocator `FREE_BAD` audit](investigation/issue-476-allocator-free-bad.md) | Allocator/resource ownership audit |
| [#477 atomic user/heap slot claim](investigation/issue-477-slot-alloc-race-fix.md) | Atomic slot-claim fix |
| [#482 `wait_for_process` publish order](investigation/issue-482-wait-for-process-publish-order-fix.md) | KPRCB vs bootstrap stack |
| [#488 boot-thread IRETQ false positive](investigation/issue-488-boot-iretq-audit-fix.md) | `IRETQ_BAD_FRAME` exemption |
| [#491 ntpd clock-set denied](investigation/issue-491-ntpd-clock-set-denied.md) | Stale RTC NEM driver in the image; build-pipeline root cause |
| [#501 NeoInit SUSP](investigation/issue-501-neoinit-suspend.md) | Bootstrap hand-off marked the wrong thread; shell never started |
| [Revisión issues `type/bug` — 2026-10-01](investigation/issue-review-type-bug-2026-10-01.md) | Issue triage on `investigate/376-phase2` |
| [Scheduler state-invariant audit](investigation/scheduler-state-invariants-2026-09-29.md) | Scheduler invariants (2026-09-29) |
| [Code Health Audit — 2026-09](investigation/audit-code-health-2026-09.md) | Code health audit |

## Design Proposals

| Document | Description |
|----------|-------------|
| [Audio API](design/audio-api-design.md) | Audio device + Ring 3 PCM API (#469) |
| [Display / Framebuffer API](design/display-api-design.md) | Ring 3 display/framebuffer API design (#465) |
| [File Seek](design/file-seek-design.md) | File handle seek / read-at-offset (#468) |
| [FPU / SSE Context](design/fpu-context-design.md) | Save/restore FP/SIMD on context switch (#471) |
| [Large NXE Binaries](design/nxe-large-binary-design.md) | Binary size cap and user stack (#470) |
| [Monotonic Clock](design/monotonic-clock-design.md) | Monotonic uptime + sleep (#467) |
| [Raw Keyboard API](design/raw-keyboard-api-design.md) | Raw keyboard event API (#466) |
| [Font Manager](design/font-manager-design.md) | Font manager design proposal |
| [NeoCfg](design/neocfg-design.md) | Configuration system design |
| [NeoKBD](design/neokbd-design.md) | Keyboard system design |
| [Registry Improvements](design/registry-improvements.md) | Registry improvements proposal |
| [Service Manager](design/service-manager-design.md) | Service Manager (Sm) design proposal |
| [Shell Improvements](design/shell-improvements.md) | Shell improvements proposal |
| [Users & Security](design/users-security-design.md) | Users and security design |

## Reference

| Document | Description |
|----------|-------------|
| [History](reference/history.md) | Project history |
| [Audit Report](reference/audit-report.md) | Previous architecture audit |
| [Glossary](reference/glossary.md) | Terminology and acronyms |
| [Boot Audit — NeoShell](boot_audit_neoshell.md) | Boot/NeoShell audit |
| [Scheduler Audit](scheduler_audit.md) | Scheduler P0 audit — runqueue invariants |

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
