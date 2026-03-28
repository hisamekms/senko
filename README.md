# senko

> **Alpha**: This project is in early development. APIs, CLI interfaces, and data formats may change without notice.

A local-only task management tool designed for Claude Code. SQLite-backed, dependency-aware, priority-driven.
Works as a Claude Code skill to let AI agents manage and execute project tasks.

[日本語ドキュメント (Japanese)](docs/README.ja.md)

## Features

- **Task lifecycle**: `draft` → `todo` → `in_progress` → `completed` / `canceled`
- **Priority levels**: P0 (highest) – P3 (lowest)
- **Dependency tracking**: Tasks block until dependencies are completed
- **Smart next-task selection**: Picks the highest-priority ready task automatically
- **Dual output**: JSON (for AI/automation) and human-readable text
- **Claude Code skill**: `/senko` skill for seamless AI-driven task management
- **Watch hooks**: Run custom commands on task events (add, complete)
- **Zero setup**: SQLite database auto-created on first run

> **Note**: senko stores data in `.senko/` under your project root. Add `.senko/` to your `.gitignore` to avoid committing local data.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh
```

Or specify a version:

```bash
VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh
```

By default, the binary is installed to `~/.local/bin`. Set `SENKO_INSTALL_DIR` to change the location.

### Build from source

```bash
cargo build --release
```

The binary is at `target/release/senko`. Add it to your `PATH`.

## Claude Code Integration

senko is primarily used as a Claude Code skill. Run `skill-install` to set it up:

```bash
senko skill-install
```

This generates `.claude/skills/senko/SKILL.md` in your project, registering the `/senko` skill with Claude Code.

### What the skill provides

The `/senko` skill gives Claude Code a full task management workflow:

- **Auto-select and execute** the next eligible task
- **Add tasks** with interactive planning or simple mode
- **List tasks** and **visualize dependency graphs**
- **Complete / cancel** tasks with DoD (Definition of Done) tracking
- **Manage dependencies** between tasks

## Typical Usage

Once the skill is installed, use it directly in Claude Code:

```
/senko add Implement user authentication
```
Add a task with interactive planning — Claude will ask clarifying questions, discover dependencies, and finalize the task.

```
/senko
```
Auto-select the highest-priority ready task and start working on it.

```
/senko list
```
Show all tasks with their status and priority.

```
/senko graph
```
Visualize task dependencies as a text-based graph.

```
/senko complete 3
```
Mark task #3 as completed (checks DoD items first).

## Hooks

Hooks are shell commands that run automatically when CLI commands change task state. No daemon required — they fire inline as fire-and-forget child processes. Configure in `.senko/config.toml`:

```toml
[hooks]
# Single command
on_task_added = "echo 'New task' | notify-send -"

# Multiple commands per event
on_task_completed = [
  "curl -X POST https://example.com/webhook",
  "echo 'Task done!' >> /tmp/tasks.log"
]
```

Hooks receive the event payload as JSON on stdin and are executed via `sh -c`. All five lifecycle events are supported: `on_task_added`, `on_task_ready`, `on_task_started`, `on_task_completed`, `on_task_canceled`.

For full details on event payloads, see [CLI Reference – Hooks](docs/CLI.md#hooks--automatic-actions-on-task-state-changes).

## Workflow Configuration

Control task completion behavior via `[workflow]` in `.senko/config.toml`:

```toml
[workflow]
completion_mode = "pr_then_complete"  # or "merge_then_complete" (default)
auto_merge = false                    # default: true
```

| Setting | Values | Description |
|---------|--------|-------------|
| `completion_mode` | `merge_then_complete` (default), `pr_then_complete` | When `pr_then_complete`, `complete` verifies the PR is merged via `gh` |
| `auto_merge` | `true` (default), `false` | When `false`, `complete` also verifies PR approval |

Use `senko config` to view current settings, or `senko config --init` to generate a template.

To use a config file at a custom location, use the `--config` flag or the `SENKO_CONFIG` environment variable:

```bash
senko --config /path/to/config.toml list
SENKO_CONFIG=/path/to/config.toml senko list
```

When `completion_mode = "pr_then_complete"`:
1. Set the PR URL on the task: `senko edit <id> --pr-url <url>`
2. The PR must be merged before `senko complete <id>` succeeds
3. Use `--skip-pr-check` to bypass verification when needed

## CLI Reference

For direct CLI usage, see [CLI Reference](docs/CLI.md).

## Development

See [Development Guide](docs/DEVELOPMENT.md) for status transitions, data storage, and testing.

## License

MIT
