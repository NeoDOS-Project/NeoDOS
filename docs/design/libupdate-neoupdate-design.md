# Signed Updates (`libupdate` + `neoupdate`) — Design Document

> **Version:** v0.1-draft
> **Status:** Design (no implementation)
> **Target Release:** v0.59 — Installation & Bootstrap (foundations), updater later
> **Related:** #411 (package manager), #105 (INSTALL-PACKAGES), #103 (install.nxe),
> #89/#57/#582 (signing), #356/#467 (clock), `docs/architecture/package-manager-arch.md`,
> `docs/userland/nxp-format.md`; `docs/design/http-tls-update-prerequisites-design.md`,
> `docs/design/libtls-https-design.md`
> **ABI Impact:** none directly; depends on `FileSync=53` / `FileReplace=54`
> (prerequisite doc)

---

## 1. Research

| Source | What it establishes |
| --- | --- |
| `docs/architecture/package-manager-arch.md` | full package-manager design: `.nxp`/`.nxi`, Registry DB, `\Package` objects, transactions, Ed25519 — **0% implemented** |
| `docs/userland/nxp-format.md`, `docs/userland/packages.md` | NXP container format + NeoGet vision |
| `neotools/nxpkg/src/main.rs` (sibling repo) | implements an NXP **subset**: header CRC + per-entry CRC32; `flags=0`, `signature_offset=0` (no signatures) |
| `libnlt/src/signature.rs`, `libneodos/src/i18n.rs` | Ed25519 verify is real and tested; the only signature code |
| `neodos-kernel/src/fs/vfs/mod.rs`, `fs/neofs/neodos_v2.rs` | no `sync`; `write` does not commit the superblock; `rename` = delete+insert, same-directory, untested |
| `neodos-kernel/src/main.rs`, `build.rs` | `\Global\Info\Version` = `vX.Y.Z (git <rev>)` |
| `neodev/src/image.rs` | `.nxe` → `Programs/`/`System/Tools/`, `.nxl` → `Libraries/`; registry hive baked at build; **no runtime install path** |
| repo-wide grep | no `libneopkg`/`neoget`; no runtime installer |

Verified: NXP exists as a host tool subset; the package manager is design-only
with an ObType ABI collision (#411: it proposes `Package=19`, but 19–22 are
taken); artifact signatures are not implemented; activation durability is absent.

---

## 2. Problem analysis

There is no way to deliver an update safely:

1. **No manifest/format** for a component release with hashes and signatures.
2. **No authenticity/integrity verification** for artifacts (Ed25519 is not
   applied to NXP/NXE; `nxverify` checks only the magic).
3. **No trust model**: no key provisioning, rotation, revocation, or rollback
   control.
4. **No safe activation**: no `fsync`, no atomic replace, and `write` does not
   commit the superblock.
5. **No recovery/rollback state.**

Why existing abstractions cannot solve it:

- **`nxpkg`** is a host tool that produces unsigned CRC-only archives; it is not
  a runtime updater and has no signature verification.
- **Registry** stores configuration but has no transaction/recovery semantics.
- **VFS** has no durability or replace primitive (the prerequisite doc adds them).
- **`libnlt` Ed25519** is scoped to language packs; it must be extracted/shared.

