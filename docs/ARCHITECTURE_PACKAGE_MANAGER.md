# NeoDOS Package Manager — Architecture

> **Document:** `docs/ARCHITECTURE_PACKAGE_MANAGER.md`
> **Status:** Draft v1 — design complete, pending implementation
> **Binary:** `neoget` — `userbin/neoget/`
> **Library:** `libneopkg` — `libneopkg/` (new internal crate)
> **Kernel ABI:** No new syscalls — uses Ob (RAX 60-66) + Cm (RAX 67-74)
> **Tracking:** `docs/IMPROVEMENTS.md` sec PKG-1

---

## Table of Contents

1. [Principles](#1-principles)
2. [What Is a Package](#2-what-is-a-package)
3. [Package Format (.nxp)](#3-package-format-nxp)
4. [Local Database](#4-local-database)
5. [Object Manager Integration](#5-object-manager-integration)
6. [Component Architecture](#6-component-architecture)
7. [Dependency Resolution](#7-dependency-resolution)
8. [Repositories](#8-repositories)
9. [Transactions](#9-transactions)
10. [Security](#10-security)
11. [CLI Design](#11-cli-design)
12. [Internal API](#12-internal-api)
13. [Future Roadmap](#13-future-roadmap)
14. [Comparison with Existing Systems](#14-comparison-with-existing-systems)

---

## 1. Principles

### 1.1 Design axioms

1. **Ob-as-namespace.** Everything is reachable via the Object Manager namespace. Packages, repositories, transactions — all are `Ob` objects that can be opened, queried, and enumerated with standard syscalls.
2. **Transactional by default.** Every mutating operation (install, remove, upgrade) is a transaction. If the machine crashes mid-operation, the system recovers to a known state.
3. **Layered trust.** Packages are authenticated at three levels: package signature, repository index signature, and optional admin authorization. Each layer is independent.
4. **No kernel changes for v1.** All package logic lives in userland. The kernel only provides the existing Ob and Cm syscall interface. `MODE_CORE` and delete guards are the only kernel additions.
5. **Pluggable backends.** The resolver, downloader, and verifier are traits. Adding a new repository format or download protocol requires no changes to the core engine.
6. **Package = Object.** An installed package is a first-class Ob object at `\Package\<name>`. The same API used for processes, threads, and files works for packages.

### 1.2 Terminology

| Term | Definition |
| ------ | ------------ |
| **Package** | Versioned collection of files + metadata, distributed as `.nxp` |
| **NeoGet** | CLI tool (`neoget.nxe`) — frontend to the package system |
| **libneopkg** | Internal library crate — engine shared by CLI, GUI, and services |
| **Repository** | Remote or local source of packages, identified by URL + fingerprint |
| **Index** | Signed manifest of all packages in a repository (`.nxi` format) |
| **Transaction** | Atomic sequence of operations with rollback capability |
| **CORE** | Protection level: package flagged as critical, guarded by kernel |
| **Ob** | Object Manager — the central namespace abstraction |

---

## 2. What Is a Package

### 2.1 Definition

A package is a **named, versioned, signed archive** containing:

- Executables (`.nxe`), shared libraries (`.nxl`), or drivers (`.nem`)
- Configuration files, data files, documentation
- Metadata (name, version, description, author, dependencies, license)
- Optional lifecycle scripts (pre-install, post-install, pre-remove, post-remove)
- Cryptographic signature (Ed25519)

### 2.2 Package identity

A package is uniquely identified by the triple:

```text
(namespace, name, version)
```

Where:

| Field | Format | Example |
| ------- | -------- | --------- |
| `namespace` | Reverse domain (like Java/pkgr) | `org.neodos` , `com.github.user` |
| `name` | PascalCase, alphanumeric + hyphens | `NeoShell`, `NeoTop` |
| `version` | Semver `major.minor.patch` | `1.2.0`, `2.0.0-alpha.1` |

The full qualified name is `namespace/name`. The user-visible short name is `name` when the namespace is `org.neodos` (official).

### 2.3 What a package can contain

| Content type | Installed to | Example |
| ------------- | ------------- | --------- |
| Executable | `C:\Programs\<name>\` | `neotop.nxe` |
| Library | `C:\System\NXL\` | `libfoo.nxl` |
| Driver | `C:\System\Drivers\` | `driver.nem` |
| Config | `C:\Data\<name>\` | `config.toml` |
| Data | `C:\Data\<name>\` | `assets/*` |
| Doc | `C:\Programs\<name>\` | `README.txt` |

### 2.4 Package lifecycle states

```text
Available ──> Downloaded ──> Verified ──> Installed ──> Upgradable
                      │                    │
                      └──> Deleted         └──> Removed
```

States stored in the local database (Registry) per package.

### 2.5 Metadata fields

| Field | Type | Required | Description |
| ------- | ------ | ---------- | ------------- |
| `Namespace` | String | Yes | Reverse domain |
| `Name` | String | Yes | Package name |
| `Version` | String | Yes | Semver |
| `Description` | String | No | Short description |
| `Author` | String | No | Author/maintainer |
| `License` | String | No | SPDX identifier |
| `Arch` | String | Yes | `x86_64` |
| `Deps` | List | No | `{name, version_req, optional}` |
| `Conflicts` | List | No | `{name, version_req}` |
| `Replaces` | List | No | `{name, version_req}` |
| `Provides` | List | No | Virtual package names |
| `Scripts` | List | No | Pre/post install/remove script paths |
| `Flags` | Bitmask | Yes | `CORE`, `BOOT`, `NOKERNEL` |

---

## 3. Package Format (.nxp)

### 3.1 Overview

Binary container, little-endian, self-validating with CRC32 + optional Ed25519 signature.

```text
┌─────────────────────────────────────────────────────────┐
│ Magic "NXP1" (4 bytes)                                   │
├─────────────────────────────────────────────────────────┤
│ Header (32 bytes)                                        │
│   header_crc32 | version | flags | manifest_off/sz       │
│   file_count | unpacked_sz | signature_off/sz | reserved │
├─────────────────────────────────────────────────────────┤
│ Manifest (TLV)                                           │
│   NAME | VER | DESC | AUTH | ARCH | DEPS | CONFLICTS     │
│   REPLACES | PROVIDES | SCRIPTS | CORE | BOOT | LICENSE │
├─────────────────────────────────────────────────────────┤
│ File Entry Table (16 bytes × N)                          │
│   path_off | flags | data_off | data_sz | crc32          │
├─────────────────────────────────────────────────────────┤
│ String Table (null-terminated UTF-8 paths)               │
├─────────────────────────────────────────────────────────┤
│ File Data (raw blobs)                                    │
├─────────────────────────────────────────────────────────┤
│ Optional: Signature Block (Ed25519)                       │
│   sig_sz(u32) | sig_type(u16) | key_id(u32) | sig[..]   │
├─────────────────────────────────────────────────────────┤
│ Footer (12 bytes)                                        │
│   manifest_crc32 | file_table_crc32 | footer_crc32       │
└─────────────────────────────────────────────────────────┘
```

### 3.2 Header (32 bytes)

| Offset | Size | Field | Notes |
| -------- | ------ | ------- | ------- |
| 0 | 4 | `magic` | `0x3150584E` = `"NXP1"` |
| 4 | 4 | `header_crc32` | CRC32 of bytes 8..31 |
| 8 | 2 | `fmt_ver_major` | Format version major |
| 10 | 2 | `fmt_ver_minor` | Format version minor |
| 12 | 4 | `flags` | Bit 0=CORE, 1=BOOT |
| 16 | 4 | `manifest_offset` | From file start |
| 20 | 4 | `manifest_size` | In bytes |
| 24 | 4 | `file_count` | Number of file entries |
| 28 | 4 | `signature_offset` | 0 if unsigned, from file start |

### 3.3 Manifest (TLV encoding)

Each entry: tag (4 ASCII bytes, space-padded) | length (u32) | value (length bytes).

| Tag | Multi | Value | Example |
| ----- | ------- | ------- | --------- |
| `NAME` | No | Qualified name | `org.neodos/NeoTop` |
| `VER` | No | Semver | `1.2.0` |
| `DESC` | No | Free text | `Process monitor and task manager` |
| `AUTH` | No | Author string | `NeoDOS Team` |
| `ARCH` | No | Target arch | `x86_64` |
| `LICE` | No | SPDX | `MIT` |
| `CORE` | No | `0` or `1` | `0` |
| `BOOT` | No | `0` or `1` | `0` |
| `NOKR` | No | `0` or `1` (no kernel dep) | `1` |
| `DEP` | Yes | `namespace/name:version_req` | `org.neodos/libneodos:>=1.0.0` |
| `DEPO` | Yes | Optional dep | `org.neodos/net:>=2.0` |
| `CONF` | Yes | Conflict | `org.legacy/oldtool:>=0.5` |
| `REPL` | Yes | Replaces | `org.legacy/oldtool:<2.0` |
| `PROV` | Yes | Virtual provides | `org.neodos/shell` |
| `PREI` | No | Pre-install script path | `scripts/pre-install.sh` |
| `POSTI` | No | Post-install script path | `scripts/post-install.sh` |
| `PRER` | No | Pre-remove script path | `scripts/pre-remove.sh` |
| `POSTR` | No | Post-remove script path | `scripts/post-remove.sh` |

### 3.4 File Entry Table

| Offset | Size | Field | Notes |
| -------- | ------ | ------- | ------- |
| 0 | 4 | `path_offset` | Index into string table |
| 2 | 2 | `flags` | Bit 0=CORE, 1=CONFIG, 2=PATCH |
| 4 | 2 | _padding_ | Reserved (0) |
| 8 | 4 | `data_offset` | From file start |
| 12 | 4 | `data_size` | Uncompressed size |
| 14 | 2 | _padding_ | Reserved (0) |

Each file's CRC32 is stored in the local database after installation.

### 3.5 Signature block (optional)

| Offset | Size | Field | Description |
| -------- | ------ | ------- | ------------- |
| 0 | 4 | `signature_size` | Total block size including this field |
| 4 | 2 | `key_algorithm` | 0 = Ed25519 |
| 6 | 4 | `key_id` | Key identifier (CRC32 of public key) |
| 10 | 4 | `signature_offset` | Offset of raw signature within block |
| 14 | _ | Signature data | |

The signature covers: `header || manifest || file_table || string_table || file_data || footer` (everything before the signature block itself).

### 3.6 Footer

| Offset | Size | Field | Description |
| -------- | ------ | ------- | ------------- |
| 0 | 4 | `manifest_crc32` | |
| 4 | 4 | `file_table_crc32` | |
| 8 | 4 | `footer_crc32` | CRC32 of bytes 0..7 |

All CRC32: polynomial `0xEDB88320`, raw (no final XOR).

### 3.7 Compression

For v1, no compression. Individual files inside the package may be compressed by convention (e.g., `.nxe` is already compact). Future versions may add zstd compression at the archive level.

### 3.8 Design rationale

- **TLV manifest** instead of JSON/TOML: Zero-copy parsing, no allocator dependency in kernel verification. A kernel-side signature checker doesn't need a JSON parser.
- **No compression in v1:** Reduces implementation complexity. Zstd can be added as a flag bit later without breaking the format.
- **Separate signature block** at the end: Allows appending signatures without rewriting the entire file. Enables multiple signatures (co-signing).
- **CRC32 footer:** Independent of the signature. A quick integrity check can run without crypto. Useful for detecting corrupted downloads before signature verification.

---

## 4. Local Database

### 4.1 Design decision: Registry

The local database lives in `\Registry\Machine\Packages\`. Rationale:

| Criterion | Registry | Flat files | Binary DB | SQLite |
| ----------- | ---------- | ------------ | ----------- | -------- |
| Already exists | ✅ | No | No | No |
| Atomic transactions | ✅ (cells) | ❌ | ❌ | ✅ |
| Accessible via syscalls | ✅ (RAX 67-74) | ✅ (Ob) | ❌ | ❌ |
| Hierarchical | ✅ | ❌ | Partial | ❌ |
| ACL support | ✅ | ❌ | ❌ | ❌ |
| Remote queryable | ❌ (no network) | ❌ | ❌ | ❌ |
| No new kernel deps | ✅ | ✅ | ❌ | ❌ |

Registry is the natural choice: it's already there, it's hierarchical, it supports ACLs, and CORE protection can be enforced at the `cm_delete_key` syscall level.

### 4.2 Schema

```text
\Registry\Machine\Packages\
  <namespace>\
    <name>\
      Version        REG_SZ     "1.2.0"
      InstalledAt    REG_QWORD  1893456000000000
      Type           REG_SZ     "User" | "Core"
      InstallPath    REG_SZ     "C:\Programs\<name>"
      State          REG_DWORD  3    (0=downloaded,1=verified,2=installed,3=current)
      Description    REG_SZ     "..."
      Author         REG_SZ     "..."
      License        REG_SZ     "MIT"
      SignatureKeyID REG_DWORD  0xABCD1234

      Files\
        <relative_path>  REG_SZ  "crc32:0xA1B2C3D4"

      Dependencies\
        <dep_name>       REG_SZ  ">=1.0.0"

      Scripts\
        pre-install   REG_SZ  "scripts/pre-install.sh"
        post-install  REG_SZ  "scripts/post-install.sh"
```

**State values:**

| Value | Meaning |
| ------- | --------- |
| 0 | Downloaded (files cached, not installed) |
| 1 | Verified (signature + CRC32 checked) |
| 2 | Installed (files copied, Registry written) |
| 3 | Current (fully active — normal state) |
| 4 | Upgrading (mid-upgrade, old version) |
| 5 | Failed (transaction aborted, needs recovery) |

### 4.3 Transaction history

```text
\Registry\Machine\Packages\.History\
  <timestamp>\
    Type       REG_SZ  "install" | "remove" | "upgrade"
    Namespace  REG_SZ  "org.neodos"
    Name       REG_SZ  "NeoTop"
    Version    REG_SZ  "1.2.0"
    Status     REG_SZ  "completed" | "rolled_back"
    BackupPath REG_SZ  "C:\System\Recovery\20260704_120000\"
```

History is append-only. Used for `neoget history` and future rollback features.

### 4.4 Repository cache

```text
\Registry\Machine\Packages\.Repositories\
  <repo_name>\
    URL         REG_SZ  "https://packages.neodos.org/official"
    Fingerprint REG_BINARY  (32 bytes Ed25519 public key hash)
    Priority    REG_DWORD  100
    Enabled     REG_DWORD  1
    LastSync    REG_QWORD  1893456000000000
    IndexCRC    REG_DWORD  0x12345678
```

---

## 5. Object Manager Integration

### 5.1 Package as Object

Every installed package appears in the Ob namespace:

```text
\Package\
  org.neodos\
    NeoShell\        → PackageObject { name, version, ... }
    NeoTop\          → PackageObject { ... }
  com.github.user\
    FooBar\          → PackageObject { ... }
```

**Supported Ob operations on `\Package\<ns>\<name>`:**

| Operation | Behavior |
| ----------- | ---------- |
| `sys_ob_open(path, READ)` | Returns fd to package object |
| `sys_ob_query_info(fd, Basic, buf)` | Returns `ObPackageInfo` struct (version, state, flags) |
| `sys_ob_query_info(fd, Name, buf)` | Returns qualified name string |
| `sys_ob_enum(fd, buf)` | On a namespace dir: lists packages |
| `sys_ob_query_info(fd, File, buf)` → on `\Package\<name>` | Returns file count + total size |

### 5.2 Repository as Object

```text
\Repository\
  official\        → RepositoryObject { url, fingerprint, priority }
  myrepo\          → RepositoryObject { ... }
```

**Supported operations:**

| Operation | Behavior |
| ----------- | ---------- |
| `sys_ob_open(path, READ)` | Returns fd to repo object |
| `sys_ob_query_info(fd, Basic, buf)` | Returns `ObRepoInfo` (url, last sync, package count) |
| `sys_ob_enum(fd, buf)` | Lists packages available in the repository |

### 5.3 Transaction as Object

```text
\Transaction\
  <uuid>\          → TransactionObject { state, operations, ... }
```

**Supported operations:**

| Operation | Behavior |
|-----------|----------|
| `sys_ob_open(path, READ)` | Returns fd to transaction object |
| `sys_ob_query_info(fd, Basic, buf)` | Returns `ObTransactionInfo` (state, progress, operations) |

### 5.4 Advantages of Ob integration

1. **Uniform tooling.** `ls` on `\Package\` lists all installed packages with zero custom code.
2. **Standard ACLs.** Package visibility can be restricted per-user via SecurityDescriptors on the objects.
3. **Enumeration.** `sys_ob_enum(\Package\, buf)` replaces listing the Registry — same API as files, processes, drivers.
4. **No new syscalls.** Everything uses existing RAX 60-66. The kernel only needs new `ObType` constants.
5. **Event bus integration.** Changes to `\Package\` objects emit EventBus events, enabling GUI updates and auto-refresh.

### 5.5 New ObTypes required

| Type | Value | Description |
| ------ | ------- | ------------- |
| `ObType::Package` | 19 | Installed package object |
| `ObType::Repository` | 20 | Configured repository object |
| `ObType::Transaction` | 21 | Active transaction object |

These are userland-facing types. No kernel code changes needed beyond defining the constants — the objects live in userland and are served by `libneopkg`'s Ob namespace server via the existing syscall interface.

### 5.6 Implementation pattern

```rust
// Pseudocode — libneopkg registers these in the Ob namespace
ob_namespace_register(
    "\Package",
    NamespaceProvider {
        open: |path| handle_package_open(path),
        enum: |path| handle_package_enum(path),
        query_info: |fd, class| handle_package_query_info(fd, class),
    },
);
```

The namespace provider reads from the Registry and materializes Ob objects on-the-fly. This is the same pattern used by `\Global\Info\*` and `\Process\*`.

---

## 6. Component Architecture

### 6.1 Module map

```text
┌─────────────────────────────────────────────────────────┐
│                    neoget (CLI)                          │
│  userbin/neoget/src/main.rs + cmd_*.rs                  │
│  Parses args, calls libneopkg API, formats output       │
└───────────────────────┬─────────────────────────────────┘
                        │ depends on
┌───────────────────────▼─────────────────────────────────┐
│              libneopkg (Engine Library)                  │
│  libneopkg/src/                                          │
│                                                          │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────────┐  │
│  │ Resolver │ │  Parser  │ │ Verifier │ │ Downloader │  │
│  │  deps.rs │ │ parse.rs │ │ verify.rs│ │ download.rs│  │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └─────┬──────┘  │
│       └────────────┼────────────┼──────────────┘         │
│                    ▼            ▼                        │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────────┐  │
│  │ Database │ │  Engine  │ │  Ob NS   │ │ Recovery   │  │
│  │  db.rs   │ │engine.rs │ │ obns.rs  │ │ recovery.rs│  │
│  └──────────┘ └──────────┘ └──────────┘ └────────────┘  │
│                                                          │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐                 │
│  │ RepoMgr  │ │  Config  │ │  Crypt   │                 │
│  │  repo.rs │ │config.rs │ │ crypt.rs │                 │
│  └──────────┘ └──────────┘ └──────────┘                 │
└─────────────────────────────────────────────────────────┘
```

### 6.2 Component responsibilities

| Component | File | Responsibility |
| ----------- | ------ | --------------- |
| **Engine** | `engine.rs` | Orchestrates transactions: resolve → download → verify → install → db update |
| **Parser** | `parse.rs` | Reads `.nxp` files: validate magic, CRC32, parse manifest + file table |
| **Resolver** | `deps.rs` | Given a package name + version req, finds all transitive deps and checks conflicts |
| **Verifier** | `verify.rs` | Validates CRC32, Ed25519 signatures, key chain of trust |
| **Downloader** | `download.rs` | Fetches `.nxp` files from repositories; supports `file://` and `https://` |
| **Database** | `db.rs` | Reads/writes `\Registry\Machine\Packages\` via Cm syscalls |
| **Repositories** | `repo.rs` | Manages repo list (add/remove/sync), fetches and caches `.nxi` indexes |
| **Ob Namespace** | `obns.rs` | Serves `\Package\`, `\Repository\`, `\Transaction\` in the Ob namespace |
| **Recovery** | `recovery.rs` | Manages backup/restore of files during transactions; rollback on failure |
| **Config** | `config.rs` | Default paths, repository URLs, keyring paths |
| **Crypto** | `crypt.rs` | Ed25519 verify, CRC32, key fingerprinting |

### 6.3 Data flow: installation

```text
1. neoget INSTALL NeoTop
       │
       ▼
2. Resolver.resolve("org.neodos/NeoTop", ">=1.0")
       │  Reads Registry for installed versions
       │  Computes transitive deps + order
       ▼
3. [For each needed package]
       │
       ├─ Downloader.fetch(repo_url, "org.neodos/NeoTop-1.2.0.nxp")
       │      ▼
       │  Parser.validate(file)  →  magic + CRC32
       │      ▼
       │  Verifier.verify(file, repo_fingerprint)  →  Ed25519 check
       │      ▼
       │  Verifier.crc32_check(file, Registry.Files.*)
       │
       ▼
4. Engine.begin_transaction()
       │  Copies current files to C:\System\Recovery\<uuid>\
       │  Prepares Registry backup (cm_flush_key)
       ▼
5. [For each file in nxp]
       │  Installer.install(file_entry, target_path)
       │     → sys_ob_create(FILE)
       │     → sys_ob_set_info(WriteContent, blob)
       │     → if CORE: sys_ob_set_info(FileFlags, MODE_CORE)
       ▼
6. Database.record_installation(...)
       │  Writes Registry keys under \Packages\<ns>\<name>\
       ▼
7. Engine.commit_transaction()
       │  Deletes backup from C:\System\Recovery\<uuid>\
       │  Sets State=3 (Current)
       ▼
8. Engine.signal_completion()
       │  EventBus event: PackageInstalled{name, version}
       │  Ob namespace refresh
```

### 6.4 Error recovery

Every step has a corresponding rollback action:

| Step failure | Rollback |
| ------------- | ---------- |
| Download CRC32 mismatch | Delete partial download, retry |
| Signature invalid | Delete downloaded file, report error |
| File write fails | Restore originals from Recovery, delete Registry keys |
| Registry write fails | Restore originals, delete files written so far |
| Script fails | Roll back entire transaction, restore Recovery |

---

## 7. Dependency Resolution

### 7.1 Version syntax

Semver 2.0 subset, as used in Cargo:

| Expression | Matches | Example |
| ----------- | --------- | --------- |
| `^1.2.3` | Compatible: `>=1.2.3, <2.0.0` | Default for deps |
| `~1.2.3` | Patch: `>=1.2.3, <1.3.0` | |
| `>=1.2.3` | Minimum | |
| `>1.2.3` | Strict | |
| `<2.0.0` | Maximum | |
| `=1.2.3` | Exact | |
| `*` | Any | |
| `>=1.0, <2.0` | Range | |

### 7.2 Constraint types

| Type | Manifest tag | Semantics |
| ------ | ------------- | ----------- |
| **Required** | `DEP` | Must be installed. Resolver fails if unsatisfied. |
| **Optional** | `DEPO` | If present in repo, install it; if not, continue. |
| **Conflict** | `CONF` | Must NOT be installed. Resolver fails if present. |
| **Replaces** | `REPL` | If installed, remove in favor of this package. |
| **Provides** | `PROV` | Virtual package: satisfies deps of other packages. |

### 7.3 Resolution algorithm (v1)

For v1, a simple backtracking resolver — sufficient for hundreds of packages:

```text
function resolve(target, constraints):
    solution = {}
    queue = [(target, constraints)]

    while queue not empty:
        (pkg, req) = queue.pop_front()

        if pkg in solution:
            if not satisfies(solution[pkg], req):
                error("Conflict: {pkg} {solution[pkg]} doesn't satisfy {req}")
            continue

        candidates = repo.query(pkg, req)
        if candidates empty:
            error("No candidate for {pkg} {req}")

        best = candidates[0]  // latest matching version
        solution[pkg] = best.version

        for dep in best.deps:
            queue.push((dep.name, dep.req))

        for conflict in best.conflicts:
            if conflict.name in solution:
                error("Conflict: {best.name} conflicts with installed {conflict}")

        for replace in best.replaces:
            if replace.name in solution or db.is_installed(replace.name):
                queue.push_front((replace.name, "<0.0.0"))  // force remove

    return solution
```

### 7.4 Future: SAT solver

For v2 and beyond, a Cargo-style SAT solver (based on `pubgrub`) can replace the simple algorithm. The architecture isolates the resolver behind a `Resolver` trait, making this a drop-in replacement.

```rust
pub trait Resolver {
    fn resolve(
        &self,
        targets: &[(PackageName, VersionReq)],
        installed: &[(PackageName, Version)],
        repo: &dyn Repository,
    ) -> Result<ResolveGraph, ResolveError>;
}
```

---

## 8. Repositories

### 8.1 Repository model

A repository is a URL + public key fingerprint. Three types:

| Type | URL scheme | Authentication | Caching |
| ------ | ----------- | --------------- | --------- |
| Official | `https://packages.neodos.org/official` | Repository index signed by NeoDOS root key | Full index + package cache |
| Third-party | `https://mirror.example.com/neodos` | Repository index signed by third-party key | Configurable |
| Local | `file:///C:\Packages\` | No network; packages verified individually | No |

### 8.2 Index format (.nxi)

The repository index is a signed file listing all available packages:

```text
┌──────────────────────────────────────────────┐
│ Magic "NXI1" (4 bytes)                        │
├──────────────────────────────────────────────┤
│ Header (16 bytes)                             │
│   index_crc32 | entry_count | ts_last_updated│
├──────────────────────────────────────────────┤
│ Entry Table (64 bytes × N)                    │
│   name(48) | ver(16) | dep_count | ...        │
│   checksum | size | path_in_repo              │
├──────────────────────────────────────────────┤
│ Signature Block (Ed25519)                     │
│   key_id | signature                          │
└──────────────────────────────────────────────┘
```

**Entry fields (64 bytes):**

| Offset | Size | Field |
| -------- | ------ | ------- |
| 0 | 48 | Qualified name (padded with 0) |
| 48 | 16 | Version string (null-terminated) |
| 64 | 4 | File size in bytes |
| 68 | 4 | CRC32 of the `.nxp` |
| 72 | 4 | Dependencies count |
| 76 | 4 | Offset to dep list in entry footer |
| 80 | _ | (repeated per entry) |

### 8.3 Repository sync

```text
neoget repo sync official

1. Download https://packages.neodos.org/official/index.nxi
2. Validate header CRC32
3. Verify Ed25519 signature against stored fingerprint
4. Compare LastUpdated with local cache
5. If newer: replace local index cache at C:\Cache\Packages\<repo>\index.nxi
6. Update Registry: \.Repositories\<name>\LastSync, IndexCRC
```

### 8.4 Repository commands (future)

```text
neoget repo add myrepo https://myrepo.example.com/neodos/
neoget repo remove myrepo
neoget repo list
neoget repo sync [name]
neoget repo priority myrepo 50
```

---

## 9. Transactions

### 9.1 Transaction lifecycle

```text
Begin ──> Prepare ──> Commit ──> Complete
            │
            └──> Rollback ──> Cleanup
```

### 9.2 Transaction object

Every mutation creates a Transaction Object at `\Transaction\<uuid>\`:

```rust
struct Transaction {
    id: Uuid,
    state: TransactionState,
    operations: Vec<Operation>,
    recovery_path: String,  // C:\System\Recovery\<uuid>\
    started_at: u64,
    completed_at: Option<u64>,
}

enum TransactionState {
    Prepared,    // Dependencies resolved, files downloaded, verified
    Committing,  // Writing files + Registry (atomic?)
    Completed,   // Done
    RollingBack, // Recovering
    RolledBack,  // Recovery complete
    Failed,      // Unrecoverable
}

enum Operation {
    Install { name, version, files: Vec<FileOp> },
    Remove { name, version, files: Vec<FileOp> },
    Upgrade { name, from_ver, to_ver, files: Vec<DiffOp> },
}
```

### 9.3 Recovery mechanism

```text
C:\System\Recovery\
  <uuid>\
    manifest.txt       ← list of backed-up files with original paths
    files\             ← copies of originals (before modification)
      neoshell.nxe
      config.toml
    registry.reg       ← cm_flush_key dump for affected keys
```

On reboot after crash:

1. Scan `C:\System\Recovery\` for orphaned transactions
2. For each: if `manifest.txt` exists → restore files from `files/` → delete directory
3. Log: `"Recovery: rolled back transaction <uuid>"`

### 9.4 Transaction isolation

- While a transaction is active, `neoget list` shows the post-transaction state (read-your-writes)
- Other processes see the pre-transaction state until commit
- After commit, all processes see the new state (EventBus notification)

---

## 10. Security

### 10.1 Trust model

```text
Root of trust (embedded in neoget binary)
    │
    ├── Official NeoDOS root public key (Ed25519)
    │       │
    │       ├── signs: official repository index (.nxi)
    │       │       │
    │       │       └── each .nxi entry includes: package_name, version, crc32
    │       │
    │       └── signs: individual packages (.nxp) at build time
    │
    └── Third-party keys (imported by user)
            │
            └── signs: third-party repository index
```

### 10.2 Verification steps

| Stage | What is verified | How |
| ------- | ----------------- | ----- |
| Index download | Repository index authenticity | Ed25519 signature against known fingerprint |
| Package download | Package integrity | CRC32 matches index entry |
| Package verification | Package authenticity | Ed25519 signature against package signer |
| Before install | Package hash matches index | CRC32 re-check against Repository index |
| After install | Files on disk match expected | CRC32 stored in Registry, verified on `neoget verify` |
| Periodic | Installed files integrity | Full scan via `neoget verify --all` |

### 10.3 Key management

```rust
struct Keyring {
    /// Embedded root key (read-only, from binary)
    root_key: [u8; 32],

    /// User-imported repository keys
    /// Stored in Registry: \Registry\Machine\Packages\.Keyring\
    repo_keys: HashMap<String, [u8; 32]>,
}
```

- Root key is embedded at compile time in `neoget.nxe`
- Repository keys are imported via `neoget repo add --fingerprint <hex>`
- Keys are stored in Registry under `\.Keyring\`

### 10.4 Rollback protection

- Before any mutation, a snapshot is taken (`C:\System\Recovery\<uuid>\`)
- Transaction log is append-only; entries cannot be deleted (only marked completed)
- Registry: `\.History\` entries are write-once

---

## 11. CLI Design

### 11.1 Command structure

```text
neoget <command> [options] [arguments]
```

Global options:

| Flag | Description |
| ------ | ------------- |
| `-y, --yes` | Assume yes to all prompts |
| `-q, --quiet` | Only errors |
| `-v, --verbose` | Detailed output |
| `--dry-run` | Show what would happen without doing it |

### 11.2 Command reference

| Command | Alias | Description | Example |
| --------- | ------- | ------------- | --------- |
| `install` | `in` | Install package(s) | `neoget install NeoTop` |
| `remove` | `rm` | Remove package(s) | `neoget remove NeoTop` |
| `upgrade` | `up` | Upgrade all upgradable packages | `neoget upgrade` |
| `update` | `ud` | Sync repository indexes | `neoget update` |
| `search` | `se` | Search packages in repos | `neoget search editor` |
| `list` | `ls` | List installed packages | `neoget list --core` |
| `info` | `show` | Show package details | `neoget info NeoTop` |
| `verify` | `check` | Verify installed files integrity | `neoget verify --all` |
| `files` | `fl` | List files belonging to a package | `neoget files NeoTop` |
| `depends` | `dep` | Show dependency tree | `neoget depends NeoTop --tree` |
| `repo` | - | Manage repositories | `neoget repo list` |
| `cache` | - | Manage local cache | `neoget cache clean` |
| `history` | `log` | Show transaction history | `neoget history` |
| `doctor` | - | Diagnose package issues | `neoget doctor` |
| `clean` | - | Clean cache + temp files | `neoget clean` |

### 11.3 Output design

```text
$ neoget install NeoTop

Reading package lists... done
Resolving dependencies...

The following packages will be INSTALLED:
  org.neodos/NeoTop v1.2.0  (process monitor)
  org.neodos/libneodos v1.5.0  (system library)

Download: 2 files (1.2 MB) ━━━━━━━━━━━━━━━━━━━━ 100% 2s
Verify: ✅ signatures OK, CRC32 OK
Install: ━━━━━━━━━━━━━━━━━━━━ 100%
Register: ✅

Done. NeoTop v1.2.0 installed.
```

### 11.4 Error output

```text
$ neoget install NeoShell

ERROR: Cannot install NeoShell v2.0.0:
  ✗ Missing dependency: org.neodos/libnet >=2.0 (not installed)
  ✗ Conflict: org.legacy/old-shell >=1.0 is installed

Use --dry-run to preview before installing.
```

---

## 12. Internal API

### 12.1 Library API (libneopkg)

```rust
// libneopkg/src/lib.rs — public API surface

/// Initialize the package engine (opens Registry, loads config)
pub fn init(config: Config) -> Result<Engine, Error>;

pub struct Engine { /* ... */ }

impl Engine {
    // ── Query ────────────────────────────────────────
    pub fn list_packages(&self, filter: PackageFilter) -> Result<Vec<PackageInfo>>;
    pub fn get_package(&self, name: &QualifiedName) -> Result<PackageInfo>;
    pub fn get_package_files(&self, name: &QualifiedName) -> Result<Vec<FileInfo>>;
    pub fn search_packages(&self, query: &str, repos: &[RepoName]) -> Result<Vec<PackageInfo>>;

    // ── Repositories ─────────────────────────────────
    pub fn list_repositories(&self) -> Result<Vec<RepoInfo>>;
    pub fn add_repository(&mut self, repo: RepoConfig) -> Result<()>;
    pub fn remove_repository(&mut self, name: &RepoName) -> Result<()>;
    pub fn sync_repository(&mut self, name: &RepoName) -> Result<()>;

    // ── Transactions ─────────────────────────────────
    pub fn install(&mut self, packages: &[PackageReq], flags: InstallFlags) -> Result<TransactionReport>;
    pub fn remove(&mut self, packages: &[QualifiedName], flags: RemoveFlags) -> Result<TransactionReport>;
    pub fn upgrade(&mut self, flags: UpgradeFlags) -> Result<TransactionReport>;
    pub fn dry_run_install(&self, packages: &[PackageReq]) -> Result<TransactionPlan>;
    pub fn dry_run_upgrade(&self) -> Result<TransactionPlan>;

    // ── Verification ─────────────────────────────────
    pub fn verify_package(&self, name: &QualifiedName) -> Result<VerificationReport>;
    pub fn verify_all(&self) -> Result<Vec<VerificationReport>>;

    // ── History ──────────────────────────────────────
    pub fn get_history(&self, limit: usize) -> Result<Vec<HistoryEntry>>;

    // ── Maintenance ──────────────────────────────────
    pub fn clean_cache(&mut self) -> Result<()>;
    pub fn doctor(&self) -> Result<Vec<Diagnostic>>;
}
```

### 12.2 Error model

```rust
pub enum Error {
    // — Resolution errors —
    PackageNotFound(QualifiedName),
    VersionNotFound(QualifiedName, VersionReq),
    DependencyFailed(QualifiedName, VersionReq, Vec<QualifiedName>),
    Conflict(QualifiedName, Version, QualifiedName, Version),

    // — Repository errors —
    RepoNotFound(RepoName),
    RepoUnreachable(RepoName, String),
    RepoIndexInvalid(RepoName, String),
    RepoSignatureInvalid(RepoName),

    // — Package errors —
    PackageCorrupt(QualifiedName, String),
    PackageSignatureInvalid(QualifiedName),
    PackageIntegrity(QualifiedName, String, Expected, Actual),

    // — Database errors —
    DatabaseCorrupt(String),
    RegistryError(i64),

    // — Transaction errors —
    TransactionFailed { id: Uuid, phase: String, reason: String },
    AlreadyInTransaction,

    // — Permission errors —
    AccessDenied(String),
    AdminRequired(String),

    // — IO errors —
    Io(String),
    DownloadFailed(String),
    InsufficientSpace { needed: u64, available: u64 },

    // — Internal —
    Internal(String),
}
```

### 12.3 Events

```rust
pub enum Event {
    DownloadProgress { package: QualifiedName, current: u64, total: u64 },
    VerifyStart { package: QualifiedName },
    VerifyResult { package: QualifiedName, passed: bool },
    TransactionBegin { id: Uuid },
    TransactionCommit { id: Uuid },
    TransactionRollback { id: Uuid, reason: String },
    PackageInstalled { name: QualifiedName, version: Version },
    PackageRemoved { name: QualifiedName },
    PackageUpgraded { name: QualifiedName, from: Version, to: Version },
    Error { message: String },
}
```

### 12.4 Integration with shell scripts

```bash
# neoget returns 0 on success, non-zero on error
# All errors go to stderr, normal output to stdout
# Machine-readable mode:
neoget list --json
neoget info NeoTop --json
neoget verify --all --json
```

---

## 13. Future Roadmap

### v1 (current design) — Core, minimum viable

| Feature | Scope |
| --------- | ------- |
| `.nxp` format + parser | Complete |
| `INSTALL`, `REMOVE`, `LIST`, `INFO` | Functional |
| `DEPENDS` (simple resolver) | Linear resolution |
| CRC32 integrity verification | Per-file |
| Registry-backed database | Read/write via Cm syscalls |
| CORE protection | Kernel MODE_CORE + cm_delete_key guard |
| Recovery (basic) | File backup before mutation |
| `file://` repository support | Local packages |

### v2 — Repositories + dependencies

| Feature | Scope |
| --------- | ------- |
| Repository index format (`.nxi`) | Complete |
| `UPDATE`, `UPGRADE`, `SEARCH` | Functional |
| `https://` downloader | TLS via host (no kernel TLS) |
| Ed25519 signature verification | Package + index |
| `repo add/remove/list/sync` | Functional |
| Dependency resolution (full) | Backtracking, transitive, conflicts |
| Virtual packages (`PROV`) | Basic support |
| Subcommand: `VERIFY`, `FILES`, `HISTORY` | Complete |

### v3 — Safety + performance

| Feature | Scope |
| --------- | ------- |
| SAT/backtracking resolver | pubgrub-style |
| Transaction objects in Ob namespace | `\Transaction\<uuid>\` |
| Repository objects in Ob namespace | `\Repository\` |
| Parallel downloads | Concurrent fetch |
| Delta packages | Binary diff for upgrades |
| Package caching | `C:\Cache\Packages\` with LRU |

### v4 — Enterprise

| Feature | Scope |
| --------- | ------- |
| Rollback to specific version | Full history traversal |
| Snapshots before upgrades | Full filesystem + Registry snapshot |
| Automatic background updates | NeoGet service (Ring 3 daemon) |
| Private repos with auth | HTTPS basic auth + token |
| Offline installation from cache | `neoget install --offline` |
| Package build tool (`neobuild`) | Create `.nxp` from spec |
| GUI frontend | GTK-like or console TUI |

### v5 — Ecosystem

| Feature | Scope |
| --------- | ------- |
| Official package repository | Hosted infrastructure |
| CI/CD integration | Automated build + sign pipeline |
| Dependency audit | CVE scanning |
| Containers/sandboxing | Per-package namespace isolation |
| Nix-style reproducibility | Hash-addressable store |

---

## 14. Comparison with Existing Systems

### 14.1 APT (Debian)

| Feature | APT | NeoGet | Take? |
| --------- | ----- | -------- | ------- |
| Package format | `.deb` (ar archive) | `.nxp` (custom binary) | Custom — simpler parser |
| Database | `/var/lib/dpkg/` (flat files) | Registry | ✅ NeoDOS-native |
| Dependency resolution | `apt-get install` SAT (apt-pkg) | Backtracking v1, SAT v2 | ✅ SAT in v2 |
| Repositories | `sources.list`, GPG-signed Release | `.nxi` index, Ed25519 | ✅ Simpler crypto |
| Triggers | dpkg triggers | Scripts (pre/post) | ✅ Leaner |
| Rollback | Manual (dpkg --force) | Built-in recovery | ✅ Better |

**Adopt:** `sources.list`-style repo configuration. Dependency marking (auto/manual). Keep: pre/post scripts.

### 14.2 Pacman (Arch)

| Feature | Pacman | NeoGet | Take? |
| --------- | -------- | -------- | ------- |
| Package format | `.pkg.tar.zst` | `.nxp` | Custom |
| Database | `/var/lib/pacman/local/` (flat files) | Registry | ✅ |
| Sync | `pacman -Sy` | `neoget update` | ✅ Same model |
| No dependency auto-remove | Manual `pacman -Rsc` | Full dep tracking | ✅ Better |

**Adopt:** Rolling-release model for dependencies. Package `groups` concept for `neoget install --as-group`.

### 14.3 DNF (Fedora/RHEL)

| Feature | DNF | NeoGet | Take? |
| --------- | ----- | -------- | ------- |
| Resolver | libsolv (SAT) | Custom | SAT in v2 |
| Transactions | `history` + undo | Built-in recovery | ✅ |
| Modules | Modularity streams | Namespace-based | ✅ Via namespaces |
| Delta RPM | drpm | Delta in v3 | ✅ |

**Adopt:** Transaction history with undo. `neoget history undo <id>` in v3.

### 14.4 Winget (Windows)

| Feature | Winget | NeoGet | Take? |
| --------- | -------- | -------- | ------- |
| Manifest | YAML | TLV binary | ✅ Simpler |
| Sources | REST API | `.nxi` index | ✅ Similar |
| Install scope | Machine/User | CORE/User | ✅ Same concept |
| Pin versions | `winget pin` | `neoget hold` in v2 | ✅ |

**Adopt:** `neoget pin <package>` to block upgrades. No YAML — TLV is better for embedded.

### 14.5 Cargo (Rust)

| Feature | Cargo | NeoGet | Take? |
| --------- | ------- | -------- | ------- |
| Resolver | pubgrub (SAT) | pubgrub in v2 | ✅ SAT algorithm |
| Registry | crates.io | `.nxi` index | ✅ Index format |
| Semver | ^1.0.0 by default | ^1.0.0 by default | ✅ Same |
| Workspaces | Multi-crate | Namespace grouping | ✅ Via namespaces |
| Lockfile | Cargo.lock | Not in v1 (v2: `.nxlock`) | ✅ |

**Adopt:** pubgrub resolver. Default `^` semver. Lockfile for reproducible installs.

### 14.6 Homebrew

| Feature | Homebrew | NeoGet | Take? |
| --------- | ---------- | -------- | ------- |
| Formula | Ruby DSL | TLV manifest | Custom |
| Cellar | `/usr/local/Cellar/` | `C:\Programs\<name>\` | ✅ Isolated per-package |
| Keg-only | No symlink | Namespace-based | ✅ |
| Casks | GUI apps | Same format | ✅ |

**Adopt:** Per-package directory isolation (already designed). Formula → not needed with TLV manifest.

### 14.7 Nix

| Feature | Nix | NeoGet | Take? |
| --------- | ----- | -------- | ------- |
| Store | `/nix/store/<hash>-<name>-<ver>` | Not in v1 | Future |
| Reproducible | Hash-addressable | v5 | Future |
| Profiles | Generations | Snapshots v4 | Future |
| No FHS | /nix/store only | FHS + Packages Ob | Hybrid |

**Adopt:** Content-addressable store in v5. Rollback via generations. Too complex for v1.

### 14.8 Summary: what NeoGet takes from each

| From | Concept |
| ------ | --------- |
| **APT** | Repository model, dependency auto-install |
| **Pacman** | Simplicity, rolling dep model |
| **DNF** | Transaction history with undo |
| **Winget** | Install scope (machine/user), pinning |
| **Cargo** | SAT resolver (pubgrub), semver with `^` |
| **Homebrew** | Per-package directory isolation |
| **Nix** | Long-term: content-addressable store + generations |

---

## Appendix A: File paths

| Path | Purpose |
| ------ | --------- |
| `userbin/neoget/` | CLI binary source |
| `userbin/neoget/Cargo.toml` | Depends on `libneopkg` |
| `userbin/neoget/src/main.rs` | Entry point, arg parsing, dispatch |
| `userbin/neoget/src/cmd_*.rs` | One file per subcommand |
| `libneopkg/` | Engine library |
| `libneopkg/src/lib.rs` | Public API + re-exports |
| `libneopkg/src/engine.rs` | Transaction orchestrator |
| `libneopkg/src/parse.rs` | `.nxp` parser |
| `libneopkg/src/deps.rs` | Dependency resolver |
| `libneopkg/src/verify.rs` | CRC32 + Ed25519 verify |
| `libneopkg/src/download.rs` | HTTP/file downloader |
| `libneopkg/src/db.rs` | Registry read/write |
| `libneopkg/src/repo.rs` | Repository management |
| `libneopkg/src/obns.rs` | Ob namespace provider |
| `libneopkg/src/recovery.rs` | Backup/restore for transactions |
| `libneopkg/src/crypt.rs` | Ed25519 + CRC32 primitives |
| `libneopkg/src/config.rs` | Default paths, keys |
| `src/fs/neodos_dir.rs` | `MODE_DIR = 0x40`, `MODE_FILE = 0x80` kernel consts |
| `src/syscall/ob.rs` | CORE guard in `ob_destroy` |
| `src/syscall/cm.rs` | CORE guard in `cm_delete_key` |

## Appendix B: Registry keys summary

```text
\Registry\Machine\Packages\
  .History\<ts>\
  .Repositories\<name>\
  .Keyring\<fingerprint>\
  <namespace>\
    <name>\
      Version, InstalledAt, Type, InstallPath, State, ...
      Files\<relpath>
      Dependencies\<dep>
      Scripts\<type>
```

## Appendix C: Object types summary

| ObType | Value | Name | Description |
| -------- | ------- | ------ | ------------- |
| `ObType::Package` | 19 | Package | Installed package |
| `ObType::Repository` | 20 | Repository | Package source |
| `ObType::Transaction` | 21 | Transaction | Active operation |

---

> **Next:** Implementation starts with Phase 1 (Foundation): libneopkg parser, libneodos CM wrappers, kernel MODE_CORE guards. See `docs/packages.md` §10 for step-by-step roadmap.
