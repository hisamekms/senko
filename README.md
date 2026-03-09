# localflow

A local-only task management CLI for single-developer and single-agent workflows.
SQLite-backed, dependency-aware, priority-driven.

[日本語ドキュメント (Japanese)](docs/README.ja.md)

## Features

- **Task lifecycle**: `draft` → `todo` → `in_progress` → `completed` / `canceled`
- **Priority levels**: P0 (highest) – P3 (lowest)
- **Dependency tracking**: Tasks block until dependencies are completed
- **Smart next-task selection**: Picks the highest-priority ready task automatically
- **Dual output**: JSON (for AI/automation) and human-readable text
- **Claude Code integration**: `skill-install` generates a skill config for Claude Code
- **Zero setup**: SQLite database auto-created on first run

## Install

### Build from source

```bash
cargo build --release
```

The binary is at `target/release/localflow`.

### Claude Code integration

```bash
localflow skill-install
```

Generates `SKILL.md` for Claude Code skill integration.

## Quick Start

```bash
# Create a task
localflow add --title "Implement auth API" --priority p1

# List tasks
localflow list

# Start the next ready task
localflow next

# Complete a task
localflow complete 1
```

## Commands

### Global Options

```
--output <FORMAT>       json or text (default: json)
--project-root <PATH>   Project root (auto-detected if omitted)
```

### `add` – Create a task

```bash
localflow add --title "Write docs" --priority p0
localflow add --title "Fix bug" \
  --background "Users report 500 errors" \
  --definition-of-done "No 500 errors in logs" \
  --in-scope "Error handler" \
  --out-of-scope "Refactoring" \
  --tag backend --tag urgent
```

### `list` – List tasks

```bash
localflow list                    # All tasks
localflow list --status todo      # Filter by status
localflow list --ready            # Todo tasks with all deps met
localflow list --tag backend      # Filter by tag
```

### `get <id>` – Task details

```bash
localflow get 1
localflow get 1 --output json
```

### `next` – Start next task

Selects the highest-priority `todo` task whose dependencies are all completed, and sets it to `in_progress`.

```bash
localflow next
localflow next --session-id "session-abc"
```

Selection order: priority (P0 first) → created_at → id.

### `edit <id>` – Edit a task

```bash
# Scalar fields
localflow edit 1 --title "New title"
localflow edit 1 --status todo
localflow edit 1 --priority p0

# Array fields (tags, definition-of-done, scope)
localflow edit 1 --add-tag "urgent"
localflow edit 1 --remove-tag "old"
localflow edit 1 --set-tags "a" "b"         # Replace all
```

### `complete <id>` – Complete a task

```bash
localflow complete 1
```

### `cancel <id>` – Cancel a task

```bash
localflow cancel 1 --reason "out of scope"
```

### `deps` – Manage dependencies

```bash
localflow deps add 5 --on 3        # Task 5 depends on task 3
localflow deps remove 5 --on 3     # Remove dependency
localflow deps set 5 --on 1 2 3    # Set exact dependencies
localflow deps list 5              # List dependencies of task 5
```

### `skill-install` – Claude Code integration

```bash
localflow skill-install
```

## Development

See [Development Guide](docs/DEVELOPMENT.md) for status transitions, data storage, and testing.

## License

MIT
