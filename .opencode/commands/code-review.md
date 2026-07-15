---
description: Review NeoDOS kernel and userspace code for Rust safety, NT design patterns, ABI compatibility, and architecture compliance.
---

# Code Review for NeoDOS

Review uncommitted changes for the NeoDOS kernel project:

1. Get changed files: `git diff --name-only HEAD`

2. For each changed file, check:

**Unsafe Rust (CRITICAL):**
- Missing // Safety: comments on unsafe blocks
- Pointer arithmetic without bounds checks
- Raw pointer dereferences without lifetime validation
- unwrap()/expect() on fallible kernel operations

**NT Design Patterns (HIGH):**
- New syscalls follow sys_ob_* naming (RAX >= 60)
- ObHandle used instead of raw pointers in public API
- ObOperation trait implemented for new ObTypes
- InfoClass enums handled exhaustively
- Reference counting balanced (ObReferenceObject/ObDereferenceObject)

**Architecture Compliance (HIGH):**
- No kernel/ -> executive/ dependency violations
- No Ring 0 shell commands (go in userbin/ as .NXE)
- Dependency check: `scripts/check_deps.py`
- Naming: kebab-case files, PascalCase types, snake_case fns

**Error Handling (HIGH):**
- All kernel fns return NtStatus, not bool/option
- Error paths documented and tested
- No magic error constants (use named NtStatus)

3. Block commit if CRITICAL or HIGH issues found
