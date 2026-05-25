# Local SQLite

A **one-developer, single-machine** setup. No server — the senko CLI touches a local SQLite database directly.

→ How the three pillars play out in this setup: [Core Concept](../explanation/core-concept.md).

```
┌──────────────────────────────────────────────────────┐
│  Developer's machine                                 │
│                                                      │
│   senko CLI (in any project dir)                     │
│     │                                                │
│     ▼                                                │
│   ~/.local/share/senko/projects/<project>/data.db    │
│   ( = $XDG_DATA_HOME/senko/projects/<project>/... )  │
└──────────────────────────────────────────────────────┘
```

## When to Choose This

- Personal project, solo developer
- You want Claude Code to manage tasks via the `/senko` skill
- You don't want to run a server / don't need one
- Keeping data on the local machine is fine

What this setup **cannot** do:

- Share the same task DB across multiple developers
- Read/write from another machine (unless you manually rsync the XDG-resident DB file)
- Produce server-side audit logs
- Integrate with SSO

If you need any of these, move to [CLI → Remote → PostgreSQL](cli-remote-postgres.md).

## Components

| Component | Role | Setup |
|---|---|---|
| senko CLI | Runs tasks, hosts the skill | Just install |
| SQLite DB | Data storage (`$XDG_DATA_HOME/senko/projects/<dir>/data.db`) | Auto-created on first run |
| Claude Code skill | Provides the `/senko` command | `senko skill-install` |

## Setup

### 1. Install the binary

```bash
curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh
```

Default install path is `~/.local/bin/senko` (change it with `SENKO_INSTALL_DIR`). Make sure `~/.local/bin` is on your `PATH`.

### 2. Initialize in the project

```bash
cd your-project
senko skill-install
```

This writes:

```
.claude/skills/senko/SKILL.md
```

### 3. Add your first task

Directly from the CLI:

```bash
senko task add --title "Implement webhook handler" --priority p1
senko task list
```

From Claude Code:

```
/senko task add Implement webhook handler
/senko                                      # auto-select a ready task
```

The first `senko` run creates the XDG-resident DB (`$XDG_DATA_HOME/senko/projects/<dir>/data.db`) and runs the initial migrations. You can override the DB location with `--db-path` / `SENKO_DB_PATH` / `[backend.sqlite] db_path`.

## Recommended Options

The minimum is no config at all, but having a `.senko/config.toml` in the project is useful (only the config file lives under the project root):

```bash
mkdir -p .senko
senko config --init > .senko/config.toml     # annotated template
```

To keep the config out of version control, write it to `.senko/config.local.toml` instead (see [Config Overview](../reference/config/overview.md)).

Common snippets:

```toml
# DoD that Claude should always fill in for every new task
[workflow.task_add]
default_dod = [
  "Unit tests pass",
  "CHANGELOG updated",
]

# Branch naming convention (worktree workflow)
[workflow]
branch_template = "feat/{{id}}-{{slug}}"

[workflow.branch_mode]
type = "worktree"
create = true

# Desktop notification on completion (macOS)
[cli.task_complete.hooks.notify]
command = "osascript -e 'display notification \"task done\" with title \"senko\"'"
mode = "async"
on_failure = "ignore"
```

## Where the Data Lives

| Path | Purpose |
|---|---|
| `$XDG_DATA_HOME/senko/projects/<dir>/data.db` | SQLite database (typically `~/.local/share/senko/projects/<dir>/data.db`) |
| `<project>/.senko/config.toml` | Config (optional, OK to commit) |
| `<project>/.senko/config.local.toml` | Per-developer overrides (suggested: gitignored) |
| `$XDG_STATE_HOME/senko/` | Hook execution logs (default `~/.local/state/senko/`) |

The DB isn't written inside the project directory, so you don't need to add anything to `.gitignore`. Only gitignore `.senko/config.local.toml` when you put secrets in it.

Environments previously using `<project>/.senko/data.db` are auto-migrated to the XDG location on first start (the original file is kept for verification).

## Backup and Migration

- Copy the DB file **as-is** to restore on another machine
- On a version upgrade, any unapplied migrations run automatically on next start
- Downgrades are **not supported** — test against a separate DB if you need to

```bash
# Manual backup
DB="$HOME/.local/share/senko/projects/$(basename $PWD)/data.db"
cp "$DB" "$DB.bak.$(date +%Y%m%d)"

# To another machine
scp "$DB" other-host:"$DB"
```

## When to Move to a Remote Setup

Consider [CLI → Remote → PostgreSQL](cli-remote-postgres.md) if any of the following applies:

- A second developer needs access to the same task DB
- You want to call `senko` from PRs / CI (multiple clients writing concurrently)
- You want audit logs
- You want SSO-based access control

## See Also

- Configure workflow stages → [Workflow Stage Examples](../guides/cli/workflow-stages.md)
- Hook examples → [`[cli.*]` Hook Examples](../guides/cli/hooks.md)
