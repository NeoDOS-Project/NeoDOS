---
name: release
description: Cut releases, bump versions, prepare changelogs
---

# Release

## When to use

Cutting a new release, bumping the version, or preparing a changelog.

## Goal

Produce a consistent, tested, and documented release with proper versioning.

## Steps

1. **Check the current version**
   Read the `Version:` line in `AGENTS.md` and classify the release change
   (Major: breaking ABI or public API; Minor: new feature, backward compatible;
   Patch: bug fix). Do this before touching any version source.

2. **Bump the version in the right places**
   - `AGENTS.md` (version string; update the test count and ABI if changed).
   - Kernel: `neodos-kernel/Cargo.toml` and `KERNEL_VERSION_CODE`
     (`neodos-kernel/src/main.rs`) must carry the same version.
   - Bootloader: `neodos-bootloader/Cargo.toml` and `BOOT_VERSION`
     (`neodos-bootloader/src/main.rs`) must carry the same version, and
     `BOOT_VERSION` must match `KERNEL_VERSION_CODE` (otherwise the kernel logs
     a version-mismatch warning at boot).
   - `.NXE` / `.NXL`: follow rule 15 in `AGENTS.md`; bump the crate version of
     every userland binary whose code changed, plus its NXE metadata
     (`NEM_PRODUCT_VER` / `NEM_FILE_VERSION`) when emitted.
   - NEM ABI: bump `ABI_TARGET`/`ABI_MAX_VALID` in
     `src/drivers/nem/format.rs` (re-exported as `crate::nem`) only for a breaking
     NEM change; then update the ABI in `AGENTS.md`.

3. **Update `CHANGELOG.md`**
   Add a heading for the new version and move the unreleased entries under it.
   Format:

   ```markdown
   ## vX.Y.Z (YYYY-MM-DD)
   - feat: ...
   - fix: ...
   ```

   Group by type (feat, fix, refactor, docs, test); keep one line per change.

4. **Sync the roadmap**

   ```bash
   scripts/sync-roadmap.sh sync
   ```

   GitHub Issues is the SSOT for planning; `roadmap/improvements.md` is the local
   idea list.

5. **Full build and test**

   ```bash
   cd neodos-kernel && cargo build
   neodev build --image
   neodev test
   neodev check-deps
   scripts/check-skills.sh
   npx markdownlint '**/*.md' --config .markdownlint.json
   ```

   `check-skills.sh` fails if any `skills/*/SKILL.md` still cites a removed
   script, an obsolete heuristic, or a stale path (issue #605).

6. **Build and boot the release image**
   Boot in QEMU (`neodev run`) and verify the shell version prompt.

7. **Commit on the release branch**

   ```bash
   git add -A
   git status                 # only intended files
   git diff --cached --stat
   git commit -m "release: vX.Y.Z"
   ```

8. **Tag and push** (publishable release)

   ```bash
   git tag vX.Y.Z
   git push && git push --tags
   ```

   Branch workflow: `release/vX.Y.Z` from `develop` → PR → `master`.

## Best practices

- Bump the NEM ABI only on a breaking change, and mirror it in `AGENTS.md`.
- Test the full boot path (QEMU) for every release, not just unit tests.
- Sync the roadmap before cutting the release.
- Keep `skills/` in sync: the release is the last checkpoint, so run
  `scripts/check-skills.sh` and fix any drift before tagging.
- Keep `CHANGELOG.md` entries short; reference PRs.
- Never release on a dirty working tree.

## Common mistakes

- Bumping the version in only one place (`AGENTS.md` but not `src/main.rs`).
- Forgetting the NEM ABI bump when driver structs changed.
- Releasing without `neodev test` / `neodev check-deps`.
- Letting `skills/` drift — run `scripts/check-skills.sh`.
- Including uncommitted changes in the release commit.
- Skipping `scripts/sync-roadmap.sh sync`.

## Final checklist

- [ ] Version bumped in `AGENTS.md`
- [ ] `neodos-kernel/Cargo.toml` + `KERNEL_VERSION_CODE` synced
- [ ] `neodos-bootloader/Cargo.toml` + `BOOT_VERSION` synced (and match `KERNEL_VERSION_CODE`)
- [ ] `.NXE` / `.NXL` versions bumped for every binary whose code changed
- [ ] NEM ABI bumped (if applicable) and reflected in `AGENTS.md`
- [ ] `CHANGELOG.md` updated
- [ ] Roadmap synced (`scripts/sync-roadmap.sh sync`)
- [ ] `cargo build` + `neodev build --image` succeed
- [ ] `neodev test` and `neodev check-deps` pass
- [ ] Skills synced (`scripts/check-skills.sh` passes)
- [ ] `markdownlint` passes
- [ ] QEMU boot verified (version string correct)
- [ ] Committed on `release/vX.Y.Z`, tagged (`vX.Y.Z`), pushed
