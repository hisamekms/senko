# Focus on the Next Task: The Execution Model

> This is **Pillar 2** of the three. Read [Core Concept: The Three Pillars](core-concept.md) first for the overall picture.

## The Problem

Telling an AI agent "drive the whole project" is a classic anti-pattern:

- **Context pollution**: mixing unrelated research / implementation code / test output into one session muddies every decision.
- **Token overflow**: working inside a single session means context grows monotonically until it breaks.
- **No parallelism**: feeding "next, do this; next, do that" serially prevents splitting across sessions.

senko solves this with **Tasks sized to one context window** plus **automatic picking based on dependency resolution and priority**.

## Task: sized to one context window

### Core attributes

```
task_number: project-scoped sequential ID (shown by the CLI)
title:       task name (required)
priority:    P0 – P3 (default P2, P0 highest)
status:      draft → todo → in_progress → completed / canceled
dependencies: references to other tasks (array of task_number)
```

### State transitions (forward only)

```
draft → todo → in_progress → completed
                    ↓
                canceled   (from any state)
```

- Transitions are one-way. You can't return from `completed`.
- Cancellation is the only emergency exit allowed from any state.
- If you need to "rewind," create a new task instead.

### What "sized to one context window" means in practice

Rules of thumb:

- The range from `task start` through commit / PR creation / `task complete` fits in a single session.
- Typical diff: 1–2 files, tens to low-hundreds of lines.
- Investigation-only tasks (read the code, write findings back to Contract Notes) sit at this grain too.
- **Wrong shape**: "refactor the entire auth layer" — that's a Contract.

Why the grain matters: after a task completes, you can **close the session and reset context**. That discipline is what keeps the next task from being polluted.

## <a id="dependency"></a>Dependency: "B must complete before A can start"

### Basics

```
task A → task B  :  "A depends on B"  : A cannot start until B is completed
```

- Directed edge between Tasks.
- Cycles are detected and rejected at both the CLI and API level.
- Cycle detection runs on both add and edit.

### Editing

```bash
senko task deps add <task> --on <dep>
senko task deps remove <task> --on <dep>
senko task deps set <task> --on <dep1>,<dep2>  # replace all
senko task deps list <task>
senko graph                                     # visualize as a Mermaid graph
```

### Contracts don't have dependencies

Contracts carry no dependency relationships. If you need to order Contracts, use tags or a naming convention. (A Contract describes "what we want to achieve," not "when each thing runs.")

## ready and Auto-Picking

### What `ready` means

A Task is **ready** — i.e. a valid candidate to start — when **`status = todo` and all its dependencies are `completed`**.

```
task #3 (todo, deps = [#1, #2])

  #1 completed, #2 completed   →  #3 is ready
  #1 completed, #2 in_progress →  #3 is not ready yet
  #1 canceled,  #2 completed   →  #3 is not ready
                                    (canceled is not treated as completed)
```

### The `senko task next` selection algorithm

`task next` picks exactly one task from the ready set by this order:

```
priority (P0 → P3 ascending)
    └─ tie breaker: created_at (oldest first)
        └─ tie breaker: id (ascending)
```

This lets the agent operate under a strict "don't think about what's next, don't ask a human" rule. That determinism is the concrete implementation of **Pillar 2**.

### How the skill uses it

`/senko` (no arguments) calls `task next` internally, displays the chosen task, and proceeds into work:

```
/senko                 # auto-pick the next task and start it
/senko start 3         # explicitly pick ID 3 (warn if not ready)
```

## Parallel Pick

When multiple ready tasks exist, **separate sessions/worktrees can pick them at the same time**:

```
ready: [#5 (P0), #8 (P1), #12 (P1)]

  developer A: /senko          → claims #5 (P0 wins)
  developer B: /senko          → claims #8 (tie-broken by oldest)
```

senko records **who or which session picked which task** via `assignee_user_id` / `assignee_session_id`. Already-claimed tasks are removed from other sessions' `task next` candidates, so double-assignment doesn't happen.

In team settings:

- **One worktree / one session per person** is the baseline.
- `assignee_user_id` can filter "tasks not mine" out of candidates.
- Claude Code running in parallel sessions (separate terminals) picks tasks through the same mechanism without conflict.

## Task vs. Contract

| Axis | Task | Contract |
|---|---|---|
| Grain | One context window | Spans multiple tasks |
| Status | Explicit state machine | No status (all DoD checked = `is_completed`) |
| Dependencies | Yes | No |
| Cumulative findings | Not retained by default | Written back as Notes |
| Examples | "Implement webhook handler", "Normalize function naming in X" | "Migrate auth layer to OIDC", "Add auditing" |

Contracts are covered under **Pillar 3**. → [Holding the Big Picture with Contracts](contract.md)

## Common Decomposition Patterns

### Pattern A: new feature

```
Contract: Implement webhook delivery
  └─ Task 1 (P1): Add receiver endpoint in axum
  └─ Task 2 (P1, deps=[1]): Insert auth middleware
  └─ Task 3 (P2, deps=[1]): Add e2e tests
  └─ Task 4 (P2, deps=[2,3]): Update documentation
```

Task 3 doesn't depend on Task 2 (tests can be written independently of the auth logic), so **2 and 3 can be picked in parallel**.

### Pattern B: refactor

```
Contract: Refactor auth middleware
  └─ Task 1 (P1): Add characterization tests that pin current behavior
  └─ Task 2 (P0, deps=[1]): Define the new AuthProvider trait
  └─ Task 3 (P1, deps=[2]): Adapt the existing impl to match the trait
  └─ Task 4 (P2, deps=[3]): Remove the old impl
```

Refactors **tend to have linear dependency chains**, so parallel pick buys little. The value is still in not cramming everything into one session — context resets let you observe and adjust between steps.

### Pattern C: investigation

```
Contract: Investigate PostgreSQL migration path
  └─ Task 1 (P2): Catalog SQLite-specific SQL in the current codebase → Contract notes
  └─ Task 2 (P2): Draft migration strategy for Postgres → notes
  └─ Task 3 (P1, deps=[1,2]): Check off the decision on the Contract DoD
```

Investigation tasks **don't produce code**, but findings accumulate in the Contract via Notes, so the next task starts with richer context.

## Design Decisions

- **Why `task next` is deterministic**: to keep the agent from inferring "what's next." If candidate selection isn't mechanical, agents drift on tasks of similar priority.
- **Why the state machine is forward-only**: if `completed` could be rewound, hook idempotency (e.g. audit log shipping) breaks. Rewinds are expressed as new tasks instead.
- **Why `canceled` isn't equivalent to `completed`**: it doesn't satisfy dependencies. Cancellation is "not achieved"; blocking downstream tasks from moving on is intentional.

## What to Read Next

- Relationship to Contracts → [Holding the Big Picture with Contracts](contract.md)
- Task creation / edit commands → [CLI Reference](../reference/cli.md)
- Hook firing across task events → [Event-Driven Workflow](event-driven-workflow.md)
