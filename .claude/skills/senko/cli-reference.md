# senko CLI Reference

All commands use `senko`. Default output is JSON. The `--output` flag is **global** and must precede the subcommand.

Task-related operations are grouped under `senko task <verb>`, mirroring `senko contract <verb>`.

```bash
# Add a task (created in draft status)
senko task add --title "Title" --priority p1 --description "Description"

# List tasks (with filters) — response is { items, next_cursor }
senko --output text task list
senko task list --status todo
senko task list --status in_progress
senko task list --status todo --status in_progress  # combine multiple filters
senko task list --ready                 # todo tasks with all deps completed
senko task list --tag backend
senko task list --limit 20              # limit: 1..=200 (default 50)
senko task list --limit 20 --after <cursor>  # fetch next page using cursor from previous response

# Get task details (JSON only, no text output)
senko task get <id>

# Auto-select next task (highest-priority ready task → in_progress)
senko task next

# Status transitions (dedicated commands)
senko task publish <id>                     # draft → todo
senko task start <id>                       # todo → in_progress
senko task resume <id> [--session-id <id>] [--metadata '{...}']  # session/metadata refresh on an in_progress task (no status change; rejects any non-in_progress status)
senko task complete <id>                    # in_progress → completed (fails if unchecked DoD)
senko task complete <id> --skip-pr-check    # bypass PR merge/review checks
senko task cancel <id> --reason "Reason text"  # any active → canceled

# Edit task fields (no status changes — use dedicated commands above)
senko task edit <id> --title "New Title" --add-tag backend
senko task edit <id> --add-definition-of-done "Write unit tests"
senko task edit <id> --pr-url "https://github.com/org/repo/pull/42"
senko task edit <id> --plan-file /path/to/plan.md  # read plan from file

# Metadata
senko task edit <id> --metadata '{"key":"val"}'         # shallow-merge into existing metadata
senko task edit <id> --replace-metadata '{"key":"val"}'  # replace entire metadata
senko task edit <id> --clear-metadata                    # remove all metadata

# Definition of Done (DoD) check/uncheck (1-based index)
senko task dod check <task_id> <index>      # mark DoD item as done
senko task dod uncheck <task_id> <index>    # unmark DoD item

# Dependencies
senko task deps add <task_id> --on <dep_id>
senko task deps remove <task_id> --on <dep_id>
senko task deps set <task_id> --on <dep_id1> <dep_id2>
senko task deps list <task_id> [--limit 20] [--after <cursor>]  # { items, next_cursor }

# Contract — independent aggregate shared by sub-tasks of a split
senko contract add --title "Title" [--description "..."] [--definition-of-done "..." ...] [--tag ...] [--metadata '{...}']
senko contract add --from-json                       # read JSON from stdin
senko contract add --from-json-file <path>
senko contract list [--tag <tag>] [--limit 20] [--after <cursor>]  # { items, next_cursor }
senko contract get <id>
senko contract edit <id> --title "New" --add-tag demo --add-definition-of-done "Verify X"
senko contract edit <id> --description "..." | --clear-description
senko contract edit <id> --metadata '{"k":"v"}' | --replace-metadata '{...}' | --clear-metadata
senko contract edit <id> --set-tags t1 t2 | --add-tag t | --remove-tag t
senko contract edit <id> --set-definition-of-done "a" "b" | --add-definition-of-done "c" | --remove-definition-of-done "a"
senko contract delete <id>

# Contract DoD (1-based index, same semantics as task DoD)
senko contract dod check <contract_id> <index>
senko contract dod uncheck <contract_id> <index>

# Contract notes (append-only, timestamped by server)
senko contract note add <contract_id> --content "..." [--source-task <task_id>]
senko contract note list <contract_id> [--limit 20] [--after <cursor>]  # { items, next_cursor }

# Link a task to a contract (reuses the existing `edit` command)
senko task edit <task_id> --contract <contract_id>        # set link
senko task edit <task_id> --clear-contract                # remove link

# Project members (moved from top-level `senko members` to `senko project members`)
senko project members list [--limit 20] [--after <cursor>]  # { items, next_cursor }
senko project members add --user-id <id> [--role <owner|member|viewer>]
senko project members remove --user-id <id>
senko project members set-role --user-id <id> --role <owner|member|viewer>

# All list commands share the same shape and cursor rules as `task list`:
#   response JSON: { "items": [...], "next_cursor": "<opaque string>|null }
#   --limit N: 1..=200 (default 50); --after <cursor>: opaque — pass next_cursor verbatim.
#   --output text appends a trailing `... more: --after <cursor>` line when there is another page.
#   Covers: task list, task deps list, contract list, contract note list,
#           project list, project metadata-field list, project members list, user list, auth sessions.
#   auth sessions filters expired keys in-memory after each page, so pages may contain fewer than --limit items.

# Configuration
senko config                           # show current configuration
senko config --init                    # generate template config.toml
```

