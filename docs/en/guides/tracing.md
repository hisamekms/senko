# OTel Tracing Operations Guide

A practical guide for running the CLI → Remote attribute propagation and the Remote-side OTel trace / log emission on a real shell.

For the full spec, see the [Tracing Reference](../reference/tracing.md).

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

- Non-reserved keys like `deployment.environment` or `team` are picked up by senko from `OTEL_RESOURCE_ATTRIBUTES`, loaded into `baggage`, and promoted to span attributes like `baggage.team = "backend"` on the Remote.
- Reserved-namespace keys like `service.name` are **not** placed into senko's baggage. Both Claude Code's and senko's OTel SDKs read them directly as **Resource attributes**, so the same values still land on the backend via that separate path.

## Local Verification: Console Exporter

Before running a collector, sanity-check that the SDK is producing output:

```bash
OTEL_TRACES_EXPORTER=console \
OTEL_LOGS_EXPORTER=console \
senko serve
```

Spans and logs fall out of stdout as JSON. From another terminal:

```bash
senko --attr run.id=demo1 task list
```

Inspect the server's stdout and confirm the span carries `baggage.run.id = "demo1"`.

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

Open `http://localhost:16686`, filter by `Service = senko-dev`, and confirm the span's attributes include `baggage.run.id=demo1` and `baggage.user.slot=alice`.

## Visualizing with Tempo

Tempo also accepts OTLP gRPC, so the same recipe works with just the endpoint swapped. Wire a Tempo datasource into Grafana and query with TraceQL, e.g. `baggage.run.id="demo1"`.

## Verification Checklist

Minimal checks that propagation is working end to end:

- [ ] The Remote's request logs show the inbound `baggage` header value.
- [ ] Each span carries `baggage.<key>` attributes.
- [ ] Setting `OTEL_RESOURCE_ATTRIBUTES="service.name=foo"` does **not** surface `foo` on the Remote via baggage (reserved-namespace filtering).
- [ ] When the same key is set in both `--attr` and `OTEL_RESOURCE_ATTRIBUTES`, the **`--attr`** value wins.
- [ ] Under `OTEL_SDK_DISABLED=true`, neither console nor OTLP exporter produces output (fmt logs still flow).
- [ ] Sending a baggage value longer than 256 bytes produces a `tracing::warn!` truncation warning in the Remote's logs.

## Disabling Telemetry

The full kill switch:

```bash
OTEL_SDK_DISABLED=true senko serve
```

No collector traffic, no console output (only fmt logs remain). To disable per-pillar:

```bash
OTEL_TRACES_EXPORTER=none OTEL_LOGS_EXPORTER=none senko serve
```

## Security Considerations

- **Do not use for authorization.** Baggage and resource attributes are freely writable on the CLI side (`--attr` and any env). Never derive authorization decisions (user identity, role interpretation, etc.) from them on the Remote. Always go through an **authenticated identity** — OIDC, API key, or trusted headers.
- **Do not put PII in attributes.** Baggage travels on the wire as an HTTP header and is **stored long-term** as spans / logs in your backend. Keep email addresses, passwords, API tokens, and request bodies out of the values.
- **Mind the value size.** Anything over 256 bytes is truncated on the Remote with a warn log. Do not build queries or aggregations that assume the untruncated value is present.
- **Malformed values disappear silently.** Per the OTel spec, malformed entries in `OTEL_RESOURCE_ATTRIBUTES` / `SENKO_TRACE_ATTRIBUTES` are **silently skipped**. If you want loud errors on misconfiguration, use `--attr` instead — it rejects malformed input with a parse error.

## See Also

- Full spec: [Tracing Reference](../reference/tracing.md)
- Remote deployment: [server-remote deploy](server-remote/deploy.md)
