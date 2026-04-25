# OTel Tracing Operations Guide

A practical guide for running the CLI → Remote attribute propagation and the Remote / Relay OTel trace + business event LogRecord emission on a real shell.

For the full spec (33 `event.name`s, the common attribute schema, the legacy → new event mapping, baggage limits) see the [Tracing Reference](../reference/tracing.md).

## Sharing Environment with Claude Code

Claude Code already emits telemetry using the **standard OTel environment variables** (`OTEL_RESOURCE_ATTRIBUTES` / `OTEL_EXPORTER_OTLP_ENDPOINT` and friends). senko reads the same variables, so a single `export` in the shell drives both.

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
export OTEL_RESOURCE_ATTRIBUTES="deployment.environment=dev,team=backend"

# Dynamic attributes specific to senko go in SENKO_TRACE_ATTRIBUTES or --attr
export SENKO_TRACE_ATTRIBUTES="run.id=$(uuidgen),session.id=$SESSION_ID"

# Both tools ship to the same collector and share the same resource attrs
claude ...
senko task complete 42
```

What this gives you:

- Non-reserved keys like `deployment.environment` or `team` are picked up by senko from `OTEL_RESOURCE_ATTRIBUTES`, loaded into `baggage`, and surface on the Remote two ways: as `baggage.team = "backend"` on the span and as `team = "backend"` (no prefix) on every business event LogRecord emitted during the request.
- Reserved-namespace keys like `service.name` are **not** placed into senko's baggage (mixed-case variants like `SERVICE.NAME` are filtered too). Both Claude Code's and senko's OTel SDKs read them directly as **Resource attributes**, so the same values still land on the backend via that separate path.

## Local Verification: Console Exporter

Before running a collector, sanity-check that the SDK is producing output:

```bash
OTEL_TRACES_EXPORTER=console \
OTEL_LOGS_EXPORTER=console \
senko serve
```

Spans and logs fall out of stdout as JSON. From another terminal:

```bash
senko --attr run.id=demo1 task complete 42
```

Inspect the server's stdout and confirm both channels:

- **Span side**: the request span's `attributes` include `baggage.run.id = "demo1"` (with the `baggage.` prefix).
- **LogRecord side**: a record with `event_name = "senko.task.completed"` carrying `senko.task.id = 42`, `from_status = "in_progress"`, `to_status = "completed"`, `enduser.id = "<your-user>"`, `senko.operation.id = "<UUID>"`, and `run.id = "demo1"` (no prefix).

To see only business events, narrow `RUST_LOG`:

```bash
RUST_LOG=senko_business=info \
OTEL_LOGS_EXPORTER=console \
senko serve
```

Infra-level lines (`Listening on …`, `OTel telemetry initialized`, …) drop away and you only see records with `event_name = "senko.*"`.

## Visualizing with Jaeger

Start the Jaeger all-in-one container:

```bash
docker run -d --rm \
  -p 16686:16686 \
  -p 4317:4317 \
  jaegertracing/jaeger:latest
```

Run the Remote with OTLP:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
OTEL_SERVICE_NAME=senko-dev \
senko serve
```

Drive the CLI from another terminal:

```bash
senko --attr run.id=demo1 --attr user.slot=alice task list
```

Open `http://localhost:16686`, filter by `Service = senko-dev`, and confirm the span's attributes include `baggage.run.id=demo1`, `baggage.user.slot=alice`, `http.route=/api/projects/{project_id}/tasks`, and so on.

In the trace detail view, the **Logs** tab joins business event LogRecords back onto the trace by `trace_id`. Filter by `event_name=senko.task.published` (or any `senko.*`) to surface just the publish-style transitions on that trace.

## Visualizing with Tempo

Tempo also accepts OTLP gRPC, so the same recipe works with just the endpoint swapped. Wire a Tempo datasource into Grafana and use TraceQL:

