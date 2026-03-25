# CLI Reference

[日本語](CLI.ja.md) | [Back to README](../README.md)

## Global Options

```
--output <FORMAT>       json or text (default: json)
--project-root <PATH>   Project root (auto-detected if omitted)
--dry-run               Show what would happen without executing (state-changing commands only)
```

> **Note**: `--output` and `--dry-run` are global flags — place them **before** the subcommand: `localflow --output text list`

## `add` – Create a task

```bash
localflow add --title "Write docs" --priority p0
localflow add --title "Fix bug" \
  --background "Users report 500 errors" \
  --definition-of-done "No 500 errors in logs" \
  --in-scope "Error handler" \
  --out-of-scope "Refactoring" \
  --tag backend --tag urgent
```

New tasks start in `draft` status. Default priority is `p2`.

## `list` – List tasks

```bash
localflow list                    # All tasks
localflow list --status todo      # Filter by status
localflow list --ready            # Todo tasks with all deps met
localflow list --tag backend      # Filter by tag
```

Status values use snake_case in CLI flags: `todo`, `in_progress`, `completed`, `canceled`, `draft`.

## `get <id>` – Task details

```bash
localflow get 1
```

> `get` outputs JSON only (no `--output text` support).

## `next` – Start next task

Selects the highest-priority `todo` task whose dependencies are all completed, and sets it to `in_progress`.

```bash
localflow next
localflow next --session-id "session-abc"
```

Selection order: priority (P0 first) → created_at → id.

## `edit <id>` – Edit a task

```bash
# Scalar fields
localflow edit 1 --title "New title"
localflow edit 1 --description "What to do"
localflow edit 1 --plan "How to do it"
localflow edit 1 --clear-description
localflow edit 1 --clear-plan
localflow edit 1 --status todo
localflow edit 1 --priority p0

# Array fields (tags, definition-of-done, scope)
localflow edit 1 --add-tag "urgent"
localflow edit 1 --remove-tag "old"
localflow edit 1 --set-tags "a" "b"         # Replace all

# Definition of Done
localflow edit 1 --add-definition-of-done "Write unit tests"

# PR URL
localflow edit 1 --pr-url "https://github.com/org/repo/pull/42"
localflow edit 1 --clear-pr-url
```

## `complete <id>` – Complete a task

```bash
localflow complete 1
localflow complete 1 --skip-pr-check    # Bypass PR merge/review checks
```

Fails if any DoD items are unchecked. Use `dod check` to mark items before completing.

When `completion_mode = "pr_then_complete"` in config, also verifies the PR is merged (and approved if `auto_merge = false`). Use `--skip-pr-check` to bypass.

## `cancel <id>` – Cancel a task

```bash
localflow cancel 1 --reason "out of scope"
```

## `dod` – Manage Definition of Done items

```bash
localflow dod check <task_id> <index>      # Mark DoD item as done (1-based)
localflow dod uncheck <task_id> <index>    # Unmark DoD item
```

## `deps` – Manage dependencies

```bash
localflow deps add 5 --on 3        # Task 5 depends on task 3
localflow deps remove 5 --on 3     # Remove dependency
localflow deps set 5 --on 1 2 3    # Set exact dependencies
localflow deps list 5              # List dependencies of task 5
```

## `config` – Show or initialize configuration

```bash
localflow config              # Show current configuration (JSON)
localflow --output text config # Show current configuration (text)
localflow config --init       # Generate a template .localflow/config.toml
```

Shows current configuration values (including defaults for missing settings). Use `--init` to generate a commented template file.

## `skill-install` – Claude Code integration

```bash
localflow skill-install
```

Generates a skill definition under `.claude/skills/localflow/` for Claude Code integration.

## `web` – Start a read-only web viewer

```bash
localflow web                # Listen on 127.0.0.1:3141
localflow web --port 8080    # Listen on 127.0.0.1:8080
localflow web --host         # Listen on 0.0.0.0:3141 (all interfaces)
```

