# Installing and Updating the Claude Code Skill

`senko skill-install` places the senko-owned skill files under your project and registers the `/senko` slash command with Claude Code.

## First-Time Install

From the project root:

```bash
senko skill-install
```

It writes:

```
.claude/
├── skills/
│   └── senko/
│       ├── SKILL.md             # skill entry point
│       ├── cli-reference.md     # CLI command cheat sheet
│       ├── scripts/             # helper scripts called by the skill
│       └── workflows/           # individual workflow runbooks
│           ├── auto-select.md
│           ├── add-task.md
│           ├── execute-task.md
│           ├── complete-task.md
│           ├── ...
└── agents/
    └── dod-verifier.md          # subagent used for DoD verification
```

Restart Claude Code or run `/help` → skill list to confirm the skill is recognized.

## What `/senko` Provides

| Slash command | Purpose |
|---|---|
| `/senko` | Auto-pick a ready task and start it |
| `/senko start <id>` | Run the task with the given ID |
| `/senko add <description>` | Add a task with an interactive planning phase |
| `/senko add --simple <description>` | Add a task without planning |
| `/senko list` | List tasks |
| `/senko graph` | Visualize dependencies as a Mermaid graph |
| `/senko complete <id>` | Complete (checks DoD first) |
| `/senko cancel <id>` | Cancel |
| `/senko dod check <task_id> <index>` | Mark a DoD item as checked |
| `/senko dod uncheck <task_id> <index>` | Uncheck a DoD item |
| `/senko deps add <task_id> --on <dep_id>` | Add a dependency |
| `/senko deps remove <task_id> --on <dep_id>` | Remove a dependency |
| `/senko deps list <task_id>` | List dependencies |
| `/senko config-explain` | Explain the current configuration |
| `/senko config-setup` | Interactively build or improve `config.toml` |

Contract operations are done directly via the CLI (`senko contract add`, etc.). The skill's wrappers focus on Tasks for now.

## Updating

After upgrading the `senko` binary, refresh the skill:

```bash
senko skill-install
```

- Identical files are skipped (shows `is up to date`).
- If a file differs, you get a per-file overwrite prompt (`--yes` to accept all).
- `--force` wipes the senko-owned directory first and does a clean install.

## Using a Custom Install Path

Default is under `.claude/`. Override with `--output-dir`:

```bash
senko skill-install --output-dir /custom/path
```

With `--output-dir`, all files are written **flat**. To be recognized per the Claude Code convention, the final layout must still be `.claude/skills/<name>/SKILL.md`.

## Relation to Project Workflow Config

At runtime, the skill calls `senko config --output json` and folds `[workflow.*]` `instructions` / `prompt` into the agent's instructions.

So:

1. Edit `[workflow.*]` in `.senko/config.toml`.
2. **No reinstall required** — the next `/senko` run picks up the latest config.

Only refresh via `senko skill-install` when the skill's shape itself changes (e.g., you upgraded the `senko` binary).

## Using Across Multiple Projects

The senko skill is project-local (`.claude/`), so each project can have its own `[workflow.*]`. For settings shared across all projects, put them in `~/.config/senko/config.toml` — they merge at a lower priority than per-project config.

## Troubleshooting

| Symptom | What to do |
|---|---|
| `/senko` doesn't appear in Claude Code | Confirm `.claude/skills/senko/SKILL.md` exists; restart Claude Code |
| Skill behavior looks stale | `senko skill-install --force` to re-place files |
| Workflow config not taking effect | Check `senko config` for the expected `[workflow.*]` after merging; run `senko doctor` |

## What to Read Next

- Workflow-config concepts → [Event-Driven Workflow](../../explanation/event-driven-workflow.md)
- `[workflow.*]` TOML → [`[workflow.*]` Config](../../reference/config/workflow.md)
- Examples → [Workflow Stage Examples](workflow-stages.md)
