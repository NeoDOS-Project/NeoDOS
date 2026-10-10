---
name: review
description: Review pull requests or code changes for architecture, deps, tests, docs
---

# Review

## When to use

You are reviewing a pull request or code change, or need a systematic checklist
before committing.

## Goal

Catch architectural violations, missed invariants, missing docs, and breaking
changes before they land.

## Steps

1. **Verify AGENTS.md rules**
   - Rule 1: no automatic builds — no CI/staging build workflow added.
   - Rule 4: NT-like design — Ob is the central abstraction; verify handles, not
     raw pointers.
   - Rule 5: all new interactive commands are Ring 3 `.NXE` in `userbin/` —
     reject any new Ring 0 shell command.
   - Rule 6: new syscalls are `sys_ob_*` operating on Ob objects (**no numeric
     threshold** — the old "RAX >= 77" heuristic is obsolete).
   - Rule 8: read `docs/architecture/source-of-truth.md` — invariants hold.
   - Rule 10: kebab-case files/dirs, PascalCase types, snake_case fns/vars.
   - Rule 11: the work is on a dedicated branch.
   - Rule 12: module layout — one dir = one subsystem, dependencies flow downward.

2. **Check subsystem dependencies**

   ```bash
   neodev check-deps
   ```

   Fix/reject forbidden cross-subsystem imports (e.g. scheduler importing
   filesystem types).

3. **Review test coverage**
   - New tests use the `test_case!` harness (not raw `assert!`/`panic!`).
   - Error paths covered, not only the happy path.
   - Run `neodev test` — all tests pass.

4. **Verify public API docs**
   - Syscall added/changed → `docs/kernel/syscalls.md`.
   - `ObInfoClass`/`ObSetInfoClass` variant → `docs/kernel/objects.md`.
   - NEM ABI changed → `docs/drivers/overview.md` and the ABI in `AGENTS.md`.
   - `libneodos/` struct changed → `docs/userland/libneodos.md`.
   - Architecture change → `docs/architecture/source-of-truth.md`.

5. **Check commit hygiene**
   - No secrets or keys; no large binaries; no `target/` or `Cargo.lock` unless
     intended.
   - Commit message style: `feat|fix|refactor: ... (#issue)`.
   - Only intended files staged.

6. **Run full verification**

   ```bash
   cd neodos-kernel && cargo build
   neodev build --quick --image && neodev test
   neodev check-deps
   scripts/check-skills.sh
   npx markdownlint '**/*.md' --config .markdownlint.json
   ```

7. **Approve or request changes**
   - All checks pass → approve.
   - Minor issues → request changes with file/line references.
   - Major architectural violation → reject with the violated rule reference.

## Best practices

- Be specific — reference file paths and line numbers.
- Distinguish style nits (non-blocking) from correctness issues (blocking).
- Every `unsafe` block needs a safety comment.
- Don't silently ignore `Result`/`Option`.
- Check integer overflow, arithmetic edge cases, and signed/unsigned mismatches.

## Common mistakes

- Approving a new Ring 0 shell command.
- Missing an ABI bump when NEM structs change.
- Missing docs for a public API change.
- Approving `skills/` that cite removed scripts or obsolete paths
  (`scripts/check-skills.sh` catches both).
- Catching raw handle dereferences instead of table lookups.
- Missing safety comments on `unsafe`.

## Final checklist

- [ ] All AGENTS.md permanent rules satisfied
- [ ] `neodev check-deps` passes
- [ ] Public API docs updated (syscalls, objects, drivers, libneodos)
- [ ] `skills/` in sync (`scripts/check-skills.sh` passes)
- [ ] Tests added/updated; error paths covered
- [ ] `cargo build` + `neodev test` pass
- [ ] `npx markdownlint '**/*.md' --config .markdownlint.json` passes
- [ ] No secrets, no stray binaries, no unintended changes
- [ ] `unsafe` blocks have safety comments
