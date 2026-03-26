# CLI Reference

[日本語](CLI.ja.md) | [Back to README](../README.md)

## Global Options

```
--output <FORMAT>       json or text (default: json)
--project-root <PATH>   Project root (auto-detected if omitted)
--config <PATH>         Path to config file (env: LOCALFLOW_CONFIG, default: .localflow/config.toml)
--dry-run               Show what would happen without executing (state-changing commands only)
--log-dir <PATH>        Override log output directory (default: $XDG_STATE_HOME/localflow)
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

## `serve` – Start the JSON API server

```bash
localflow serve                # Listen on 127.0.0.1:3142
localflow serve --port 8080    # Listen on 127.0.0.1:8080
localflow serve --host 0.0.0.0 # Listen on 0.0.0.0:3142 (all interfaces)
```

| Option | Description |
|--------|-------------|
| `--port <PORT>` | Port to listen on (env: `LOCALFLOW_PORT`, default: `3142`) |
| `--host <ADDR>` | Bind address, e.g. `0.0.0.0` or `192.168.1.5` (env: `LOCALFLOW_HOST`, default: `127.0.0.1`) |

Provides a full JSON REST API under `/api/v1/...` for all task operations (CRUD, status transitions, dependencies, DoD, config, stats). Hooks fire the same way as CLI commands.

## `web` – Start a read-only web viewer

```bash
localflow web                # Listen on 127.0.0.1:3141
localflow web --port 8080    # Listen on 127.0.0.1:8080
localflow web --host 0.0.0.0 # Listen on 0.0.0.0:3141 (all interfaces)
```

| Option | Description |
|--------|-------------|
| `--port <PORT>` | Port to listen on (env: `LOCALFLOW_PORT`, default: `3141`) |
| `--host <ADDR>` | Bind address, e.g. `0.0.0.0` or `192.168.1.5` (env: `LOCALFLOW_HOST`, default: `127.0.0.1`) |

## Docker

### Dockerfile

```dockerfile
FROM debian:bookworm-slim
ARG LOCALFLOW_VERSION=0.10.0
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && case "${TARGETARCH}" in \
       amd64) TARGET="x86_64-unknown-linux-musl" ;; \
       arm64) TARGET="aarch64-unknown-linux-musl" ;; \
       *) echo "Unsupported architecture: ${TARGETARCH}" && exit 1 ;; \
     esac \
  && curl -fsSL "https://github.com/hisamekms/localflow/releases/download/v${LOCALFLOW_VERSION}/localflow-v${LOCALFLOW_VERSION}-${TARGET}.tar.gz" \
     | tar xz -C /usr/local/bin localflow
WORKDIR /project
ENTRYPOINT ["localflow"]
```

> **Note**: `TARGETARCH` is automatically set by Docker BuildKit based on the build platform. This Dockerfile supports both `amd64` and `arm64`.

### Build and run

```bash
# Build the image
docker build -t localflow .

# Run a one-off command
docker run --rm -v "$(pwd)/.localflow:/project/.localflow" localflow list

# Start the API server
docker run --rm -p 3142:3142 \
  -v "$(pwd)/.localflow:/project/.localflow" \
  localflow serve --host 0.0.0.0
