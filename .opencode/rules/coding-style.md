# Coding Style (NeoDOS Kernel)

## Naming Conventions

- kebab-case for files and directories
- PascalCase for types, enums, traits, type aliases
- snake_case for functions, variables, modules

## Unsafe Rust Rules

- Every unsafe block MUST have a // Safety: comment
- Keep unsafe blocks as small as possible
- Wrap unsafe in a safe function with clear preconditions
- Document safety invariants for raw pointer types

## Error Handling

- Kernel functions return NtStatus, not bool or Option
- Use named NtStatus constants, never magic numbers
- Match error cases exhaustively
- No unwrap() or expect() in kernel code

## Ob Object Patterns

- Every ObType implements ObOperation trait
- create/open/close lifecycle for all objects
- Reference counting with ObReferenceObject/ObDereferenceObject
- Security descriptor on all named objects

## File Organization

- One type per file (except small tightly-coupled types)
- Files under neodos-kernel/src/<subsystem>/
- Public API in lib.rs, implementation in submodules
- Max 800 lines per file

## Code Quality Checklist

- [ ] Unsafe has // Safety: comment
- [ ] Returns NtStatus, not bool
- [ ] No unwrap()/expect()
- [ ] Naming follows conventions
- [ ] Cross-subsystem deps clean (scripts/check_deps.py)
- [ ] test_case! exists for new functionality
