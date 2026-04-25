# Development Setup

How to build senko from source and work on it.

## Requirements

- Rust (edition 2024 compatible)
- `mise` — used to pin project tool versions (`mise.toml`)
- git
- (To work on the PostgreSQL backend) Docker or a local Postgres

## First-Time Setup

```bash
git clone https://github.com/hisamekms/senko.git
cd senko

# Install the toolchain via mise
mise install

# Build
cargo build

# Run
cargo run -- task list
```

## Feature Flags

```toml
# Cargo.toml
[features]
aws-secrets = ["dep:aws-sdk-secretsmanager", "dep:aws-config"]
postgres    = ["dep:sqlx"]
```

- **`aws-secrets`**: AWS Secrets Manager integration (resolves `_arn` keys).
- **`postgres`**: PostgreSQL backend.

Build with everything:

```bash
cargo build --all-features
```

## Directory Structure

```
src/
├── domain/        Domain models (no dependencies)
├── application/   Use cases + port traits
│   └── port/
├── infra/         Port implementations (sqlite / postgres / http / hook / auth)
│   └── postgres/migrations/
├── presentation/  cli / api / web
└── bootstrap.rs   Dependency graph assembly

tests/
└── e2e/           Shell-script-based end-to-end tests
```

Design principles: [Layered Architecture (4 Layers)](architecture.md).

## Working Day-to-Day

### Work in a worktree

This project **forbids direct edits on the main branch**:

```bash
# Create a worktree
./scripts/bin/wth add my-feature

# Move into it
cd worktrees/my-feature
```

Details: [Worktree Workflow](worktree.md).

### Build and test

```bash
mise test           # unit + doc tests
mise run e2e        # end-to-end (bash scripts)
```

Details: [Testing](testing.md).

### Architecture check

To detect layer-boundary violations:

```
/arch-review
```

(Claude Code skill.) To run it manually, refer to the latest files in `docs/arch-review/`.

## Implementation Flow

1. Create a worktree and start work.
2. Make changes (top-down: domain → application → infra → presentation).
3. Add or update corresponding unit tests.
4. Verify end-to-end via e2e tests.
5. Open a PR (the `/review-pr` / `/security-review` skills are available).

## IDE / Editor

- **rust-analyzer**: no special config needed.
- **clippy**: `cargo clippy --all-features --all-targets -- -D warnings`
- **rustfmt**: `cargo fmt` (default config).

CI runs both clippy and rustfmt — run them locally before opening a PR.

## Adding Migrations

SQLite:

```
Add a Migration entry to the MIGRATIONS constant in src/infra/sqlite/mod.rs
```

PostgreSQL:

```
Add a timestamped SQL file under src/infra/postgres/migrations/
```

Add both at the same time and keep the schemas in sync. `schema_migrations` tracks versions, so numbers must increase monotonically.

## Dependency Updates

- **Cargo / GitHub Actions**: Dependabot (`.github/dependabot.yml`)
- **mise tools**: Renovate (`renovate.json5`)

Renovate waits 7 days after a release before opening a PR. Auto-merge is disabled on both — review manually.

## Debugging

```bash
# Verbose logging
cargo run -- --log-dir /tmp/senko-logs task list
RUST_LOG=debug cargo run -- task list

# Try with a throwaway DB
cargo run -- --db-path /tmp/test.db task add --title hello

# Try against a local Postgres (postgres feature)
docker run -d --name senko-pg -e POSTGRES_PASSWORD=pw -p 5432:5432 postgres:16
SENKO_POSTGRES_URL="postgres://postgres:pw@127.0.0.1:5432/postgres" \
  cargo run --features postgres -- task list
```

## Releasing

Use the `/release` skill for the full pipeline (e2e → version bump detection → update Cargo.toml → commit → tag → push). Details: [Release Procedure](releasing.md).
