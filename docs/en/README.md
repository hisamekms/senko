# senko Documentation

senko is a **workflow orchestrator that lets AI agents drive work autonomously**.
It's less of a "task manager" and more of a tool for **codifying project-specific ways of working and teaching them to agents** — designed primarily for use with Claude Code.

> [日本語](../ja/README.md) / **English (this directory)**

## Core Concept: The Three Pillars

senko supports autonomous AI-agent behavior with three pillars:

1. **Event-Driven Workflow** — Inject and verify project-specific rules (DoD / branch conventions / required metadata / per-phase instructions) automatically, in step with the agent's actions. Hooks and workflow stages carry this load.
2. **An execution model that lets the agent focus on the next task** — Split large work into dependency-aware, priority-ordered tasks so the agent always focuses on just the next one. Instead of cramming everything into one huge prompt, context is reset per task. Multiple tasks whose dependencies have all cleared can be picked in parallel from separate sessions.
3. **Contracts hold the big picture** — A task is sized to complete and be discarded within one context window, but a Contract is sized to **group multiple tasks together**, persisting cross-cutting context and findings along with Notes. The overall arc stays visible as the task count grows.

→ Deep dive: [Core Concept: The Three Pillars](explanation/core-concept.md)

## Try It in 30 Seconds

```bash
# 1. Install the binary
curl -fsSL https://raw.githubusercontent.com/hisamekms/senko/main/install.sh | sh

# 2. Install the skill from your project root
cd your-project
senko skill-install

# 3. In Claude Code
#    /senko task add Implement webhook handler
#    /senko
```

## Documentation Layout

Organized in four layers by reader intent.

### I want to get hands-on first — [getting-started/](getting-started/)

Covers three typical deployments with an overview, an architecture diagram, and end-to-end setup.

- [Local SQLite](getting-started/local-sqlite.md) — personal development, solo
- [CLI → Remote → PostgreSQL](getting-started/cli-remote-postgres.md) — team sharing a server
- [CLI → Relay → Remote → PostgreSQL](getting-started/cli-relay-remote-postgres.md) — AI sandbox (CLI holds no secret; relay concentrates the credentials)

### I want to understand the thinking — [explanation/](explanation/)

Explains **why** senko is designed the way it is, framed around the three pillars.

- [Core Concept: The Three Pillars](explanation/core-concept.md) — read this first
- [Event-Driven Workflow](explanation/event-driven-workflow.md) — Pillar 1: how hooks and workflow stages work
- [Focus on the Next Task: The Execution Model](explanation/task-decomposition.md) — Pillar 2: dependencies, priority, parallel pick
- [Holding the Big Picture with Contracts](explanation/contract.md) — Pillar 3: long-lived context and Notes
- [Choosing a Runtime](explanation/runtimes.md) — delivery substrate: cli / server.remote / server.relay

### I want to set up or deploy — [guides/](guides/)

How-to guides organized by deployment shape.

**If you use the CLI** — [guides/cli/](guides/cli/)
- [Installing and Updating the Skill](guides/cli/skill-install.md)
- [Workflow Stage Examples](guides/cli/workflow-stages.md)
- [`[cli.*]` Hook Examples](guides/cli/hooks.md)
- [Switching CLI Backends](guides/cli/backends.md) — SQLite / PostgreSQL / HTTP

**Server operators (`senko serve`)** — [guides/server-remote/](guides/server-remote/)
- [Deploy](guides/server-remote/deploy.md)
- [OIDC Authentication](guides/server-remote/auth-oidc.md) — recommended for production (humans use PKCE, bots use Client Credentials)
- [Trusted Headers Authentication](guides/server-remote/auth-trusted-headers.md) — behind an API Gateway
- [AWS Deployment](guides/server-remote/aws-deployment.md) — API Gateway + Cognito + Lambda
- [`[server.remote.*]` Hook Examples](guides/server-remote/hooks.md)
- [API Key Authentication](guides/server-remote/auth-api-key.md) — for smoke tests and bootstrapping; not meant for production

**Relay operators (`senko serve` in relay mode)** — [guides/server-relay/](guides/server-relay/)
- [Deploy](guides/server-relay/deploy.md)
- [Token Relay Pattern](guides/server-relay/token-relay.md)
- [`[server.relay.*]` Hook Examples](guides/server-relay/hooks.md)

### I want to look up a spec — [reference/](reference/)

- [CLI Reference](reference/cli.md) — every subcommand
- [REST API Reference](reference/api.md) — every endpoint
- [Data Model](reference/data-model.md) — DB schema
- [Hooks Reference](reference/hooks.md) — envelope and trigger matrix
- **Config Reference** — [reference/config/](reference/config/)
  - [Overview](reference/config/overview.md) — file layout, precedence, runtime filtering
  - [`[cli.*]`](reference/config/cli.md)
  - [`[server.remote.*]` / `[backend.*]` / `[server.auth.*]`](reference/config/server-remote.md)
  - [`[server.relay.*]`](reference/config/server-relay.md)
  - [`[workflow.*]`](reference/config/workflow.md)
  - [`[project]` / `[user]` / `[log]` / `[web]`](reference/config/common.md)

### Contributing — [contributing/](contributing/)

- [Development Setup](contributing/development.md)
- [Layered Architecture (4 Layers)](contributing/architecture.md) — code structure (domain / application / infra / presentation)
- [Testing](contributing/testing.md) — unit / e2e
- [Release Procedure](contributing/releasing.md)
- [Worktree Workflow](contributing/worktree.md)

## License

MIT
