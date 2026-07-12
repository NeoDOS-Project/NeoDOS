# Release

## When to use

Cutting a new release, bumping the version, or preparing a changelog.

## Goal

Produce a consistent, tested, and documented release with proper versioning and changelog.

## Steps

1. **Check current version**
   Read `AGENTS.md` — the version is at the top (e.g., `v0.48.6`).
   Determine if the change is major, minor, or patch based on API/ABI compatibility:
   - **Major**: Breaking ABI or public API change.
   - **Minor**: New feature, backward compatible.
   - **Patch**: Bug fix, no API change.

2. **Update AGENTS.md**
   Bump the version string. Update ABI version if the NEM ABI changed.
   Update the test count if tests were added/removed.

3. **Update CHANGELOG.md**
   Add a new heading for the release version. Move unreleased changes under it.
   Format:

   ```markdown
   ## v0.49.0 (2026-07-03)
   - feat: new Ob type for synchronization primitives
   - fix: slab allocator double-free on hot cache refill
   ```

   Keep entries concise, grouped by type (feat, fix, refactor, docs, test).

4. **Update IMPROVEMENTS.md**
   Move completed items from `docs/IMPROVEMENTS.md` to `docs/IMPROVEMENTS_COMPLETED.md`.
   Add a note with the version that completed each item.

5. **Full build and test**

   ```bash
   cargo build
   python3 scripts/auto_test.py
   scripts/check_deps.py
   ```

6. **Build release image**

   ```bash
   bash scripts/build.sh --neodos-image
   ```

   If this is a release candidate, boot in QEMU and verify the shell prompt shows the correct version.

7. **Commit**

   ```bash
   git add -A
   git status                     # verify only intended files
   git diff --cached --stat       # review staged changes
   ```

   Commit message: `release: vX.Y.Z`.

8. **Tag and push** (if publishable release)

   ```bash
   git tag vX.Y.Z
   git push && git push --tags
   ```

   If CI is configured, verify the release pipeline passes.

## Best practices

- Bump ABI version (`KERNEL_ABI_VERSION` in `src/nem/mod.rs`) on any breaking NEM change.
- Test the full boot path (QEMU) for every release — not just unit tests.
- Update `docs/IMPROVEMENTS.md` before, not after, cutting the release.
- Keep `CHANGELOG.md` entries short — one line per change, link to PRs if available.
- Never release on a dirty working tree — commit or stash first.

## Common mistakes

- Forgetting to bump ABI version when NEM driver structs or ABIs changed.
- Releasing without updating `docs/IMPROVEMENTS.md` — items show as pending that are done.
- Releasing without running `scripts/auto_test.py` — CI catches it but it's embarrassing.
- Including uncommitted changes in the release commit.
- Bumping the version in only one place (AGENTS.md but not in-kernel version constant).

## Final checklist

- [ ] Version bumped in `AGENTS.md`
- [ ] ABI version bumped (if NEM ABI changed)
- [ ] `CHANGELOG.md` updated with release notes
- [ ] `docs/IMPROVEMENTS.md` items moved to `docs/IMPROVEMENTS_COMPLETED.md`
- [ ] `cargo build` + `python3 scripts/auto_test.py` pass
- [ ] `scripts/check_deps.py` passes
- [ ] `bash scripts/build.sh --neodos-image` succeeds
- [ ] QEMU boot verified (version string correct)
- [ ] Committed, tagged (`vX.Y.Z`), pushed
