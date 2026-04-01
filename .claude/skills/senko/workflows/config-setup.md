# Config Setup

Interactively create or improve `.senko/config.toml` for the current project.

## Procedure

### Step 1: Check Existing Config

Check if `.senko/config.toml` exists in the project root.

- **Exists**: Read the file and proceed to **Improve Mode** (Step 3).
- **Does not exist**: Proceed to **Create Mode** (Step 2).

### Step 2: Create Mode

Create a new `.senko/config.toml` by walking through each section with the user.

For each section below, use `AskUserQuestion` to ask the user about their preferences. Skip sections the user doesn't need — only include sections with non-default values.

Walk through sections in this order:

1. **project** — Project name (used for hooks/identification)
2. **user** — User name (for task assignment). **Do not write to config.toml.** Instead, ask the user how they want to set their name:
   - **Option A: config.local.toml** — Ask the user for their name, then write `[user]` section with `name` to `.senko/config.local.toml` (create if it doesn't exist, merge if it does).
   - **Option B: Environment variable `SENKO_USER`** — Do nothing in config-setup. Inform the user they can set `export SENKO_USER="Name"` in their shell profile.
3. **workflow** — How tasks are completed and branches managed:
   - `completion_mode`: merge first then complete, or PR-based completion?
   - `auto_merge`: require PR approval before completion?
   - `branch_mode`: use git worktrees or regular branches?
   - `merge_strategy`: rebase or squash merge?
4. **backend** — Remote backend settings (skip if local-only use):
   - `api_url`: remote API URL
   - `hook_mode`: where hooks run (server/client/both)
5. **storage** — Custom database path (skip if default is fine)
6. **log** — Logging preferences:
   - `level`: trace/debug/info/warn/error
   - `format`: json or text
   - `dir`: custom log directory
7. **web** — Web server host (skip if default is fine)
8. **hooks** — Task lifecycle hooks:
   - Which events to hook into (on_task_added, on_task_ready, on_task_started, on_task_completed, on_task_canceled, on_no_eligible_task)
   - For each: command, enabled state, required env vars

After all sections are covered, generate the TOML and write it to `.senko/config.toml` using the Write tool.

### Step 3: Improve Mode

1. Show the user their current config (read and display the file).
2. Use `AskUserQuestion` to ask which section(s) they want to modify. Present the sections as options:
   - `workflow` — Completion mode, merge strategy, branch mode
   - `backend` — Remote API settings
   - `storage` — Database path
   - `log` — Logging configuration
   - `project` — Project name
   - `user` — User name (stored in config.local.toml or environment variable, not config.toml)
   - `web` — Web server settings
   - `hooks` — Task lifecycle hooks
3. For the selected section(s), walk through the same questions as Create Mode, showing current values. For `user`, follow the same Option A / Option B flow as Create Mode (write to config.local.toml or advise on environment variable).
4. Update only the modified sections in the appropriate config file using the Edit tool (config.toml for most sections, config.local.toml for user).

### Notes

- **Scope**: Only project-level config (`.senko/config.toml` and `.senko/config.local.toml`). Do not modify user-level config (`~/.config/senko/config.toml`).
- **User name**: Always write to `.senko/config.local.toml` (git-ignored), never to `.senko/config.toml`. Alternatively, advise using the `SENKO_USER` environment variable.
- **Defaults**: Only write sections/keys where the user wants non-default values. Comment out defaults for reference.
- **Validation**: Ensure values are valid (e.g., `completion_mode` must be `merge_then_complete` or `pr_then_complete`).
- **Hooks**: Each hook entry needs a unique name under the event key (e.g., `[hooks.on_task_ready.my-hook]`).
