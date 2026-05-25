# Config Explain

Explain the current senko configuration values and their meanings.

## Procedure

### Step 1: Get Current Config

```bash
senko config
```

Parse the JSON output to extract all configuration sections.

### Step 2: Explain Each Section

For each section, explain every item's **current value**, whether it's the **default**, what it **means**, and the **available options**.

#### Sections

**workflow**
| Key | Default | Options | Description |
|---|---|---|---|
| `merge_via` | `direct` | `direct`, `pr` | Controls whether the branch is merged directly or via PR. `pr` requires a PR URL and merge status check. |
| `auto_merge` | `true` | `true`, `false` | Applies only to `merge_via = "direct"`. Controls whether the branch is merged automatically or requires user confirmation. Has no effect when `merge_via = "pr"`. |
| `branch_mode.type` | `worktree` | `worktree`, `branch` | How task branches are realized. `worktree` uses git worktrees (parallel work), `branch` uses regular branches in the current checkout. |
| `branch_mode.create` | `true` | `true`, `false` | Whether the skill provisions a new resource. `false` reuses externally-provisioned resources: with `type="worktree"` the skill stops if the worktree is missing (no fallback); with `type="branch"` the skill works on the current branch without any branch operations. The legacy string form `branch_mode = "worktree"` / `"branch"` is accepted and expands to `{ type=<value>, create=true }`. |
| `merge_strategy` | `rebase` | `rebase`, `squash` | Git merge strategy when merging task branches back to main. |
| `branch_template` | `null` | string | Template for branch names. Variables: `{{id}}`, `{{slug}}`, `{{context.<key>}}` (from session context), `{{<name>:<opt1>\|<opt2>\|...}}` (enum, inferred from task). Example: `{{prefix:feat\|fix\|chore}}/{{id}}-{{slug}}`. |

**workflow stages** — built-in names consumed by the skill:

```
task_add               task_publish           task_start             task_complete
task_cancel            task_select            branch_set             branch_cleanup
branch_merge           pr_create              pr_update              plan
implement              contract_add           contract_edit          contract_delete
contract_dod_check     contract_dod_uncheck   contract_note_add
```

The `contract_*` stages are emitted ad-hoc from workflow markdown (`add-task`, `execute-task`, `contract-terminal`) at each `senko contract <verb>` call site, not from `generate-plan-sections.sh`.

User-defined stage names are also accepted — unknown stages are preserved in the `senko config` output so external scripts can consume them. The skill only fires on the built-in names above.

Each stage supports:
| Key | Default | Description |
|---|---|---|
| `instructions` | `[]` | Text instructions for the agent at this stage. |
| `hooks` | `{}` | Named HookDef entries under `[workflow.<stage>.hooks.<name>]`. See the **hooks** section below for the full HookDef shape. |
| `metadata_fields` | `[]` | Metadata fields collected at this stage. Values are shallow-merged into the task's metadata. |

Stage-specific keys (unknown keys are preserved as pass-through extras):
| Stage | Extra Keys | Description |
|---|---|---|
| `workflow.task_add` | `default_dod`, `default_tags`, `default_priority` | Defaults applied when creating new tasks. |
| `workflow.plan` | `required_sections` | Required sections in the implementation plan. |

> **Note**: The old `pre_hooks` / `post_hooks` arrays have been removed. Use the `hooks` map with `when = "pre"` or `when = "post"` on each HookDef instead.

**backend.sqlite**
| Key | Default | Options | Description |
|---|---|---|---|
| `db_path` | auto (`$XDG_DATA_HOME/senko/projects/<hash>/data.db`) | file path | Path to the SQLite database file. |

**backend.postgres** (requires `postgres` feature)
| Key | Default | Options | Description |
|---|---|---|---|
| `url` | `null` | URL string | PostgreSQL connection URL (e.g., `postgres://user:pass@host/db`). Also settable via `--postgres-url` CLI flag. |
| `url_arn` | `null` | ARN string | AWS Secrets Manager ARN for connection URL. Resolved at startup (requires `aws-secrets` feature). |
| `rds_secrets_arn` | `null` | ARN string | AWS Secrets Manager ARN for RDS JSON secret. The JSON must contain `username`, `password`, `host`, and optionally `port` (default 5432) and `dbname` (default `postgres`). |
| `sslrootcert` | `null` | file path | Path to SSL root certificate for TLS connections. When used with `rds_secrets_arn`, appends `sslmode=verify-full` to the built URL. |
| `max_connections` | `null` | positive integer | Maximum number of connections in the database pool. |

