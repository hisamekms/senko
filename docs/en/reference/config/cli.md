# `[cli.*]` Config

Sections active when running as the local CLI binary (i.e. anything other than `senko serve`).

## `[cli]`

| Key | Type | Default | Description |
|---|---|---|---|
| `browser` | bool | `true` | Whether `senko auth login` opens a browser automatically |

## `[cli.remote]`

The upstream the CLI connects to **instead of a local DB**. Setting this disables local SQLite / PostgreSQL and routes every operation over HTTP.

| Key | Type | Default | Description |
|---|---|---|---|
| `url` | string | `null` | Remote server URL (e.g. `https://senko.example.com`) |
| `token` | string | `null` | API key or OIDC session token |

env overrides: `SENKO_CLI_REMOTE_URL` / `SENKO_CLI_REMOTE_TOKEN`

If you don't want the token hard-coded:

- Inject it via env.
- Put it in `.senko/config.local.toml` (gitignored).
- Use `senko auth login` so the token lands in the OS keychain; keep only the URL in config.

## `[cli.<action>.hooks.<name>]`

Hooks that fire while the active runtime is `cli`.

Actions:

- Task: `task_add` / `task_ready` / `task_start` / `task_complete` / `task_cancel` / `task_select`
- Contract: `contract_add` / `contract_edit` / `contract_delete` / `contract_dod_check` / `contract_dod_uncheck` / `contract_note_add`

```toml
[cli.task_complete.hooks.notify]
command = "notify-send 'senko: task done'"
mode = "async"
on_failure = "ignore"

[cli.task_select.hooks.prompt_for_add]
command = "echo 'No ready tasks'"
on_result = "none"
```

See [Hooks Reference](../hooks.md) for hook field details.

## Key Environment Variables

| Variable | Corresponding key |
|---|---|
| `SENKO_CLI_REMOTE_URL` | `[cli.remote] url` |
| `SENKO_CLI_REMOTE_TOKEN` | `[cli.remote] token` |
| `SENKO_PROJECT` | `[project] name` |
| `SENKO_USER` | `[user] name` |
| `SENKO_DB_PATH` | `[backend.sqlite] db_path` |

## Common Patterns

### Solo development, local DB + desktop notifications

```toml
[cli.task_complete.hooks.notify]
command = "notify-send 'done' '$SENKO_TASK_TITLE'"
mode = "async"
```

(`SENKO_TASK_TITLE` is only injected if declared via `env_vars`. For direct access, parsing the stdin JSON with `jq` is more reliable.)

### Remote (OIDC)

```toml
[cli.remote]
url = "https://senko.example.com"

[cli]
browser = true
```

The token lands in the keychain via `senko auth login`.

### Bot execution from CI

For production, inject a JWT obtained via OIDC Client Credentials (M2M) through env. Only the URL goes into the config file:

```toml
[cli.remote]
url = "https://senko.example.com"
```

CI job:

```bash
# Fetch a JWT via M2M
JWT=$(curl -s https://accounts.example.com/oauth/token \
  -H "Content-Type: application/json" \
  -d '{
    "client_id":     "senko-bot",
    "client_secret": "'"$SENKO_BOT_CLIENT_SECRET"'",
    "audience":      "https://senko.example.com",
    "grant_type":    "client_credentials"
  }' | jq -r '.access_token')

export SENKO_CLI_REMOTE_TOKEN="$JWT"
senko task list --status todo --output json
```

Full walkthrough: the "CI / Bots (OAuth Client Credentials / M2M)" section in [OIDC Authentication](../../guides/server-remote/auth-oidc.md). Evaluation-grade API-key usage: [API Key Authentication](../../guides/server-remote/auth-api-key.md).
