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
--attr KEY=VALUE        attribute to attach to trace/baggage (repeatable, malformed input errors out)
```

> Global options can be placed **before or after the subcommand**: both `senko --output text task list` and `senko task list --output text` are accepted.

> For the full behavior of `--attr` / `SENKO_TRACE_ATTRIBUTES` / `OTEL_RESOURCE_ATTRIBUTES` — precedence, reserved-namespace exclusion — see the [Tracing Reference](tracing.md).

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
| Developer-only (`dev-tools` feature) | `senko dev seed [reset\|append]` |

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
senko task list                          # default limit 50
senko task list --status todo            # filter by status (repeatable)
senko task list --ready                  # todo with all deps completed
senko task list --tag backend            # filter by tag (repeatable)
senko task list --contract 42            # filter by Contract
senko task list --metadata "team=backend"
senko task list --id-min 100 --id-max 199
senko task list --limit 20               # limit: 1..=200 (default 50)
senko task list --limit 20 --after <cursor>   # fetch next page via opaque cursor
senko task list --ready --include-unassigned
```

**Response shape (JSON)**

```json
{
  "items": [ { "id": 1, "title": "...", ... }, ... ],
  "next_cursor": "eyJpZCI6MjB9"
}
```

`next_cursor` is `null` when there are no more results. Pass it back as `--after <cursor>` to fetch the next page. Cursors are opaque — do not decode them by hand.

In `--output text`, the `... more: --after <cursor>` line is appended at the end of the task list whenever `next_cursor` is set.

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
senko task deps list <task_id> [--limit 20] [--after <cursor>]
```

`task deps list` returns `{items, next_cursor}` — same shape and cursor semantics as `task list`.

## `senko contract`

```bash
senko contract add --title "..." [--description ...] [--definition-of-done ...] \
                   [--tag ...] [--metadata '{...}']
senko contract add --from-json / --from-json-file <path>

senko contract list [--tag ...] [--limit 20] [--after <cursor>]
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
senko contract note list <contract_id> [--limit 20] [--after <cursor>]
```

`contract list` and `contract note list` return `{items, next_cursor}` — same cursor semantics as `task list`.

## `senko project`

```bash
senko project list [--limit 20] [--after <cursor>]
senko project create --name <name> [--description ...]
senko project delete <id>
```

### `project metadata-field`

```bash
senko project metadata-field add --name <name> --type string|number|boolean \
                                 [--required-on-complete] [--description ...]
senko project metadata-field list [--limit 20] [--after <cursor>]
senko project metadata-field remove --name <name>
```

### `project members`

```bash
senko project members list [--limit 20] [--after <cursor>]
senko project members add --user-id <id> [--role owner|member|viewer]
senko project members remove --user-id <id>
senko project members set-role --user-id <id> --role owner|member|viewer
```

## `senko user`

```bash
senko user list [--limit 20] [--after <cursor>]
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
senko auth sessions [--limit 20] [--after <cursor>]   # list my sessions
senko auth revoke <id>                     # revoke a specific session
senko auth revoke --all                    # revoke every session
```

> **Pagination note.** All the list commands above (`project list`, `project metadata-field list`, `project members list`, `user list`, `auth sessions`) return `{items, next_cursor}` — same shape and cursor rules as `task list`. In `--output text` a trailing `... more: --after <cursor>` line is appended whenever there is another page. Expired sessions are filtered in-memory after each page is fetched, so `auth sessions` pages may contain fewer items than `--limit` — keep following `next_cursor` until it is `null`.

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

## `senko dev` (developer-only, `dev-tools` feature)

> **Not available in the public binary.** `senko dev` is compiled in only when senko is built with `cargo build --features dev-tools`. The release binary published via `cargo install senko` does not contain it.

### `senko dev seed [reset|append]`

Loads a deterministic sample dataset (3 users / 5 contracts / 60 tasks / 30 dependency edges / 15 contract notes / DoD) for local development and the e2e test harness.

```bash
# Keep existing data; load fixtures only if the DB has no seeded data yet (default).
senko dev seed
senko dev seed append

# Wipe all senko-managed rows and load the fixtures fresh
# (the bootstrap rows for the default project/user with id=1 are preserved).
senko dev seed reset
```

- **Backends**: works against SQLite and PostgreSQL. Refused if the config points at a remote URL (`cli.remote.url`).
- **Idempotency**: every seeded entity carries the `seed` tag, so `append` short-circuits to a noop on an already-seeded DB.
- **Reset is destructive**: deletes rows from `tasks`, `contracts`, `contract_notes`, `task_dependencies`, `task_definition_of_done`, `task_tags`, `metadata_fields`, `api_keys`, `project_members`, `users (id != 1)`, `projects (id != 1)`, etc. Do NOT run it against a production database.
- **Write path**: calls domain repositories directly and bypasses the application/service layer, so no hook events or event-store entries are emitted by the seed itself.

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
