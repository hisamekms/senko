# Hooks Reference

A hook is **a shell command that fires before or after a state transition**. The same machinery runs across every runtime.

For the concept see [Event-Driven Workflow](../explanation/event-driven-workflow.md); for runtime choice see [Choosing a Runtime](../explanation/runtimes.md).

## Key Structure

```
<runtime>.<aggregate>_<action>.hooks.<name>
```

| Part | Allowed values |
|---|---|
| `<runtime>` | `cli` / `server.remote` / `server.relay` / `workflow` |
| `<aggregate>_<action>` | `task_add` / `task_publish` / `task_start` / `task_complete` / `task_cancel` / `task_select` / `contract_add` / `contract_edit` / `contract_delete` / `contract_dod_check` / `contract_dod_uncheck` / `contract_note_add` / (workflow only: any stage name) |
| `<name>` | Freeform (alphanumeric + `_`) |

Examples:

```toml
[cli.task_complete.hooks.notify]
command = "notify-send 'done'"

[server.remote.task_add.hooks.audit]
command = "logger -t senko-audit 'new task'"
mode = "async"

[workflow.plan.hooks.review]
command = "true"
prompt = "Have a human review the plan before proceeding to implementation"
when = "pre"
```

## HookDef Schema

| Field | Type | Default | Description |
|---|---|---|---|
| `command` | string | **required** | Command executed via `sh -c` |
| `when` | `"pre"` / `"post"` | `"post"` | Before or after the state transition |
| `mode` | `"sync"` / `"async"` | `"async"` | `sync` waits; `async` is fire-and-forget |
| `on_failure` | `"abort"` / `"warn"` / `"ignore"` | `"abort"` | Behavior on a non-zero exit (see below) |
| `enabled` | bool | `true` | `false` keeps the definition but prevents it from firing |
| `env_vars` | `EnvVarSpec[]` | `[]` | Required environment variable declarations (below) |
| `on_result` | `"selected"` / `"none"` / `"any"` | `"any"` | **`task_select` only**; ignored elsewhere |
| `prompt` | string | `null` | **`workflow.<stage>.hooks.*` only**; injected by the skill as agent instructions |

### `on_failure` Semantics

- `abort`: cancels the state transition (raises `DomainError::HookAborted`) **only with `sync + pre`**. In every other combination it behaves like `warn`.
- `warn`: logs the failure at WARN.
- `ignore`: ignores the failure (INFO only).

### `on_result` (task_select only)

| Value | Fires when |
|---|---|
| `selected` | `task next` picked a task |
| `none` | `task next` found nothing to pick |
| `any` (default) | Either |

The old `on_no_eligible_task` event has been replaced with `task_select` + `on_result = "none"`.

## EnvVarSpec

```toml
[[cli.task_complete.hooks.webhook.env_vars]]
name = "WEBHOOK_URL"
required = true
default = "https://example.com/fallback"
description = "Where to POST on task completion"
```

| Field | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | required | Environment variable name |
| `required` | bool | `true` | When unset and `default` missing, the **hook is skipped** with a warn |
| `default` | string? | — | Fallback value |
| `description` | string? | — | Explanatory text for config readers |

## Hook Envelope (JSON on stdin)

### Task action envelope

```json
{
  "runtime": "cli",
  "backend": {
    "type": "sqlite",
    "db_file_path": "/home/alice/.local/share/senko/projects/my-project/data.db"
  },
  "project": { "id": 1, "name": "default" },
  "user":    { "id": 1, "name": "default" },
  "event": {
    "event_id": "uuid-v4",
    "event": "task_complete",
    "timestamp": "2026-04-19T12:00:00Z",
    "from_status": "in_progress",
    "task": { ... elided ... },
    "stats": { "draft": 1, "todo": 3, "in_progress": 1, "completed": 5, "canceled": 0 },
    "ready_count": 2,
    "is_ready": false,
    "unblocked_tasks": [{ "id": 3, "title": "...", "priority": "P1", "metadata": null }]
  }
}
```

