# Worktree Workflow

senko's development rule is **no direct edits on the main branch**. Every change happens inside a worktree; after merge, the worktree is deleted.

`/workspaces/senko` is main; `/workspaces/senko/worktrees/*` are the working worktrees.

## Why Worktrees?

- **Build artifacts don't pollute main's `target/`** — each worktree is an independent working tree.
- **Parallel tasks without switching** — you can touch `docs-v1` and `fix-auth-bug` concurrently.
- **Main stays clean** — temporary pre-push commits don't live on main.
- **AI agent sandboxing** — keeps an agent from accidentally editing main.

## Tool

Use the dedicated `wth` script:

```bash
./scripts/bin/wth <command> <name>
```

Or invoke the `/wth` skill from Claude Code (currently, calling the script directly is more reliable depending on skill-registration state).

> **Don'ts**:
> - Don't run `git worktree add/remove` **directly**.
> - Don't use Claude Code's `EnterWorktree` tool.
> - Don't edit files on main.

## Basic Operations

`wth` provides three subcommands only: `add` / `rm` / `help`. Listing and switching use plain git / shell.

### Create

```bash
./scripts/bin/wth add my-feature
```

- Creates a new worktree at `worktrees/my-feature/`.
- **Creates a new** branch `wth/my-feature` and checks it out (to reuse an existing branch, `wth` isn't the tool — use plain `git worktree add`).

### List

```bash
git worktree list
```

### Switch

```bash
cd worktrees/my-feature
```

### Remove

```bash
./scripts/bin/wth rm my-feature
```

- Internally runs `git worktree remove --force` + `git branch -D wth/my-feature` — **there is no merged-check**, so an in-progress worktree is removed immediately.
- Runs `.wth/hooks/rm/*.sh` before removal (if present).

## Typical Flow

```bash
# 1. Start working (from main at /workspaces/senko)
./scripts/bin/wth add fix-auth-bug
cd worktrees/fix-auth-bug
#   At this point branch wth/fix-auth-bug is already checked out.

# 2. Edit, commit, push, PR
vim src/...
git add . && git commit -m "fix: auth"
git push origin wth/fix-auth-bug
gh pr create

# 3. After the PR is merged into main
cd /workspaces/senko
git pull
./scripts/bin/wth rm fix-auth-bug
```

## How Main Is Handled

- Access main under `/workspaces`.
- Main is treated as read-only (no direct commits outside of merges).
- Claude Code sessions refuse edits on the main side per project rules.

## Running Multiple Worktrees at Once

- It's fine to hold `worktrees/docs-v1` and `worktrees/fix-x` simultaneously.
- Each has its own `target/`. senko's SQLite paths land under separate XDG directories (`$XDG_DATA_HOME/senko/projects/docs-v1/data.db` vs `.../projects/fix-x/data.db`), so they don't collide.
- They share **the same `.git/`**, so git handles branch-level consistency.

## Troubleshooting

| Symptom | What to do |
|---|---|
| `wth add` says `worktree already exists` | Remove the existing one with `wth rm`, or pick a different name |
| `git status` looks weird inside a worktree | Usually the branch itself is off — `wth rm` + `wth add` is faster than debugging |
| Tempted to write to main | Don't — create a worktree |
| Accidentally committed `worktrees/` to main | `worktrees/` should already be in `.gitignore`. If it slipped in, `git rm -r --cached worktrees/` |

## Peek at the Script

```bash
cat ./scripts/bin/wth
```

- A thin wrapper around git worktree.
- Enforces the convention of `worktrees/` paths and `wth/<name>` branches.
- Runs `.wth/hooks/add/*.sh` on `add` and `.wth/hooks/rm/*.sh` on `rm` in lex order (if present).
- `WTH_DIR` env overrides the worktree base directory.