```

### Data persistence with volume mounts

localflow stores its SQLite database and configuration in the `.localflow/` directory. Mount this directory as a volume to persist data across container runs:

```
-v "$(pwd)/.localflow:/project/.localflow"
```

This mount includes:
- `tasks.db` – the SQLite database
- `config.toml` – hook and workflow configuration

Without a volume mount, all data is lost when the container stops.

## Hooks – Automatic actions on task state changes

Hooks are shell commands that run automatically when CLI commands change task state. They fire inline (no daemon required) as fire-and-forget child processes, so they never block the CLI.

### Configuration

Create `.localflow/config.toml` to define hooks:

```toml
[hooks]
on_task_added = "echo 'New task' | notify-send -"
on_task_ready = "curl -X POST https://example.com/ready"
on_task_started = "slack-notify started"
on_task_completed = "curl -X POST https://example.com/webhook"
on_task_canceled = "echo canceled"
```

Multiple commands per event are supported as arrays:

```toml
[hooks]
on_task_completed = ["notify-send 'Done'", "curl https://example.com/done"]
```

| Hook | Trigger |
|------|---------|
| `on_task_added` | `localflow add` creates a new task |
| `on_task_ready` | `localflow ready` transitions a task from draft to todo |
| `on_task_started` | `localflow start` or `localflow next` starts a task |
| `on_task_completed` | `localflow complete` completes a task |
| `on_task_canceled` | `localflow cancel` cancels a task |

Hooks receive the full event payload as JSON on **stdin** and are executed via `sh -c`.

### Event Payload

The JSON object passed to hooks on stdin:

```json
{
  "event_id": "550e8400-e29b-41d4-a716-446655440000",
  "event": "task_completed",
  "timestamp": "2026-03-24T12:00:00Z",
  "from_status": "in_progress",
  "task": { },
  "stats": { "draft": 1, "todo": 3, "in_progress": 1, "completed": 5 },
  "ready_count": 2,
  "unblocked_tasks": [{ "id": 3, "title": "Next task", "priority": "P1", "metadata": null }]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `event_id` | string | UUID v4 unique identifier |
| `event` | string | Event name (e.g. `"task_added"`, `"task_completed"`) |
| `timestamp` | string | ISO 8601 (RFC 3339) timestamp |
| `from_status` | string \| null | Previous status before the transition |
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

| Level | Description |
|-------|-------------|
| `INFO` | Normal operations (start, event detection, successful hook execution) |
| `WARN` | Hook returned non-zero exit code |
| `ERROR` | Hook execution failure |

## Environment Variables

All settings follow the precedence: **CLI flag > environment variable > config.toml > default**.

### Server

| Variable | Description | Default |
|----------|-------------|---------|
| `LOCALFLOW_PORT` | Port for `web` and `serve` commands | `3141` (web) / `3142` (serve) |
| `LOCALFLOW_HOST` | Bind address (e.g. `0.0.0.0`, `192.168.1.5`) | `127.0.0.1` |
| `LOCALFLOW_PROJECT_ROOT` | Project root directory | Auto-detected |
| `LOCALFLOW_CONFIG` | Path to config file | `.localflow/config.toml` |

### Workflow

| Variable | Description | Default |
|----------|-------------|---------|
| `LOCALFLOW_COMPLETION_MODE` | `merge_then_complete` or `pr_then_complete` | `merge_then_complete` |
| `LOCALFLOW_AUTO_MERGE` | `true` or `false` | `true` |

### Backend

| Variable | Description | Default |
|----------|-------------|---------|
| `LOCALFLOW_API_URL` | API server URL (enables HTTP backend instead of SQLite) | _(unset = SQLite)_ |
| `LOCALFLOW_HOOK_MODE` | `server`, `client`, or `both` | `server` |

### Log

| Variable | Description | Default |
|----------|-------------|---------|
| `LOCALFLOW_LOG_DIR` | Directory for hook log output | `$XDG_STATE_HOME/localflow` |

### Hooks

| Variable | Description |
|----------|-------------|
| `LOCALFLOW_HOOK_ON_TASK_ADDED` | Shell command to run when a task is created |
| `LOCALFLOW_HOOK_ON_TASK_READY` | Shell command to run when a task becomes ready |
| `LOCALFLOW_HOOK_ON_TASK_STARTED` | Shell command to run when a task is started |
| `LOCALFLOW_HOOK_ON_TASK_COMPLETED` | Shell command to run when a task is completed |
| `LOCALFLOW_HOOK_ON_TASK_CANCELED` | Shell command to run when a task is canceled |

Hook environment variables override the corresponding `[hooks]` section in `config.toml`.

### Example: Docker deployment

```bash
docker run -e LOCALFLOW_PORT=8080 \
  -e LOCALFLOW_HOST=0.0.0.0 \
  -e LOCALFLOW_HOOK_ON_TASK_COMPLETED="curl -X POST https://example.com/webhook" \
  localflow serve
```

## Status Transitions

```
draft → todo → in_progress → completed
                            → canceled
(any active status → canceled)
```