- `task`: same schema as `senko task get` (see [CLI Reference](cli.md)).
- `unblocked_tasks`: **only present on `task_complete`** — other tasks that became ready because of this completion.
- `stats`: task counts by status for this project.
- `is_ready`: whether the task in the event is itself ready to start (`status == todo` and all dependencies `completed`). Present on every `task_*` event (`task_add` / `task_publish` / `task_start` / `task_complete` / `task_cancel`).

### Contract action envelope

For `contract_*` events the outer envelope is the same but the inner shape changes:

```json
{
  "runtime": "server.remote",
  "backend": { ... },
  "project": { ... },
  "user":    { ... },
  "event": {
    "event_id": "uuid-v4",
    "event": "contract_note_add",
    "timestamp": "...",
    "contract": { "id": 42, "title": "...", "definition_of_done": [...], "notes": [...], "is_completed": false, ... }
  }
}
```

`from_status` / `stats` / `ready_count` / `is_ready` / `unblocked_tasks` are task-aggregate-only and don't appear here.

### The `backend` field

| `type` | Additional fields |
|---|---|
| `sqlite` | `db_file_path` |
| `postgres` | `connection_url` (host/dbname only; credentials masked) |
| `http` | `api_url` (when going through remote / relay) |

## Firing Timing Summary

| Action | `pre` fires when | `post` fires when |
|---|---|---|
| `task_add` | Before creation (after validation) | After creation |
| `task_publish` | Before draft → todo | After |
| `task_start` | Before todo → in_progress | After |
| `task_complete` | Before in_progress → completed (after DoD validation) | After |
| `task_cancel` | Before transitioning to canceled | After |
| `task_select` | After candidate decision, before state change | After state change |
| `contract_add` | Before creation | After creation |
| `contract_edit` | Before update | After update |
| `contract_delete` | Before delete | After delete |
| `contract_dod_check/uncheck` | Before update | After update |
| `contract_note_add` | Before append | After append |

## Hooks Outside the Active Runtime Don't Fire

Hooks written under sections that don't match the active runtime are **skipped, and a single warn is logged at startup**. If things aren't firing, start by checking `senko doctor` and the startup logs.

## Hooks with a Remote (HTTP) Backend

With `[cli.remote] url` set (and on relay servers), `cli.*` / `server.relay.*` hooks fire on the client side (the side sending the request):

- `when = "pre"` runs **before the HTTP request is sent**; with `sync` + `on_failure = "abort"` it cancels the request entirely.
- `when = "post"` runs after the response is received.

Client-side hooks can be bypassed by each client's own config, so **put enforcing validation in the upstream server's `[server.remote.*]` hooks**. Relay / CLI pre-hooks are for early feedback only.

## Load-Time Validation

`senko doctor` / the server startup emits warnings about:

- Combinations where `on_failure = "abort"` can't take effect — abort only works with `sync` + `pre`. `pre` + `async` and `sync` + `post` are effectively `warn`. (The all-default combination `post` + `async` + `abort` is not flagged since it's the idiomatic minimal config.)
- `on_result` attached to anything other than `task_select` — ignored (startup only).

Beyond these, `senko doctor` also checks hook script existence / execute permission / required env vars, and prints the effective backend on its `Backend:` line.

## Testing

Fire manually:

```bash
senko hooks test task_complete 3            # build the envelope from real task 3 and fire synchronously
senko hooks test task_complete --dry-run    # print the envelope JSON (no firing)
senko hooks test contract_note_add 42       # test contract hooks using contract id 42
```

## Logs

- Default output: `$XDG_STATE_HOME/senko/` (override with `[log] dir`).
- `senko hooks log -f` is `tail -f` equivalent.
- `[log] hook_output = "file" | "stdout" | "both"` selects where the hook's stdout/stderr goes.

## Examples

- CLI-side notifications → [`[cli.*]` Hook Examples](../guides/cli/hooks.md)
- Server-side audit logs → [`[server.remote.*]` Hook Examples](../guides/server-remote/hooks.md)
- Relay request logging → [`[server.relay.*]` Hook Examples](../guides/server-relay/hooks.md)
