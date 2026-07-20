# NeoGet — Package Manager

> **Architecture:** `docs/ARCHITECTURE_PACKAGE_MANAGER.md`
> **Format:** `.nxp` — NeoDOS eXecutable Package (magic `b"NXP1"`)
> **Binary:** `userbin/neoget/` → `C:\Programs\neoget.nxe`
> **Library:** `libneopkg/` — engine shared by CLI, GUI, and services
> **Registry root:** `\Registry\Machine\Packages\`
> **Install root:** `C:\Programs\<name>\`
> **Tracking:** `docs/IMPROVEMENTS.md` sec PKG-1

---

## Resources

| Document | Contents |
| ---------- | ---------- |
| [`ARCHITECTURE_PACKAGE_MANAGER.md`](ARCHITECTURE_PACKAGE_MANAGER.md) | Complete architecture: format, components, Ob integration, security, transactions, CLI, API, comparison with APT/Pacman/Cargo/Nix |
| [`IMPROVEMENTS.md`](IMPROVEMENTS.md) sec PKG-1 | Implementation tracking, phases, tasks |
| `userbin/neoget/` | CLI source (future) |
| `libneopkg/` | Engine library source (future) |

## Quick reference

```text
neoget install  <name>      # Install package
neoget remove   <name>      # Remove package
neoget list                 # List installed
neoget info     <name>      # Show package details
neoget search   <query>     # Search repositories
neoget update               # Sync repository indexes
neoget upgrade              # Upgrade all packages
neoget verify   <name>      # Verify file integrity
neoget depends  <name>      # Show dependency tree
neoget repo list            # List repositories
neoget history              # Show transaction history
neoget doctor               # Diagnose issues
```

## Implementation phases

See [`ARCHITECTURE_PACKAGE_MANAGER.md`](ARCHITECTURE_PACKAGE_MANAGER.md) §13 for the full roadmap (v1-v5).

**Phase 1 — Foundation (v1 core):**

- `.nxp` parser in `libneopkg`
- `INSTALL`, `REMOVE`, `LIST`, `INFO` in `neoget`
- Registry-backed database via Cm syscalls
- Simple dependency resolver (linear)
- CRC32 integrity verification
- Kernel: `MODE_CORE` flag + delete guards
- Recovery: file backup before mutation
