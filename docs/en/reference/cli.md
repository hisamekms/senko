# CLI Reference

Complete list of `senko` subcommands.

## Global Options

```
--output <FORMAT>       json or text (default: json)
--project-root <PATH>   project root (auto-detected when omitted)
--config <PATH>         config file path (env: SENKO_CONFIG, default: .senko/config.toml)
--dry-run               show what would happen without executing (state-changing commands only)
--log-dir <PATH>        log output directory (default: $XDG_STATE_HOME/senko)
--db-path <PATH>        SQLite DB file path (env: SENKO_DB_PATH)
--postgres-url <URL>    PostgreSQL connection URL (env: SENKO_POSTGRES_URL)
--project <NAME>        project to operate on (env: SENKO_PROJECT)
--user <NAME>           user to act as (env: SENKO_USER)
```

> Place `--output` and `--dry-run` **before the subcommand**: `senko --output text task list`.

## Command Overview

| Aggregate | Subcommands |
|---|---|
| Task | `senko task add/list/get/next/ready/start/edit/complete/cancel/dod/deps` |
| Contract | `senko contract add/list/get/edit/delete/dod/note` |
| Project | `senko project list/create/delete/metadata-field/members` |
| User | `senko user list/create/update/delete` |
| Auth | `senko auth login/token/status/logout/sessions/revoke` |
| Hooks | `senko hooks log/test` |
| Mode commands | `senko serve` / `senko web` / `senko config` / `senko doctor` / `senko skill-install` |

## `senko task`

### `task add`

```bash
senko task add --title "..." [--priority p2] [--background ...] [--description ...] \
               [--definition-of-done ...] [--in-scope ...] [--out-of-scope ...] \
               [--tag ...] [--depends-on <id>] [--branch ...] [--metadata '{...}'] \
               [--assignee-user-id self|<id>]

# Bulk create from JSON
echo '{"title":"...", ...}' | senko task add --from-json
senko task add --from-json-file task.json
```

- New tasks start as `draft`.
- Default priority: `p2`.
- `--depends-on` is repeatable (`--depends-on 3 --depends-on 5`).

### `task list`

```bash
senko task list                          # everything (default limit 50)
senko task list --status todo            # filter by status (repeatable)
senko task list --ready                  # todo with all deps completed
senko task list --tag backend            # filter by tag (repeatable)
senko task list --contract 42            # filter by Contract
senko task list --metadata "team=backend"
senko task list --id-min 100 --id-max 199
senko task list --limit 20 --offset 40   # limit: 1..=200 (default 50) / offset: default 0
senko task list --ready --include-unassigned
```

### `task get <id>`

Task detail (JSON only).

### `task next`

```bash
senko task next [--session-id <id>] [--metadata '{...}'] [--include-unassigned]
```

From the ready set, picks one by **priority → created_at → id** and transitions it to `in_progress`.

### `task publish <id>` / `task start <id>`

Manual state transitions:

- `task publish`: `draft → todo`
- `task start`: `todo → in_progress`

### `task edit <id>`

```bash
# Scalar updates
senko task edit 1 --title "..." --description "..." --plan "..." --priority p0
senko task edit 1 --branch "feature/x" --pr-url "https://..."
senko task edit 1 --contract 42

# Clear
senko task edit 1 --clear-description --clear-plan --clear-branch --clear-pr-url
senko task edit 1 --clear-contract --clear-assignee-user-id

# Arrays: set / add / remove (tag / definition-of-done / in-scope / out-of-scope)
senko task edit 1 --set-tags "a" "b" "c"
senko task edit 1 --add-tag x --add-tag y
senko task edit 1 --remove-tag old

# Metadata
senko task edit 1 --metadata '{"key":"value"}'          # shallow merge
senko task edit 1 --replace-metadata '{"only":"this"}'  # full replace
senko task edit 1 --clear-metadata

# Assignee
senko task edit 1 --assignee-user-id self
senko task edit 1 --assignee-user-id 3
```

### `task complete <id>`

```bash
senko task complete 1                # in_progress → completed (errors if DoD incomplete)
senko task complete 1 --skip-pr-check  # skip PR verification in merge_via=pr setups
```

### `task cancel <id>`

```bash
senko task cancel 1 [--reason "..."]
```

### `task dod`

```bash
senko task dod check <task_id> <index>     # 1-based index
senko task dod uncheck <task_id> <index>
```

### `task deps`

```bash
senko task deps add <task_id> --on <dep_id>
senko task deps remove <task_id> --on <dep_id>
senko task deps set <task_id> --on <id1> <id2> ...
senko task deps list <task_id>
```

## `senko contract`

