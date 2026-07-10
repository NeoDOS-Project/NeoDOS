# Roadmap

Current: **v0.49.0** (NeoFS robustness — indirect blocks, journaling, checksums). Target: **v1.0** (stable NT-like executive).

See [IMPROVEMENTS.md](IMPROVEMENTS.md) for the full detailed roadmap with per-item descriptions, files, prerequisites, and test plans.

## Milestones

| Version | Focus | Priority |
|---------|-------|----------|
| v0.48 | NeoFS stability (NS-1/2, FS-1/2/4) | HIGH |
| v0.49 | NeoFS robustness (indirect blocks, journaling, checksums) | MEDIUM |
| v0.50 | Async I/O + Registry persistence | LOW |
| v0.51 | ASLR v2, benchmarking | LOW |
| v0.52 | Networking complete (UDP, DNS, DHCP) | LOW |
| v0.53 | Performance (zero-copy pipes, COW fork) | LOW |
| v0.54-v0.59 | Documentation, hardening, fuzzing | LOW |
| v1.0 | Stable API | LOW |

## Priority Overview

- **CRITICAL**: Registry disk persistence (`cm_flush_key`)
- **HIGH**: libneodos syscall wrappers, NeoFS fixes, networking userland (libneodos SOCKET, net.nxl, netcfg), VirtIO architecture, registry defaults, NeoInit registry-driven config, NeoGet v1 (`.nxp` packages)
- **MEDIUM**: VFS layer separation, cache unification, shell redirection, kernel tracing, driver certification features
- **LOW**: VirtIO peripheral drivers, experimental features, documentation

## Key Architectural Issues

TOCTOU race in `kobj_register`, incomplete `ObInfoClass`/`ObSetInfoClass` enums, global VFS lock contention, NeoFS fixed 256 inode limit, stale I/O completions in AHCI NCQ, ownership tracking in Ob namespace.
