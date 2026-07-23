---
name: review
description: Review pull requests or code changes for architecture, deps, tests, docs
---

# Review

## When to use

You are reviewing a pull request or code change, or need a systematic checklist before committing.

## Goal

Catch architectural violations, missed invariants, missing docs, and breaking changes before they land.

## Steps

1. **Verify AGENTS.md rules**
   - Rule 1: No automatic builds — ensure no CI/staging workflow was added.
   - Rule 4: NT-like design — Ob is the central abstraction; verify new code uses Ob handles, not raw pointers.
   - Rule 5: All new interactive commands are Ring 3 .NXE in `userbin/` — reject any Ring 0 shell code.
   - Rule 6: RAX >= 77 syscalls are `sys_ob_*` — verify naming and Ob operation.
   - Rule 8: Read `docs/architecture/source-of-truth.md` — check invariants aren't violated.
   - Rule 10: kebab-case files/dirs, PascalCase types, snake_case fns/vars.

2. **Check subsystem dependencies**
   Run `scripts/check_deps.py` to validate that no forbidden cross-subsystem imports were added.
   For example: scheduler code must not import filesystem types directly.

3. **Review test coverage**
   - Were new tests added for the change? All tests must pass.
   - Do the tests cover error paths, not just success?
   - Run `python3 scripts/auto_test.py` to confirm.

4. **Verify public API docs**
   - Syscall added? `docs/kernel/syscalls.md` updated.
   - ObInfoClass variant added? `docs/kernel/objects.md` updated.
   - NEM ABI changed? `docs/drivers/overview.md` and AGENTS.md ABI version updated.
   - Struct in `libneodos/` changed? `docs/userland/libneodos.md` updated.
   - Architecture change? `docs/architecture/source-of-truth.md` updated.

5. **Check commit hygiene**
   - No secrets or keys committed.
   - No merge commits or large binary files.
   - Commit message matches repo style (`feat|fix|refactor: ...`).
   - Only intended files staged (no `Cargo.lock`, no `target/`, no IDE files).

6. **Run full verification**

   ```bash
   cargo build
   python3 scripts/auto_test.py
   scripts/check_deps.py
   ```

7. **Approve or request changes**
   - If all checks pass: approve.
   - If minor issues: request changes with specific file/line references.
   - If major architectural violation: reject with reference to the violated rule.

## Best practices

- Be specific in review comments — reference file paths and line numbers.
- Distinguish between style nits (non-blocking) and correctness issues (blocking).
- Check for unsafe code — every `unsafe` block needs a safety comment.
- Verify error handling — don't silently ignore `Result` or `Option`.
- Review for integer overflow, arithmetic edge cases, and signed/unsigned mismatches.

## Common mistakes

- Approving changes that add new Ring 0 shell commands.
- Missing ABI version bumps when NEM driver structs change.
- Letting through commits that modify `AGENTS.md` without updating doc pointers.
- Not catching direct Ob handle dereferencing instead of using `ObObjectTable::with_handle()`.
- Missing safety comments on `unsafe` blocks.

## Final checklist

- [ ] All AGENTS.md permanent rules satisfied
- [ ] `scripts/check_deps.py` passes (no forbidden imports)
- [ ] Public API docs updated (syscalls, objects, drivers, libneodos)
- [ ] Tests added for all new functionality, error paths covered
- [ ] `cargo build` and `python3 scripts/auto_test.py` pass
- [ ] No secrets, no binary files, no unintended changes
- [ ] `unsafe` blocks have safety comments
