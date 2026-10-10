# NeoDOS — AI Agent Context

**Version:** v0.51.5 | **Tests:** 888 (kernel) | **ABI:** v8 | **SSDT:** RAX 0-99 (37 assigned) | **Dev Server:** [neodos-dev-server](https://github.com/NeoDOS-Project/neodos-dev-server) | **NeoTools:** [NeoTools](https://github.com/NeoDOS-Project/NeoTools)

## Permanent Rules (MUST always follow)

1. **No automatic builds.** Only build/test when explicitly asked.
2. **Test before commit:** `cargo build` in `neodos-kernel/` → `neodev test` → `neodev check-deps` → `npx markdownlint '**/*.md' --config .markdownlint.json`.
3. **Never modify public API without updating docs.** Syscalls, ObInfoClass, NEM ABI, structs in `libneodos/`.
4. **NT-like design philosophy:** Object Manager (`Ob`) is the central abstraction for syscalls, handles, security, and namespace.
5. **No new Ring 0 shell commands.** All interactive commands go to `userbin/` as `.NXE` Ring 3 binaries.
6. **Every new syscall MUST be `sys_ob_*`** — operate on Ob objects, receive/return Ob handles (see `docs/kernel/syscalls.md`).
7. **Code is truth.** Documentation explains design, it does not replicate code. Update docs when architecture changes.
8. **Before architecture decisions:** read `docs/architecture/source-of-truth.md` — invariants are enforceable rules.
9. **Keep AGENTS.md minimal.** Move specialized instructions to `docs/` and procedural checklists to `skills/`.
10. **Naming:** kebab-case for files/dirs, PascalCase for types/enums/traits, snake_case for fns/vars.
11. **Branch first.** Every feature, bug fix, investigation, refactor, audit, or architectural change MUST be developed on its own dedicated branch. Never work directly on `develop`. See [Branching Rule](#branching-rule).
12. **Module layout.** One directory = one subsystem, with dependencies flowing only downward (enforced by `neodev check-deps`). Never reuse a module name across layers. Use `mod.rs` for directory modules. During incremental moves, keep historical `crate::<name>` paths working via re-exports in the parent, and refresh `check-deps-baseline.txt` for the moved paths.

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

### Branching Rule

**Every new feature, bug fix, investigation, refactor, audit, or architectural
change MUST be developed on its own dedicated Git branch.** Do not work directly
on `develop` for new work. Do not reuse an unrelated existing branch for a new
task. The branch is the task-isolation mechanism and must stay focused on a
single piece of work.

Required workflow:

1. Ensure the working tree is in a known (clean) state.
2. Update/sync from the appropriate base branch when necessary.
3. Create a dedicated branch for the work.
4. Perform all changes and commits exclusively on that branch.
5. Validate the work.
6. Merge the branch back through the normal Git workflow only after validation.

Branch naming follows project conventions:

```text
feat/<short-description>
fix/<short-description>
investigation/<short-description>
refactor/<short-description>
audit/<short-description>
docs/<short-description>
```

When the work corresponds to a GitHub Issue, the branch should reference that
Issue when practical, e.g. `fix/service-graceful-shutdown-358`.

**Exception:** purely administrative/read-only operations that do not modify
project files or Git history (inspecting the repository, searching code, reading
issues, running tests, checking `git status`, reviewing logs, analysing
architecture without modifying files) may be performed without a branch. When in
doubt, **branch first**.

If the current branch already contains unrelated work, stop and report it before
making changes.

### Git Workflow (commits)

1. `cargo build` in `neodos-kernel/` (or `neodev build --quick`)
2. `neodev test`
3. `npx markdownlint '**/*.md' --config .markdownlint.json`
4. `scripts/sync-roadmap.sh check`
5. If all pass: `git add -A && git commit -m "feat|fix|refactor: descripción (#123)" && git push`
6. Open PR → `develop`, get approval, merge (squash).
7. On completion: update `CHANGELOG.md`, run `sync-roadmap.sh sync`, update relevant `docs/*.md`.

## Proactive Capability Discovery

While working on **any** NeoDOS task, the agent must actively observe the whole
system and detect relevant capabilities that are missing, partial, limited, or
insufficient — even when the discovery is unrelated to the current task or
belongs to another subsystem.

Scope of what to watch for: missing APIs, missing kernel capabilities, missing
userland capabilities, incomplete infrastructure, architectural limitations,
pending integrations, tools that should exist, significant technical debt, and
dependencies that do not exist yet.

The agent **never** turns a discovery into an implementation on its own. The
mandatory workflow is:

```text
Discover → Verify → Search existing Issue → Create Issue if necessary → Document → Continue current task
```

### 1. Discover

When something looks like it is missing, briefly decide whether it is a real
gap (e.g. "no network-interface enumeration API", "no Ctrl+C handling in the
shell", "no persistent configuration storage", "no per-process memory query").
Do not open an Issue for every trivial helper — the capability needs enough
substance to stand as its own task.

### 2. Verify

Before opening an Issue, confirm the capability truly does not exist, using the
repository as the source of truth: code, APIs, syscalls, docs, tests, existing
tools, configuration, infrastructure. Do not assume absence just because it is
not obvious, and do not invent APIs from stale documentation.

### 3. Search existing Issues

```bash
gh issue list --search "<relevant terms>"
```

Try several term combinations. If an equivalent Issue exists, do **not**
duplicate it: reference it and link it to the current task when relevant.

### 4. Create Issue

If the capability is genuinely absent and no equivalent Issue exists, create a
GitHub Issue useful enough to become a standalone task. Include, when
applicable:

```text
## Context
## Missing capability
## Evidence
## Why it matters
## Current implementation
## Possible approaches
## Scope
## Related work
```

Title style: clear, project conventions, e.g.
`[NETWORK] Add network interface enumeration API`,
`[SHELL] Add Ctrl+C / interrupt handling`,
`[MEMORY] Expose process memory information`.

Do not implement the capability as part of the current task unless the user
explicitly asked for it.

### 5. Blocking vs Non-blocking

Distinguish the two cases explicitly.

- **Blocking capability** — the gap prevents completing the current task
  correctly: `Detect → Verify → Search Issue → Create/reference Issue → Document
  blocker`. Never build a fake, temporary, or architecturally wrong
  implementation just to sidestep the block.
- **Non-blocking capability** — relevant but not blocking: `Detect → Verify →
  Search Issue → Create/reference Issue → Document → Continue current task`.
  A non-blocking gap must **never** cause the agent to widen the scope of the
  current task.

### 6. Never implement discoveries automatically

Discovering a missing capability is **not** authorization to build it. Example:
while implementing DNS, the agent notices NeoShell has no Ctrl+C — it must
verify, search for an Issue, create `[SHELL] Add Ctrl+C / interrupt handling`
if absent, document it, and **continue with DNS**.

### 7. End-of-task report

Every agent must separate its final report into:

```text
## Implemented
## Discovered Capabilities
## Existing Issues
## New Issues
## Blockers
## Follow-up Work
```

This distinguishes what was implemented, what was discovered, what was already
tracked, what just entered the backlog, and what blocked progress.

### Architectural principle

GitHub Issues is also the mechanism for **progressive architecture discovery**:
the agent helps grow the backlog while working, without turning every task into
an excuse to expand scope.

```text
Current Task
      │
      ├── Required capability  → implement if authorized
      │
      └── Discovered capability → Issue + continue
```

> **Discover → Verify → Issue → Document → Continue** — not *Discover → Implement everything*.

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
| NeoFS v2 | `docs/filesystem/neofs-v2.md` | NE2 design, indirect blocks, copy-on-write |
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
| Package Manager Arch | `docs/architecture/package-manager-arch.md` | Package manager design |
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

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

When the user types `/graphify`, use the installed graphify skill or instructions before doing anything else.

Rules:

- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- Dirty graphify-out/ files are expected after hooks or incremental updates; dirty graph files are not a reason to skip graphify. Only skip graphify if the task is about stale or incorrect graph output, or the user explicitly says not to use it.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
