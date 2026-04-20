# Config Setup

Interactively create or improve `.senko/config.toml` for the current project.

## Procedure

### Step 1: Check Existing Config

Check if `.senko/config.toml` exists in the project root.

- **Exists**: Read the file and proceed to **Improve Mode** (Step 3).
- **Does not exist**: Proceed to **Create Mode** (Step 2).

### Step 2: Create Mode

Create a new `.senko/config.toml` by walking through each section with the user.

For each section below, use `AskUserQuestion` to ask the user about their preferences. Skip sections the user doesn't need — only include sections with non-default values.

Walk through sections in this order:

1. **project** — Project name (used for hooks/identification)
2. **user** — User name (for task assignment). **Do not write to config.toml.** Instead, ask the user how they want to set their name:
   - **Option A: config.local.toml** — Ask the user for their name, then write `[user]` section with `name` to `.senko/config.local.toml` (create if it doesn't exist, merge if it does).
   - **Option B: Environment variable `SENKO_USER`** — Do nothing in config-setup. Inform the user they can set `export SENKO_USER="Name"` in their shell profile.
3. **workflow** — How tasks are completed and branches managed:
   - `merge_via`: direct merge or PR-based completion?
   - `auto_merge`: when merge_via=direct, auto-merge branch without user confirmation?
   - `branch_mode`: use git worktrees or regular branches?
   - `merge_strategy`: rebase or squash merge?
   - `branch_template`: custom branch name template? Guide the user through variable selection:
     - `{{id}}` — task ID (always available)
     - `{{slug}}` — kebab-case slug from task title (always available)
     - `{{context.<key>}}` — value resolved from session context at branch-setting time (e.g., `{{context.sprint}}`)
     - `{{<name>:<opt1>|<opt2>|...}}` — enum variable inferred from task content (e.g., `{{prefix:feat|fix|chore}}`)
     - Present example templates: `task/{{id}}-{{slug}}`, `{{prefix:feat|fix|chore}}/{{id}}-{{slug}}`, `{{context.team}}/{{id}}-{{slug}}`
4. **cli.remote** — Remote server connection for CLI client mode (skip if local-only use):
   - `url`: remote server URL (e.g., `http://127.0.0.1:3142`)
   - `token`: API token for authentication with the remote server
   - Note: Sensitive values should preferably be set via environment variables (`SENKO_CLI_REMOTE_URL`, `SENKO_CLI_REMOTE_TOKEN`) rather than stored in config.toml.
5. **backend.sqlite** — Custom SQLite database path (skip if default is fine):
   - `db_path`: path to the SQLite database file
6. **backend.postgres** — PostgreSQL database settings (skip if not using PostgreSQL backend; requires `postgres` feature):
   - `url`: direct connection URL (e.g., `postgres://user:pass@host/db`). Also settable via `--postgres-url` CLI flag
   - `url_arn`: AWS Secrets Manager ARN for connection URL (alternative to direct `url`)
   - `rds_secrets_arn`: AWS Secrets Manager ARN for RDS JSON secret (contains username, password, host, port, dbname)
   - `sslrootcert`: path to SSL root certificate for TLS connections
   - `max_connections`: maximum database pool connections
   - Note: Sensitive values (`url`, `url_arn`, `rds_secrets_arn`) should preferably be set via environment variables (`SENKO_POSTGRES_URL`, `SENKO_POSTGRES_URL_ARN`, `SENKO_POSTGRES_RDS_SECRETS_ARN`) rather than stored in config.toml.
7. **log** — Logging preferences:
   - `level`: trace/debug/info/warn/error
   - `format`: json or pretty
   - `dir`: custom log directory
8. **server** — Server settings for `senko serve` (skip if not running a server):
   - `host`: bind address (default: 127.0.0.1)
   - `port`: port number (default: 3142)
9. **server.auth.api_key** — API key authentication for the server (skip if not using authentication):
   - `master_key`: direct master API key value
   - `master_key_arn`: AWS Secrets Manager ARN for master API key
   - Note: Sensitive values should preferably be set via environment variables (`SENKO_AUTH_API_KEY_MASTER_KEY`, `SENKO_AUTH_API_KEY_MASTER_KEY_ARN`) rather than stored in config.toml.
10. **server.auth.oidc** — OIDC authentication for the server (skip if not using OIDC):
    - `issuer_url`: OIDC issuer URL
    - `client_id`: OIDC client ID for PKCE
    - `scopes`: OIDC scopes (default: `["openid", "profile"]`)
    - `username_claim`: JWT claim to use as username
    - `required_claims`: required JWT claim key-value pairs
    - Sub-sections: `oidc.cli` (callback_ports, browser), `oidc.session` (ttl, inactive_ttl, max_per_user)
11. **web** — Web UI server settings (skip if default is fine):
    - `host`: bind address (default: 127.0.0.1)
    - `port`: port number
12. **hooks** — Task and contract lifecycle hooks. Hooks live under a runtime root and an action/stage name:
    - Runtime root: `[cli.<action>.hooks.<name>]` (CLI), `[server.relay.<action>.hooks.<name>]` (relay proxy), `[server.remote.<action>.hooks.<name>]` (direct server), or `[workflow.<stage>.hooks.<name>]` (skill-emitted plan instructions).
    - CLI/server `<action>`: `task_add`, `task_publish`, `task_start`, `task_complete`, `task_cancel`, `task_select`, `contract_add`, `contract_edit`, `contract_delete`, `contract_dod_check`, `contract_dod_uncheck`, `contract_note_add`. (Use `on_result = "selected" | "none"` on `task_select` to scope; the old `on_no_eligible_task` is replaced by `task_select` + `on_result = "none"`.)
    - Workflow stages accept the same `contract_*` names plus the task-lifecycle stages; contract-stage hooks are emitted inline by `add-task` / `execute-task` / `contract-terminal` at each `senko contract <verb>` call.
    - For each hook: `command`, `when` (`pre` / `post`, default `post`), `mode` (`sync` / `async`, default `async`), `on_failure` (`abort` / `warn` / `ignore`, default `abort`), `enabled`, `env_vars` (list of `{name, required, default}`).

After all sections are covered, generate the TOML and write it to `.senko/config.toml` using the Write tool.

### Step 3: Improve Mode

1. Show the user their current config (read and display the file).
2. Use `AskUserQuestion` to ask which section(s) they want to modify. Present the sections as options:
   - `workflow` — Merge via, merge strategy, branch mode, branch template
   - `cli.remote` — Remote server connection (URL, token)
   - `backend.sqlite` — SQLite database path
   - `backend.postgres` — PostgreSQL database settings (connection URL, RDS secrets, SSL, pool size)
   - `log` — Logging configuration
   - `project` — Project name
   - `user` — User name (stored in config.local.toml or environment variable, not config.toml)
   - `server` — Server host/port for `senko serve`
   - `server.auth.api_key` — API key authentication (master key)
   - `server.auth.oidc` — OIDC authentication (issuer, client ID, sessions)
   - `web` — Web UI server settings (host, port)
   - `hooks` — Task lifecycle hooks
3. For the selected section(s), walk through the same questions as Create Mode, showing current values. For `user`, follow the same Option A / Option B flow as Create Mode (write to config.local.toml or advise on environment variable).
4. Update only the modified sections in the appropriate config file using the Edit tool (config.toml for most sections, config.local.toml for user).

### Notes

- **Scope**: Only project-level config (`.senko/config.toml` and `.senko/config.local.toml`). Do not modify user-level config (`~/.config/senko/config.toml`).
- **User name**: Always write to `.senko/config.local.toml` (git-ignored), never to `.senko/config.toml`. Alternatively, advise using the `SENKO_USER` environment variable.
- **Defaults**: Only write sections/keys where the user wants non-default values. Comment out defaults for reference.
- **Validation**: Ensure values are valid (e.g., `merge_via` must be `direct` or `pr`, `log.format` must be `json` or `pretty`).
- **Sensitive values**: Recommend environment variables over config.toml for secrets (`cli.remote.token` → `SENKO_CLI_REMOTE_TOKEN`, `server.relay.token` → `SENKO_SERVER_RELAY_TOKEN`, `server.auth.api_key.master_key` → `SENKO_AUTH_API_KEY_MASTER_KEY`, `server.auth.api_key.master_key_arn` → `SENKO_AUTH_API_KEY_MASTER_KEY_ARN`, PostgreSQL URLs).
- **Hooks**: Each hook entry needs a unique name under its runtime + action key (e.g., `[cli.task_publish.hooks.my-hook]` or `[workflow.branch_merge.hooks.mise_check]`).