```traceql
{ event_name = "senko.task.completed" && enduser.id = "alice" }
```

Note: `baggage.run.id` (a span attribute) and `run.id` (the LogRecord attribute) are different fields; pick the right one depending on which channel you're querying.

## External-System Integration (Aviary and friends)

When senko is invoked from an outside orchestrator like Aviary, pass that system's correlation IDs via `--attr`. They are **automatically attached to every business event LogRecord** the Remote emits during the call. This is one of the primary motivations for Contract #8.

```bash
senko \
  --attr aviary.session.id=sess-abc \
  --attr aviary.nest.id=nest-42 \
  --attr aviary.task.id=at-99 \
  task complete 42
```

The Remote's `senko.task.completed` LogRecord then carries:

| Category | Attributes |
|---|---|
| Domain (target) | `senko.task.id=42`, `senko.project.id=…`, `from_status=in_progress`, `to_status=completed` |
| Actor | `enduser.id=<resolved>`, `enduser.name=<resolved>` |
| Common (caller-supplied) | `senko.operation.id=<UUID>`, `aviary.session.id=sess-abc`, `aviary.nest.id=nest-42`, `aviary.task.id=at-99` |
| Resource | `service.name=senko-server`, `service.version=…`, `senko.version=…` |

Aviary's own observability stack (Loki / Datadog / Splunk / …) can then group every senko operation in one session by filtering on `aviary.session.id = "sess-abc"`.

### The Same Attributes Survive Through the Relay

`senko serve --proxy` (the Relay) propagates all of this end-to-end:

```text
[CLI]
  └── --attr aviary.session.id=sess-abc
       │
       ▼  HTTP (baggage header)
[Relay (senko serve --proxy)]
  ├── Relay's own senko.api.call carries aviary.session.id=sess-abc too
  ├── Resolves enduser.* via upstream /auth/me with a 5-minute LRU cache
  └── Forwards baggage upstream verbatim
       │
       ▼
[Remote (senko serve)]
  └── senko.task.completed LogRecord carries aviary.session.id=sess-abc + enduser.* + the full Resource set
```

Because the Relay's own telemetry also gets `enduser.*`, intermediate failures (request reached the Relay but never the Remote) are still visible under the same session ID.

### `senko.version` Identifies the Binary

`senko.version` is a Resource attribute that's **always** present and **cannot** be overridden via env. In mixed-version deployments (Remote upgrades, Relay/Remote skew), `senko.version` lets you pin which binary emitted any given record.

## Audit & Operation Tracking

Contract #8 split actor (`enduser.*`) and target (`senko.*.id`) so the business event LogRecords are usable for audit queries.

### "Who did it?" — actor axis

```traceql
{ event_name =~ "senko\\..*" && enduser.id = "alice" }
```

Returns every operation alice performed (task completions, project edits, API key issuance, …) in chronological order. `enduser.id` is the OTel semantic-convention field, so the same query works in Loki / Tempo / Datadog under the same name.

### "Who was it done to?" — target axis

