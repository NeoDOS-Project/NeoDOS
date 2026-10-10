# NXP Packages, MSI Installation and Package Management — Design Document

> **Version:** v0.1-draft
> **Status:** Design (no implementation). Extends
> `docs/design/system-configuration-time-network-security-update-design.md`.
> **Target Release:** v0.59 (Installation & Bootstrap) for foundations; later for
> repositories/upgrades
> **Related:** #411 (package manager), #105 (INSTALL-PACKAGES), #103 (install.nxe),
> #100 (TOOL-NXE), #147/#149/#586 (closed), NXE-ECO-12..15, INSTALL-*;
> `docs/architecture/package-manager-arch.md`, `docs/userland/nxp-format.md`,
> `docs/userland/packages.md`, `docs/userland/nxe-ecosystem.md`;
> `docs/design/libupdate-neoupdate-design.md`,
> `docs/design/libtls-https-design.md`,
> `docs/design/http-tls-update-prerequisites-design.md`
> **ABI Impact:** none required for M1; the documented Ob integration
> (`\Package`, `\Repository`, `\Transaction`) needs an ObType reconciliation
> (see §16.3)

---

## 16.1 Repository audit

### 16.1.1 `.nxp` — package format

| Aspect | State | Evidence |
| --- | --- | --- |
| Specification | **Designed (complete)** | `docs/userland/nxp-format.md`; `docs/architecture/package-manager-arch.md` §3 (header 32 B, TLV manifest, file entries, string pool, signature block, footer) |
| Host tool | **Partial (implemented subset)** | `neotools/nxpkg/src/main.rs` (519 lines): `create/extract/list/info/verify`; magic `NXP1`; TLV tags only `NAME`, `VER`, `DESC`, `ARCH`; per-file CRC32 |
| Signature block | **Designed, not implemented** | `nxpkg` writes `flags=0`, `signature_offset=0`; `nxp-format.md` reserves it; `package-manager-arch.md` §3.5 defines it |
| Compression | **Designed, not implemented** | flag bit `COMPRESSED` reserved; §3.7 says "no compression in v1" |
| Dependency/conflict tags | **Designed, not implemented** | §3.3 defines `DEP/DEPO/CONF/REPL/PROV`; `nxpkg` emits none |
| Runtime parser/installer | **Absent** | no `libneopkg`, no `neoget`; `nxverify.nxe` only checks the 4-byte magic and prints "CRC OK" **without computing CRC32** |
| In-image packages | **Absent** | `neodev/src/image.rs` ships individual `.nxe`/`.nxl`, not `.nxp` |

### 16.1.2 `.msi` — installer format

| Aspect | State | Evidence |
| --- | --- | --- |
| NeoDOS `.msi` installer format | **Absent** | repo-wide grep for `.msi`/installer finds only **Message Signaled Interrupts** (`neodos-kernel/src/interrupts/msi.rs`, `idt`, `nvme`) |
| Microsoft MSI compatibility | **Not defined anywhere** | no `.msi` binary format code, no Windows Installer semantics |
| OS installer | **Designed** | #103 `install.nxe`; `roadmap/improvements.md` INSTALL-*; not implemented |
| Conclusion | `.msi` is a **greenfield proposal**; it must be defined by this design, not assumed | — |

### 16.1.3 Deployment, layout, Registry

