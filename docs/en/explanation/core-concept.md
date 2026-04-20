# Core Concept: The Three Pillars

senko is less a "task manager" and more a **workflow orchestrator that lets AI agents drive work autonomously**.

A typical task manager is humans talking to humans. senko's emphasis is on **codifying project-specific ways of working and teaching them to agents**. Designed primarily to pair with Claude Code, senko supports autonomous AI-agent behavior through three pillars.

## Pillar 1: Event-Driven Workflow

Every project has its own rules — "this repo requires CI to be green before completing a task," "branch names follow this template," "include an audit checkbox in the DoD." Teaching an agent these rules by hand each time isn't realistic, and cramming them into prompts bloats the skill and eats up context.

senko hooks into **state-transition events (`task_add` / `task_start` / `task_complete` / `contract_note_add`, etc.)** and **automatically injects and verifies rules** against the agent's actions.

- **Hook**: runs a shell command before/after state transitions (CI checks, notifications, audit log shipping).
- **Workflow stage**: a container of "instructions and checklists" the Claude Code skill reads per phase (`plan`, `implement`, `branch_set`, etc.).
- **Runtime separation**: for the same event you can decide whether it fires under `cli` / `server.remote` / `server.relay`, per config section.

→ Deep dive: [Event-Driven Workflow](event-driven-workflow.md)

## Pillar 2: Focus on the Next Task

Asking an AI agent to "handle the whole project" is a classic anti-pattern. A huge prompt bloats context, muddies decisions, and cannot finish in a single session.

senko **splits work into a queue of tasks with dependencies and priorities** and steers the agent to commit to just one task at a time.

- **Task = a unit sized to finish inside one context window** — close the session after each task and reset context for the next.
- **Auto-select from ready tasks whose dependencies have cleared** — the agent doesn't decide what's next. `senko task next` picks by priority → `created_at` → id.
- **Parallel pick** — when multiple ready tasks exist, separate sessions/worktrees can grab them simultaneously.

→ Deep dive: [Focus on the Next Task: The Execution Model](task-decomposition.md)

## Pillar 3: Contracts Hold the Big Picture

Because tasks are sized to one context window, findings from a single task tend to vanish. But real work — feature additions, migrations — spans **multiple tasks**, and you need a container for "the big picture," "accumulated constraints," and "decision history."

**Contracts** play that role in senko.

- A Contract groups multiple Tasks and holds **DoD and Notes** across them.
- When a task writes back a Note with `source_task_id`, you can later trace which task produced which insight.
- A Contract is roughly the grain of "epic," "a bundle of design decisions," or "a migration's purpose."

→ Deep dive: [Holding the Big Picture with Contracts](contract.md)

## How the Three Pillars Fit Together

A typical flow:

```
  Create Contract          ← Pillar 3: declare what you want to achieve
     │
     ▼
  Split into Tasks         ← Pillar 2: line them up with dependencies and priority
     │
     ▼
  task_add hook / stage    ← Pillar 1: auto-inject DoD templates, naming conventions, required metadata
     │
     ▼
  task next picks one      ← Pillar 2: the agent handles just this one
     │
     ▼
  plan / implement stage   ← Pillar 1: per-phase instructions and verification hooks
     │
     ▼
  task complete hook       ← Pillar 1: CI / DoD / PR merge verification
     │
     ▼
  Add a Contract note      ← Pillar 3: fold findings back into the big picture
     │
     ▼
  Next task becomes ready  ← Pillar 2: dependency clears, the next one surfaces
```

## Supporting Concepts

A few concepts underpin the three pillars.

| Concept | Role | Detail |
|---|---|---|
| **Project** | Data isolation unit. Every Task / Contract / Member belongs to one | [Data Model](../reference/data-model.md) |
| **User / Member / API key** | Who acts, with what role, on which projects | [Data Model](../reference/data-model.md) |
| **MetadataField** | Typed schema for `task.metadata` / `contract.metadata` (per project) | [Event-Driven Workflow](event-driven-workflow.md#metadata-field) |
| **Runtime** | Execution mode (`cli` / `server.remote` / `server.relay` / `workflow`). Governs which config sections and hooks are active | [Choosing a Runtime](runtimes.md) |
| **Dependency** | A directed edge Task → Task: "B must complete before A can start" | [Focus on the Next Task](task-decomposition.md#dependency) |

## Why It's Shaped This Way

- **Why Task and Contract are separate**: mixing two grains in one table muddies what "done" means. A Task is frozen once completed. A Contract is a container whose DoD fills in progressively.
- **Why MetadataField is per-project schema**: teams want different required fields ("estimate," "owning team," "risk level"). Rather than fixed columns, senko externalizes it as a schema definition.
- **Why hooks are runtime-scoped**: when the same project is touched both from a local CLI and from a server, some hooks should only run on one side (desktop notification vs. audit log).

## What to Read Next

- Each pillar in depth:
  - [Event-Driven Workflow](event-driven-workflow.md)
  - [Focus on the Next Task: The Execution Model](task-decomposition.md)
  - [Holding the Big Picture with Contracts](contract.md)
- Runtime substrate → [Choosing a Runtime](runtimes.md)