```bash
senko contract add --title "..." [--description ...] [--definition-of-done ...] \
                   [--tag ...] [--metadata '{...}']
senko contract add --from-json / --from-json-file <path>

senko contract list [--tag ...]
senko contract get <id>

senko contract edit <id> --title ... --description ...
                         --set-tags / --add-tag / --remove-tag
                         --set-definition-of-done / --add-definition-of-done / --remove-definition-of-done
                         --metadata / --replace-metadata / --clear-metadata
                         --clear-description

senko contract delete <id>

senko contract dod check <contract_id> <index>
senko contract dod uncheck <contract_id> <index>

senko contract note add <contract_id> --content "..." [--source-task <task_id>]
senko contract note list <contract_id>
```

## `senko project`

```bash
senko project list
senko project create --name <name> [--description ...]
senko project delete <id>
```

### `project metadata-field`

```bash
senko project metadata-field add --name <name> --type string|number|boolean \
                                 [--required-on-complete] [--description ...]
senko project metadata-field list
senko project metadata-field remove --name <name>
```

### `project members`

```bash
senko project members list
senko project members add --user-id <id> [--role owner|member|viewer]
senko project members remove --user-id <id>
senko project members set-role --user-id <id> --role owner|member|viewer
```

## `senko user`

```bash
senko user list
senko user create --username <name> [--sub <oidc-sub>] [--display-name ...] [--email ...]
senko user update <id> [--username ...] [--display-name ...]
senko user delete <id>
```

## `senko auth`

```bash
senko auth login [--device-name <name>]   # OIDC browser login; token goes to the keychain
senko auth token                           # print the stored token to stdout (for scripting)
senko auth status                          # current login info
senko auth logout                          # revoke current session + remove from keychain
senko auth sessions                        # list my sessions
senko auth revoke <id>                     # revoke a specific session
senko auth revoke --all                    # revoke every session
```

## `senko hooks`

```bash
senko hooks log [-n 20] [-f] [--clear] [--path]
senko hooks test <event_name> [task_id] [--dry-run]
```

Accepted `event_name` values:
`task_add` / `task_publish` / `task_start` / `task_complete` / `task_cancel` / `task_select` /
`contract_add` / `contract_edit` / `contract_delete` / `contract_dod_check` / `contract_dod_uncheck` / `contract_note_add`

## `senko serve` / `senko web`

```bash
senko serve [--port 3142] [--host 127.0.0.1]            # REST API server
senko web   [--port 3141] [--host 127.0.0.1]            # read-only web viewer
```

> There's no dedicated flag to enable relay mode. When `[server.relay] url` (env: `SENKO_SERVER_RELAY_URL`) is set, `senko serve` automatically runs as a relay that forwards to the upstream.

Environment variables:

| Variable | Purpose | Default |
|---|---|---|
| `SENKO_PORT` | Port for both `web` and `serve` | 3141 (web) / 3142 (serve) |
| `SENKO_HOST` | Bind address for both `web` and `serve` | 127.0.0.1 |
| `SENKO_SERVER_PORT` | Server-only port | 3142 |
| `SENKO_SERVER_HOST` | Server-only bind address | 127.0.0.1 |

## `senko config`

```bash
senko config            # show the current merged config as JSON
senko config --init     # emit a template to stdout
```

## `senko doctor`

Health-checks config / hooks / migrations. Warns about things like runtime-mismatched hooks and the `pre+async+abort` combination.

## `senko skill-install`

```bash
senko skill-install [--output-dir .claude] [--yes] [--force]
```

- Default: install `SKILL.md` under the current project at `.claude/skills/senko/`.
- `--yes`: skip confirmation prompts.
- `--force`: wipe senko-owned directories before installing, for a clean install.

## Environment Variables (Selection)

| Variable | Purpose |
|---|---|
| `SENKO_CONFIG` | Config file path |
| `SENKO_PROJECT_ROOT` | Project root |
| `SENKO_PROJECT` | Project to operate on |
| `SENKO_USER` | User to act as |
| `SENKO_DB_PATH` | SQLite DB path |
| `SENKO_POSTGRES_URL` | PostgreSQL connection URL |
| `SENKO_CLI_REMOTE_URL` | Remote server URL |
| `SENKO_CLI_REMOTE_TOKEN` | API token for remote access |
| `SENKO_SERVER_RELAY_URL` | Relay → upstream URL |
| `SENKO_SERVER_RELAY_TOKEN` | Relay → upstream auth token |
| `SENKO_AUTH_API_KEY_MASTER_KEY` | Master API key (direct value) |
| `SENKO_AUTH_API_KEY_MASTER_KEY_ARN` | Master API key via AWS Secrets Manager ARN |
| `SENKO_LOG_DIR` | Hook log output directory |

## State Transitions

```
draft → todo → in_progress → completed
                 ↓
              canceled       (allowed from any active state)
```

- Forward only (reverse / self-loops are rejected).
- `senko task next` only does `todo → in_progress`.
- `senko task complete` only does `in_progress → completed`.
