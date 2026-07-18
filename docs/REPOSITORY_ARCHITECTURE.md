# NeoDOS Repository Architecture

> **Version:** v1.0
> **Date:** 2026-07-16
> **Status:** Proposal — no automatic changes yet.

This document contains the complete audit of the NeoDOS monorepo and a proposal for
future multi-repository organization under the [NeoDOS-Project](https://github.com/NeoDOS-Project) organization.

---

## 1. Current Structure

```
neodos/
├── neodos-kernel/          # Kernel (52,945 lines, 49 modules)
├── neodos-bootloader/      # UEFI bootloader
├── libneodos/              # Standard user library (syscall wrappers)
├── libneodos-nxl/          # NXL DLL — core runtime
├── libmath-nxl/            # NXL DLL — math
├── libconsole-nxl/         # NXL DLL — console/progress
├── libnet-nxl/             # NXL DLL — networking
├── libnet/                 # User-mode networking library
├── userbin/                # 28 user binaries (PID 1, shell, utilities)
├── drivers/                # 10 standalone NEM drivers
├── tools/                  # 6 Rust tools + 1 Python script
│   ├── neodev/             # Build/run/test toolchain
│   ├── nltc/               # NLT compiler
│   ├── nxdump/             # ELF/NXE/NEM dumper
│   ├── nxeinfo/            # NXE binary inspector
│   ├── nxpkg/              # NXP package tool
│   └── kbdcompile/         # Keyboard layout compiler
├── scripts/                # Python/bash utilities + MCP server
├── data/                   # Locale TOML (81 files) + keyboard layouts
├── docs/                   # 45 Markdown documentation files
├── skills/                 # 18 AI agent skill definitions
├── preferences/            # Test preferences
├── .opencode/              # AI dev environment config
└── opencode.json           # OpenCode configuration
```

### 1.1 By the numbers

| Metric | Value |
|--------|-------|
| Total `.rs` files | 294 |
| Total lines of Rust | 81,008 |
| Kernel lines | 52,945 (65.4%) |
| User binaries lines | ~15,000 |
| Tools lines | ~4,000 |
| Python scripts | 35 files |
| Documentation | 45 `.md` files |
| Locale files | 81 `.toml` files |
| Keyboard layouts | 4 files |
| Skills | 18 `.md` files |

---

## 2. Dependency & Coupling Analysis

### 2.1 Dependency graph

```
neodos-bootloader ───→ kernel (BootInfo ABI)
kernel              ─── (no external deps beyond Rust std)
kernel (NEM ABI)    ←─── drivers/* (10 standalone)
kernel (syscall ABI) ←── libneodos
kernel (NXL ABI)    ←─── libneodos-nxl, libmath-nxl, libconsole-nxl, libnet-nxl
libneodos           ←─── userbin/* (28 binaries), libnet, libneodos-nxl
libnet-nxl          ───→ kernel (net subsystem)
libnet              ───→ libneodos, libnet-nxl
neodev (host)       ───→ build artifacts (read-only)
nltc (host)         ───→ data/locale (translation sources)
kbdcompile (host)   ───→ data/keyboard (layout sources)
```

### 2.2 Coupling classification

#### Tightly coupled (MUST stay together)

| Component | Reason |
|-----------|--------|
| `neodos-kernel` | The OS itself. No external deps. |
| `neodos-bootloader` | BootInfo ABI tied to kernel version. |
| `libneodos` | Syscall ABI, SSDT table, ObInfoClass — must match kernel exactly. |
| `libneodos-nxl` | NXL export table ABI tied to kernel. |
| `libmath-nxl` | Loaded by kernel NXL loader at fixed address. |
| `libconsole-nxl` | Loaded by kernel NXL loader at fixed address. |
| `libnet-nxl` | Tied to kernel networking ABI |
| `libnet` | Wraps libnet-nxl + libneodos |
| `userbin/*` | Each depends on libneodos syscall ABI |

#### Moderate coupling (could separate with ABI versioning)

| Component | Reason |
|-----------|--------|
| `drivers/*` | NEM ABI is versioned (currently v8). Already compile independently. Could publish ABI spec. |

#### Loose coupling (standalone already)

| Component | Reason |
|-----------|--------|
| `neodev` | Host-only tool, no kernel deps. Already has own Cargo.toml. |
| `neodos-lsp` | **Migrated** to [neodos-dev-server](https://github.com/NeoDOS-Project/neodos-dev-server) |
| `scripts/mcp_server/` | **Migrated** to [neodos-dev-server](https://github.com/NeoDOS-Project/neodos-dev-server) (rewritten in Rust as neodos-mcp) |
| `nltc` | Host tool, parses TOML → NLT binary. |
| `nxdump` | Host tool, reads ELF/NXE/NEM files. |
| `nxeinfo` | Host tool, reads NXE headers. |
| `nxpkg` | Host tool, creates/extracts NXP packages. |
| `kbdcompile` | Host tool, compiles keyboard layouts. |
| `scripts/mcp_server` | Python, completely independent. |
| `scripts/check_deps.py` | Python, standalone validation. |
| `scripts/gen_*` | Python, host-side data generation. |
| `data/locale/*` | Translation data, independent of code. |
| `docs/*` | Documentation, independent. |
| `skills/*` | AI agent config, independent. |
| `.opencode/` | AI dev environment config, independent. |

---

## 3. Classification

### 3.1 Permanecer en NeoDOS

| Component | Justification |
|-----------|---------------|
| `neodos-kernel` | Core OS. Cannot be separated. |
| `neodos-bootloader` | BootInfo ABI tied to kernel. One version = one bootloader. |
| `libneodos` | Syscall ABI lockstep with kernel. Version must match. |
| `libneodos-nxl` | Loaded by kernel, export ABI tied to kernel version. |
| `libmath-nxl` | Loaded by kernel at fixed address. |
| `libconsole-nxl` | Loaded by kernel at fixed address. |
| `libnet-nxl` | Loaded by kernel, net ABI tied to kernel. |
| `libnet` | Wraps libnet-nxl + libneodos. |
| `userbin/*` | Every binary depends on libneodos. Tight release coupling. |

### 3.2 Candidato a repositorio independiente

| Component | Repository | Reasoning |
|-----------|-----------|-----------|
| `neodev` | `NeoDev` | Fully standalone host tool. Standard Rust deps (clap, anyhow, serde). Changes at own pace. Can version independently. Build/test/image creation for any repo. |
| `neodos-lsp` + `scripts/mcp_server` | `NeoDOS Dev Server` (merged) | **Migrated.** LSP + MCP merged into single Rust workspace at [neodos-dev-server](https://github.com/NeoDOS-Project/neodos-dev-server). Shared toolkit (neodos-toolkit) + LSP binary + MCP binary. |
| `nltc, kbdcompile` | `NeoToolchain` or separate | Both are host-side compilers for OS data formats. Can evolve independently of kernel version. |
| `nxdump, nxeinfo, nxpkg` | `NeoTools` | UX-focused analysis tools. Independent release cycle. Useful for OS analysis even without building kernel. |
| `scripts/mcp_server` | `NeoMCP` | Python MCP server for AI-assisted development. Completely independent of kernel. |
| `data/locale/*` | `NeoTranslations` | Translation data. Community contributions don't need kernel access. Can be translated entirely by non-developers. |

### 3.3 Mantener integrado por el momento

| Component | Reasoning |
|-----------|-----------|
| `drivers/*` | NEM ABI is stable (v8), but drivers are still in active development. Separation would increase build complexity without clear benefit. Re-evaluate at v1.0. |
| `scripts/*` | Most scripts are tightly integrated with the build process. Only `mcp_server` and `check_deps.py` are independent. |
| `docs/*` | Documentation references kernel code directly. Separation would create maintenance overhead. Move to `NeoDocs` after v1.0 when docs are stable. |
| `skills/*` | Tied to AGENTS.md which lives in the main repo. Separation would require cross-repo sync. Keep integrated. |

---

## 4. Future Organization Proposal

### 4.1 Target structure

```
NeoDOS-Project/
├── NeoDOS                    # Kernel, bootloader, libneodos, NXLs, userbin, drivers, scripts, docs
│                              # (core OS — tightly coupled components)
├── NeoDev                    # Build/run/test toolchain
├── neodos-dev-server         # LSP server + MCP server + shared toolkit (merged)
├── NeoTools                  # nxdump, nxeinfo, nxpkg (OS analysis tools)
├── NeoTranslations           # Locale data (NLT files) + translation tools (nltc)
├── .github                   # Community health files (already exists)
```

### 4.2 Repository details

#### `NeoDOS`

| Attribute | Value |
|-----------|-------|
| Contents | Kernel, bootloader, libneodos, all NXLs, userbin/, drivers/, core scripts (build, check_deps), core docs |
| Dependencies | None (self-contained) |
| Versioning | Semantic v0.x.x (current: v0.49.0) |
| Release strategy | Tagged releases, CHANGELOG-driven |
| CI | Build kernel + bootloader, run kernel tests, build userbin, build disk image |
| Artifacts | `kernel.elf`, `bootloader.efi`, `*.nxe`, `*.nxl`, `*.nem`, `disk_image.img` |
| Cadence | Active development, frequent releases (every 1-2 weeks pre-v1.0) |

#### `NeoDev`

| Attribute | Value |
|-----------|-------|
| Contents | Full neodev tool: build, image, run, test, clean, report, discovery, VMM helpers |
| Dependencies | None on NeoDOS source (reads build artifacts) |
| Versioning | Semantic v0.x.y, independent from kernel |
| Release strategy | Tagged releases per breaking changes |
| CI | Build, lint, integration test against latest NeoDOS release |
| Artifacts | Single `neodev` binary |
| Cadence | Changes only when dev workflow needs evolve |

#### `NeoDOS-LSP`

| Attribute | Value |
|-----------|-------|
| Contents | LSP server implementation (handlers, indexer, diagnostics, etc.) |
| Dependencies | 10+ host Rust crates |
| Versioning | Semantic v0.x.y, independent |
| Release strategy | Tagged releases |
| CI | Build, lint, unit tests |
| Artifacts | Single `neodos-lsp` binary |
| Cadence | Changes as language features are added |

#### `NeoTools`

| Attribute | Value |
|-----------|-------|
| Contents | `nxdump`, `nxeinfo`, `nxpkg` (each as subcrate or workspace members) |
| Dependencies | Minimal: serde_json, optionally clap |
| Versioning | Semantic v0.x.y, shared or per-tool |
| Release strategy | Tagged releases |
| CI | Build each tool, lint, smoke tests |
| Artifacts | `nxdump`, `nxeinfo`, `nxpkg` binaries |
| Cadence | Slow, only when OS formats change |

#### `NeoMCP`

| Attribute | Value |
|-----------|-------|
| Contents | MCP server Python package: server, parsers, tools |
| Dependencies | Python 3, `pyelftools` |
| Versioning | Semantic v0.x.y |
| Release strategy | Tagged releases |
| CI | Lint (ruff), pytest |
| Artifacts | Python package (PEP 621) |
| Cadence | Changes as analysis needs evolve |

#### `NeoTranslations`

| Attribute | Value |
|-----------|-------|
| Contents | `data/locale/*.toml` files, `nltc` compiler, `gen_nlt*.py` scripts |
| Dependencies | nltc (Rust, standalone), Python 3 |
| Versioning | Independent from kernel. Could track by locale version. |
| Release strategy | Tagged releases bundled with NeoDOS releases |
| CI | Compile all NLTs, validate format, check completeness |
| Artifacts | Compiled `.nlt` binary files |
| Cadence | Community-driven, as translations are contributed |

### 4.3 Dependency graph (target)

```
NeoDOS (core)
├── NeoDev              [dev dependency: reads artifacts]
├── neodos-dev-server   [dev dependency: reads source / artifacts]
│   ├── neodos-lsp      [LSP server]
│   └── neodos-mcp      [MCP server]
├── NeoTools            [dev dependency: reads artifacts]

NeoTranslations   [independent, consumed by NeoDOS at build time]
```

No circular dependencies.

### 4.4 CI/CD strategy

| Repository | CI Triggers | Tests | Artifacts |
|-----------|-------------|-------|-----------|
| NeoDOS | push, PR | kernel tests, build, check_deps | kernel.elf, disk image |
| NeoDev | push, PR | build, lint, integration vs NeoDOS release | neodev binary |
| NeoDOS-LSP | push, PR | build, lint, unit tests | neodos-lsp binary |
| NeoTools | push, PR | build, lint, format validation | nxdump, nxeinfo, nxpkg |
| NeoMCP | push, PR | ruff, pytest | Python package |
| NeoTranslations | push, PR | nltc compile, completeness check | .nlt files |

NeoDOS CI would:
1. Build kernel + bootloader
2. Run kernel tests in QEMU
3. Build user binaries
4. Build disk image
5. Run `check_deps.py`

Sub-repo CI would trigger independently.

---

---

## 5. Branch Strategy (pre-v1.0)

While NeoDOS lives in a monorepo, a clear branch strategy keeps development organized:

```
master         ← Stable. Only merges from develop via release PRs.
develop        ← Daily integration. Default branch for PRs.
├── feat/*     ← Features (feat/ob-namespace-v2, feat/ahci-msi)
├── fix/*      ← Bug fixes (fix/page-fault-leak)
├── refactor/* ← Code refactors (refactor/driver-abi)
└── release/*  ← Release preparation (release/v0.51.0)
```

### Rules

| Branch | Protected | Requires PR | Requires CI | Deletes after merge |
|--------|-----------|-------------|-------------|---------------------|
| `master` | Yes | Yes | Yes | — |
| `develop` | Yes | Yes (≥1 approval) | Yes | — |
| `feat/*`, `fix/*`, `refactor/*` | No | Yes → `develop` | Yes | Yes |
| `release/*` | No | Yes → `master` | Yes | Yes |

### Workflow

1. Create feature branch from `develop`: `feat/name`
2. Work, commit, push
3. Open PR → `develop` with label (`feat:`, `fix:`, `refactor:`)
4. CI must pass + 1 approval
5. Squash-merge to `develop`
6. Delete feature branch
7. When ready for release: `release/vX.Y.Z` from `develop` → PR → `master`
8. Tag `master` with version, create GitHub Release

### Hotfix flow (rare)

For critical bugs in `master`:
1. Branch `fix/critical-name` from `master`
2. Fix, PR → `master` (urgent, skip `develop`)
3. Cherry-pick to `develop` to keep sync

---

## 6. Migration Plan

No immediate migration. All proposals are for **post-v1.0** unless otherwise noted.

### Phase 1 (now — no action needed)
- Document the proposed architecture
- Add items to `docs/IMPROVEMENTS.md`
- Ensure new components are written with separation in mind

### Phase 2 (v0.60+ — completed)
- Extract `neodev` to standalone repo ✅
- Merge `neodos-lsp` + `NeoMCP` into single `neodos-dev-server` Rust workspace ✅
- Rewrite MCP server from Python to Rust ✅

### Phase 3 (v1.0+ — major separation)
- Extract `NeoTranslations` for community translation contributions
- Extract `NeoTools` for independent release cadence
- Extract `NeoDOS-LSP` for editor integration distribution
- Move `docs/` to `NeoDocs` if documentation becomes stable

### What to never separate
- `neodos-kernel` — core OS, must stay
- `neodos-bootloader` — tied to kernel ABI
- `libneodos` — syscall API lockstep
- `userbin/*` — all depend on libneodos, tight coupling
- `lib*-nxl` — loaded by kernel at fixed addresses

---

## 6. Benefits & Risks

### Benefits of separation

| Factor | NeoDev | NeoTools | NeoTranslations | NeoMCP | NeoDOS-LSP |
|--------|--------|----------|-----------------|--------|------------|
| Faster CI | ✓ | ✓ | ✓ | ✓ | ✓ |
| Independent versioning | ✓ | ✓ | ✓ | ✓ | ✓ |
| Community contributions | ✓ | ✓ | ✓ | ✓ | ✓ |
| Smaller clone | ✓ | ✓ | ✓ | ✓ | ✓ |
| Domain-specific issues/PRs | ✓ | ✓ | ✓ | ✓ | ✓ |
| Independent releases | ✓ | ✓ | ✓ | ✓ | ✓ |

### Risks of separation

| Risk | Severity | Mitigation |
|------|----------|------------|
| Cross-repo sync complexity | Medium | Use git submodules or release artifacts |
| Breaking changes across repos | Medium | Lockstep releases for critical ABI |
| Build coordination overhead | Medium | NeoDev pins NeoDOS release for CI |
| Contributor confusion | Low | Clear documentation in each repo |
| Loss of atomic commits | Low | Infrequent cross-repo changes |

---

## 7. Improvements detected for IMPROVEMENTS.md

- **REPO-SEP-001.** Extract NeoDev to separate repo (post-v1.0)
- **REPO-SEP-002.** Extract NeoTools (nxdump, nxeinfo, nxpkg) to separate repo
- **REPO-SEP-003.** Extract NeoMCP (Python MCP server) to separate repo
- **REPO-SEP-004.** Extract NeoTranslations (locale data + nltc) to separate repo
- **REPO-SEP-005.** Extract NeoDOS-LSP to separate repo
- **REPO-SEP-006.** Move docs/ to NeoDocs repo after v1.0 stability
- **REPO-SEP-007.** Re-evaluate drivers/ separation at v1.0 when NEM ABI is frozen
