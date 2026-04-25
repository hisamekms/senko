# Release Procedure

senko releases are automated via GitHub Actions. Pushing a version tag triggers the full build-and-publish pipeline.

## Normal Flow (`/release` skill recommended)

In Claude Code:

```
/release
```

The skill runs:

1. `mise run e2e`.
2. Decide bump type (patch / minor / major) from the commit delta.
3. Update `Cargo.toml` version.
4. Commit with `chore: bump version to X.Y.Z`.
5. Create tag `vX.Y.Z`.
6. Push.

The push triggers `.github/workflows/release.yml`, which builds and publishes the GitHub Release.

## Manual Flow

```bash
# 1. Confirm e2e passes
mise run e2e

# 2. Bump the version
vim Cargo.toml     # update: version = "1.0.0"
cargo build         # refresh Cargo.lock

# 3. Commit
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 1.0.0"

# 4. Tag and push
git tag v1.0.0
git push origin main
git push origin v1.0.0
```

> The `v` prefix is required (`v1.0.0`, not `1.0.0`).

## Target Platforms

| Target | OS | Arch |
|---|---|---|
| `aarch64-apple-darwin` | macOS | ARM64 (Apple Silicon) |
| `aarch64-unknown-linux-musl` | Linux | ARM64 |
| `x86_64-unknown-linux-musl` | Linux | x86_64 |

Intel macOS / Windows are not currently supported.

## Artifacts

Each Release contains:

```
senko-vX.Y.Z-<target>.tar.gz
senko-vX.Y.Z-<target>.tar.gz.sha256
```

- The tarball holds a single `senko` binary.
- Use the `.sha256` to verify integrity.

## Installing (user side)

```bash
curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh
# Specific version
VERSION=v1.0.0 curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh
```

## Versioning Policy

From v1 on, **SemVer** applies:

- **MAJOR**: breaking changes (removed CLI, removed config keys, DB schema incompatibilities).
- **MINOR**: backward-compatible additions (new CLI / new config keys / new API endpoints).
- **PATCH**: bug fixes, dependency bumps, docs.

During 0.x.y, MINOR carried breaking changes — **from v1.0.0 onward, breaking changes only land in MAJOR**.

## CHANGELOG

We use GitHub Release auto-generated notes (PR titles), so format your PR titles accordingly:

- `feat: ...` → new feature
- `fix: ...` → fix
- `docs: ...` → docs
- `chore: ...` → chore
- `refactor: ...` → refactor

Following Conventional Commits gives the cleanest categorization.

## Pre-Release Checklist

- [ ] `mise test` passes
- [ ] `mise run e2e` passes
- [ ] `cargo clippy --all-features --all-targets -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] Update docs (especially migrations) if there are breaking changes
- [ ] For a MAJOR bump, run an RC for a week or more

## User-Side Verification

```bash
# SHA-256 verification
sha256sum -c senko-v1.0.0-x86_64-unknown-linux-musl.tar.gz.sha256

# Sanity check
./senko --version
./senko task list
```

## Rollback

To "demote" a published release:

1. Mark the tag / Release **as Pre-release** (don't delete it).
2. `install.sh` picks the latest Release unless `VERSION` is pinned — the step above removes it from `latest`.
3. If needed, promote the previous tag to `latest`.

Prefer shipping a PATCH over deletion.

## Troubleshooting

| Symptom | What to do |
|---|---|
| Release workflow fails | Check GitHub Actions logs — transient musl toolchain issues are common; re-run |
| SHA-256 mismatch | Possible tampering — open an issue |
| Pushed a tag but no workflow runs | Confirm the tag matches `v*` (e.g. `v1.0` and `v1` don't qualify) |
