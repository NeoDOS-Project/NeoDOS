---
description: Enforce test-driven development for NeoDOS kernel. Write test_case! entries FIRST, then implement minimal kernel code to pass. Run in QEMU via neodev.
agent: tdd-guide
---

# TDD for NeoDOS

This command invokes the tdd-guide agent to enforce test-driven development.

## What This Command Does

1. **Define test_case! stub** with expected behavior
2. **Write failing test** (test_case! macro in appropriate module)
3. **Run test** via neodev — verify it fails
4. **Write minimal kernel implementation** to make test pass
5. **Run test** — verify it passes
6. **Refactor** code while keeping tests green

## TDD Cycle

```
RED → GREEN → REFACTOR → REPEAT
```

## Test Framework

Tests use `test_case!` in neodos-kernel/src/testing.rs.
Run with: `cargo run --manifest-path tools/neodev/Cargo.toml -- test <group>`
Groups: ob, syscall, mm, scheduler, vfs, hal, driver, registry, security, ipc, boot
