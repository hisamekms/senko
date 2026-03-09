# localflow

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
- **Claude Code skill**: `/localflow-task` skill for seamless AI-driven task management
- **Zero setup**: SQLite database auto-created on first run

> **Note**: localflow stores data in `.localflow/` under your project root. Add `.localflow/` to your `.gitignore` to avoid committing local data.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/hisamekms/localflow/main/install.sh | sh
```

Or specify a version:

```bash
VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/hisamekms/localflow/main/install.sh | sh
```

By default, the binary is installed to `~/.local/bin`. Set `LOCALFLOW_INSTALL_DIR` to change the location.

### Build from source

```bash
cargo build --release
```

The binary is at `target/release/localflow`. Add it to your `PATH`.

## Claude Code Integration

localflow is primarily used as a Claude Code skill. Run `skill-install` to set it up:

```bash
localflow skill-install
```

This generates `.claude/skills/localflow-task/SKILL.md` in your project, registering the `/localflow-task` skill with Claude Code.

### What the skill provides

The `/localflow-task` skill gives Claude Code a full task management workflow:

- **Auto-select and execute** the next eligible task
- **Add tasks** with interactive planning or simple mode
- **List tasks** and **visualize dependency graphs**
- **Complete / cancel** tasks with DoD (Definition of Done) tracking
- **Manage dependencies** between tasks

## Typical Usage

Once the skill is installed, use it directly in Claude Code:

```
/localflow-task add Implement user authentication
```
Add a task with interactive planning — Claude will ask clarifying questions, discover dependencies, and finalize the task.

```
/localflow-task
```
Auto-select the highest-priority ready task and start working on it.

```
/localflow-task list
```
Show all tasks with their status and priority.

```
/localflow-task graph
```
Visualize task dependencies as a text-based graph.

```
/localflow-task complete 3
```
Mark task #3 as completed (checks DoD items first).

## CLI Reference

For direct CLI usage, see [CLI Reference](docs/CLI.md).

## Development

See [Development Guide](docs/DEVELOPMENT.md) for status transitions, data storage, and testing.

## License

MIT
