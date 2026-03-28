# Releasing

## Overview

Releases are automated via GitHub Actions. Pushing a version tag triggers the build and publishes a GitHub Release with pre-built binaries.

## Supported Platforms

| Target | OS | Arch |
|---|---|---|
| `aarch64-apple-darwin` | macOS | ARM64 (Apple Silicon) |
| `aarch64-unknown-linux-musl` | Linux | ARM64 |
| `x86_64-unknown-linux-musl` | Linux | x86_64 |

## How to Release

1. Update the version in `Cargo.toml`
2. Commit the version bump
3. Create and push a tag:

```bash
git tag v0.2.0
git push origin v0.2.0
```

The workflow (`.github/workflows/release.yml`) will:
- Build binaries for all 3 platforms
- Create `.tar.gz` archives with SHA256 checksums
- Publish a GitHub Release with auto-generated notes

## Artifacts

Each release includes per-platform archives:

```
senko-v0.2.0-aarch64-apple-darwin.tar.gz
senko-v0.2.0-aarch64-apple-darwin.tar.gz.sha256
senko-v0.2.0-aarch64-unknown-linux-musl.tar.gz
senko-v0.2.0-aarch64-unknown-linux-musl.tar.gz.sha256
senko-v0.2.0-x86_64-unknown-linux-musl.tar.gz
senko-v0.2.0-x86_64-unknown-linux-musl.tar.gz.sha256
```

## Installation from Release

```bash
# Download and extract (example: Linux x86_64)
curl -LO https://github.com/<owner>/senko/releases/download/v0.2.0/senko-v0.2.0-x86_64-unknown-linux-musl.tar.gz
tar xzf senko-v0.2.0-x86_64-unknown-linux-musl.tar.gz
sudo mv senko /usr/local/bin/
```

## Verifying Checksums

```bash
sha256sum -c senko-v0.2.0-x86_64-unknown-linux-musl.tar.gz.sha256
```
