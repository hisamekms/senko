# `[workflow.*]` Config

**Logical stage definitions** read by the Claude Code skill. May be consumed under any runtime (distinct from real-runtime state-transition hooks).

Concept: [Event-Driven Workflow](../../explanation/event-driven-workflow.md).

## `[workflow]` (Top-Level)

Project-wide workflow defaults.

| Key | Type | Default | Description |
|---|---|---|---|
| `merge_via` | string | `"direct"` | `"direct"` (git merge) or `"pr"` (requires PR merge verification) |
| `auto_merge` | bool | `true` | With `merge_via="direct"`, auto-merge on task complete |
| `branch_mode.type` | string | `"worktree"` | `"worktree"` (git worktree) or `"branch"` (regular branch) |
| `branch_mode.create` | bool | `true` | `true`: the skill provisions a new resource / `false`: reuse an existing resource (the skill does not create one) |
| `merge_strategy` | string | `"rebase"` | `"rebase"` or `"squash"` |
| `branch_template` | string | `null` | Branch naming template; supports `{{id}}` / `{{slug}}` (e.g. `"senko/{{id}}-{{slug}}"`) |

### The four `branch_mode` combinations

| `type` | `create` | Behavior |
|---|---|---|
| `worktree` | `true` (default) | The skill provisions a fresh worktree per task and switches into it |
| `worktree` | `false` | Reuse an externally-managed worktree (created in advance by a human or another tool). If no matching worktree exists, the skill **stops with an error — it does not fall back** |
| `branch` | `true` | The skill creates / switches the branch in the current checkout (no worktree) |
| `branch` | `false` | Work on the currently checked-out branch; the skill performs no branch operations at all |

**Backward compatibility:** the legacy string form `branch_mode = "worktree"` / `"branch"` is still accepted and treated as `{ type = <value>, create = true }`. You do not need to rewrite an existing `config.toml`. For new configurations, the table form is recommended.

```toml
[workflow.branch_mode]
type = "worktree"
create = true
```

env overrides: `SENKO_MERGE_VIA` / `SENKO_AUTO_MERGE` / `SENKO_BRANCH_MODE_TYPE` / `SENKO_BRANCH_MODE_CREATE` / `SENKO_MERGE_STRATEGY` (the old `SENKO_BRANCH_MODE` has been removed)

## `[workflow.<stage>]`

Fields common to every stage:

| Key | Type | Default | Description |
|---|---|---|---|
| `instructions` | string[] | `[]` | Instructions shown to the agent on entering this stage |
| `hooks.<name>` | HookDef | `{}` | Hooks that fire for this stage (may carry a `prompt` field) |
| `metadata_fields` | object[] | `[]` | Metadata to collect in this stage; values are shallow-merged into the task/contract metadata |

Stage-specific extras:

| Stage | Key | Type | Description |
|---|---|---|---|
| `workflow.task_add` | `default_dod` | string[] | Default DoD for new tasks |
| `workflow.task_add` | `default_tags` | string[] | Default tags |
| `workflow.task_add` | `default_priority` | string | Default priority |
| `workflow.plan` | `required_sections` | string[] | Required sections in the plan document |

**Unknown keys are preserved (not discarded)** and surfaced via `senko config --output json`. You can therefore have custom skills read custom keys.

## Built-in Stages

Stages the skill fires:

```
task_add       task_publish   task_start    task_complete
task_cancel    task_select    branch_set    branch_cleanup
branch_merge   pr_create      pr_update     plan
implement      contract_add   contract_edit contract_delete
contract_dod_check   contract_dod_uncheck   contract_note_add
```

> The currently bundled workflow actually fires `task_*` + `plan` / `implement` / `branch_*` / `pr_*` + `contract_add` / `contract_note_add` / `contract_dod_check`. `contract_edit` / `contract_delete` / `contract_dod_uncheck` are reserved but not used in the default skill scenarios.

**Any name is accepted**, so you can define project-specific stages like `security_review`.

## Hook `prompt` Field

Workflow hooks have a special behavior — on top of firing a shell command they can **inject instructions into the agent**:

```toml
[workflow.contract_note_add.hooks.review]
command = "true"                                     # shell side is no-op
prompt = "Check existing notes and skip adding if the same observation is already there"
when = "pre"
```

- `command` still runs in the shell (same as any hook).
- `prompt` is assembled by the skill as an instruction to the agent.
- When you don't need any shell-side work, set `command = "true"`.

## `metadata_fields`

```toml
[[workflow.task_add.metadata_fields]]
key = "team"
source = "value"
value = "backend"

[[workflow.plan.metadata_fields]]
key = "estimate_points"
source = "prompt"
prompt = "Estimate in Fibonacci"
```

| Key | Type | Description |
|---|---|---|
| `key` | string | Metadata key |
| `source` | `"value"` / `"prompt"` | Where the value comes from |
| `value` | string? | For `source="value"`: the constant to inject |
| `prompt` | string? | For `source="prompt"`: the question posed to the agent |

A typical pattern: always inject a project-level `metadata_fields` entry that's also declared `required_on_complete = true`.

## Minimum Example

```toml
[workflow]
branch_template = "senko/{{id}}-{{slug}}"
merge_via = "pr"

[workflow.task_add]
default_dod = ["Unit tests pass", "Docs updated"]
default_priority = "p2"

[workflow.plan]
required_sections = ["Overview", "Acceptance Criteria"]
instructions = ["Save the plan in the task.plan field"]

[workflow.task_complete.hooks.ci_check]
command = "gh pr checks $SENKO_PR_URL --required"
when = "pre"
mode = "sync"
on_failure = "abort"
```

Examples: [Workflow Stage Examples](../../guides/cli/workflow-stages.md).