| Option | Description |
|--------|-------------|
| `--port <PORT>` | Port to listen on (default: `3141`) |
| `--host` | Expose to all network interfaces (bind `0.0.0.0` instead of `127.0.0.1`) |

The `--host` flag can also be set via the `LOCALFLOW_WEB_HOST` environment variable (any non-empty value other than `0` or `false` enables it).

## `watch` – Watch for task events and run hooks

Polls the task database for changes and runs configured hooks when events are detected.

```bash
localflow watch                           # Foreground (5s interval)
localflow watch --interval 10             # Custom polling interval
localflow watch -d                        # Start as background daemon
localflow watch -d --interval 10          # Daemon with custom interval
localflow watch --log-file /tmp/watch.log # Custom log file path
localflow watch stop                      # Stop the daemon
localflow watch status                    # Show daemon status
```

| Option | Description |
|--------|-------------|
| `--interval <SECONDS>` | Polling interval in seconds (default: `5`) |
| `-d, --daemon` | Run as background daemon |
| `--log-file <PATH>` | Log file path (default: `.localflow/watch.log` when running as daemon) |

| Subcommand | Description |
|------------|-------------|
| `stop` | Stop a running daemon |
| `status` | Show daemon status (running/stopped, PID, uptime) |

### Configuration

Create `.localflow/config.toml` to define hooks:

```toml
[hooks]
on_task_added = "echo 'New task' | notify-send -"
on_task_completed = "curl -X POST https://example.com/webhook"
```

| Hook | Trigger |
|------|---------|
| `on_task_added` | A new task appears in the database |
| `on_task_completed` | A task transitions to `completed` status |

Hooks receive the full event payload as JSON on **stdin** and are executed via `sh -c`.

> Events are only detected when the corresponding hook is configured.

### Event Payload

The JSON object passed to hooks on stdin:

```json
{
  "event_id": "550e8400-e29b-41d4-a716-446655440000",
  "event": "task_added",
  "timestamp": "2026-03-24T12:00:00Z",
  "task": { },
  "stats": { "draft": 1, "todo": 3, "in_progress": 1, "completed": 5 },
  "ready_count": 2,
  "unblocked_tasks": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `event_id` | string | UUID v4 unique identifier |
| `event` | string | `"task_added"` or `"task_completed"` |
| `timestamp` | string | ISO 8601 (RFC 3339) timestamp |
| `task` | object | Full task object (same schema as `localflow get`) |
| `stats` | object | Task count by status (`{"todo": 3, "completed": 5, ...}`) |
| `ready_count` | integer | Number of `todo` tasks with all dependencies met |
| `unblocked_tasks` | array \| null | Tasks newly unblocked by this event (only on `task_completed`) |

#### `unblocked_tasks` items

Present only in `task_completed` events when completing a task unblocks other tasks.

| Field | Type | Description |
|-------|------|-------------|
| `id` | integer | Task ID |
| `title` | string | Task title |
| `priority` | string | `"P0"` – `"P3"` |
| `metadata` | object \| null | Task metadata (arbitrary JSON) |

### Logging

When running as a daemon (`-d`), logs are written to `.localflow/watch.log` by default. Use `--log-file` to override the path. In foreground mode, `--log-file` enables file logging.

Log format:

```
[2026-03-24T12:00:00Z] [INFO] watch started (interval: 5s)
[2026-03-24T12:00:05Z] [INFO] event detected: task_added task #1 "Write docs"
[2026-03-24T12:00:05Z] [INFO] hook executed: task_added (exit: 0)
[2026-03-24T12:00:10Z] [WARN] hook executed: task_completed (exit: 1)
[2026-03-24T12:00:15Z] [ERROR] hook failed: task_added: No such file or directory
```

| Level | Description |
|-------|-------------|
| `INFO` | Normal operations (start, event detection, successful hook execution) |
| `WARN` | Hook returned non-zero exit code |
| `ERROR` | Hook execution failure |

## Status Transitions

```
draft → todo → in_progress → completed
                            → canceled
(any active status → canceled)
```
