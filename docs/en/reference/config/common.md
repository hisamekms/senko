# `[project]` / `[user]` / `[log]` / `[web]` Config

Common sections that are always read regardless of runtime.

## `[project]`

| Key | Type | Default | Description |
|---|---|---|---|
| `name` | string | `null` | Project to operate on. Defaults to `default` (id=1) when unset |

env override: `SENKO_PROJECT`
CLI override: `--project <name>`

In remote mode (`[cli.remote]` set), the name must refer to a project that actually exists on the server — otherwise 404.

## `[user]`

| Key | Type | Default | Description |
|---|---|---|---|
| `name` | string | `null` | User to act as. Defaults to `default` (id=1) when unset |

env override: `SENKO_USER`
CLI override: `--user <name>`

Used to resolve things like `task add --assignee-user-id self` and the hook envelope's `user` field.

## `[log]`

| Key | Type | Default | Description |
|---|---|---|---|
| `dir` | string | `$XDG_STATE_HOME/senko` | Log-file directory |
| `level` | string | `"info"` | `trace` / `debug` / `info` / `warn` / `error` |
| `format` | string | `"json"` | `"json"` or `"pretty"` |
| `hook_output` | string | `"file"` | `"file"` / `"stdout"` / `"both"` — where hook stdout/stderr go |

env override: `SENKO_LOG_DIR` (for `dir` only).
CLI override: `--log-dir <path>`.

`hook_output`:

- `file`: hook output goes only to the log file (not the console).
- `stdout`: stream to the CLI console.
- `both`: both.

For debugging, `--log-dir` + `[log] level = "debug"` is handy.

## `[web]`

For `senko web` (the read-only web viewer).

| Key | Type | Default | Description |
|---|---|---|---|
| `host` | string | `127.0.0.1` | Bind address |
| `port` | u16 | `3141` | Port |

env overrides: `SENKO_HOST` / `SENKO_PORT` (shared with `serve`).

> `senko web` is **an unauthenticated read-only viewer**. It's intended for internal networks, not public exposure.
