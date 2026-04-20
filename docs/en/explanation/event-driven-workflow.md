# Event-Driven Workflow

> This is **Pillar 1** of the three. Read [Core Concept: The Three Pillars](core-concept.md) first for the overall picture.

## The Problem

Every project has **project-specific rules**:

- Tasks in this repo can only be completed after the PR is merged
- Branch names must follow `<prefix>/<task-id>-<slug>`
- A task without an `estimate_points` value cannot be completed
- Task completion must ship an audit log to SIEM
- The `plan` phase must contain an "Acceptance Criteria" section

Teaching these to the agent by prompt each time isn't realistic; it only bloats the prompt. senko's approach is to **use domain events as the hook point and inject/verify rules automatically**.

## Architecture Overview

```
                 ┌─ CLI subcommand ─┐
  user / agent ──┤                  ├── state transition ──┐
                 └─ REST API ───────┘                      │
                                                           ▼
                                       ┌─ HookTrigger ─┐
                                       │   task_add    │
                                       │   task_start  │
                                       │   task_complete
                                       │   contract_*  │
                                       └───┬───────────┘
                                           │
              ┌────── hooks (runtime × action) ──────┐
              │  [cli.task_complete.hooks.ci_green]   │── Pillar 1: auto-fire
              │  [server.remote.task_add.hooks.audit] │   project-specific rules
              │  [server.relay.task_start.hooks.log]  │
              └────────────────────────────────────────┘

              ┌────── workflow stages (logical phases) ─┐
              │  [workflow.plan]                         │── Pillar 1: instructions
              │    instructions = [...]                  │   for the agent's logical
              │    hooks.<name> (with prompt)            │   phases
              │  [workflow.branch_set]                   │
              │  [workflow.task_complete] ...            │
              └────────────────────────────────────────────┘
```

senko branches **two mechanisms** off the same event source:

1. **Hook** — the senko binary runs a shell command before/after a state transition.
2. **Workflow stage** — an instruction set that the Claude Code skill reads when it judges "I'm in this phase now."

## Mechanism 1: Hooks

### Trigger points

Hooks are tied to `HookTrigger`s identified by **aggregate × action**:

| Aggregate | Action | Fires when |
|---|---|---|
| Task | `task_add` | Before / after creation |
| Task | `task_ready` | draft → todo |
| Task | `task_start` | todo → in_progress (including auto-selection via `task next`) |
| Task | `task_complete` | in_progress → completed (after DoD verification) |
| Task | `task_cancel` | transition to canceled |
| Task | `task_select` | when `task next` picks a candidate (branched by `on_result`) |
| Contract | `contract_add` / `contract_edit` / `contract_delete` | CRUD |
| Contract | `contract_dod_check` / `contract_dod_uncheck` | DoD updates |
| Contract | `contract_note_add` | Notes appended |

→ Envelope and field details: [Hooks Reference](../reference/hooks.md).

### Runtime scoping

The same `task_complete` fires under **different sections depending on which runtime is active**:

```
Running as cli              → only [cli.task_complete.hooks.*] fires
Running as server.remote    → only [server.remote.task_complete.hooks.*] fires
Running via server.relay    → only [server.relay.task_complete.hooks.*] fires
```

→ When to use which: [Choosing a Runtime](runtimes.md).

### The four knobs that shape a hook's behavior

```toml
[cli.task_complete.hooks.ci_green]
command = "gh pr checks $SENKO_PR_URL --required"
when = "pre"          # before / after the state transition (post by default)
mode = "sync"         # wait for completion, or fire-and-forget
on_failure = "abort"  # on non-zero exit: abort / warn / ignore
```

- Only the combination **`when = "pre"` + `mode = "sync"` + `on_failure = "abort"`** can **cancel a state transition** (other combinations degrade to `warn`).
- `async` is fire-and-forget, so it can't `abort`.
- What skill users see as "blocking verification" is always this trio.

### Patterns for injecting project-specific rules

| Goal | Setup |
|---|---|
| Require CI green before task complete | `[cli.task_complete.hooks.ci] when = pre, mode = sync, on_failure = abort` |
| Ship an audit log on task add (server side) | `[server.remote.task_add.hooks.audit] when = post, mode = async` |
| Notify when `task next` finds nothing | `[cli.task_select.hooks.idle] on_result = "none"` |
| Post a Slack message on new Contract notes | `[server.remote.contract_note_add.hooks.slack]` |

## Mechanism 2: Workflow Stages

Hooks run commands, but **telling the agent "what to think about while acting"** is a different layer — that's the **workflow stage**.

### What's a stage

The Claude Code skill operates in **logical phases** like "I'm planning now" or "I'm implementing now." These don't necessarily correspond 1:1 to CLI commands:

- `plan` phase: `senko task edit --plan ...` hasn't been called yet, but the agent is designing.
- `implement` phase: `senko task start` has already run; the agent is writing code.
- `branch_set` phase: just before the git branch is created — a good spot to inject a naming template or a pre-check.

We treat these as **logical stages** separate from "CLI actions," all under `[workflow.<stage>]`.

### Built-in stages

| Stage | Meaning |
|---|---|
| `task_add` | Before / after adding a new task |
| `task_ready` | draft → todo |
| `task_start` | todo → in_progress (including `task next` auto-select) |
| `task_complete` | in_progress → completed |
| `task_cancel` | transition to canceled |
| `task_select` | when `task next` attempts to pick a task |
| `branch_set` | just before cutting the working branch |
| `branch_cleanup` | before deleting a branch |
| `branch_merge` | just before a merge operation |
| `pr_create` | before creating a PR |
| `pr_update` | before updating a PR |
| `plan` | the phase where the design is being written |
| `implement` | the implementation phase |
| `contract_add` / `contract_edit` / `contract_delete` | Contract CRUD |
| `contract_dod_check` / `contract_dod_uncheck` | Contract DoD updates |
| `contract_note_add` | before appending a Contract note |

