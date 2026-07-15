---
name: documentation
description: Update or create documentation, doc comments, and architectural docs
---

# Documentation

## When to use

Updating or creating documentation, adding doc comments to code, or correcting an architectural document.

## Goal

Keep docs accurate and useful — they explain design, not replicate code.

## Steps

1. **Determine what changed**
   - Architecture change → `docs/ARCHITECTURE_SOURCE_OF_TRUTH.md` (invariants that are enforceable rules)
   - Subsystem change → the relevant `docs/<subsystem>.md` (design explanation)
   - Public API change (syscall, struct, ObInfoClass) → subsystem doc + possibly `docs/libneodos.md`
   - Release/version change → `AGENTS.md` version field, `CHANGELOG.md`, `docs/IMPROVEMENTS.md`

2. **Read the existing doc**
   Before editing, read the doc you need to update. Understand the current framing. Don't duplicate what's already there.

3. **Apply the "Code is truth" principle**
   - Docs explain *why*, not *what* — the code is the source of truth for implementation.
   - Don't copy function signatures, struct fields, or enum variants into docs — they drift.
   - Do explain design rationale, trade-offs, invariants, and usage patterns.
   - Do document API contracts (preconditions, postconditions, error semantics).

4. **Update `docs/<subsystem>.md`**
   Structure per doc: overview, design rationale, key types, interactions with other subsystems.
   When adding a new section, follow the existing format (headings, code blocks for examples only).

5. **Update `AGENTS.md`**
   If permanent rules change. Keep it minimal — move specialized instructions to `docs/` and procedural checklists to `skills/`.

6. **Update `docs/IMPROVEMENTS.md`** (if completing an item)
   Move the completed item from `docs/IMPROVEMENTS.md` to `docs/IMPROVEMENTS_COMPLETED.md`.
   Mark with the commit hash or PR number that completed it.

7. **Update `CHANGELOG.md`**
   Add an entry under the current version heading. Format: `- feat/fix/refactor: brief description (#PR)`.

8. **Review for consistency**
   Run `scripts/check_deps.py` and the MCP consistency check:

   ```text
   tools call neodos-mcp_check_consistency targets=docs
   ```

## Best practices

- Use present tense, imperative mood ("The scheduler selects threads..." not "The scheduler will select...").
- Include diagrams (ASCII art) for complex flows — they break down at subsystem boundaries.
- Cross-reference related docs with relative links (`../objects.md#lifecycle`).
- Keep one doc per subsystem — don't create overview docs that duplicate individual ones.
- When deleting a feature, also delete or mark its documentation.

## Common mistakes

- Copying function signatures from code — they go out of date and the compiler already verifies them.
- Writing tutorials instead of reference docs — tutorials belong elsewhere.
- Leaving stale docs after refactoring — check all docs that reference changed code.
- Adding doc comments that just restate the type name ("Process struct — represents a process").
- Not updating `docs/IMPROVEMENTS.md` when a task is completed.

## Final checklist

- [ ] Doc explains *design*, not *code* — no function/struct copy-paste
- [ ] `AGENTS.md` updated if permanent rules changed
- [ ] `docs/IMPROVEMENTS.md` → `docs/IMPROVEMENTS_COMPLETED.md` if items completed
- [ ] `CHANGELOG.md` updated
- [ ] Cross-references valid (relative links work)
- [ ] `scripts/check_deps.py` passes
- [ ] MCP consistency check passes (`targets=docs`)