| Aspect | State | Evidence |
| --- | --- | --- |
| Build-time deployment | Implemented | `neodev/src/image.rs:500-579`: `.nxe` → `/Programs` or `/System/Tools`; `.nxl` → `/System/Libraries` (hardcoded 4-entry map) |
| Runtime installation | Absent | no installer/`neoget` binary in the image; no install command |
| Install layout (design) | Documented | `C:\Programs\<name>\`, `C:\Packages\<name>.nxp` (`nxe-ecosystem.md` §8.1) |
| Registry DB (design) | Documented | `\Registry\Machine\Packages\<ns>\<name>\{Version,InstallPath,State,Files\...}` (`package-manager-arch.md` §4.2) |
| Ob namespace (design) | Documented | `\Package\`, `\Repository\`, `\Transaction\`; requires `ObType::Package/Repository/Transaction` (§5.5) |

### 16.1.4 Integrity, signatures, trusted keys

| Aspect | State | Evidence |
| --- | --- | --- |
| Ed25519 | Implemented (NLT only) | `libnlt/src/signature.rs`, `libneodos/src/i18n.rs` |
| NXP signature verification | Absent | `nxpkg` emits none; no runtime verifier |
| Trusted-key store | Absent | design only (`\.Keyring\`) |
| Hashes | CRC32 only | `nxpkg`, kernel `fs/crc32.rs`; **CRC32 is not cryptographic** |
| SHA-256 | Absent | (see the TLS doc) |

### 16.1.5 Filesystem guarantees (for install transactions)

| Primitive | State | Evidence |
| --- | --- | --- |
| `fsync`/sync | **Absent** | `fs/vfs/mod.rs` trait has no `sync` |
| Atomic replace | **Absent / unproven** | `Vfs::rename` same-directory, delete+insert; `NeoDosFsV2::write` no `save_sb` |
| Journaling | Absent (COW instead) | `docs/filesystem/neofs-v2.md` |
| Recovery | Manual FSCK only | `fs/fsck/*`; no boot-time auto-recovery |

**Prerequisites (from `docs/design/http-tls-update-prerequisites-design.md`):**
`FileSync=53`, `FileReplace=54`, and committing the superblock on write.

### 16.1.6 Existing mechanisms to reuse

- **Registry (Cm)** as the package DB and key store (do not invent a store).
- **NXP container** already documented and partially produced by `nxpkg`.
- **Ed25519** (extract to `libcrypto`).
- **`libhttp` + `libtls`** for repositories (later).
- **`libupdate`/`neoupdate`** for update flow (must delegate installation).
- **NeoDev image builder** for provisioning defaults and the root key.

---

## 16.2 Responsibilities of each format

### 16.2.1 `.nxp` — package format (recommended: keep and complete)

`.nxp` is the **unit of software**: a self-contained container holding a manifest,
a file table, the payload, and an optional signature block. Recommendation:

- **Keep the documented NXP container** (`magic "NXP1"`, 32-byte header, TLV
  manifest, file entry table, string pool, raw data, optional signature, footer).
- **Complete the manifest tag set** already designed in
  `package-manager-arch.md` §3.3: identity/format version (`NAME`, `VER`, `ARCH`,
  format version in the header), compatibility (`CORE`, `BOOT`, `NOKR`, min OS),
  dependencies/conflicts (`DEP`, `DEPO`, `CONF`, `REPL`, `PROV`), install metadata
  (`PREI`, `POSTI`, `PRER`, `POSTR`), and file hashes.
- **Add the signature block** (§3.5) and a real hash: keep CRC32 as a fast
  corruption check, but add **SHA-256** per file and over the whole payload for
  authenticity. CRC32 MUST NOT be treated as a security mechanism.
- **Path model:** relative POSIX-style paths inside a package root; destination
  paths derived by the installer from file flags (`EXEC`, `CONFIG`, `LOCALE`) and
  the install root, never taken verbatim from the archive.

Deterministic path validation (mandatory):

- Reject absolute paths, drive letters, `..` components, empty components, and
  backslashes outside the normalized form.
- Reject symlinks/hardlinks/reparse-like entries (NXP has no such concept; the
  format MUST NOT gain one without an explicit decision).
- Reject duplicate normalized paths and case-insensitive collisions.
- Enforce limits: metadata bytes, entry count, per-file size, total extracted
  size, and nesting depth; refuse decompression bombs (ratio cap).
- Normalize to a canonical relative path and re-verify after normalization.

### 16.2.2 `.msi` — installation format and experience (recommended: bundle)

The NeoDOS `.msi` is **not** Microsoft's MSI and has **no binary compatibility
with it**. Recommended model (option 2, with a scriptable description):

- An **installation bundle / install description** that references or embeds one
  or more `.nxp` packages plus an ordered install plan and initial configuration.
- It carries: a bundle manifest (identity, target, minimum OS), a list of
  embedded/referenced `.nxp` artifacts (with SHA-256 + signature), an install
  order/selection, optional configuration payloads, and a signature.
- It contains **no installation engine**: `.msi` is applied by the package
  manager (`libneopkg`), so there is a single authoritative installer.

Why both formats exist:

- **`.nxp` = unit of software** (one component, published by developers).
- **`.msi` = unit of deployment** (a product/ISO/vendor bundle that installs a
  set of components and seeds configuration), used by administrators and
  distribution images.

Users/developers choose `.nxp` to install a component; administrators/vendors
choose `.msi` to deploy a set of components or an offline bundle.

Explicit compatibility statement: NeoDOS `.msi` is a **NeoDOS-specific** format.
It MUST NOT be advertised as compatible with Microsoft Windows Installer.

---

## 16.3 Package manager architecture

Reuse the documented names (searching for conventions found them already in
`docs/userland/packages.md`): `libneopkg` (engine) and `neoget` (CLI). Do **not**
create an NXL wrapper for a single executable: `libneopkg` is a plain crate with
multiple consumers (`neoget`, `neoupdate`, and later a service), which is the
condition for an independent library.

```text
neoget (CLI) ─┐
neoupdate ────┼──> libneopkg (engine: parse, verify, resolve, plan, apply, db)
future svc ───┘         │
                        ├── libcrypto (SHA-256, Ed25519)
                        ├── libhttp + libtls (repositories, later)
                        ├── Registry (Cm) — package DB + keyring
                        └── VFS — stage + FileSync/FileReplace (prerequisite)
```

| Capability | Where | Notes |
| --- | --- | --- |
| Install/uninstall/inspect/verify/list | `libneopkg` | engine |
| Dependency resolution + constraints | `libneopkg` | linear v1, backtracking later |
| Conflict detection | `libneopkg` | `CONF`/`REPL`/`PROV` |
| Installed inventory | Registry (`\Packages\`) | reuse Cm |
| File ownership tracking | Registry (`Files\<relpath>`) | CRC/SHA per file |
| Shared-library dependency mgmt | `libneopkg` | NXL deps as packages |
| Upgrade/downgrade policy | `libneopkg` | anti-rollback via release counter |
| Repair/verify | `libneopkg` | re-hash installed files |
| Transaction planning/rollback | `libneopkg` + VFS | needs FileSync/FileReplace |
| Interrupted-install recovery | `libneopkg` + boot scan | recovery records |
| Concurrent operations | lock in Registry | single-writer transaction |
| Disk-space checks | `libneopkg` | pre-plan |
| Offline install | `libneopkg` (`file://`) | no repo needed |
| Repositories | `libneopkg` + `libhttp`/`libtls` | later |
| CLI/frontend | `neoget` | `.nxp`; `.msi` via bundle subcommand |
| Interactive OS install | `install.nxe` (#103) | distinct from `.msi` app bundles |

**Ob integration caveat (#411):** `package-manager-arch.md` proposes
`ObType::Package=19/Repository=20/Transaction=21`, but the kernel already assigns
`Session=19`, `Service=20`, `PowerManager=21`, `KeyboardDevice=22`. Only 19 is
free; the enum must be reconciled before implementation. M1 needs **no** new
ObType: the Registry DB suffices; Ob objects can be a later, additive layer.

---

## 16.4 Security and trust

Integrated with the TLS/updater architecture **without conflating responsibilities**:

- **Package/manifest signature:** Ed25519 over the manifest and the payload
  (the NXP signature block, §3.5). Reuse `libcrypto` (extracted from `libnlt`).
- **Hashes:** SHA-256 for authenticity; CRC32 only as a fast corruption check.
- **Trusted keys:** root key provisioned in the image; repository/user keys in
  `\Registry\Machine\Packages\.Keyring\`; rotation ships a new trust store
  through a signed update; revocation list honoured.
- **Compatibility checks before install:** arch, minimum OS version, `CORE`/
  `BOOT` flags, conflicts.
- **Unsigned local packages:** rejected by default; a developer mode (Registry
  policy) may allow them with an explicit warning. Never silently accepted.
- **Archive/path traversal:** the deterministic validation in §16.2.1.
- **Limits:** metadata bytes, entry count, extracted size, nesting, decompression
  ratio.
- **Malformed/malicious packages:** parse defensively; fail with a specific
  error; never trust offsets/sizes; bound all reads.
- **Privilege boundary:** system destinations (`/System`, `/System/Libraries`,
  `/Programs` for shared tools) require admin; user packages install under the
  user's area. Installation destinations are chosen by the engine, not the
  package.
- **Scripts/hooks:** **do not execute package-provided scripts with elevated
  privileges.** Default: scripts disabled. If ever enabled, they require a
  signature, an explicit admin policy, and a non-elevated sandbox; a downloaded
  package is never sufficient authorization.
- **HTTPS ≠ authenticity:** TLS protects transport and authenticates the server;
  it does not replace package signature verification.

---

## 16.5 Installation transactions and recovery

Filesystem guarantees (traced in §16.1.5) are **insufficient today**. The exact
missing prerequisites are `FileSync` and `FileReplace` (and committing the
superblock on write), from the prerequisites design doc. The transaction model:

1. **Validate** the package and compatibility.
2. **Authenticate** per policy (Ed25519 + trust store + revocation).
3. **Resolve** dependencies and conflicts.
4. **Calculate** required disk space.
5. **Produce an installation plan** (files → destinations, backup set).
6. **Stage and verify** files (temp location; SHA-256; signature).
7. **Record** file ownership and package state in the Registry.
8. **Commit** using `FileSync` + `FileReplace` (proven primitives only).
9. **Recover/roll back** after interruption where feasible (recovery records +
   boot scan).
10. **Report partial failures accurately.**

**Never claim full atomicity** the filesystem cannot provide. Until
`FileReplace` exists and is tested, installation is "stage + verify + best-effort
replace + recovery record", and the tool must say so.

**Special handling (out of scope M1):** kernel, bootloader, filesystem drivers,
and system libraries. They must not be replaced by this mechanism until a
verified recovery mechanism exists (the same boundary as `neoupdate`).

---

## 16.6 Relationship with `neoupdate`

`neoupdate` is the **update front-end**; it must **delegate installation to
`libneopkg`**, not implement a second installation engine.

| Responsibility | `neoupdate` | `libneopkg` |
| --- | --- | --- |
| Check for new versions | Yes | — |
| Download signed manifests/artifacts | Yes (via `libhttp`/`libtls`) | — |
| Resolve package versions | No | Yes |
| Install/replace files | No (delegates) | Yes |
| Record installed state | No | Yes |
| Recover from failed updates | No | Yes (shared recovery) |
| Manifest/signature policy | shared | shared |

One authoritative installation mechanism regardless of interactive, offline, or
update-driven invocation; no duplicated dependency resolution, signature policy,
file ownership, or rollback logic.

---

## 16.7 Required package-format decisions

### 16.7.1 Comparison

| Dimension | `.nxp` (package) | `.msi` (installation bundle) |
| --- | --- | --- |
| Purpose | Unit of software (one component) | Unit of deployment (a set of components + config) |
| Container structure | `NXP1` header + TLV manifest + file table + string pool + data + optional signature + footer | Bundle manifest (TLV/TOML) + embedded/referenced `.nxp` artifacts + install plan |
| Manifest format | TLV (`package-manager-arch.md` §3.3) | TLV/TOML bundle manifest (new) |
| Payload/compression | raw v1; optional zstd later (flag) | none (references/embeds `.nxp`) |
| Versioning | header `fmt_ver` + `VER` + min OS | bundle `schema` + target + min OS |
| Dependencies | `DEP/DEPO/CONF/REPL/PROV` | selects/orders `.nxp` (delegates to engine) |
| Signature/hash | Ed25519 + SHA-256 (+ CRC32 fast check) | Ed25519 over bundle + SHA-256 of embedded `.nxp` |
| Installation semantics | extract + register | plan + apply via `libneopkg` |
| Offline behavior | `file://` install | self-contained bundle (embeds `.nxp`) |
| Compatibility policy | arch/min-OS/`CORE`/`BOOT` | target/min-OS; per-component via `.nxp` |
| Recovery guarantees | bounded by VFS (needs FileSync/FileReplace) | inherits `.nxp` guarantees |
| Expected consumers | developers, `neoget`, `neoupdate` | administrators, vendors, distribution images |

### 16.7.2 Existing-format evaluation (do not finalize custom prematurely)

Before committing to the NXP TLV container, compare with existing formats:

| Option | Pros | Cons | Verdict |
| --- | --- | --- | --- |
| **NXP TLV (existing)** | already documented + partially built; zero-copy; no allocator needed for verification; designed for kernel-side checks | custom parser to maintain | **Recommended**: reuse and complete |
| tar + manifest | ubiquitous, streaming | no random access; weak validation; path-traversal footguns | rejected as primary |
| ZIP | compression, random access, tooling | large parser; decompression bombs; licensing/CRC-only; overkill for M1 | rejected for M1 |
| cpio | simple streaming | no compression/random access; weak metadata | rejected |
| Rust archive crate | less custom code | external dependency; `no_std`/target constraints | rejected for M1 |

### 16.7.3 Decisions now vs later

| Decision | Now or later |
| --- | --- |
| Keep NXP container, complete manifest tags + signature block | **Now** |
| `.msi` = NeoDOS bundle referencing/embedding `.nxp`; not Microsoft MSI | **Now** |
| SHA-256 (not CRC32) for authenticity | **Now** |
| Path-validation rules and limits | **Now** |
| Scripts disabled by default; never elevated | **Now** |
| ObType reconciliation (#411) | **Now** (decide numbering) |
| Compression (zstd) | Later (flag reserved) |
| Repository protocol (`.nxi`) | Later |
| SAT resolver, deltas, sandboxing | Later |
| Exact `.msi` binary layout | Later (after `.nxp` contract frozen) |

---

## 16.8 Testing

Deterministic local fixtures + fault injection; no live remote repository.

| Area | Tests |
| --- | --- |
| Valid/invalid packages | parse a valid `.nxp`; reject bad magic/CRC/truncated; unsupported `fmt_ver` |
| Hashes/signatures | SHA-256 mismatch; bad Ed25519; untrusted key; revoked key; unsigned policy |
| Archive corruption | truncated file table; offsets beyond EOF; overlapping entries |
| Path traversal | `..`, absolute, drive-letter, duplicate normalized, case collision, backslash |
| Limits | entry-count, extracted-size, per-file-size, decompression ratio |
| Dependencies | missing, incompatible version, cyclic, conflict, replace |
| Disk space | plan rejects when `needed > available` |
| Interrupted install | fault injection mid-apply → recovery restores; reboot recovery |
| Failed upgrade/uninstall | rollback restores previous files + Registry |
| Concurrency | second transaction rejected while one is active |
| Format compatibility | old-format packages rejected/accepted per policy |
| `.msi` bundle | apply bundle → components installed via engine; unsigned bundle rejected |

Integration: offline `file://` install of a fixture `.nxp`; `neoget verify`
detects a tampered installed file.

---

## 16.9 Roadmap integration

Candidate milestones (not mandatory ordering), with prerequisites:

1. **Audit + prerequisites** — VFS `FileSync`/`FileReplace`, SHA-256, Ed25519 in
   `libcrypto` (see prerequisites/TLS docs). **Blocks everything.**
2. **Package metadata + format versioning** — freeze the NXP tag set, SHA-256,
   signature block, path-validation rules.
3. **Read-only inspection/verification** — `libneopkg` parser + verifier;
   `neoget info|list|verify` on local `.nxp`; fix `nxverify` to actually compute
   CRC/SHA.
4. **Safe local/offline installation** — stage + verify + `FileSync`/
   `FileReplace`; recovery records.
5. **Installed inventory + uninstall** — Registry DB, file ownership, uninstall.
6. **Dependency resolution + upgrades** — constraints, conflicts, anti-rollback.
7. **`.msi` bundles** — define and implement after the `.nxp` and engine
   contracts are frozen.
8. **Repositories + `neoupdate`** — only after trust and recovery are satisfied;
   `neoupdate` delegates to `libneopkg`.

Master-design updates (applied in the master doc):

- **Dependency graph edges:** `libneopkg → libcrypto, Registry, VFS`; `neoget →
  libneopkg`; `neoupdate → libneopkg`; `.msi → libneopkg`; `libneopkg → libhttp/
  libtls` (repositories, later).
- **Test strategy:** add the package/fixture/fault-injection layer above.
- **Risks:** custom-format maintenance; CRC32-as-security; path traversal;
  no atomicity; unsigned local packages; ObType collision.
- **Decision log:** NXP retained and completed; `.msi` = NeoDOS bundle (not
  Microsoft MSI); SHA-256 for authenticity; scripts disabled by default;
  single installer engine.
- **Proposed backlog:** see below.

---

## 16.10 Existing issues and proposed backlog

### 16.10.1 Existing issues to reuse

| Issue | Title | Relationship |
| --- | --- | --- |
| #411 | [PKG] Package manager (libneopkg / neoget) — design exists, 0% implemented, ABI collision | Primary tracking issue; needs the ObType reconciliation |
| #105 | INSTALL-PACKAGES: Despliegue de paquetes base | Base package deployment |
| #103 | INSTALL-NXE: install.nxe | OS installer (distinct from `.msi` app bundles) |
| #100 | TOOL-NXE: NXE tools completion | `nxpkg`/`nxverify` completion |
| #147 | NXPKG: nxpkg tool | Closed (host tool) |
| #149 | LIB-RES: libneodos Resource API | Closed |
| #586 | [NXL] Data-driven NXL packaging in the image builder | Closed; deployment/packaging contract |
| NXE-ECO-12..15 | metadata/signature/validation | Related |

### 16.10.2 Proposed new issues (do not create in this task)

| Proposed | Title | Rationale |
| --- | --- | --- |
| PKG-1 | `[PKG] Freeze NXP v1 manifest tags + SHA-256 + signature block` | Complete the format before a parser |
| PKG-2 | `[PKG] Deterministic path validation + archive limits` | Security |
| PKG-3 | `[PKG] libneopkg parser + verifier (read-only)` | First engine milestone |
| PKG-4 | `[PKG] Registry package DB + file ownership + uninstall` | Inventory |
| PKG-5 | `[PKG] Safe local/offline install transaction` | Depends on FileSync/FileReplace |
| PKG-6 | `[PKG] Dependency resolver + conflicts + anti-rollback` | Upgrades |
| PKG-7 | `[MSI] Define NeoDOS .msi bundle format (not Microsoft MSI)` | Greenfield |
| PKG-8 | `[PKG] neoupdate delegates to libneopkg` | Single installer engine |
| PKG-9 | `[PKG] Reconcile package ObType numbering (#411)` | ABI |
| PKG-10 | `[TOOL] nxverify computes real CRC32/SHA-256` | Fix cosmetic verification |

---

*End of design. No code, public API, or issue state is changed by this document.*
