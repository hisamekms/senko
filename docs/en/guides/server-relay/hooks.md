# `[server.relay.*]` Hook Examples

Practical patterns for hooks that fire along the relay-mode `senko serve` path.

Schema: [Hooks Reference](../../reference/hooks.md).

## Positioning of Relay Hooks

The relay just forwards to the upstream and holds no DB, so most **"hooks to fire on the relay"** are for **audit and observability**:

- Record frequency and patterns of requests passing through the relay.
- Watch for upstream error rates as seen from the forwarder.
- Fan requests matching a condition out to other systems (log aggregation, DLQ, etc.).

> **Identity limitation**: since the relay **does no inbound authentication** (`auth_mode: None`), hook envelope's `.user` / `.project` reflect **the relay container's own `[user] name` / `[project] name`** (determined by startup env / config). You cannot distinguish per-client. For per-client auditing, the standard approach is **one relay instance per client**.

Heavy work (external integrations, notifications) is structurally cleaner on the **upstream** via `[server.remote.*]`. Keep the relay side "observing only."

## Audit Logs (per relay instance)

In substitution mode, upstream logs only record actions for the single identity the relay holds (the session API key owner or M2M bot). Collecting audit on the relay side is mandatory:

```toml
[server.relay.task_add.hooks.audit]
command = '''
jq -c "{
  ts: .event.timestamp,
  via: .user.name,
  project: .project.name,
  action: .event.event,
  task: .event.task.id,
  title: .event.task.title
}" >> /var/log/senko-relay/audit.jsonl
'''
mode = "async"
```

The `via` (relay name) field tells you which relay the request went through.

Duplicate the same hook on each action (`task_ready`, `task_start`, `task_complete`, `task_cancel`, `contract_*`) for full-path auditing.

## Ship to Fluent Bit / Vector

Instead of a local file, send directly to a log shipper's socket:

```toml
[server.relay.task_complete.hooks.fluent]
command = 'jq -c "." | nc -u -w 1 127.0.0.1 5140'
mode = "async"
```

## Counting Upstream Errors

The relay passes upstream errors straight back to the client. Observe error rates from the relay side:

```toml
[server.relay.task_add.hooks.error_count]
command = '''
# The hook is only invoked when the envelope arrives, i.e. after a successful
# upstream call. Incrementing this counter gives you the "success rate."
curl -s -X POST "$METRICS_URL" --data "senko_relay_success 1"
'''
mode = "async"

[[server.relay.task_add.hooks.error_count.env_vars]]
name = "METRICS_URL"
required = true
```

> **Important**: relay hooks fire **after a successful upstream forward**. They don't fire when the upstream returns 5xx. For failure-rate metrics, aggregating HTTP logs on the nginx / reverse proxy in front is more accurate.

## Relay Instance as an Actor Identifier

`.user.name` is fixed at the relay container's startup `[user] name` / `SENKO_USER`. If you run several sandboxes, run several relays — each with a different `SENKO_USER` — to reflect it in the audit:

```bash
# Relay for sandbox A
podman run -e SENKO_USER=sandbox-A ... senko-relay

# Relay for sandbox B
podman run -e SENKO_USER=sandbox-B ... senko-relay
```

The envelope's `.user.name` tells you which relay a request passed through:

```toml
[server.relay.task_complete.hooks.who_did_it]
command = '''
echo "$(date -u +%FT%TZ) task_complete project=$(jq -r '.project.name') task=$(jq -r '.event.task.id') via=$(jq -r '.user.name')" \
  >> /var/log/senko-relay/actors.log
'''
mode = "async"
```

Push this to S3 / Splunk / etc. to correlate "upstream log × relay log."

## Gotchas When Writing Relay Hooks

- **The envelope's `runtime` is `"server.relay"`** on stdin. Branch on this if a hook script is shared with cli / server.remote.
- The envelope's `.project` / `.user` are **the relay container's own `[project]` / `[user]` settings**, not the client's identity (the relay doesn't authenticate).
- Even with fire-and-forget hooks, **server liveness** isn't affected. That said, running commands that log huge amounts synchronously hurts latency — default to `async`.

## Choosing Between Relay and Upstream Hooks

| Goal | Where to place |
|---|---|
| Audit every request that passes through the relay | `[server.relay.*]` |
| Notify on upstream DB state changes (all paths) | `[server.remote.*]` |
| Personal notifications for a CLI operator | `[cli.*]` |
| Prompt augmentation for the agent | `[workflow.*]` |

Putting the same hook on both the relay and the upstream causes **double firing**. Normally, pick one side.