**server**
| Key | Default | Options | Description |
|---|---|---|---|
| `host` | `127.0.0.1` | IP address | Bind address for `senko serve`. |
| `port` | `3142` | port number | Port for `senko serve`. |

**server.auth.api_key**
| Key | Default | Options | Description |
|---|---|---|---|
| `master_key` | `null` | string | Direct master API key value for authentication. |
| `master_key_arn` | `null` | ARN string | AWS Secrets Manager ARN for master API key. Resolved at startup (requires `aws-secrets` feature). |

**server.auth.oidc**
| Key | Default | Options | Description |
|---|---|---|---|
| `issuer_url` | `null` | URL string | OIDC issuer URL for JWT verification. |
| `client_id` | `null` | string | OIDC client ID for PKCE authentication. |
| `scopes` | `["openid", "profile"]` | list of strings | OIDC scopes to request. |
| `username_claim` | `null` | string | JWT claim to use as username. |
| `required_claims` | `{}` | map of string to string | Required JWT claims (key-value pairs that must match). |
| `callback_ports` | `[]` (empty) | list of port strings | Local ports for OIDC callback during CLI login. Supports individual ports and ranges (e.g., `["8400", "9000-9010"]`). |

**server.auth.oidc.session**
| Key | Default | Options | Description |
|---|---|---|---|
| `ttl` | `null` | duration string | Session time-to-live (e.g., `24h`). |
| `inactive_ttl` | `null` | duration string | Session inactive timeout (e.g., `7d`). |
| `max_per_user` | `null` | positive integer | Maximum number of sessions per user. |

**cli**
| Key | Default | Options | Description |
|---|---|---|---|
| `browser` | `true` | `true`, `false` | Auto-open browser for OIDC login. |

**cli.remote**
| Key | Default | Options | Description |
|---|---|---|---|
| `url` | `null` | URL string | Remote server URL for CLI client mode. When set, CLI forwards commands to this server. |
| `token` | `null` | string | API token for authentication with the remote server. |

**log**
| Key | Default | Options | Description |
|---|---|---|---|
| `dir` | auto (`$XDG_STATE_HOME/senko`) | directory path | Directory for log files. |
| `level` | `info` | `trace`, `debug`, `info`, `warn`, `error` | Minimum log level. |
| `format` | `json` | `json`, `pretty` | Log output format. |

**project**
| Key | Default | Options | Description |
|---|---|---|---|
| `name` | `null` (auto-detected) | string | Project name. Used for hook environment variables and identification. |

**user**
| Key | Default | Options | Description |
|---|---|---|---|
| `name` | `null` (auto-detected) | string | User name for task assignment. |

**hooks**

Hooks live under three runtime roots (fired only on the matching process) plus workflow stages:

- `[cli.<action>.hooks.<name>]` — fired by the CLI process on task state transitions.
- `[server.relay.<action>.hooks.<name>]` — fired by `senko serve` when running in Relay mode (enabled by setting `server.relay.url`).
- `[server.remote.<action>.hooks.<name>]` — fired by the direct server (`senko serve`).
- `[workflow.<stage>.hooks.<name>]` — guidance hooks emitted into the plan by the skill at a workflow stage; agents execute them as plan instructions.

CLI / server actions (fixed):

| Action | Fires When |
|---|---|
| `task_add` | A new task is created (draft). |
| `task_publish` | A task moves to `todo`. |
| `task_start` | A task moves to `in_progress`. |
| `task_complete` | A task is completed. |
| `task_cancel` | A task is canceled. |
| `task_select` | `senko task next` runs (use `on_result = "selected"` or `"none"` to scope). |
| `contract_add` | A new contract is created. |
| `contract_edit` | A contract's fields are edited. |
| `contract_delete` | A contract is deleted. |
| `contract_dod_check` | A contract DoD item is checked. |
| `contract_dod_uncheck` | A contract DoD item is unchecked. |
| `contract_note_add` | A note is added to a contract. |

Each HookDef supports:

| Key | Default | Options | Description |
|---|---|---|---|
| `command` | required (unless `prompt` is set for workflow stages) | shell string | Command to execute. Stdin receives the hook envelope JSON. |
| `when` | `post` | `pre`, `post` | Fire before or after the state transition / stage. |
| `mode` | `async` | `sync`, `async` | `sync` waits for completion; `async` detaches. Only `sync` + `pre` + `on_failure = "abort"` can abort the state transition. |
| `on_failure` | `abort` | `abort`, `warn`, `ignore` | `abort` only takes effect in `sync` + `pre`; otherwise acts as `warn`. |
| `enabled` | `true` | `true`, `false` | Disabled hooks are skipped entirely. |
| `env_vars` | `[]` | `[ { name, required (default true), default, description } ]` | Env vars passed to the child. Missing required-no-default vars cause skip + warn. |
| `on_result` | `any` | `any`, `selected`, `none` | `task_select` only — filter by result branch. |
| `prompt` | _(none)_ | string | Workflow-stage hooks can use `prompt` to render an agent instruction instead of (or in addition to) `command`. |

