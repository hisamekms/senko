# Workflow Stage Examples

Practical examples for tuning Claude Code's behavior to match project-specific rules.

For the concepts see [Event-Driven Workflow](../../explanation/event-driven-workflow.md); for the TOML schema see [`[workflow.*]` Config](../../reference/config/workflow.md).

## Pattern 1: Defaults for new tasks

When every new task needs the same tags / DoD / priority:

```toml
[workflow.task_add]
default_dod = [
  "Unit tests added",
  "CHANGELOG.md updated",
]
default_tags = ["backend"]
default_priority = "p2"
instructions = [
  "State Acceptance Criteria in the description",
  "Split tasks larger than ~3 days",
]
```

These are applied by the skill, which reads them via `senko config --output json` (the senko CLI itself does not inject them on `task add`). They take effect when Claude creates a task through `/senko add`.

## Pattern 2: Require sections in the plan stage

When you want a consistent plan format:

```toml
[workflow.plan]
required_sections = ["Overview", "Acceptance Criteria", "Risks"]
instructions = [
  "Save the plan in the task.plan field",
  "Keep Overview within three sentences",
  "Reject a plan with no Risks",
]
```

The skill reads this while generating the plan and re-prompts the agent if anything is missing.

## Pattern 3: Unify branch naming in `branch_set`

```toml
[workflow]
branch_template = "senko/{{id}}-{{slug}}"
branch_mode = "worktree"

[workflow.branch_set]
instructions = [
  "No feature/ or fix/ prefix (branch_template already standardizes it)",
  "Check for an existing worktree first",
]
```

## Pattern 4: Tell the skill what to check before `task_complete`

To have the agent verify something before completing, use `instructions` / `prompt`:

```toml
[workflow]
merge_via = "pr"

[workflow.task_complete]
instructions = [
  "Always verify the PR is merged before completing",
  "If any DoD items remain unchecked, ask a human for review before completing",
]
```

To **mechanically enforce** something (i.e., stop the state transition) — e.g., require CI green — use a **CLI-runtime hook** instead of a workflow stage. Put `when = "pre"` + `mode = "sync"` + `on_failure = "abort"` under `[cli.task_complete.hooks.*]`; the check runs at `senko task complete` time (see [`[cli.*]` Hook Examples](hooks.md)).

## Pattern 5: Dedup guidance on `contract_note_add`

To discourage writing the same note twice on a Contract:

```toml
[workflow.contract_note_add.hooks.dedup_check]
command = "true"
prompt = "Re-read existing notes and skip this one if the same observation already exists."
when = "pre"
```

`command = "true"` is a no-op on the shell side; `prompt` injects the instruction into Claude.

## Pattern 6: Collect required metadata in the plan stage

Assuming you've declared `estimate_points` at the project level with `required_on_complete = true`:

```toml
[[workflow.plan.metadata_fields]]
key = "estimate_points"
source = "prompt"
prompt = "Estimate in Fibonacci (1, 2, 3, 5, 8, 13, 21)."
```

The value is injected into metadata at the end of plan, so completion won't fail validation later.

## Pattern 7: Add a custom stage (skill ignores it; external scripts can read it)

```toml
[workflow.security_review]
instructions = [
  "If the change touches credential / secrets handling, consult SRE",
]
```

senko's built-in skill doesn't fire this stage, but it's exposed via `senko config --output json`, so your own skill or CI script can pick it up.

## Verifying

After editing:

```bash
senko config                # inspect the merged config
senko doctor                # warn about mismatched runtimes / invalid hook combinations
senko hooks test task_complete 1    # actually fire the hook to test it
```

## Anti-Patterns

- **Dumping code conventions into `instructions`** — slows the agent down and reduces compliance. Keep conventions in `docs/` and tell the agent "read `docs/code-style.md` before implementing."
- **Long-running hooks (`when = "pre"` + `mode = "sync"` for minutes)** — the CLI hangs. Switch to `async` or hand the work off to a job queue.
- **Secrets hard-coded in `command = "..."`** — inject them through `env_vars` with `required = true` from CI secrets.
