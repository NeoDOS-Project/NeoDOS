# NeoDOS — AI Agent Context

**Version:** v0.50.2 | **Tests:** 665 (kernel) | **ABI:** v8 | **SSDT:** RAX 0-59 (34 syscalls) | **Dev Server:** [neodos-dev-server](https://github.com/NeoDOS-Project/neodos-dev-server) | **NeoTools:** [NeoTools](https://github.com/NeoDOS-Project/NeoTools)

## Permanent Rules (MUST always follow)

1. **No automatic builds.** Only build/test when explicitly asked.
2. **Test before commit:** `cargo build` in `neodos-kernel/` → `neodev test` → `scripts/check_deps.py` → `npx markdownlint '**/*.md' --config .markdownlint.json`.
3. **Never modify public API without updating docs.** Syscalls, ObInfoClass, NEM ABI, structs in `libneodos/`.
4. **NT-like design philosophy:** Object Manager (`Ob`) is the central abstraction for syscalls, handles, security, and namespace.
5. **No new Ring 0 shell commands.** All interactive commands go to `userbin/` as `.NXE` Ring 3 binaries.
6. **New syscalls (RAX ≥ 60) MUST be `sys_ob_*`** — operate on Ob objects, receive/return Ob handles.
7. **Code is truth.** Documentation explains design, it does not replicate code. Update docs when architecture changes.
8. **Before architecture decisions:** read `docs/architecture/source-of-truth.md` — invariants are enforceable rules.
9. **Keep AGENTS.md minimal.** Move specialized instructions to `docs/` and procedural checklists to `skills/`.
10. **Naming:** kebab-case for files/dirs, PascalCase for types/enums/traits, snake_case for fns/vars.

## Quick Reference

NeoDev is now an [independent project](https://github.com/NeoDOS-Project/NeoDev).
Install it first, then use:

```bash
scripts/sync-roadmap.sh sync     # sync roadmap → GitHub Issues (idempotent)
scripts/sync-roadmap.sh check    # verify GitHub connection and local files
neodev build --quick --image     # build kernel + bl + image 
neodev build --image             # build everything + image (preferred)
neodev run                       # QEMU + OVMF + GDB :1234
neodev test                      # run automated tests
neodev list                      # show discovered projects
neodev clean                     # clean artifacts
```

Install: `cargo install --git https://github.com/NeoDOS-Project/NeoDev.git`

## GitHub Workflow (SSOT)

**GitHub Issues es el sistema oficial de planificación, seguimiento e histórico.**

El archivo local `roadmap/improvements.md` es una lista de ideas que la IA convierte
automáticamente en Issues. GitHub Issues es la fuente de verdad de planificación.
(Los antiguos `docs/IMPROVEMENTS.md` y `docs/IMPROVEMENTS_COMPLETED.md` fueron eliminados
en la reestructuración de docs; consulta `docs/README.md` para la documentación actual.)

### Sincronización

```bash
scripts/sync-roadmap.sh sync          # Sincroniza todo: labels + milestones + issues
scripts/sync-roadmap.sh labels        # Solo labels
scripts/sync-roadmap.sh milestones    # Solo milestones
scripts/sync-roadmap.sh issues        # Solo issues
scripts/sync-roadmap.sh changelog     # Genera changelog desde milestones/issues
scripts/sync-roadmap.sh check         # Verifica conexión y archivos
```

Completamente idempotente. Ejecutable múltiples veces sin crear duplicados.

### Flujo de desarrollo

1. Nueva idea → añadir a `roadmap/improvements.md` → `sync-roadmap.sh sync`.
2. La IA o el comando crean la Issue en GitHub.
3. Trabajar en feature branch: `feat/NOMBRE-DE-LA-ISSUE`.
4. Commits: `git commit -m "feat: descripción (#123)"` con referencia a la Issue.
5. PR → `develop` → merge. El PR cierra la Issue automáticamente.
6. Al completar: `sync-roadmap.sh sync` actualiza `improvements.md`.

### Releases

Cada versión es una Milestone. Cuando todas sus Issues están cerradas, la versión
está terminada. El changelog se genera con `sync-roadmap.sh changelog`.

### Ramas

1. `develop` — integración (default).
2. `feat/*`, `fix/*`, `refactor/*` — ramas de trabajo referenciando Issues.
3. `release/vX.Y.Z` — rama de release desde `develop` → PR → `master`.
4. `master` — releases estables.

### Git Workflow (commits)

1. `cargo build` in `neodos-kernel/` (or `neodev build --quick`)
2. `neodev test`
3. `npx markdownlint '**/*.md' --config .markdownlint.json`
4. `scripts/sync-roadmap.sh check`
5. If all pass: `git add -A && git commit -m "feat|fix|refactor: descripción (#123)" && git push`
6. Open PR → `develop`, get approval, merge (squash).
7. On completion: update `CHANGELOG.md`, run `sync-roadmap.sh sync`, update relevant `docs/*.md`.

## Architecture

For every subsystem, consult its doc — not this file:

| Subsystem | Doc | Contents |
| ----------- | ----- | ---------- |
| NeoDev | `https://github.com/NeoDOS-Project/NeoDev` | Development tool: build, image, run, test |
| NeoDOS Dev Server | `https://github.com/NeoDOS-Project/neodos-dev-server` | LSP server + MCP server + shared toolkit |
| NeoTools | `https://github.com/NeoDOS-Project/NeoTools` | Host tools: nxeinfo, nxpkg, nxdump |
| Architecture | `docs/architecture/overview.md` | Boot flow, GPT layout, subsystem map |
| Source of Truth | `docs/architecture/source-of-truth.md` | Enforceable invariants, rules |
| Vision | `docs/architecture/vision.md` | Long-term strategy v0.40→v1.0 |
| Repository Architecture | `docs/architecture/repository.md` | Multi-repo proposal, dependency analysis |
| Syscalls | `docs/kernel/syscalls.md` | Full table, calling convention, migration status |
| Object Manager | `docs/kernel/objects.md` | Ob types, namespace, operations, handles |
| Object Manager Design | `docs/kernel/obj-arch.md` | Historical design doc, migration plan |
| IPC | `docs/kernel/ipc.md` | Pipes, IRP, work queue, event bus |
| Interrupts | `docs/kernel/interrupts.md` | IRQL, IOAPIC, MSI-X, DPC, IPI |
| HAL | `docs/kernel/hal.md` | HAL architecture, ABI, GDT/IDT |
| Logging | `docs/kernel/logging.md` | Kernel logging infrastructure |
| Scheduler | `docs/scheduler/scheduler.md` | Priorities, aging, SMP, work stealing |
| Memory | `docs/memory/memory.md` | Buddy allocator, slab, demand paging, mmap |
| Drivers | `docs/drivers/overview.md` | NEM format, lifecycle, caps, isolation, ABI |
| NEM Spec | `docs/drivers/nem-spec.md` | NEM driver format specification |
| Driver Migration | `docs/drivers/driver-migration.md` | Driver migration guide |
| KCR Compliance | `docs/drivers/kcr-compliance.md` | Kernel Certification Requirements |
| Filesystem | `docs/filesystem/overview.md` | NeoFS, VFS, IoStack, FAT32, page cache |
| NeoFS v2 | `docs/filesystem/neofs-v2.md` | NE2 design, indirect blocks, journaling |
| VFS Patterns | `docs/filesystem/vfs-patterns.md` | VFS usage patterns and conventions |
| Network | `docs/networking/stack.md` | TCP/IP stack, sockets, DHCP, e1000 |
| Network Userland | `docs/networking/userland.md` | Network userland architecture |
| Security | `docs/security/security.md` | SID, Token, ACL, SAM, SeAccessCheck |
| Registry | `docs/registry/registry.md` | Cm syscalls, cell-based hive, paths |
| Power Manager | `docs/services/power-manager.md` | Power plans, ACPI, shutdown coordination |
| Shell | `docs/userland/shell.md` | Commands, pipeline, TAB, user binaries |
| libneodos | `docs/userland/libneodos.md` | User-mode library API, modules |
| NXE Ecosystem | `docs/userland/nxe-ecosystem.md` | NXE/NXP format, resources, i18n, tools |
| NXE Format | `docs/userland/nxe-format.md` | ELF note metadata, TLV tags |
| NXP Format | `docs/userland/nxp-format.md` | Package container format, manifest |
| NLT/i18n | `docs/userland/nlt.md` | NLTv2 format, API, compiler, workflow |
| Packages | `docs/userland/packages.md` | Package system overview |
| Boot | `docs/boot/boot-flow.md` | Bootloader, kernel boot phases |
| Debug | `docs/development/debugging.md` | GDB setup, debug tips |
| QEMU Setup | `docs/development/qemu.md` | QEMU + OVMF setup |
| VirtualBox | `docs/development/virtualbox.md` | VirtualBox setup guide |
| Configuration | `docs/development/configuration.md` | Build configuration options |
| Testing | `docs/development/testing.md` | Test suites, how to add tests |
| History | `docs/reference/history.md` | Project history |
| Audit Report | `docs/reference/audit-report.md` | Previous architecture audit |
| Package Manager Arch | `docs/reference/package-manager-arch.md` | Package manager design |
| Roadmap | `ROADMAP.md` | Master roadmap: phases, milestones, priorities (project root) |
| GitHub Sync | `scripts/sync-roadmap.sh` | Sync roadmap local ↔ GitHub Issues (idempotent) |
| Roadmap Data | `roadmap/` | Labels, milestones, improvements.md, issue templates |
| Docs Index | `docs/README.md` | Master documentation index |

## Skills (specialized task checklists)

| Skill | When to use | File |
| ------- | ------------- | ------ |
| Build | Build/run/test cycle | `skills/build/SKILL.md` |
| Syscalls | Add/modify a syscall | `skills/syscalls/SKILL.md` |
| Object Manager | Extend Ob types/API | `skills/object-manager/SKILL.md` |
| Scheduler | Scheduler changes | `skills/scheduler/SKILL.md` |
| Memory | Memory subsystem changes | `skills/memory/SKILL.md` |
| Shell | Add shell command | `skills/shell/SKILL.md` |
| Registry | Cm hive, keys, values, persistence | `skills/registry/SKILL.md` |
| Drivers | Develop NEM driver | `skills/drivers/SKILL.md` |
| Filesystem | FS development | `skills/filesystem/SKILL.md` |
| Testing | Write/run tests | `skills/testing/SKILL.md` |
| Review | Code review checklist | `skills/review/SKILL.md` |
| Documentation | Update docs | `skills/documentation/SKILL.md` |
| Release | Release process | `skills/release/SKILL.md` |
| Boot | Bootloader, boot phases, BootInfo ABI | `skills/boot/SKILL.md` |
| IPC | Pipes, handle table, IRP, work queue, event bus | `skills/ipc/SKILL.md` |
| NeoDev | NeoDev development tool | `skills/neodev/SKILL.md` |
| Network | TCP/IP stack, sockets, ARP, DNS, e1000 | `skills/network/SKILL.md` |
| Security | SID, Token, ACL, SAM, SeAccessCheck | `skills/security/SKILL.md` |