> The global `[hooks].enabled` switch and `SENKO_HOOKS_ENABLED` / `SENKO_HOOK_ON_TASK_*` env vars have been removed. Hooks are defined only in `config.toml`. Runtime sections that don't match the current process (e.g., `[server.relay.*]` hooks on a CLI invocation) are ignored with a startup warning.

### Step 3: Explain Config Layering

Explain how configuration is resolved (highest priority first):
1. **CLI flags** (`--config <path>`)
2. **Environment variables** (`SENKO_*` — see table below)
3. **Local config** (`.senko/config.local.toml` — git-ignored, per-user overrides)
4. **Project config** (`.senko/config.toml` in the project root)
5. **User config** (`~/.config/senko/config.toml`)

Higher-priority sources override lower ones. The `senko config` output shows the **merged** result.

#### Environment Variables

| Variable | Config Key | Notes |
|---|---|---|
| `SENKO_MERGE_VIA` | `workflow.merge_via` | |
| `SENKO_AUTO_MERGE` | `workflow.auto_merge` | |
| `SENKO_BRANCH_MODE_TYPE` | `workflow.branch_mode.type` | `worktree` \| `branch` |
| `SENKO_BRANCH_MODE_CREATE` | `workflow.branch_mode.create` | `true`/`false`/`1`/`0`/`yes`/`no` |
| `SENKO_MERGE_STRATEGY` | `workflow.merge_strategy` | |
| `SENKO_CLI_REMOTE_URL` | `cli.remote.url` | |
| `SENKO_CLI_REMOTE_TOKEN` | `cli.remote.token` | |
| `SENKO_SERVER_RELAY_URL` | `server.relay.url` | |
| `SENKO_SERVER_RELAY_TOKEN` | `server.relay.token` | |
| `SENKO_AUTH_API_KEY_MASTER_KEY` | `server.auth.api_key.master_key` | |
| `SENKO_AUTH_API_KEY_MASTER_KEY_ARN` | `server.auth.api_key.master_key_arn` | |
| `SENKO_OIDC_ISSUER_URL` | `server.auth.oidc.issuer_url` | |
| `SENKO_OIDC_CLIENT_ID` | `server.auth.oidc.client_id` | |
| `SENKO_OIDC_USERNAME_CLAIM` | `server.auth.oidc.username_claim` | |
| `SENKO_OIDC_CALLBACK_PORTS` | `server.auth.oidc.callback_ports` | Comma-separated |
| `SENKO_AUTH_OIDC_SESSION_TTL` | `server.auth.oidc.session.ttl` | |
| `SENKO_AUTH_OIDC_SESSION_INACTIVE_TTL` | `server.auth.oidc.session.inactive_ttl` | |
| `SENKO_AUTH_OIDC_SESSION_MAX_PER_USER` | `server.auth.oidc.session.max_per_user` | Parsed as u32 |
| `SENKO_SERVER_HOST` | `server.host` | |
| `SENKO_SERVER_PORT` | `server.port` | Parsed as u16 |
| `SENKO_POSTGRES_URL` | `backend.postgres.url` | |
| `SENKO_POSTGRES_URL_ARN` | `backend.postgres.url_arn` | |
| `SENKO_POSTGRES_RDS_SECRETS_ARN` | `backend.postgres.rds_secrets_arn` | |
| `SENKO_POSTGRES_SSLROOTCERT` | `backend.postgres.sslrootcert` | |
| `SENKO_POSTGRES_MAX_CONNECTIONS` | `backend.postgres.max_connections` | Parsed as u32 |
| `SENKO_USER` | `user.name` | |
| `SENKO_PROJECT` | `project.name` | |
| `SENKO_DB_PATH` | `backend.sqlite.db_path` | |
| `SENKO_LOG_DIR` | `log.dir` | |
| `SENKO_LOG_LEVEL` | `log.level` | |
| `SENKO_LOG_FORMAT` | `log.format` | |
| `SENKO_HOST` | `server.host` | Bind address for `senko serve` |
| `SENKO_PORT` | `server.port` | Port for `senko serve` |

### Step 4: Present to User

Format the explanation clearly, highlighting:
- Values that differ from defaults
- Any potentially important settings (e.g., `merge_via`, configured hooks under `[cli.*]` / `[server.*]` / `[workflow.*]`)
- Hooks that are currently configured
