# NeoDOS Agents

## Available Agents

| Agent | Purpose | When to Use |
|-------|---------|-------------|
| architect | Kernel architecture design | New subsystems, syscalls, Ob types |
| code-reviewer | Rust/unsafe code review | After modifying kernel code |
| security-reviewer | Kernel security (SID/Token/Rings) | Syscalls, drivers, access control |
| tdd-guide | Kernel TDD with test_case! | New features, bug fixes |

## When to Use Agents

- New subsystem or syscall → **architect** first
- Code modified → **code-reviewer** immediately
- Security-sensitive code → **security-reviewer**
- Any new feature → **tdd-guide** (tests first)
