# senko

> **Alpha**: This project is in early development. APIs, CLI interfaces, and data formats may change without notice.

senko is a **workflow orchestrator that lets AI agents drive work autonomously**.
It's less of a "task manager" and more of a tool for **codifying project-specific ways of working and teaching them to agents** — designed primarily for use with Claude Code.

> [日本語ドキュメント (Japanese)](docs/ja/README.md) / **English (this page)**
>
> **Translation note**: The Japanese docs (`docs/ja/`) are the source of truth. English docs (this page and `docs/en/`) are AI-generated — please open a PR or issue if you find mistakes.

## Core Concept: The Three Pillars

senko supports autonomous AI-agent behavior with three pillars:

1. **Event-Driven Workflow** — Inject and verify project-specific rules (DoD / branch conventions / required metadata / per-phase instructions) automatically, in step with the agent's actions. Hooks and workflow stages carry this load.
2. **An execution model that lets the agent focus on the next task** — Split large work into dependency-aware, priority-ordered tasks so the agent always focuses on just the next one. Instead of cramming everything into one huge prompt, context is reset per task. Multiple tasks whose dependencies have all cleared can be picked in parallel from separate sessions.
3. **Contracts hold the big picture** — A task is sized to complete and be discarded within one context window, but a Contract is sized to **group multiple tasks together**, persisting cross-cutting context and findings along with Notes. The overall arc stays visible as the task count grows.

→ Deep dive: [Core Concept: The Three Pillars](docs/en/explanation/core-concept.md)

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

## Documentation

Organized in four layers by reader intent.

### I want to get hands-on first — [docs/en/getting-started/](docs/en/getting-started/)

Covers three typical deployments with an overview, an architecture diagram, and end-to-end setup.

- [Local SQLite](docs/en/getting-started/local-sqlite.md) — personal development, solo
- [CLI → Remote → PostgreSQL](docs/en/getting-started/cli-remote-postgres.md) — team sharing a server
- [CLI → Relay → Remote → PostgreSQL](docs/en/getting-started/cli-relay-remote-postgres.md) — AI sandbox (CLI holds no secret; relay concentrates the credentials)

### I want to understand the thinking — [docs/en/explanation/](docs/en/explanation/)

Explains **why** senko is designed the way it is, framed around the three pillars.

- [Core Concept: The Three Pillars](docs/en/explanation/core-concept.md) — read this first
- [Event-Driven Workflow](docs/en/explanation/event-driven-workflow.md) — Pillar 1: how hooks and workflow stages work
- [Focus on the Next Task: The Execution Model](docs/en/explanation/task-decomposition.md) — Pillar 2: dependencies, priority, parallel pick
- [Holding the Big Picture with Contracts](docs/en/explanation/contract.md) — Pillar 3: long-lived context and Notes
- [Choosing a Runtime](docs/en/explanation/runtimes.md) — delivery substrate: cli / server.remote / server.relay

### I want to set up or deploy — [docs/en/guides/](docs/en/guides/)

How-to guides organized by deployment shape.

**If you use the CLI** — [docs/en/guides/cli/](docs/en/guides/cli/)
- [Installing and Updating the Skill](docs/en/guides/cli/skill-install.md)
- [Workflow Stage Examples](docs/en/guides/cli/workflow-stages.md)
- [`[cli.*]` Hook Examples](docs/en/guides/cli/hooks.md)
- [Switching CLI Backends](docs/en/guides/cli/backends.md) — SQLite / PostgreSQL / HTTP

**Server operators (`senko serve`)** — [docs/en/guides/server-remote/](docs/en/guides/server-remote/)
- [Deploy](docs/en/guides/server-remote/deploy.md)
- [OIDC Authentication](docs/en/guides/server-remote/auth-oidc.md) — recommended for production (humans use PKCE, bots use Client Credentials)
- [Trusted Headers Authentication](docs/en/guides/server-remote/auth-trusted-headers.md) — behind an API Gateway
- [AWS Deployment](docs/en/guides/server-remote/aws-deployment.md) — API Gateway + Cognito + Lambda
- [`[server.remote.*]` Hook Examples](docs/en/guides/server-remote/hooks.md)
- [API Key Authentication](docs/en/guides/server-remote/auth-api-key.md) — for smoke tests and bootstrapping; not meant for production

**Relay operators (`senko serve` in relay mode)** — [docs/en/guides/server-relay/](docs/en/guides/server-relay/)
- [Deploy](docs/en/guides/server-relay/deploy.md)
- [Token Relay Pattern](docs/en/guides/server-relay/token-relay.md)
- [`[server.relay.*]` Hook Examples](docs/en/guides/server-relay/hooks.md)

### I want to look up a spec — [docs/en/reference/](docs/en/reference/)

- [CLI Reference](docs/en/reference/cli.md) — every subcommand
- [REST API Reference](docs/en/reference/api.md) — every endpoint
- [Data Model](docs/en/reference/data-model.md) — DB schema
- [Hooks Reference](docs/en/reference/hooks.md) — envelope and trigger matrix
- **Config Reference** — [docs/en/reference/config/](docs/en/reference/config/)
  - [Overview](docs/en/reference/config/overview.md) — file layout, precedence, runtime filtering
  - [`[cli.*]`](docs/en/reference/config/cli.md)
  - [`[server.remote.*]` / `[backend.*]` / `[server.auth.*]`](docs/en/reference/config/server-remote.md)
  - [`[server.relay.*]`](docs/en/reference/config/server-relay.md)
  - [`[workflow.*]`](docs/en/reference/config/workflow.md)
  - [`[project]` / `[user]` / `[log]` / `[web]`](docs/en/reference/config/common.md)

### Contributing — [docs/en/contributing/](docs/en/contributing/)

- [Development Setup](docs/en/contributing/development.md)
- [Layered Architecture (4 Layers)](docs/en/contributing/architecture.md) — code structure (domain / application / infra / presentation)
- [Testing](docs/en/contributing/testing.md) — unit / e2e
- [Release Procedure](docs/en/contributing/releasing.md)
- [Worktree Workflow](docs/en/contributing/worktree.md)

## License

MIT