**Any name is accepted**, so you can add a custom stage like `[workflow.my_phase]` for a custom skill to consume. senko itself won't fire non-built-in stages, but they are surfaced verbatim via `senko config --output json`.

### Fields a stage can declare

Under `[workflow.<stage>]`:

| Key | Type | Role |
|---|---|---|
| `instructions` | string[] | Instructions the agent should follow in this stage |
| `hooks.<name>` | HookDef | Fire shell hooks (same schema as hooks for any other runtime) |
| `metadata_fields` | object[] | Metadata keys / values to collect or inject in this stage |

Stage-specific extras (examples):

| Stage | Key | Role |
|---|---|---|
| `task_add` | `default_dod` / `default_tags` / `default_priority` | Defaults for new tasks |
| `plan` | `required_sections` | Required section names in the plan document |

Unknown keys are **preserved, not discarded**, and external scripts can read them through `senko config --output json`.

### Stage hooks vs. runtime hooks

| Hook location | Fired by | When |
|---|---|---|
| `[cli/server.*/server.relay.<action>.hooks.<name>]` | senko binary | Before/after the state transition (automatic) |
| `[workflow.<stage>.hooks.<name>]` | Claude Code skill | When the skill judges it has entered that stage |

Workflow hooks carry a special field: **`prompt`**. The skill treats this string as **an instruction for the agent itself** (a prompt addition, not a shell command):

```toml
[workflow.contract_note_add.hooks.review_before_note]
command = "true"                                       # no-op
prompt = "Skip the note if the same observation already exists in earlier notes."
when = "pre"
```

In this example, just before appending a Contract note, the agent is told "check if the same observation already exists in earlier notes."

## Schema Injection via `metadata_fields`

You can declare metadata that must be filled in a given stage:

```toml
[[workflow.task_add.metadata_fields]]
key = "team"
source = "value"
value = "backend"

[[workflow.plan.metadata_fields]]
key = "estimate_points"
source = "prompt"
prompt = "Estimate in Fibonacci (1, 2, 3, 5, 8, 13, 21)."
```

`source` can be:

- `value`: inject a constant.
- `prompt`: ask the agent for input using the `prompt` text.
- `env`: take it from an environment variable.
- `command`: use the output of a shell command.

### <a id="metadata-field"></a>MetadataField (per-project schema)

Stage-level `metadata_fields` defines **what value is filled in that stage**, while **MetadataField** defines, at the project level, **what is allowed or required in `metadata`**.

```bash
senko project metadata-field add \
  --name estimate_points \
  --type number \
  --required-on-complete \
  --description "Relative estimate (Fibonacci)"
```

- `field_type` is `string` / `number` / `boolean`.
- Setting **`required_on_complete = true`** means `task complete` fails if that key is missing.
- Metadata is edited with `--metadata '{"estimate_points": 5}'` (shallow merge) or `--replace-metadata '...'` (full replacement).

Pairing a stage's `metadata_fields` (the injection side) with the project's MetadataField (the verification side) lets you build "fill it in during plan → verify at complete."

## Common Patterns

### 1. Require a plan format

```toml
[workflow.plan]
required_sections = ["Overview", "Acceptance Criteria", "Risks"]
instructions = [
  "Save the plan in the task.plan field",
  "Always ask a human to review the plan before implementing",
]
```

### 2. Unify branch naming via `branch_set`

```toml
[workflow]
branch_template = "senko/{{id}}-{{slug}}"

[workflow.branch_set]
instructions = ["No feature/, fix/, or chore/ prefix (branch_template already standardizes it)"]
```

### 3. Require CI green on `task_complete`

```toml
[cli.task_complete.hooks.ci_green]
command = "gh pr checks $SENKO_PR_URL --required"
when = "pre"
mode = "sync"
on_failure = "abort"
```

### 4. Notify when `task_select` finds nothing

```toml
[cli.task_select.hooks.idle_notify]
command = "notify-send 'No ready tasks. Run /senko task list to review.'"
on_result = "none"
```

## How the Skill Picks This Up

The `SKILL.md` installed by `senko skill-install` calls `senko config --output json` internally, reads the current workflow config, and assembles per-stage `instructions` / hook `prompt`s **as the agent's current instructions**.

So the flow is:

1. Write `[workflow.*]` / `[cli.*]` / `[server.*.*]` in `.senko/config.toml` per project.
2. The developer refreshes SKILL.md with `senko skill-install`.
3. Claude Code runs `/senko`, consulting the workflow config along the way.

## Design Decisions

- **Why have both hooks and workflow stages**: hooks fire shell commands (mechanical verification), workflow stages instruct the agent (judgment-based verification). Same event, two vectors.
- **Why hooks are scoped per runtime**: even for the same event, you want different hooks for the CLI (developer desktop notification) vs. the server (SIEM).
- **Why only `pre + sync + abort` can cancel a transition**: async can't be waited for, so there's no decision point to block the transition before it completes.

## What to Read Next

- Pillar 2 → [Focus on the Next Task: The Execution Model](task-decomposition.md)
- Pillar 3 → [Holding the Big Picture with Contracts](contract.md)
- Choosing a runtime → [Choosing a Runtime](runtimes.md)
- Hook envelope and firing timing table → [Hooks Reference](../reference/hooks.md)
- `[workflow.*]` TOML details → [`[workflow.*]` Config](../reference/config/workflow.md)
- Examples → [Workflow Stage Examples](../guides/cli/workflow-stages.md) / [`[cli.*]` Hook Examples](../guides/cli/hooks.md)