This feature is a **subset** of the package manager (#411). It must not
reimplement the whole package manager; it should be forward-compatible with NXP.

---

## 3. Solution design

### 3.1 Types / structs / enums

Plain `no_std` crate `libupdate` (host-testable). No new `ObType`.

```rust
pub struct Manifest {
    pub schema: u16,                 // manifest format version
    pub product: String,             // e.g. "neodos"
    pub component: String,           // e.g. "datetime"
    pub version: String,             // release version
    pub min_os_version: String,      // compatibility floor
    pub arch: String,                // "x86_64"
    pub release_counter: u64,        // anti-rollback
    pub artifacts: Vec<Artifact>,
    pub dependencies: Vec<Dependency>,
}
pub struct Artifact {
    pub url: String,                 // HTTPS only
    pub size: u64,
    pub sha256: [u8; 32],
    pub sig: Option<[u8; 64]>,       // Ed25519 over the artifact
    pub key_id: Option<u32>,
    pub path: String,                // activation target
    pub kind: ArtifactKind,
}
pub enum ArtifactKind { Nxe, Nxl, Data }
pub struct Dependency { pub component: String, pub min_version: String }

pub struct TrustStore { /* public keys by id + revoked ids */ }
pub struct UpdatePolicy {
    pub trusted_keys: TrustStore,
    pub allow_rollback: bool,
    pub min_release_counter: u64,
}
pub enum UpdateError {
    ManifestFormat, UntrustedKey, BadSignature, RevokedKey, RollbackRejected,
    Incompatible, HashMismatch, SizeMismatch, Truncated, FormatInvalid,
    DependencyMissing, SyncFailed, ActivationFailed, Io,
}
pub fn plan(manifest: &Manifest, policy: &UpdatePolicy) -> Result<UpdatePlan, UpdateError>;
pub fn apply(plan: &UpdatePlan, io: &mut dyn UpdateIo) -> Result<(), UpdateError>;
```

### 3.2 New syscalls / ObType / classes

**None in this feature.** It uses:

- `FileSync=53` / `FileReplace=54` (prerequisite doc) for durability/activation,
- `\Global\Info\Version` for the OS version,
- the Registry for update state,
- `libhttp` + `libtls` + `libcrypto` for fetch/verify.

### 3.3 New files / modules

```text
libupdate/Cargo.toml
libupdate/src/{lib,manifest,verify,plan,apply,state,error}.rs
libupdate/src/tests.rs
userbin/neoupdate/Cargo.toml
userbin/neoupdate/src/main.rs
```

### 3.4 Changes to existing files

| Path | Change |
| --- | --- |
| `libcrypto` | expose Ed25519 verify + SHA-256 (from the TLS doc) |
| `libhttp`/`libtls` | used for fetch (no change to their APIs) |
| `neodev/src/image.rs` | add `neoupdate`; provision `\Update` Registry defaults + trust keys |
| `tools/gen-hiv` (sibling) | seed update state / trust keys |
| `docs/architecture/package-manager-arch.md` | reference this subset and the ObType reconciliation (#411) |

### 3.5 Manifest format

TLV, aligned with NXP (`nxp-format.md`) so the formats converge later. Fields as
in §3.1. The manifest is distributed over HTTPS and authenticated by an Ed25519
signature (detached or embedded). `release_counter` is monotonic and drives
anti-rollback.

### 3.6 Trust model

- **Authenticity:** Ed25519 over the manifest and (optionally) over each
  artifact; `key_id` selects the public key.
- **Provisioning/rotation:** keys are provisioned in the image; a rotation ships
  a new trust store through a signed update.
- **Revocation:** `TrustStore` carries revoked key ids; a revoked key is
  rejected.
- **Rollback:** `release_counter` below `min_release_counter` is rejected unless
  `allow_rollback`.
- **Hash vs signature vs TLS:** all three are checked; none substitutes for
  another (transport encryption ≠ server auth ≠ publisher signature ≠ integrity).

### 3.7 Safe transaction

```text
1. Fetch metadata over validated HTTPS (libhttp + libtls).
2. Authenticate the manifest (Ed25519 + TrustStore + revocation + anti-rollback).
3. Validate compatibility (arch, min_os_version) and dependencies.
4. Download each artifact to C:\System\Updates\staging\<id>.
5. Enforce size; verify SHA-256.
6. Verify the artifact signature when present.
7. Validate artifact format (NXE/NXL headers).
8. FileSync the staging artifacts and metadata.        <-- prerequisite
9. FileReplace each target with its staged artifact.   <-- prerequisite
10. Record state (release_counter, component, previous version) in the Registry.
11. Re-verify the installed artifacts; report failure accurately.
```

Steps 8–9 require the prerequisite doc's `FileSync`/`FileReplace`.

### 3.8 Scope boundary

- **M1 updates user-mode `.nxe`/`.nxl` and data only.**
- **No kernel/bootloader/FS-format replacement** until a verified recovery
  mechanism exists.
- **Manual only** (`neoupdate`); automatic scheduling is a later feature.
- **Forward-compatible** with NXP; do not build the whole package manager.

---

## 4. Alternatives

- **Use `nxpkg` archives as-is.** Rejected: unsigned, CRC-only, host tool; no
  trust model.
- **HTTPS-only trust (no signatures).** Rejected: TLS authenticates the server,
  not the publisher; a compromised mirror would pass.
- **Atomic rename assumption.** Rejected: not proven (prerequisite doc).
- **Automatic kernel self-update in M1.** Rejected: no recovery mechanism.
- **Implement the full package manager (#411) now.** Rejected: post-1.0,
  ABI collision, out of scope; this is a signed-update subset.
- **Boot-time apply instead of `FileReplace`.** Viable fallback, but `FileSync`
  and `FileReplace` are smaller, testable now, and also fix Registry durability.
  Chosen.

---

## 5. Affected components

| Subsystem | Change | Impact |
| --- | --- | --- |
| Kernel FS | `FileSync`/`FileReplace` (prerequisite doc) | Medium |
| `libcrypto` | SHA-256 + Ed25519 verify | Low |
| `libhttp`/`libtls` | fetch + validated HTTPS | Low |
| `libupdate` (new) | manifest/verify/plan/apply | New crate |
| `neoupdate` (new) | CLI | New `.nxe` |
| Registry | `\Update` state + trust keys | Low |
| Image | `neoupdate` + keys + defaults | Low |
| Scheduler/memory/drivers | None | None |

---

## 6. API contract

| Function | Args | Returns | Errors |
| --- | --- | --- | --- |
| `Manifest::parse` | `&[u8]` | `Manifest` | `ManifestFormat` |
| `verify::manifest` | `&Manifest`, `&TrustStore` | `()` | `UntrustedKey`, `BadSignature`, `RevokedKey` |
| `verify::artifact` | `&Artifact`, `&[u8]`, `&TrustStore` | `()` | `BadSignature`, `HashMismatch`, `SizeMismatch` |
| `plan` | `&Manifest`, `&UpdatePolicy` | `UpdatePlan` | `Incompatible`, `RollbackRejected`, `DependencyMissing` |
| `apply` | `&UpdatePlan`, `&mut dyn UpdateIo` | `()` | `SyncFailed`, `ActivationFailed`, `Io` |
| `neoupdate` | `neoupdate <manifest-url> [--dry-run]` | report; exit 1 on failure | — |

Preconditions: HTTPS transport validated; trust store non-empty; clock plausibly
synchronized (fail closed otherwise). Postconditions: on success, targets are
replaced and durable; on failure, targets are unchanged and the reason is
reported. `neoupdate` requires admin.

---

## 7. Test plan (≥3 per invariant)

**INV-1 — Manifest authenticity and integrity.**

| Test | Expected |
| --- | --- |
| `manifest_bad_signature_rejected` | `BadSignature` |
| `manifest_untrusted_key_rejected` | `UntrustedKey` |
| `manifest_revoked_key_rejected` | `RevokedKey` |
| `artifact_hash_mismatch_rejected` | `HashMismatch` |

**INV-2 — Version policy.**

| Test | Expected |
| --- | --- |
| `rollback_rejected` | `release_counter` below minimum → `RollbackRejected` |
| `rollback_allowed_when_policy_set` | succeeds with `allow_rollback` |
| `incompatible_arch_rejected` | `Incompatible` |
| `min_os_version_enforced` | older OS → `Incompatible` |

**INV-3 — Transaction safety.**

| Test | Expected |
| --- | --- |
| `truncated_download_no_activate` | staging incomplete → `Truncated`, targets unchanged |
| `interrupted_apply_recovers` | crash mid-apply → previous version intact, state resumable |
| `sync_failure_no_activate` | `FileSync` fails → `SyncFailed`, no replace |
| `replace_then_verify` | post-activation verification detects corruption |

**INV-4 — Dependency/format checks.**

| Test | Expected |
| --- | --- |
| `missing_dependency_rejected` | `DependencyMissing` |
| `bad_nxe_header_rejected` | `FormatInvalid` |
| `data_only_update_applies` | data artifact replaced, no executable touched |

**Integration (QEMU/VBox):** `neoupdate --dry-run`; full apply + rollback against
a local HTTPS fixture; no public-Internet dependency.

---

## 8. Implementation plan

1. **`FileSync`/`FileReplace`** (prerequisite doc) + tests.
2. **`libcrypto`** SHA-256 + Ed25519 verify (TLS doc) + tests.
3. **`libupdate`** manifest parse + verify + tests (INV-1).
4. **Policy** (compatibility, rollback, dependencies) + tests (INV-2/INV-4).
5. **Transaction** (staging, sync, replace, state) + tests (INV-3).
6. **`neoupdate` CLI** + image/Registry provisioning.
7. **Docs** (package-manager-arch cross-reference, #411 reconciliation) + lint.

Dependencies: steps 1–2 gate everything; the updater must not ship before
durability is proven.

---

## 9. Open questions

1. Reconcile the package-manager ObType plan (#411) — this subset needs none.
2. Manifest container: standalone TLV vs NXP with a signature block.
3. Trust-key storage: Registry vs read-only image file.
4. Rollback semantics (keep one previous version vs N).
5. Whether `neoupdate` may update `.nxl` in place while loaded (NXLs are never
   unloaded) — likely requires a reboot.

---

*End of design. No code, public API, or issue state is changed by this document.*