To track operations performed against a specific user (e.g. `bob`'s sessions revoked, role changed):

```traceql
{ event_name = "senko.user.session_revoked" && senko.user.id = 7 }
{ event_name = "senko.project.member_role_changed" && senko.user.id = 7 }
```

`senko.user.id` is the senko-specific **target** identifier — do not confuse it with `enduser.id` (the username of the operator). LogRecords that carry both express "alice revoked bob's session": both actor and target are present.

### One CLI invocation, all records

If a higher-level script bundles several `senko …` commands as one logical operation, export `SENKO_TRACE_ATTRIBUTES=senko.operation.id=<own-id>` before invoking — every invocation will share the same correlation ID:

```traceql
{ senko.operation.id = "abc-123" }
```

picks up every span and LogRecord belonging to that bundle.

## Verification Checklist

Minimal checks that propagation is working end to end:

- [ ] Remote-side `console` exporter / log backend shows LogRecords with `event_name = "senko.*"` (e.g. running `senko task complete N` produces `senko.task.completed`).
- [ ] Each LogRecord carries `enduser.id` / `enduser.name` (when authenticated) and `senko.operation.id`.
- [ ] Resource attributes `service.name` / `service.version` / `senko.version` appear on every record.
- [ ] When you `--attr aviary.session.id=foo`, the `senko.task.completed` LogRecord's attributes include `aviary.session.id = "foo"` (no prefix).
- [ ] The matching span carries `baggage.run.id = "demo1"` (separate channel, same `trace_id`).
- [ ] Setting `OTEL_RESOURCE_ATTRIBUTES="service.name=foo"` does **not** surface `foo` on the Remote via baggage; mixed-case variants like `SERVICE.NAME` are filtered too.
- [ ] When the same key is set in both `--attr` and `OTEL_RESOURCE_ATTRIBUTES`, the **`--attr`** value wins.
- [ ] Under `OTEL_SDK_DISABLED=true`, neither console nor OTLP exporter produces output (fmt logs still flow).
- [ ] Sending a baggage value longer than 256 bytes produces a `tracing::warn!` truncation warning.
- [ ] Sending more than 32 baggage keys triggers a `tracing::warn!` and drops keys past index 32 alphabetically.
- [ ] Sending more than 8 KB of baggage triggers a `tracing::warn!` and drops keys from the alphabetical tail.
- [ ] `senko.api.call` carries `http.route` as a template (e.g. `/api/projects/{project_id}/tasks`) — never the raw URL or a query string.
- [ ] `senko.api.error` with `error.type=internal` carries a Display-formatted anyhow chain on `error.message` (not the Debug `?e` form).
- [ ] Hook success emits `senko.hook.fired`; hook failure emits `senko.hook.failed` with the appropriate `failure.reason`. Sync-mode hooks carry `enduser.*`.

## Disabling Telemetry

The full kill switch:

```bash
OTEL_SDK_DISABLED=true senko serve
```

No collector traffic, no console output (only fmt logs remain). To disable per-pillar:

```bash
OTEL_TRACES_EXPORTER=none OTEL_LOGS_EXPORTER=none senko serve
```

`OTEL_LOGS_EXPORTER=none` stops business event LogRecords from going to OTLP, but they still appear in the fmt layer (stdout).

## Security Considerations

- **Do not use for authorization.** Baggage, resource attributes, and `enduser.*` are observable, not authoritative. Baggage / resource attrs are user-writable on the CLI side (`--attr` and any env). `enduser.*` is resolved by the auth middleware from an **authenticated** identity, so it is trustworthy for observability — but never wire it directly into authorization checks. Always derive permissions from the authenticated identity (OIDC / API key / trusted headers).
- **Do not put PII in attributes.** Baggage travels on the wire as an HTTP header and is **stored long-term** as spans and business event LogRecords in your backend. Keep email addresses, passwords, API tokens, request bodies, and sensitive path IDs out of values. Switching to `http.route` (template) instead of `http.target` (raw URL) was part of this — sensitive path segments don't leak into telemetry.
- **Mind the size limits.** Anything over 256 bytes per value, more than 32 keys, or 8 KB total is truncated / dropped on the receiver. Don't build queries or aggregations that assume the full payload survives.
- **Malformed values disappear silently.** Per the OTel spec, malformed entries in `OTEL_RESOURCE_ATTRIBUTES` / `SENKO_TRACE_ATTRIBUTES` are **silently skipped**. If you want loud errors on misconfiguration, use `--attr` instead — it rejects malformed input with a parse error.
- **Reserved-namespace filtering is case-insensitive.** `SERVICE.NAME` and friends cannot bypass the filter (Phase F2 closed the bypass).

## See Also

- Full spec: [Tracing Reference](../reference/tracing.md)
- Remote deployment: [server-remote deploy](server-remote/deploy.md)
