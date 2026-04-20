# Switching CLI Backends

At runtime, the `senko` CLI picks **where to store task data** from three options:

| Backend | How to enable | Use case |
|---|---|---|
| SQLite (local) | Default | Personal development, single-project scope |
| PostgreSQL (local / remote DB) | `SENKO_POSTGRES_URL`, etc. | CLI talking to a Postgres directly (rare, server-flavored setups) |
| HTTP (remote server) | `[cli.remote]` | CLI connecting to a team server |

## Resolution Priority

bootstrap picks a backend in this order:

1. `[cli.remote] url` or `SENKO_CLI_REMOTE_URL` is set → **HTTP backend**
2. `[backend.postgres] url` or `SENKO_POSTGRES_URL` is set (with the feature enabled) → **PostgreSQL**
3. Otherwise → **SQLite**

## SQLite (default)

```
Data location: $XDG_DATA_HOME/senko/projects/<dir-name>/data.db
               (= usually ~/.local/share/senko/projects/<dir-name>/data.db)
```

`<dir-name>` is the project-root directory name. If same-named projects collide, set an explicit `db_path`.

Override:

```toml
[backend.sqlite]
db_path = "/custom/location/data.db"
```

Or via CLI/env:

```bash
senko --db-path /custom/data.db task list
SENKO_DB_PATH=/custom/data.db senko task list
```

If a legacy install had `<project_root>/.senko/data.db`, the first run on an upgraded senko migrates it to the XDG location automatically (the original file is kept for verification).

## PostgreSQL (direct CLI connection — rare)

PostgreSQL is intended primarily as the backend for `senko serve`, but the CLI can also connect to it directly (for dev / migration purposes):

```bash
cargo build --release --features postgres
export SENKO_POSTGRES_URL="postgres://user:pass@localhost/senko"
senko task list
```

- Unapplied migrations run on startup.
- Postgres transactions prevent simultaneous-write corruption, but direct CLI → DB isn't recommended — prefer going through the HTTP backend.

## HTTP (remote server)

The most common shape for team use:

```toml
# .senko/config.toml
[cli.remote]
url = "https://senko.example.com"
token = "sk_..."
```

Or via env:

```bash
export SENKO_CLI_REMOTE_URL="https://senko.example.com"
export SENKO_CLI_REMOTE_TOKEN="sk_..."
senko task list
```

In remote mode, **no local DB is touched** — every operation is sent over HTTP to the server.

### Letting the keychain hold the token (OIDC)

When the server has OIDC enabled:

```bash
senko auth login
```

After a browser login, the token is stored in the OS keychain. From then on you don't need `token` in config:

```toml
[cli.remote]
url = "https://senko.example.com"
# token comes from the keychain automatically
```

## One-Off Backend Swaps

To flip backend temporarily in the same repo:

```bash
# Disable remote just for this call → fall back to local DB
SENKO_CLI_REMOTE_URL= senko task list

# Touch a different DB file
senko --db-path /tmp/scratch.db task list

# Point at a different Postgres (e.g. for dev migration work)
SENKO_POSTGRES_URL=postgres://... senko task list
```

## Checking Which Backend Is Active

```bash
senko config
```

Look for which of `cli.remote.url` / `backend.sqlite.db_path` / `backend.postgres.url` has a value.

## Migrating Data (SQLite → Postgres, etc.)

**There is no supported backend-to-backend migration path at this point.** No official migration command exists and none is on the roadmap. If you need to move existing data to a different backend, plan to roll your own dump/restore procedure.
