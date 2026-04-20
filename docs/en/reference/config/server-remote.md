# `[server.*]` / `[backend.*]` / `[server.auth.*]` Config

Sections active when running as `senko serve` (direct mode).

## `[server]`

| Key | Type | Default | Description |
|---|---|---|---|
| `host` | string | `127.0.0.1` | Bind address |
| `port` | u16 | `3142` | Port |

env overrides: `SENKO_SERVER_HOST` / `SENKO_SERVER_PORT` (or `SENKO_HOST` / `SENKO_PORT` shared with `web`).

## `[backend.sqlite]`

When running on SQLite.

| Key | Type | Default | Description |
|---|---|---|---|
| `db_path` | string | auto | DB file path. Default: `$XDG_DATA_HOME/senko/projects/<dir-name>/data.db` (typically `~/.local/share/senko/projects/<dir-name>/data.db`). A legacy `<project_root>/.senko/data.db` is migrated to the XDG path on first detection |

## `[backend.postgres]` (requires the `postgres` feature)

| Key | Type | Default | Description |
|---|---|---|---|
| `url` | string | `null` | Connection URL of the form `postgres://user:pass@host:port/dbname?sslmode=require` |
| `url_arn` | string | `null` | ARN to fetch the URL from AWS Secrets Manager (requires the `aws-secrets` feature) |
| `rds_secrets_arn` | string | `null` | ARN of an RDS-format JSON secret (`username`/`password`/`host` required, `port`/`dbname` optional) |
| `sslrootcert` | string | `null` | Path to a TLS root certificate |
| `max_connections` | u32 | sqlx default | Connection-pool cap |

env override: `SENKO_POSTGRES_URL`

> If both `url` and `rds_secrets_arn` are set, the ARN wins.

> **Important: auth modes are pairwise exclusive.**
>
> `[server.auth.api_key]` (`master_key`) / `[server.auth.oidc]` / `[server.auth.trusted_headers]` can only have **one** enabled at a time. Configuring more than one bails at startup. How master privilege is granted differs per mode (see below).

## `[server.auth.api_key]`

Enables API-key authentication. If either `master_key` or `master_key_arn` is set, the server is in **API-key mode** (exclusive with the other modes).

| Key | Type | Default | Description |
|---|---|---|---|
| `master_key` | string | `null` | Direct master key value |
| `master_key_arn` | string | `null` | AWS Secrets Manager ARN for the master key |

env overrides: `SENKO_AUTH_API_KEY_MASTER_KEY` / `SENKO_AUTH_API_KEY_MASTER_KEY_ARN`

Master-key semantics (**only in API-key mode**):

- A privileged key not bound to any User. Sending the `master_key` value as Bearer authenticates as `is_master = true`.
- Used for bootstrap APIs like `POST /api/v1/users`.
- For normal API operations, use per-user keys issued via `POST /users/{id}/api-keys`.
- OIDC / trusted-headers modes have no `master_key` concept; they use `master_group` instead.

## `[server.auth.oidc]`

OIDC auth mode (exclusive with the others). **Users are JIT-provisioned on first auth.**

| Key | Type | Default | Description |
|---|---|---|---|
| `issuer_url` | string | `null` | IdP issuer URL |
| `client_id` | string | `null` | PKCE client_id (used by the senko CLI login) |
| `scopes` | string[] | `["openid","profile"]` | Scopes to request |
| `username_claim` | string | `null` | Which JWT claim to use as username. Fallback when unset: `preferred_username` → `email` → `sub` |
| `required_claims` | map | `{}` | Required claims (key=value equality). All must match, or auth fails |
| `groups_claim` | string | `"groups"` | JWT claim name (array type) used for the `master_group` check |
| `master_group` | string | `null` | JWTs that contain this value in `groups_claim` become `is_master=true`. No super-admin when unset |
| `callback_ports` | string[] | `[]` | Callback ports for CLI login. Individual (`"8400"`) or ranges (`"9000-9010"`) |

## `[server.auth.oidc.session]`

| Key | Type | Default | Description |
|---|---|---|---|
| `ttl` | string | `null` | Absolute TTL (e.g. `"24h"`, `"30d"`) |
| `inactive_ttl` | string | `null` | Inactivity TTL (e.g. `"7d"`) |
| `max_per_user` | u32 | `null` | Per-user session cap |

`null` means unlimited.

## `[server.auth.trusted_headers]`

Header-based authentication for use **behind an API Gateway / reverse proxy**. senko does no token verification and trusts header values unconditionally.

> **⚠️ Security warning**
>
> In `trusted_headers` mode, **never expose senko directly to the Internet**. The API Gateway must be the sole entry point; clients must not be able to send these headers directly.

Exclusive with other modes. **Users are JIT-provisioned on first access** (same as OIDC).

| Key | Type | Default | Description |
|---|---|---|---|
| `subject_header` | string | `null` | **Setting this enables trusted_headers mode**. Header carrying `sub` |
| `name_header` | string | `null` | Display name |
| `display_name_header` | string | `null` | Fallback when `name_header` is absent |
| `email_header` | string | `null` | Email |
| `groups_header` | string | `null` | Header carrying comma-separated group names |
| `master_group` | string | `null` | A value inside `groups_header` that makes the caller `is_master=true`. No super-admin when unset |
| `scope_header` | string | `null` | OAuth scope |
| `oidc_issuer_url` | string | `null` | Returned from `GET /auth/config` (for CLI login) |
| `oidc_client_id` | string | `null` | Same |

## `[server.remote.<action>.hooks.<name>]`

Hooks that fire when a state transition happens under `senko serve` (direct). Same action list as [`[cli.*]` Config](cli.md).

```toml
[server.remote.task_complete.hooks.audit]
command = "logger -t senko-audit 'task complete'"
mode = "async"

[server.remote.task_complete.hooks.metrics]
command = "curl -X POST $METRICS_URL -d 'task_complete=1'"
mode = "async"

[[server.remote.task_complete.hooks.metrics.env_vars]]
name = "METRICS_URL"
required = true
```

Hook schema: [Hooks Reference](../hooks.md).

## Mutual Exclusion of Auth Modes

Only one of `api_key` / `oidc` / `trusted_headers` can be enabled. Configuring more than one is a startup error (the server refuses to start). To switch, remove the keys belonging to the other mode.
