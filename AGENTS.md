# NeoDOS — AI Agent Context

**Version:** v0.51.5 | **Tests:** 890 (kernel) | **ABI:** v8 | **SSDT:** RAX 0-99 (37 assigned) | **Dev Server:** [neodos-dev-server](https://github.com/NeoDOS-Project/neodos-dev-server) | **NeoTools:** [NeoTools](https://github.com/NeoDOS-Project/NeoTools)

## Permanent Rules (MUST always follow)

1. **No automatic builds by the agent.** Only build/test when explicitly asked. CI (`.github/workflows/ci.yml`) may compile the kernel and run static checks, but never publishes artifacts, builds images or boots VMs.
2. **Test before commit:** `cargo build` in `neodos-kernel/` → `neodev test` → `neodev check-deps` → `scripts/check-skills.sh` → `npx markdownlint '**/*.md' --config .markdownlint.json`.
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
13. **Issue acceptance criteria are the definition of done.** If an Issue has checkboxes (acceptance criteria), the work MUST satisfy **all** of them before it is considered complete — never close or claim an Issue done with unchecked criteria. When finished, **mark every satisfied checkbox in the Issue body** (e.g. `gh issue edit`, or edit the PR body if the checkboxes live there). If any criterion cannot be met, leave the Issue open (do not use `Closes #N`) and report the unmet item explicitly.

## Quick Reference

NeoDev is now an [independent project](https://github.com/NeoDOS-Project/NeoDev).
Install it first, then use:

```bash
scripts/sync-roadmap.sh sync     # sync roadmap → GitHub Issues (idempotent)
scripts/sync-roadmap.sh check    # verify GitHub connection and local files
scripts/check-skills.sh          # verify skills/*/SKILL.md vs current docs/code
neodev build --quick --image     # build kernel + bl + image 
neodev build --image             # build everything + image (preferred)
neodev run                       # QEMU + OVMF + GDB :1234
neodev test                      # run automated tests
scripts/build-test.sh            # neodev build --quick --image + neodev test
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

### Sincronización (idempotente)

```bash
scripts/sync-roadmap.sh sync          # labels + milestones + issues
scripts/sync-roadmap.sh labels        # solo labels
scripts/sync-roadmap.sh milestones    # solo milestones
scripts/sync-roadmap.sh issues        # solo issues
scripts/sync-roadmap.sh changelog     # changelog desde milestones/issues
scripts/sync-roadmap.sh check         # conexión + archivos locales
```

Ejecutable múltiples veces sin crear duplicados.

### Ramas

1. `develop` — integración (default).
2. `feat/*`, `fix/*`, `refactor/*`, `investigation/*`, `audit/*`, `docs/*` — ramas
   de trabajo referenciando Issues.
3. `release/vX.Y.Z` — release desde `develop` → PR → `master`.
4. `master` — releases estables.

### Branching Rule

**Every new feature, bug fix, investigation, refactor, audit, or architectural
change MUST be developed on its own dedicated Git branch.** Do not work directly
on `develop`; do not reuse an unrelated branch. The branch is the task-isolation
mechanism and must stay focused on a single piece of work.

1. Ensure the working tree is clean and synced from the appropriate base branch.
2. Create a dedicated branch (`feat/`, `fix/`, `investigation/`, `refactor/`,
   `audit/`, `docs/<short-description>`); when the work corresponds to a GitHub
   Issue, reference it, e.g. `fix/service-graceful-shutdown-358`.
3. Perform all changes and commits exclusively on that branch, then validate.
4. Merge back through the normal Git workflow only after validation.

**Exception:** purely administrative/read-only operations that do not modify
project files or Git history (inspecting the repository, searching code, reading
issues, running tests, `git status`, reviewing logs, analysing architecture)
may be performed without a branch. When in doubt, **branch first**. If the
current branch already contains unrelated work, stop and report it before
making changes.

### Flujo de desarrollo

1. Nueva idea → `roadmap/improvements.md` → `sync-roadmap.sh sync` crea la Issue.
2. Commits: `git commit -m "feat: descripción (#123)"` referenciando la Issue.
3. Verificación antes de commit (todo debe pasar):

   ```bash
   scripts/build-test.sh          # neodev build --quick --image + neodev test
   scripts/check-skills.sh
   npx markdownlint '**/*.md' --config .markdownlint.json
   scripts/sync-roadmap.sh check
   ```

   `neodev test` carga el `kernel.elf` raíz, así que construye siempre la imagen
   antes de testear. Si todo pasa:
   `git add -A && git commit -m "feat|fix|refactor: descripción (#123)" && git push`.
4. PR → `develop`, aprobación y merge (squash). El PR cierra la Issue **solo** si
   se cumplen todos sus criterios de aceptación (regla 13); al terminar, marcar
   los checkboxes, actualizar `CHANGELOG.md` y los `docs/*.md` relevantes, y
   `sync-roadmap.sh sync`.

### Releases

Cada versión es una Milestone; cuando todas sus Issues están cerradas, la versión
está terminada. El changelog se genera con `sync-roadmap.sh changelog`.

## Mandatory Scheduler Change Validation Policy

Any change affecting the scheduler, context switching, thread lifecycle,
wait/wakeup paths, timer handling, CPU-local state or scheduler synchronization
MUST follow the validation policy in
[`skills/scheduler/SKILL.md`](skills/scheduler/SKILL.md): regression coverage for
the changed invariant, **1 / 2 / 4-vCPU** runs, progress and starvation testing,
stress runs proportional to risk, build + full-diff checks, and a PR that
documents the invariant, workloads and any unvalidated items.

**Principle: scheduler correctness requires demonstrated progress, not just
successful execution of tests.**

## Proactive Capability Discovery

While working on **any** task, actively watch the whole system for capabilities
that are missing, partial, limited or insufficient — missing kernel/userland
APIs, incomplete infrastructure, architectural limitations, pending
integrations, missing tools, significant technical debt — even when the gap is
unrelated to the current task. **Never implement a discovery on your own.**

```text
Discover → Verify → Search existing Issue → Create if necessary → Document → Continue current task
```

- **Verify** against the repository (code, syscalls, docs, tests, tools,
  configuration) — never stale docs; then run
  `gh issue list --search "<terms>"` before creating anything. If an equivalent
  Issue exists, reference it instead of duplicating it.
- **Blocking gap** → create/reference the Issue and document the blocker; never
  build a fake, temporary or architecturally wrong workaround.
- **Non-blocking gap** → create/reference the Issue, document it and continue;
  never widen the scope of the current task.
- **End-of-task report** must be split into: `## Implemented`,
  `## Discovered Capabilities`, `## Existing Issues`, `## New Issues`,
  `## Blockers`, `## Follow-up Work`.

Full specification (Issue template, examples, architectural rationale):
[`docs/development/capability-discovery.md`](docs/development/capability-discovery.md).

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
