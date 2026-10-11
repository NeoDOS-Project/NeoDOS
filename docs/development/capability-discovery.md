# Proactive Capability Discovery

> **Status:** mandatory agent workflow. Summary and links live in
> [`AGENTS.md`](../../AGENTS.md); this document is the full specification.

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

## 1. Discover

When something looks like it is missing, briefly decide whether it is a real
gap (e.g. "no network-interface enumeration API", "no Ctrl+C handling in the
shell", "no persistent configuration storage", "no per-process memory query").
Do not open an Issue for every trivial helper — the capability needs enough
substance to stand as its own task.

## 2. Verify

Before opening an Issue, confirm the capability truly does not exist, using the
repository as the source of truth: code, APIs, syscalls, docs, tests, existing
tools, configuration, infrastructure. Do not assume absence just because it is
not obvious, and do not invent APIs from stale documentation.

## 3. Search existing Issues

```bash
gh issue list --search "<relevant terms>"
```

Try several term combinations. If an equivalent Issue exists, do **not**
duplicate it: reference it and link it to the current task when relevant.

## 4. Create Issue

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

## 5. Blocking vs Non-blocking

Distinguish the two cases explicitly.

- **Blocking capability** — the gap prevents completing the current task
  correctly: `Detect → Verify → Search Issue → Create/reference Issue → Document
  blocker`. Never build a fake, temporary, or architecturally wrong
  implementation just to sidestep the block.
- **Non-blocking capability** — relevant but not blocking: `Detect → Verify →
  Search Issue → Create/reference Issue → Document → Continue current task`.
  A non-blocking gap must **never** cause the agent to widen the scope of the
  current task.

## 6. Never implement discoveries automatically

Discovering a missing capability is **not** authorization to build it. Example:
while implementing DNS, the agent notices NeoShell has no Ctrl+C — it must
verify, search for an Issue, create `[SHELL] Add Ctrl+C / interrupt handling`
if absent, document it, and **continue with DNS**.

## 7. End-of-task report

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

## Architectural principle

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
