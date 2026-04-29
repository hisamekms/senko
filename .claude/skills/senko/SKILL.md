---
name: senko
description: "Task management using senko CLI. Provides workflows for adding, auto-selecting, executing, completing, canceling tasks and managing dependencies. Triggers on \"/senko\", \"タスク追加\", \"次のタスク\", \"タスク実行\", \"タスクを作って\", \"タスク一覧\", \"タスク完了\", \"タスクキャンセル\", \"依存関係\", \"依存グラフ\", \"タスクグラフ\", \"DoDチェック\", \"add task\", \"next task\", \"complete task\", \"cancel task\", \"task list\", \"task dependencies\", \"dependency graph\", \"dod check\" or similar task management requests."
argument-hint: "[start <id> | resume <id> | add <description> | list | graph | complete <id> | cancel <id> | deps ... | config-explain | config-setup]"
---

# senko — Task Management Skill

Manage and execute project tasks using the `senko` CLI. senko is a SQLite-backed task management tool with priority-driven selection and dependency tracking.

## Commands

- `/senko` — Auto-select and execute the next eligible task
- `/senko start <id>` — Execute a specific task by ID (requires `status=todo` with all dependencies completed)
- `/senko resume <id>` — Resume an `in_progress` task: refresh `assignee_session_id`, reuse the existing worktree, re-enter PlanMode (no status change)
- `/senko add <description>` — Add a new task (interactive planning)
- `/senko add --simple <description>` — Add a task without planning phase
- `/senko list` — Show task list
- `/senko graph` — Show dependency graph (Mermaid diagram)
- `/senko complete <id>` — Mark a task as completed
- `/senko cancel <id>` — Cancel a task
- `/senko dod check <task_id> <index>` — Mark a DoD item as checked
- `/senko dod uncheck <task_id> <index>` — Unmark a DoD item
- `/senko deps add <task_id> --on <dep_id>` — Add a dependency
- `/senko deps remove <task_id> --on <dep_id>` — Remove a dependency
- `/senko deps list <task_id>` — List dependencies of a task
- `/senko config-explain` — Explain current configuration values
- `/senko config-setup` — Interactively create or improve config.toml

## Argument Parsing

**Default action: When `$ARGUMENTS` is empty, blank, or contains only the literal placeholder `$ARGUMENTS`, execute Auto-Select (run `senko task next`).** Do NOT show the task list — always run `task next` when no arguments are provided.

Parse `$ARGUMENTS` with these rules (check in order):

1. **Empty / blank / literal `$ARGUMENTS`**: → Auto-select next task. Read file: `${CLAUDE_SKILL_DIR}/workflows/auto-select.md`
2. **Starts with `add`**: Create a new task with the rest as description. Read file: `${CLAUDE_SKILL_DIR}/workflows/add-task.md`
   - If `--simple` is present, use simple mode (skip planning phase)
3. **`list`**: Show task list. Read file: `${CLAUDE_SKILL_DIR}/workflows/list-tasks.md`
4. **`graph`**: Show dependency graph. Read file: `${CLAUDE_SKILL_DIR}/workflows/dependency-graph.md`
5. **`complete <id>`**: Complete the specified task. Read file: `${CLAUDE_SKILL_DIR}/workflows/complete-task.md`
6. **`cancel <id>`**: Cancel the specified task. Read file: `${CLAUDE_SKILL_DIR}/workflows/cancel-task.md`
7. **`resume <id>`**: Resume an `in_progress` task (session/metadata refresh, reuse worktree). Read file: `${CLAUDE_SKILL_DIR}/workflows/resume-task.md`
8. **`start <id>`**: Execute that task. Read file: `${CLAUDE_SKILL_DIR}/workflows/execute-task.md`
    - Execute only if the task's `status` is `todo` and every dependency has `status == completed`. Any other status (`draft` / `in_progress` / `completed` / `canceled`) or incomplete dependency must stop the flow with a clear reason. To resume an `in_progress` task, use `/senko resume <id>` instead.
9. **Starts with `dod`**: Manage DoD check state. Read file: `${CLAUDE_SKILL_DIR}/workflows/dod-check.md`
10. **Starts with `deps`**: Manage dependencies. Read file: `${CLAUDE_SKILL_DIR}/workflows/manage-dependencies.md`
11. **`config-explain`**: Explain current config. Read file: `${CLAUDE_SKILL_DIR}/workflows/config-explain.md`
12. **`config-setup`**: Interactively create/improve config. Read file: `${CLAUDE_SKILL_DIR}/workflows/config-setup.md`

**After matching a rule above, read the referenced file for the full workflow procedure.** Also read the CLI reference for command syntax: `${CLAUDE_SKILL_DIR}/cli-reference.md`

## Reference Files

| File | Description |
|---|---|
| `${CLAUDE_SKILL_DIR}/cli-reference.md` | CLI command syntax, flags, status transitions, workflow config |
| `${CLAUDE_SKILL_DIR}/workflows/auto-select.md` | Auto-select next eligible task |
| `${CLAUDE_SKILL_DIR}/workflows/add-task.md` | Add task (normal and simple mode) |
| `${CLAUDE_SKILL_DIR}/workflows/list-tasks.md` | List and filter tasks |
| `${CLAUDE_SKILL_DIR}/workflows/dependency-graph.md` | Visualize dependency graph |
| `${CLAUDE_SKILL_DIR}/workflows/execute-task.md` | Execute a task (worktree, plan, implement) |
| `${CLAUDE_SKILL_DIR}/workflows/resume-task.md` | Resume an in_progress task (session refresh, reuse worktree) |
| `${CLAUDE_SKILL_DIR}/workflows/complete-task.md` | Complete a task (DoD verification, PR checks) |
| `${CLAUDE_SKILL_DIR}/workflows/contract-terminal.md` | Execute and complete a `contract-terminal` task (verify Contract DoD; spawn follow-ups on gaps) |
| `${CLAUDE_SKILL_DIR}/workflows/dod-check.md` | Check/uncheck Definition of Done items |
| `${CLAUDE_SKILL_DIR}/workflows/cancel-task.md` | Cancel a task |
| `${CLAUDE_SKILL_DIR}/workflows/manage-dependencies.md` | Add, remove, list dependencies |
| `${CLAUDE_SKILL_DIR}/workflows/config-explain.md` | Explain current configuration values |
| `${CLAUDE_SKILL_DIR}/workflows/config-setup.md` | Interactively create or improve config.toml |

## Notes

- **Language**: Respond in the same language the user uses
- **Errors**: When senko returns an error, clearly communicate the error details to the user
- **Safety**: Be careful with worktree creation/deletion and branch operations
- **Output format**: Use `--output text` (global flag, before subcommand) for human display; use JSON (default) for programmatic processing
