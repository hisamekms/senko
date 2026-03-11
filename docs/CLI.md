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
```

## `complete <id>` – Complete a task

```bash
localflow complete 1
```

Fails if any DoD items are unchecked. Use `dod check` to mark items before completing.

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

## Status Transitions

```
draft → todo → in_progress → completed
                            → canceled
(any active status → canceled)
```
