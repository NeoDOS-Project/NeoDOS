---
name: documentation
description: Update or create documentation, doc comments, and architectural docs
---

# Documentation

## When to use

Updating or creating documentation, adding doc comments, or correcting an
architectural document.

## Goal

Keep docs accurate and useful — they explain design, not replicate code.

## Steps

1. **Determine what changed**
   - Architecture → `docs/architecture/source-of-truth.md` (enforceable
     invariants) and/or `docs/architecture/overview.md`.
   - Subsystem → the relevant `docs/<subsystem>/<doc>.md` (see `docs/README.md`
     for the index).
   - Public API (syscall, Ob class, libneodos) → subsystem doc; syscalls →
     `docs/kernel/syscalls.md`; Ob classes → `docs/kernel/objects.md`.
   - Release/version → the version field in `AGENTS.md`, `CHANGELOG.md`, then
     `scripts/sync-roadmap.sh sync`.

2. **Read the existing doc first.** Understand the current framing; don't
   duplicate.

3. **Apply "Code is truth"** (AGENTS.md rule 7)
   - Docs explain *why*, not *what*; the code is the source of truth.
   - Don't copy function signatures, struct fields, or enum variants — they drift.
   - Do document design rationale, trade-offs, invariants, and API contracts
     (preconditions, postconditions, error semantics).

4. **Update the doc** following its existing structure (overview, design
   rationale, key types, interactions). Keep one doc per subsystem.

5. **Update `AGENTS.md`** only for permanent rules — keep it minimal; move
   specialized instructions to `docs/` and checklists to `skills/`.

6. **Sync the roadmap** (when completing an item)

   ```bash
   scripts/sync-roadmap.sh sync
   ```

   GitHub Issues is the SSOT for planning; `roadmap/improvements.md` is the local
   idea list.

7. **Update `CHANGELOG.md`** under the current version heading:
   `- feat|fix|refactor: brief description (#PR)`.

8. **Review for consistency**

   ```bash
   neodev check-deps
   scripts/check-skills.sh
   npx markdownlint '**/*.md' --config .markdownlint.json
   ```

   And run the MCP `neodos-mcp check_consistency` tool with `targets=docs`.

## Best practices

- Present tense, imperative mood.
- Use ASCII diagrams for complex flows; cross-reference with relative links.
- One doc per subsystem — no duplicate overview docs.
- When deleting a feature, delete or mark its documentation.
- Keep the procedural checklists in `skills/` in sync too — run
  `scripts/check-skills.sh`; it catches removed scripts, obsolete heuristics,
  and stale paths before they reach an agent.
- Keep the `docs/README.md` index in sync with new/removed files.

## Common mistakes

- Copying signatures/fields from code (drift).
- Writing tutorials instead of reference material.
- Leaving stale docs after a refactor (`src/` paths and type names). Note: kernel
  source paths in docs are relative to `neodos-kernel/` (e.g. `src/fs/vfs/`).
- Forgetting `scripts/sync-roadmap.sh sync` when a task completes.
- Skipping `markdownlint` after edits.

## Final checklist

- [ ] Doc explains *design*, not code (no copy-paste of signatures)
- [ ] `docs/README.md` index updated if files were added/removed
- [ ] `AGENTS.md` updated only for permanent rules
- [ ] `scripts/sync-roadmap.sh sync` run if items completed
- [ ] `CHANGELOG.md` updated
- [ ] Cross-references valid; `markdownlint` passes
- [ ] Skills synced (`scripts/check-skills.sh` passes)
- [ ] `neodev check-deps` and the MCP `check_consistency` (targets=docs) pass
