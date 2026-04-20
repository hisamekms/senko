# Holding the Big Picture with Contracts

> This is **Pillar 3** of the three. Read [Core Concept: The Three Pillars](core-concept.md) first for the overall picture.

## The Problem

Following [Focus on the Next Task: The Execution Model](task-decomposition.md), Tasks are sized to **one context window**. But real work — "add a feature," "do a refactor," "migrate something" — happens at a grain that **spans multiple tasks**, and Tasks alone fail to hold:

- **The big picture**: what you were trying to achieve in the first place.
- **Accumulated constraints and decisions**: a constraint spotted in task 1 becomes critical for task 3.
- **Cross-cutting DoD**: completion conditions like "including SIEM integration" that won't fit under any single task.
- **Investigation findings**: insights produced by an investigation task evaporate the moment that task completes.

**Contracts** cover that grain in senko.

## Where Contracts Sit

```
Contract (coarse grain)
  │ Holds purpose, overall DoD, accumulated Notes
  │
  └─ Task (fine grain)
       │ Fits in one context window
       │ Forward-only state transitions
       │
       └─ Writes Notes back to the Contract
          (tagged with source_task_id)
```

## Contract Fields

| Field | Type | Meaning |
|---|---|---|
| `id` | int | Contract ID |
| `title` | string | Contract name (required) |
| `description` | string? | Summary |
| `definition_of_done` | `{content, checked}[]` | Contract-level completion conditions |
| `tags` | string[] | Categorization |
| `metadata` | JSON | Freeform (can be schema'd via project-level MetadataField) |
| `notes` | `{content, source_task_id, created_at}[]` | Findings collected along the way |
| `created_at` / `updated_at` | ISO 8601 | Timestamps |

**A key asymmetry**: Contracts have no `status`. Instead,

```
is_completed = DoD has at least one item AND every item is checked
```

derives "completed or not" (a Contract with an empty DoD is never `is_completed` — there's nothing to evaluate).

## Relationship with Tasks

A Task **may link to at most one Contract** via `contract_id` (optional).

```bash
senko task add --title "Add webhook endpoint" --contract 7
senko task list --contract 7       # list tasks under Contract 7
```

- A single Task belongs to at most one Contract.
- Task state transitions and Contract DoD checks are **independent** (task completion ≠ Contract DoD check).

## Notes: writing findings back

### Basics

Notes append **findings from a task back to the Contract**:

```bash
senko contract note add 7 \
  --content "Postgres migration requires RDS Proxy due to Lambda connection pooling" \
  --source-task 23
```

- `source_task_id` lets you later trace **which task produced which insight**.
- Notes are append-only (don't edit or delete them later).
- Timestamped.

### Why write Notes back to the Contract

When a task completes, its context is closed. Anything the task revealed (library pitfalls, a new dependency, a newly-identified risk) becomes **invisible to the agent starting the next task** if not persisted.

Writing findings into the Contract as Notes means `senko contract get 7` surfaces the accumulated knowledge in one place. The Claude Code skill automatically reads a Contract's Notes at the start of `task execute`, so this mechanism is active in practice.

### How to write good Notes

Good:

```
"Postgres migration requires RDS Proxy due to Lambda connection pooling"
"Existing auth middleware stores session tokens in a way that fails SOC2 review — need to rewrite, not patch"
"DB migration must run before server rollout; coordinated deploy needed"
```

- A **fact, constraint, or decision** useful for judging future tasks.
- One Note = one observation (don't merge multiple).

Bad:

```
"Task 23 done"                         ← adds nothing; read the task log
"Worked hard"                          ← not a finding
"Finished everything"                  ← not useful for future decisions
```

## DoD: Contract-Level Completion Conditions

Task DoD is per task. Contract DoD represents **cross-task requirements**.

Example: DoD of the "Implement webhook delivery" Contract

```
- [x] Receiver endpoint implemented
- [x] Auth middleware applied
- [x] e2e tests pass
- [ ] SIEM is receiving shipped logs (ops verified)
- [ ] Documentation describes the procedure
```

These are checked/unchecked at points separate from individual task completion:

```bash
senko contract dod check 7 4        # check DoD item #4
senko contract dod uncheck 7 4      # uncheck it
```

The operating principle: treat a Contract as "still in progress" until `is_completed = true`.

## When to Use Contracts

### Good fits

- **Multi-task feature work**: "Implement webhook delivery"
- **Migrations / refactors**: "Migrate auth to OIDC," "Rewrite auth middleware"
- **Investigations**: "Explore the Postgres migration path"
- **Cross-cutting DoD**: "Pass SOC2 review"

### Poor fits

- **One-off small tasks**: independent bug fixes, comment edits, typo fixes — just a Task, no Contract needed.
- **Ongoing maintenance**: persistent rules like "lint must always pass" belong in hooks / workflow stages, not Contracts.
- **Ordered sequences of Contracts**: senko doesn't model inter-Contract dependencies (use tags or naming conventions).

## Contract DoD × Hooks

`contract_dod_check` / `contract_note_add` fire hooks like any other event:

```toml
[server.remote.contract_dod_check.hooks.audit]
command = "logger -t senko-audit 'contract DoD checked'"
mode = "async"

[workflow.contract_note_add.hooks.dedup]
command = "true"
prompt = "Skip the note if the same observation already exists in earlier notes."
when = "pre"
```

→ Hook mechanics: [Event-Driven Workflow](event-driven-workflow.md).

## Typical Lifecycle

```
1. senko contract add --title "Migrate auth to OIDC" \
      --definition-of-done "Existing users can log in without disruption" \
      --definition-of-done "Legacy API keys are revoked"

2. senko task add --title "Add OIDC config skeleton" --contract 7
   senko task add --title "Wire JWT verifier" --contract 7
   senko task add --title "Migrate first internal service" --contract 7

3. During each task,
   senko contract note add 7 --content "..." --source-task <id>

4. After all tasks complete, review the Contract DoD and
   senko contract dod check whatever is satisfied.

5. When is_completed = true, the Contract is done.
```

## Design Decisions

- **Why Contracts have no status**: they're a container for "what we want to achieve," which isn't a one-way state machine like a Task. They're an aggregate whose DoD fills in progressively.
- **Why Notes are append-only**: if rewrites / deletes were allowed, the cumulative record loses trust. Correct a mistake by appending a new Note.
- **Why Task ↔ Contract is 1:N (a Task belongs to at most one Contract)**: keeping each Task in at most one Contract keeps `source_task_id` in Notes unambiguous.

## What to Read Next

- Task grain → [Focus on the Next Task: The Execution Model](task-decomposition.md)
- Contract-related events and hooks → [Event-Driven Workflow](event-driven-workflow.md)
- CLI command details → [CLI Reference](../reference/cli.md)
- DB schema → [Data Model](../reference/data-model.md)
