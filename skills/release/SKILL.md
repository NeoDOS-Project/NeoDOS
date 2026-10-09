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
   Read `AGENTS.md` (version at the top, e.g. `v0.51.5`). Classify the change:
   - **Major**: breaking ABI or public API change.
   - **Minor**: new feature, backward compatible.
   - **Patch**: bug fix, no API change.

2. **Bump the version in the right places**
   - `AGENTS.md` (version string; update the test count and ABI if changed).
   - `KERNEL_VERSION_CODE` / `BOOT_VERSION` in `src/main.rs` — currently stale
     (`0x0A05`, tracked as `CH-11`); sync it on release.
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
   npx markdownlint '**/*.md' --config .markdownlint.json
   ```

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
- Keep `CHANGELOG.md` entries short; reference PRs.
- Never release on a dirty working tree.

## Common mistakes

- Bumping the version in only one place (`AGENTS.md` but not `src/main.rs`).
- Forgetting the NEM ABI bump when driver structs changed.
- Releasing without `neodev test` / `neodev check-deps`.
- Including uncommitted changes in the release commit.
- Skipping `scripts/sync-roadmap.sh sync`.

## Final checklist

- [ ] Version bumped in `AGENTS.md`
- [ ] `KERNEL_VERSION_CODE` / `BOOT_VERSION` synced (`src/main.rs`)
- [ ] NEM ABI bumped (if applicable) and reflected in `AGENTS.md`
- [ ] `CHANGELOG.md` updated
- [ ] Roadmap synced (`scripts/sync-roadmap.sh sync`)
- [ ] `cargo build` + `neodev build --image` succeed
- [ ] `neodev test` and `neodev check-deps` pass
- [ ] `markdownlint` passes
- [ ] QEMU boot verified (version string correct)
- [ ] Committed on `release/vX.Y.Z`, tagged (`vX.Y.Z`), pushed