## Important CLI details

- `--output text|json` and `--dry-run` are **global flags** — place them before the subcommand: `senko --output text task list`, `senko --dry-run task publish 1`
- `--dry-run` shows what would happen without actually executing the command. Available for all state-changing commands (`task add`, `task edit`, `task publish`, `task start`, `task resume`, `task complete`, `task cancel`, `task next`, `task deps add/remove/set`, `task dod check/uncheck`). Read-only commands (`task list`, `task get`, `task deps list`) ignore it.
- **DoD items have a checked state.** `task complete` will fail if any DoD items are unchecked. Use `task dod check <task_id> <index>` (1-based index) to mark items before completing. Tasks with no DoD items can complete freely.
- **Status transitions use dedicated commands**, not `task edit --status`:
  - `task publish`: draft → todo
  - `task start`: todo → in_progress
  - `task resume`: refresh `assignee_session_id` / `metadata` on an `in_progress` task (no status change)
  - `task complete`: in_progress → completed
  - `task cancel`: any active status → canceled
- `task get` only outputs JSON (no `--output text` support)
- New tasks start in `draft` status. Status transitions: draft → todo → in_progress → completed. Any active status → canceled.
- Priority levels: `p0` (highest) through `p3` (lowest). Default is `p2`.
- **Workflow configuration** (`[workflow]` in `.senko/config.toml`):
  - `merge_via`: `direct` (default) or `pr`
  - `auto_merge`: `true` (default) / `false` — applies only to `merge_via = "direct"`
  - `branch_mode`: table with `type` and `create` fields:
    - `type`: `worktree` (default) or `branch`
    - `create`: `true` (default) or `false` — whether the skill provisions a new resource or expects an existing one
    - Four combinations:
      - `{type="worktree", create=true}` (default): skill creates a fresh worktree per task
      - `{type="worktree", create=false}`: reuse an externally-managed worktree; the skill stops with an error if the worktree is missing (no fallback)
      - `{type="branch", create=true}`: skill creates/switches the branch in the current checkout (no worktree)
      - `{type="branch", create=false}`: skill works on the current branch without any branch operations
    - Legacy string form `branch_mode = "worktree"` / `"branch"` is still accepted and treated as `{ type=<value>, create=true }` (backward compatible).
    - Environment overrides: `SENKO_BRANCH_MODE_TYPE` (`worktree`|`branch`), `SENKO_BRANCH_MODE_CREATE` (`true`|`false`/`1`|`0`/`yes`|`no`). The previous `SENKO_BRANCH_MODE` variable has been removed.
  - `merge_strategy`: `rebase` (default) or `squash`
  - `branch_template`: optional branch name template. Supported variables:
    - `{{id}}` — task ID
    - `{{slug}}` — kebab-case slug derived from the task title
    - `{{context.<key>}}` — resolved from session context at branch-setting time. If the value is unavailable, the user is asked via `AskUserQuestion`.
    - `{{<name>:<opt1>|<opt2>|...}}` — enum variable. The skill infers the value from the task content. If unclear, the user is asked to choose. Example: `{{prefix:feat|fix|chore}}`
    - Examples: `task/{{id}}-{{slug}}`, `{{prefix:feat|fix|chore}}/{{id}}-{{slug}}`
  - Stage configs under `[workflow.<stage>]`. Built-in stage names the skill consumes: `task_add`, `task_publish`, `task_start`, `task_resume`, `task_complete`, `task_cancel`, `task_select`, `branch_set`, `branch_cleanup`, `branch_merge`, `pr_create`, `pr_update`, `plan`, `implement`, `contract_add`, `contract_edit`, `contract_delete`, `contract_dod_check`, `contract_dod_uncheck`, `contract_note_add`. Among the `contract_*` stages, only `contract_add`, `contract_note_add`, and `contract_dod_check` are emitted by the bundled workflows; `contract_edit`, `contract_delete`, and `contract_dod_uncheck` are recognized as built-ins but are reserved for user-defined workflow extensions. User-defined stage names are also accepted (preserved in `senko config` output).
  - Each stage supports `instructions` (list of text), `metadata_fields` (field list), and `hooks` (map). Workflow stage hooks are emitted as plan instructions by the skill; see the **Hooks** section below for the HookDef shape.
  - `metadata_fields`: each field has `key`, `source` (`env`, `value`, `prompt`, `command`), optional `default`, and `required` flag. `prompt` source fields are collected via `AskUserQuestion`. Values are shallow-merged into existing metadata.
  - **Metadata update semantics**: `--metadata` performs a shallow merge (top-level keys only: add/overwrite existing, null deletes key, unmentioned keys preserved). `--replace-metadata` replaces entirely. `--clear-metadata` removes all metadata.
  - `task_add.default_dod` / `task_add.default_tags` / `task_add.default_priority`: defaults for new tasks
  - `plan.required_sections`: required sections in implementation plans
  - When `merge_via = "pr"`, `task complete` requires `pr_url` to be set and the PR to be merged (checked via `gh`). Use `--skip-pr-check` to bypass.

