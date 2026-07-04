# NeoDOS — AI Agent Context

**Version:** v0.48.7 | **Tests:** 646 | **ABI:** v7 | **Ob API:** RAX 60-76

## Permanent Rules (MUST always follow)

1. **No automatic builds.** Only build/test when explicitly asked.
2. **Test before commit:** `cargo build` in `neodos-kernel/` → `python3 scripts/auto_test.py` → `scripts/check_deps.py`.
3. **Never modify public API without updating docs.** Syscalls, ObInfoClass, NEM ABI, structs in `libneodos/`.
4. **NT-like design philosophy:** Object Manager (`Ob`) is the central abstraction for syscalls, handles, security, and namespace.
5. **No new Ring 0 shell commands.** All interactive commands go to `userbin/` as `.NXE` Ring 3 binaries.
6. **New syscalls (RAX ≥ 77) MUST be `sys_ob_*`** — operate on Ob objects, receive/return Ob handles.
7. **Code is truth.** Documentation explains design, it does not replicate code. Update docs when architecture changes.
8. **Before architecture decisions:** read `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md` — invariants are enforceable rules.
9. **Keep AGENTS.md minimal.** Move specialized instructions to `docs/` and procedural checklists to `skills/`.
10. **Naming:** kebab-case for files/dirs, PascalCase for types/enums/traits, snake_case for fns/vars.

## Quick Reference

```bash
bash scripts/build.sh                          # bootloader + kernel + GPT
bash scripts/build.sh --neodos-image           # + user binaries
bash scripts/qemu-debug.sh                     # QEMU + OVMF + GDB :1234
python3 scripts/auto_test.py                   # 620 kernel tests
scripts/check_deps.py                          # subsystem dependency rules
QEMU_ACCEL=kvm bash scripts/qemu-debug.sh      # KVM mode
```

## Git Workflow

1. `cargo build` in `neodos-kernel/`
2. `python3 scripts/auto_test.py`
3. If all pass: `git add -A && git commit -m "feat|fix|refactor: ..." && git push`
4. On completion: update `CHANGELOG.md`, move item in `docs/IMPROVEMENTS.md` → completed, update relevant `docs/*.md`.

## Architecture

For every subsystem, consult its doc — not this file:

| Subsystem | Doc | Contents |
|-----------|-----|----------|
| Architecture | `docs/ARCHITECTURE.md` | Boot flow, GPT layout, subsystem map |
| Source of Truth | `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md` | Enforceable invariants, rules |
| Syscalls | `docs/syscalls.md` | Full table, calling convention, migration status |
| Scheduler | `docs/scheduler.md` | Priorities, aging, SMP, work stealing |
| Memory | `docs/memory.md` | Buddy allocator, slab, demand paging, mmap |
| Drivers | `docs/drivers.md` | NEM format, lifecycle, caps, isolation, ABI |
| Filesystem | `docs/filesystem.md` | NeoFS, VFS, IoStack, FAT32, page cache |
| Registry | `docs/registry.md` | Cm syscalls, cell-based hive, paths |
| Security | `docs/security.md` | SID, Token, ACL, SAM, SeAccessCheck |
| Shell | `docs/shell.md` | Commands, pipeline, TAB, user binaries |
| IPC | `docs/ipc.md` | Pipes, IRP, work queue, event bus |
| Network | `docs/network.md` | TCP/IP stack, sockets, DHCP, e1000 |
| Testing | `docs/testing.md` | Test suites, how to add tests |
| libneodos | `docs/libneodos.md` | User-mode library API, modules |
| Boot | `docs/boot.md` | Bootloader, kernel boot phases |
| Object Manager | `docs/objects.md` | Ob types, namespace, ObOperation trait |
| HAL | `docs/hal.md` | Hardware abstraction layer, primitives |
| Interrupts | `docs/interrupts.md` | IRQL, IOAPIC, MSI-X, DPC, IPI |
| Roadmap | `docs/IMPROVEMENTS.md` | Pending items by priority |
| Completed | `docs/IMPROVEMENTS_COMPLETED.md` | Completed roadmap items |
| Debug | `docs/DEBUG.md` | GDB setup, debug tips |
| Vision | `docs/ARCHITECTURAL_VISION.md` | Long-term strategy v0.40→v1.0 |

## Skills (specialized task checklists)

| Skill | When to use | File |
|-------|-------------|------|
| Build | Build/run/test cycle | `skills/build/SKILL.md` |
| Syscalls | Add/modify a syscall | `skills/syscalls/SKILL.md` |
| Object Manager | Extend Ob types/API | `skills/object-manager/SKILL.md` |
| Scheduler | Scheduler changes | `skills/scheduler/SKILL.md` |
| Memory | Memory subsystem changes | `skills/memory/SKILL.md` |
| Shell | Add shell command | `skills/shell/SKILL.md` |
| Drivers | Develop NEM driver | `skills/drivers/SKILL.md` |
| Filesystem | FS development | `skills/filesystem/SKILL.md` |
| Testing | Write/run tests | `skills/testing/SKILL.md` |
| Review | Code review checklist | `skills/review/SKILL.md` |
| Documentation | Update docs | `skills/documentation/SKILL.md` |
| Release | Release process | `skills/release/SKILL.md` |
