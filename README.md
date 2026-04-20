# senko

> **Alpha**: This project is in early development. APIs, CLI interfaces, and data formats may change without notice.

A local-only task management tool designed for Claude Code. SQLite-backed, dependency-aware, priority-driven.
Works as a Claude Code skill to let AI agents manage and execute project tasks.

[日本語ドキュメント (Japanese)](docs/ja/README.md) | [Documentation](docs/en/README.md)

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
/senko task add Implement user authentication
```
Add a task with interactive planning — Claude will ask clarifying questions, discover dependencies, and finalize the task.

```
/senko
```
Auto-select the highest-priority ready task and start working on it.

```
/senko task list
```
Show all tasks with their status and priority.

```
/senko graph
```
Visualize task dependencies as a text-based graph.

```
/senko task complete 3
```
Mark task #3 as completed (checks DoD items first).

## Hooks

Hooks are shell commands that run automatically when CLI commands change task state. No daemon required — they fire inline as fire-and-forget child processes. Each hook is a named entry, so you can enable/disable individual hooks independently. Configure in `.senko/config.toml`:

```toml
[hooks.on_task_added.notify]
command = "echo 'New task' | notify-send -"

[hooks.on_task_completed.webhook]
command = "curl -X POST https://example.com/webhook"

[hooks.on_task_completed.log]
command = "echo 'Task done!' >> /tmp/tasks.log"

[hooks.on_no_eligible_task.notify]
command = "notify-send 'No eligible tasks'"
```

Hooks receive the event payload as JSON on stdin and are executed via `sh -c`. All lifecycle events are supported: `on_task_added`, `on_task_ready`, `on_task_started`, `on_task_completed`, `on_task_canceled`, `on_no_eligible_task`.

For full details on event payloads, see [Hooks Reference](docs/en/reference/hooks.md).

## Workflow Configuration

Control task completion behavior via `[workflow]` in `.senko/config.toml`:

```toml
[workflow]
merge_via = "pr"        # or "direct" (default)
auto_merge = false      # default: true
```

| Setting | Values | Description |
|---------|--------|-------------|
| `merge_via` | `direct` (default), `pr` | When `pr`, `complete` verifies the PR is merged via `gh` |
| `auto_merge` | `true` (default), `false` | Applies to `merge_via = "direct"` only. Controls automatic branch merge. |

Use `senko config` to view current settings, or `senko config --init` to generate a template.

To use a config file at a custom location, use the `--config` flag or the `SENKO_CONFIG` environment variable:

```bash
senko --config /path/to/config.toml task list
SENKO_CONFIG=/path/to/config.toml senko task list
```

When `merge_via = "pr"`:
1. Set the PR URL on the task: `senko task edit <id> --pr-url <url>`
2. The PR must be merged before `senko task complete <id>` succeeds
3. Use `--skip-pr-check` to bypass verification when needed

## Master API Key

A master API key lets you bootstrap the system — create users and issue per-user API keys — without an existing user account. When authentication is enabled, the master key is checked first; if it doesn't match, senko falls back to the normal per-user key lookup.

> **Note**: `POST /users` is restricted to master key holders only.

### Generating a key

```bash
openssl rand -base64 32
```

### Storing in AWS Secrets Manager

```bash
aws secretsmanager create-secret \
  --name senko/master-api-key \
  --secret-string "$(openssl rand -base64 32)"
```

### Configuration

Set the key via environment variables:

```bash
# Direct value
export SENKO_AUTH_API_KEY_MASTER_KEY="<your-key>"

# Or via AWS Secrets Manager ARN (requires aws-secrets feature)
export SENKO_AUTH_API_KEY_MASTER_KEY_ARN="arn:aws:secretsmanager:us-east-1:123456789:secret:senko/master-api-key-AbCdEf"
```

Or in `.senko/config.toml`:

```toml
[server.auth.api_key]
master_key = "<your-key>"
# Or use an ARN (requires aws-secrets feature):
# master_key_arn = "arn:aws:secretsmanager:..."
```

### Bootstrap flow

Once the master API key is configured:

```bash
# 1. Generate and set the master key
export SENKO_AUTH_API_KEY_MASTER_KEY="$(openssl rand -base64 32)"

# 2. Start the server
senko serve

# 3. Create a user (POST /users requires master key)
curl -s -X POST http://localhost:3142/api/v1/users \
  -H "Authorization: Bearer $SENKO_AUTH_API_KEY_MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"username": "alice"}' | jq .

# 4. Issue an API key for the user (replace 1 with the user ID from step 3)
curl -s -X POST http://localhost:3142/api/v1/users/1/api-keys \
  -H "Authorization: Bearer $SENKO_AUTH_API_KEY_MASTER_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "default"}' | jq .

# 5. Use the returned API key for subsequent requests
export SENKO_CLI_REMOTE_TOKEN="<key from step 4>"
curl -s http://localhost:3142/api/v1/projects \
  -H "Authorization: Bearer $SENKO_CLI_REMOTE_TOKEN" | jq .
```

## Authentication

senko supports three authentication modes. See [OIDC Authentication](docs/en/guides/server-remote/auth-oidc.md) (recommended for production), [Trusted Headers Authentication](docs/en/guides/server-remote/auth-trusted-headers.md), and [API Key Authentication](docs/en/guides/server-remote/auth-api-key.md) (evaluation only).

## CLI Reference

For direct CLI usage, see [CLI Reference](docs/en/reference/cli.md).

## Configuration

See [Config Overview](docs/en/reference/config/overview.md) for all config keys, environment variables, and layering.

## Development

See [Development Setup](docs/en/contributing/development.md) for build instructions, testing, and worktree workflow.

## License

MIT