- **Hooks** — defined under four runtime roots, each with named HookDef entries:
  - `[cli.<action>.hooks.<name>]` — fired by the CLI on state transitions.
  - `[server.relay.<action>.hooks.<name>]` — fired by `senko serve` when running in Relay mode (enabled by setting `server.relay.url`).
  - `[server.remote.<action>.hooks.<name>]` — fired by `senko serve`.
  - `[workflow.<stage>.hooks.<name>]` — emitted as plan instructions by the skill at the matching stage.
  - CLI/server `<action>` is one of: `task_add`, `task_publish`, `task_start`, `task_resume`, `task_complete`, `task_cancel`, `task_select`, `contract_add`, `contract_edit`, `contract_delete`, `contract_dod_check`, `contract_dod_uncheck`, `contract_note_add`.
  - Workflow stage hooks under `[workflow.contract_<verb>.hooks.*]` are emitted into the plan by the skill at each `senko contract <verb>` call site (see `workflows/add-task.md`, `workflows/execute-task.md`, and `workflows/contract-terminal.md`).
  - HookDef fields: `command` (shell), `when` (`pre` / `post`, default `post`), `mode` (`sync` / `async`, default `async`), `on_failure` (`abort` / `warn` / `ignore`, default `abort`), `enabled` (default `true`), `env_vars` (list of `{name, required, default, description}`), `on_result` (`any` / `selected` / `none`, `task_select` only), `prompt` (workflow stages only — renders an agent instruction).
  - Only `mode = "sync"` + `when = "pre"` + `on_failure = "abort"` can abort a state transition. Hooks defined under a non-matching runtime section are ignored with a startup warning.
  - The global `[hooks].enabled` switch and `SENKO_HOOKS_ENABLED` / `SENKO_HOOK_ON_TASK_*` env vars have been removed; configure hooks only in `config.toml`.

## Contract semantics

- **No status field.** A contract is "completed" iff it has at least one DoD item and every DoD item is checked. Empty DoD always returns false. The `senko contract get` JSON response includes a computed `is_completed` boolean for convenience.
- **Notes are append-only** and server-timestamped. Each note carries `content`, optional `source_task_id`, and `created_at`. There is no update / delete; corrections are expressed by adding a new note.
- **Task ↔ Contract link is directional.** A task carries an `Option<i64> contract_id`; contracts do not hold a list of task IDs. Use `senko task list --tag <contract-terminal or other>` combined with `senko task get | jq .contract_id` if you need to enumerate siblings — or query the Contract notes (which the skill populates) to find authoritative source tasks.
- **`contract-terminal` tag** is a skill-level convention marking a task whose sole job is to verify the Contract's DoD items at the end of a split. See `workflows/contract-terminal.md`.
