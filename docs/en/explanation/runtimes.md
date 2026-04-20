# Choosing a Runtime

senko is one binary, but depending on how it starts it behaves as **one of three runtimes**. Which runtime is active determines which config sections and hooks are "live."

## The Three Runtimes

| Runtime | How to start it | Where data lives | Config sections |
|---|---|---|---|
| **cli** | `senko task ...` (anything other than `serve`) | Local SQLite / remote HTTP | `[cli.*]` |
| **server.remote** | `senko serve` | Local SQLite / PostgreSQL | `[server.remote.*]` `[server.auth.*]` `[backend.*]` |
| **server.relay** | `senko serve` with `[server.relay] url` set | Forwarded to an upstream `senko serve` | `[server.relay.*]` |

## Decision Flow

```
Q1. Will you stand up a server?
    │
    ├─ No → Use [cli] (local SQLite)
    │        → getting-started/local-sqlite.md
    │
    └─ Yes
        │
        Q2. Can the client talk to the DB directly?
        │
        ├─ Yes → [server.remote]  (= senko serve)
        │         → getting-started/cli-remote-postgres.md
        │
        └─ No (AI sandbox etc. — you need to relay to an upstream)
              → [server.relay]  (= `senko serve` in relay mode)
                 → getting-started/cli-relay-remote-postgres.md
```

## What Each One Does

### cli

- Active runtime when you run CLI commands like `senko task add` / `senko task next`.
- By default it talks to a local SQLite directly, but setting `[cli.remote]` turns it into a client that forwards operations to an upstream `senko serve` over HTTP.
- Hooks go under `[cli.task_add.hooks.<name>]`, etc.
- Claude Code's skill just calls `senko` internally, so any `/senko` operation runs under this runtime too.

### server.remote

- **The server with the team-shared DB**, started as `senko serve`.
- Reads/writes SQLite / PostgreSQL directly and exposes the REST API.
- Three auth modes (API key / OIDC / trusted headers).
- Hooks like `[server.remote.task_complete.hooks.audit]` go here for things you want to fire server-side.
- Examples: ship audit logs to SIEM on task completion, emit metrics, post to Slack.

### server.relay

- **A thin server with no DB that just HTTP-forwards to an upstream.** `senko serve` automatically enters this mode when `[server.relay] url` is configured (there's no dedicated flag).
- **No inbound authentication** (`auth_mode` is always `None`). Operate it under the assumption of **network isolation**; the boundary of reachability is the effective authorization.
- Use cases:
  - **AI sandbox** — the agent can't talk to the outside directly; the sandbox-internal relay routes outbound traffic.
  - **Token substitution** — the client holds no credential; the relay rewrites Authorization with a stored M2M JWT or API key on the way out.
- Hooks fire along the relay path (primarily for auditing).
- To expose it to external callers, put a reverse proxy / API Gateway in front that does the authorization.

## Does the same "action" fire in multiple runtimes?

**No.** A `task_complete` event fires only `[cli.task_complete.hooks.*]` when the active runtime is `cli`, only `[server.remote.task_complete.hooks.*]` when it's `server.remote`, and so on.

Quick mapping by use case:

| Goal | Where it goes |
|---|---|
| Developer desktop notifications | `[cli.*]` |
| Server-side audit log / SIEM integration | `[server.remote.*]` |
| Logging every request that passes through the relay | `[server.relay.*]` |

## Combined Setup Examples

### Case A: solo, local only

- Runtime: `cli`
- Config: `[cli.*]` hooks only in `.senko/config.toml`
- DB: `$XDG_DATA_HOME/senko/projects/<dir>/data.db`

### Case B: team-shared server

- Server-side runtime: `server.remote`
  - Config: `[server.remote.*]` hooks, `[server.auth.oidc]`, etc. in the server's `.senko/config.toml`
  - DB: PostgreSQL
- Developer-side runtime: `cli` + `[cli.remote]`
  - Config: each developer's `.senko/config.local.toml` sets `[cli.remote] url = ...`
  - DB: accessed via remote

### Case C: AI sandbox

- Sandbox-side runtime: `server.relay`
  - HTTP-forwards to the upstream remote server
- Upstream runtime: `server.remote`
  - Holds the real DB

## What to Read Next

- Concrete config per runtime → `reference/config/cli.md` / `server-remote.md` / `server-relay.md`
- Hook schema → [Hooks Reference](../reference/hooks.md)
- Deployment → `guides/server-remote/deploy.md` / `guides/server-relay/deploy.md`
