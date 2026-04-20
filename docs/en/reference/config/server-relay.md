# `[server.relay.*]` Config

Sections active when `senko serve` is in relay mode (when `[server.relay] url` is set). There is no dedicated relay-mode flag.

A relay has no DB; it forwards incoming API requests over HTTP to an upstream direct server. See [Choosing a Runtime](../../explanation/runtimes.md).

> **Important**: in relay mode `auth_mode` is **locked to None**, and **`[server.auth.*]` is not read and is ignored**. The relay therefore **does no inbound authentication at all**. Operate it on a closed network; authorization comes from limited reachability. To expose it, put a reverse proxy / API Gateway in front that handles authorization.

## `[server]`

`[server]` is shared between direct and relay modes (host / port). See [`[server.*]` / `[backend.*]` / `[server.auth.*]` Config](server-remote.md).

## `[server.relay]`

| Key | Type | Default | Description |
|---|---|---|---|
| `url` | string | `null` | **Required.** Upstream direct-server URL. Setting this makes `senko serve` start in relay mode |
| `token` | string | `null` | Bearer value sent to the upstream (must be accepted by the upstream). If unset, the client's Authorization is passed through |

env overrides: `SENKO_SERVER_RELAY_URL` / `SENKO_SERVER_RELAY_TOKEN`

> There's no `token_arn` / AWS Secrets Manager reference for `[server.relay]`. If you need to source it from Secrets Manager, run `aws secretsmanager get-secret-value ...` in a startup script and inject it into `SENKO_SERVER_RELAY_TOKEN` as env.

### `token` Behavior

| Setting | Behavior |
|---|---|
| `token` set | **Substitution mode**: rewrite `Authorization` with this value for every upstream request (drops the client's token) |
| `token` unset | **Passthrough mode**: pass the client's `Authorization` through as-is |

## `[server.auth.*]` (Ignored)

**In relay mode, `[server.auth.api_key]` / `[server.auth.oidc]` / `[server.auth.trusted_headers]` are all ignored.** Writing them isn't a startup error, but they don't participate in authentication — best to omit them.

To protect the relay inbound side:

- Only listen on a closed network (sandbox-only / inside a VPC / loopback).
- Put a reverse proxy (nginx / Caddy / ALB / API Gateway) in front to apply IP allowlist / mTLS / JWT verification.

## `[server.relay.<action>.hooks.<name>]`

Hooks that fire **after a successful upstream forward** on the relay path.

```toml
[server.relay.task_add.hooks.request_log]
command = "jq -c '.event.task | {id, title}' >> /var/log/senko-relay/request.jsonl"
mode = "async"

[server.relay.task_complete.hooks.audit]
command = "logger -t senko-relay 'task complete'"
mode = "async"
```

The hook envelope's `.user` / `.project` reflect **the relay container's own `[user]` / `[project]` settings** (the relay doesn't authenticate, so there's no per-client identity). To distinguish sandboxes, split the relay into separate instances each with their own `[user] name`.

## When Not to Use a Relay

- "I just want an HTTP proxy" → nginx / Caddy reverse proxy is enough.
- The client can reach the upstream directly → connect straight to the direct server for lower latency.
- You want per-client auth handling → impossible from the relay alone (it doesn't authenticate). Authorize at a fronting layer, or run separate relay instances per client.

Relays are a good fit when **the source network can't reach the upstream directly** and **you need to substitute authentication (service-token-style)**.

## Minimum Config Example

```toml
[server]
host = "127.0.0.1"              # listen only within the closed network
port = 3142

[server.relay]
url   = "https://senko-upstream.example.com"
token = "..."                   # usually injected via env SENKO_SERVER_RELAY_TOKEN

[server.relay.task_complete.hooks.audit]
command = 'jq -c "." >> /var/log/senko-relay/audit.jsonl'
mode    = "async"
```
